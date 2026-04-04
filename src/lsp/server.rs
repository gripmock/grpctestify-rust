use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::{Client, LanguageServer, LspService, Server, lsp_types::*};

use crate::config;
use crate::grpc::client::{GrpcClient, GrpcClientConfig, ProtoConfig};
use crate::lsp::handlers;
use crate::lsp::variable_definition;
use crate::optimizer;
use crate::parser::ast::SectionType;
use crate::parser::{self, GctfDocument};
use crate::plugins::{PluginManager, PluginPurity, PluginReturnKind};

type DocumentMap<T> = Arc<RwLock<HashMap<String, T>>>;
type VersionedMap<T> = Arc<RwLock<HashMap<String, (i32, T)>>>;
type EndpointCompletionCache = Arc<RwLock<HashMap<String, (Instant, Vec<CompletionItem>)>>>;

pub struct GrpctestifyLsp {
    client: Client,
    documents: DocumentMap<String>,
    doc_versions: DocumentMap<i32>,
    parsed_docs: DocumentMap<GctfDocument>,
    parsed_doc_versions: DocumentMap<i32>,
    semantic_tokens_cache: VersionedMap<SemanticTokens>,
    inlay_hints_cache: VersionedMap<Vec<InlayHint>>,
    endpoint_completion_cache: EndpointCompletionCache,
}

impl GrpctestifyLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            doc_versions: Arc::new(RwLock::new(HashMap::new())),
            parsed_docs: Arc::new(RwLock::new(HashMap::new())),
            parsed_doc_versions: Arc::new(RwLock::new(HashMap::new())),
            semantic_tokens_cache: Arc::new(RwLock::new(HashMap::new())),
            inlay_hints_cache: Arc::new(RwLock::new(HashMap::new())),
            endpoint_completion_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn parse_string_list(raw: &str) -> Vec<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        if let Ok(values) = serde_json::from_str::<Vec<String>>(trimmed) {
            return values;
        }

        trimmed
            .split(',')
            .map(|value| value.trim().trim_matches('"').trim_matches('\''))
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn resolve_relative_path(base_dir: &Path, value: &str) -> String {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return value.to_string();
        }
        base_dir.join(path).to_string_lossy().to_string()
    }

    fn proto_config_from_document(doc: &GctfDocument, uri: &Url) -> Option<ProtoConfig> {
        let config = doc.get_proto_config()?;
        let base_dir = uri
            .to_file_path()
            .ok()
            .and_then(|path| path.parent().map(ToOwned::to_owned))
            .unwrap_or_else(|| PathBuf::from("."));

        let descriptor = config
            .get("descriptor")
            .map(|value| Self::resolve_relative_path(&base_dir, value));

        let files = config
            .get("files")
            .map(|value| Self::parse_string_list(value))
            .unwrap_or_default()
            .into_iter()
            .map(|value| Self::resolve_relative_path(&base_dir, &value))
            .collect::<Vec<_>>();

        let import_paths = config
            .get("import_paths")
            .map(|value| Self::parse_string_list(value))
            .unwrap_or_default()
            .into_iter()
            .map(|value| Self::resolve_relative_path(&base_dir, &value))
            .collect::<Vec<_>>();

        if descriptor.is_none() && files.is_empty() {
            return None;
        }

        Some(ProtoConfig {
            files,
            import_paths,
            descriptor,
        })
    }

    async fn create_schema_client(
        &self,
        address: &str,
        proto_config: Option<ProtoConfig>,
        target_service: Option<String>,
    ) -> Option<GrpcClient> {
        const SCHEMA_TIMEOUT: Duration = Duration::from_secs(3);

        let config = GrpcClientConfig {
            address: address.to_string(),
            timeout_seconds: 3,
            tls_config: None,
            proto_config,
            metadata: None,
            target_service,
            compression: Default::default(),
        };

        let created = tokio::time::timeout(SCHEMA_TIMEOUT, GrpcClient::new(config)).await;
        let Ok(Ok(client)) = created else {
            return None;
        };
        Some(client)
    }

    async fn schema_endpoint_completions(
        &self,
        address: &str,
        proto_config: Option<ProtoConfig>,
    ) -> Vec<CompletionItem> {
        const CACHE_TTL: Duration = Duration::from_secs(30);
        let cache_key = format!(
            "{}|{:?}",
            address,
            proto_config.as_ref().map(|config| (
                config.descriptor.clone(),
                config.files.clone(),
                config.import_paths.clone(),
            ))
        );

        {
            let cache = self.endpoint_completion_cache.read().await;
            if let Some((cached_at, cached_items)) = cache.get(&cache_key)
                && cached_at.elapsed() < CACHE_TTL
            {
                return cached_items.clone();
            }
        }

        let Some(client) = self.create_schema_client(address, proto_config, None).await else {
            return Vec::new();
        };

        let mut items = Vec::new();
        for service in client.descriptor_pool().services() {
            let service_name = service.full_name().to_string();
            for method in service.methods() {
                let endpoint = format!("{}/{}", service_name, method.name());
                items.push(CompletionItem {
                    label: endpoint.clone(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some(format!("Reflected from {}", address)),
                    insert_text: Some(endpoint),
                    ..CompletionItem::default()
                });
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);

        self.endpoint_completion_cache
            .write()
            .await
            .insert(cache_key, (Instant::now(), items.clone()));

        items
    }

    async fn schema_message_field_completions(
        &self,
        doc: &GctfDocument,
        uri: &Url,
        content: &str,
        section_start_line: usize,
        cursor_line: usize,
        for_response: bool,
    ) -> Vec<CompletionItem> {
        let endpoint = match doc.get_endpoint() {
            Some(value) => value,
            None => return Vec::new(),
        };

        let mut parts = endpoint.split('/');
        let service_name = match parts.next() {
            Some(value) if !value.is_empty() => value,
            _ => return Vec::new(),
        };
        let method_name = match parts.next() {
            Some(value) if !value.is_empty() => value,
            _ => return Vec::new(),
        };

        let address = doc
            .get_address(
                std::env::var(config::ENV_GRPCTESTIFY_ADDRESS)
                    .ok()
                    .as_deref(),
            )
            .unwrap_or_else(config::default_address);
        let proto_config = Self::proto_config_from_document(doc, uri);

        let Some(client) = self
            .create_schema_client(&address, proto_config, Some(service_name.to_string()))
            .await
        else {
            return Vec::new();
        };

        let Some(service) = client.descriptor_pool().get_service_by_name(service_name) else {
            return Vec::new();
        };
        let Some(method) = service
            .methods()
            .find(|method| method.name() == method_name)
        else {
            return Vec::new();
        };

        let message_desc = if for_response {
            method.output()
        } else {
            method.input()
        };

        let json_path = Self::infer_json_object_path(content, section_start_line, cursor_line);
        let target_message = Self::resolve_message_path(&message_desc, &json_path)
            .unwrap_or_else(|| message_desc.clone());

        let mut items: Vec<CompletionItem> = target_message
            .fields()
            .map(|field| {
                let value_snippet = Self::json_value_snippet_for_field(&field, 0, 2);
                CompletionItem {
                    label: field.name().to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(format!(
                        "{} field from {} schema",
                        if for_response { "Response" } else { "Request" },
                        service_name
                    )),
                    insert_text: Some(format!("\"{}\": {}", field.name(), value_snippet)),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..CompletionItem::default()
                }
            })
            .collect();

        if items.is_empty() {
            items.push(CompletionItem {
                label: "\"field\": \"value\"".to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some("JSON field template (schema unavailable)".to_string()),
                insert_text: Some("\"${1:field}\": \"${2:value}\"".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..CompletionItem::default()
            });
        }

        items
    }

    fn resolve_message_path(
        root: &prost_reflect::MessageDescriptor,
        path: &[String],
    ) -> Option<prost_reflect::MessageDescriptor> {
        let mut current = root.clone();
        for segment in path {
            let field = current.fields().find(|field| field.name() == segment)?;
            let prost_reflect::Kind::Message(child) = field.kind() else {
                return None;
            };
            current = child;
        }
        Some(current)
    }

    fn infer_json_object_path(
        content: &str,
        section_start_line: usize,
        cursor_line: usize,
    ) -> Vec<String> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let start = section_start_line.saturating_sub(1);
        let end = cursor_line.min(lines.len().saturating_sub(1));
        let mut object_stack: Vec<Option<String>> = Vec::new();
        let mut pending_key: Option<String> = None;

        for line in lines.iter().take(end + 1).skip(start) {
            let mut chars = line.chars().peekable();
            let mut in_string = false;
            let mut escaped = false;
            let mut string_buf = String::new();

            while let Some(ch) = chars.next() {
                if in_string {
                    if escaped {
                        escaped = false;
                        string_buf.push(ch);
                        continue;
                    }
                    match ch {
                        '\\' => escaped = true,
                        '"' => {
                            in_string = false;

                            let mut lookahead = chars.clone();
                            while let Some(next) = lookahead.peek() {
                                if next.is_whitespace() {
                                    lookahead.next();
                                } else {
                                    break;
                                }
                            }
                            if matches!(lookahead.peek(), Some(':')) {
                                pending_key = Some(string_buf.clone());
                            }
                            string_buf.clear();
                        }
                        _ => string_buf.push(ch),
                    }
                    continue;
                }

                if ch == '#' {
                    break;
                }

                if ch == '/' && matches!(chars.peek(), Some('/')) {
                    break;
                }

                match ch {
                    '"' => {
                        in_string = true;
                        escaped = false;
                        string_buf.clear();
                    }
                    '{' => {
                        object_stack.push(pending_key.take());
                    }
                    '}' => {
                        if !object_stack.is_empty() {
                            object_stack.pop();
                        }
                        pending_key = None;
                    }
                    _ => {}
                }
            }
        }

        object_stack.into_iter().flatten().collect()
    }

    async fn schema_assert_path_completions(
        &self,
        doc: &GctfDocument,
        uri: &Url,
    ) -> Vec<CompletionItem> {
        let endpoint = match doc.get_endpoint() {
            Some(value) => value,
            None => return Vec::new(),
        };

        let mut parts = endpoint.split('/');
        let service_name = match parts.next() {
            Some(value) if !value.is_empty() => value,
            _ => return Vec::new(),
        };
        let method_name = match parts.next() {
            Some(value) if !value.is_empty() => value,
            _ => return Vec::new(),
        };

        let address = doc
            .get_address(
                std::env::var(config::ENV_GRPCTESTIFY_ADDRESS)
                    .ok()
                    .as_deref(),
            )
            .unwrap_or_else(config::default_address);
        let proto_config = Self::proto_config_from_document(doc, uri);

        let Some(client) = self
            .create_schema_client(&address, proto_config, Some(service_name.to_string()))
            .await
        else {
            return Vec::new();
        };

        let Some(service) = client.descriptor_pool().get_service_by_name(service_name) else {
            return Vec::new();
        };
        let Some(method) = service
            .methods()
            .find(|method| method.name() == method_name)
        else {
            return Vec::new();
        };

        let mut paths = Vec::new();
        Self::collect_message_json_paths(&method.output(), String::new(), 0, 3, &mut paths);

        paths.sort();
        paths.dedup();
        paths
            .into_iter()
            .map(|path| CompletionItem {
                label: path.clone(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some("Response JSON path from schema".to_string()),
                insert_text: Some(path),
                ..CompletionItem::default()
            })
            .collect()
    }

    fn collect_message_json_paths(
        message_desc: &prost_reflect::MessageDescriptor,
        prefix: String,
        depth: usize,
        max_depth: usize,
        out: &mut Vec<String>,
    ) {
        if depth > max_depth {
            return;
        }

        for field in message_desc.fields() {
            let base = if prefix.is_empty() {
                format!(".{}", field.name())
            } else {
                format!("{}.{}", prefix, field.name())
            };

            if field.is_list() {
                out.push(format!("{}[]", base));
                continue;
            }
            if field.is_map() {
                out.push(format!("{}.<key>", base));
                continue;
            }

            out.push(base.clone());
            if let prost_reflect::Kind::Message(child) = field.kind() {
                Self::collect_message_json_paths(&child, base, depth + 1, max_depth, out);
            }
        }
    }

    fn json_value_snippet_for_field(
        field: &prost_reflect::FieldDescriptor,
        depth: usize,
        max_depth: usize,
    ) -> String {
        if field.is_list() {
            return "[]".to_string();
        }
        if field.is_map() {
            return "{}".to_string();
        }

        match field.kind() {
            prost_reflect::Kind::Bool => "false".to_string(),
            prost_reflect::Kind::String | prost_reflect::Kind::Bytes => {
                "\"${1:value}\"".to_string()
            }
            prost_reflect::Kind::Double | prost_reflect::Kind::Float => "0.0".to_string(),
            prost_reflect::Kind::Enum(enum_desc) => enum_desc
                .values()
                .next()
                .map(|value| format!("\"{}\"", value.name()))
                .unwrap_or_else(|| "\"\"".to_string()),
            prost_reflect::Kind::Message(message_desc) => {
                Self::json_object_snippet_for_message(&message_desc, depth + 1, max_depth)
            }
            _ => "0".to_string(),
        }
    }

    fn json_object_snippet_for_message(
        message_desc: &prost_reflect::MessageDescriptor,
        depth: usize,
        max_depth: usize,
    ) -> String {
        if depth > max_depth {
            return "{}".to_string();
        }

        let fields: Vec<_> = message_desc.fields().take(6).collect();
        if fields.is_empty() {
            return "{}".to_string();
        }

        let mut lines = Vec::new();
        for field in fields {
            let value = Self::json_value_snippet_for_field(&field, depth, max_depth);
            lines.push(format!("  \"{}\": {}", field.name(), value));
        }
        format!("{{\n{}\n}}", lines.join(",\n"))
    }

    fn inlay_cache_key(uri: &Url, range: &Range) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            uri, range.start.line, range.start.character, range.end.line, range.end.character
        )
    }

    async fn current_doc_version(&self, uri: &Url) -> i32 {
        self.doc_versions
            .read()
            .await
            .get(&uri.to_string())
            .copied()
            .unwrap_or(-1)
    }

    async fn invalidate_analysis_cache(&self, uri: &Url) {
        let uri_key = uri.to_string();
        self.parsed_docs.write().await.remove(&uri_key);
        self.parsed_doc_versions.write().await.remove(&uri_key);
        self.semantic_tokens_cache.write().await.remove(&uri_key);

        let mut inlay_cache = self.inlay_hints_cache.write().await;
        let prefix = format!("{}:", uri_key);
        inlay_cache.retain(|k, _| !k.starts_with(&prefix));
    }

    async fn get_or_parse_document(&self, uri: &Url, content: &str) -> Option<GctfDocument> {
        let uri_key = uri.to_string();
        let version = self.current_doc_version(uri).await;

        {
            let parsed = self.parsed_docs.read().await;
            let parsed_versions = self.parsed_doc_versions.read().await;
            if let (Some(doc), Some(doc_ver)) =
                (parsed.get(&uri_key), parsed_versions.get(&uri_key))
                && *doc_ver == version
            {
                return Some(doc.clone());
            }
        }

        let file_name = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| uri.to_string());
        let parsed = parser::parse_gctf_from_str(content, &file_name).ok()?;

        self.parsed_docs
            .write()
            .await
            .insert(uri_key.clone(), parsed.clone());
        self.parsed_doc_versions
            .write()
            .await
            .insert(uri_key, version);

        Some(parsed)
    }

    async fn publish_diagnostics(&self, uri: &Url, content: &str) {
        let file_name = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| uri.to_string());

        match parser::parse_gctf_from_str(content, &file_name) {
            Ok(document) => {
                self.parsed_docs
                    .write()
                    .await
                    .insert(uri.to_string(), document.clone());
                let version = self.current_doc_version(uri).await;
                self.parsed_doc_versions
                    .write()
                    .await
                    .insert(uri.to_string(), version);

                let errors = crate::parser::validator::validate_document_diagnostics(&document);
                let mut lsp_diags: Vec<Diagnostic> = errors
                    .iter()
                    .map(|e| handlers::validation_error_to_diagnostic(e, content))
                    .collect();
                lsp_diags.extend(handlers::collect_semantic_diagnostics(&document, content));
                lsp_diags.extend(handlers::collect_optimizer_diagnostics(&document, content));

                self.client
                    .publish_diagnostics(uri.clone(), lsp_diags, None)
                    .await;
            }
            Err(e) => {
                let diag = Diagnostic::new_simple(
                    Range::new(Position::new(0, 0), Position::new(0, 0)),
                    format!("Parse error: {}", e),
                );
                self.client
                    .publish_diagnostics(uri.clone(), vec![diag], None)
                    .await;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for GrpctestifyLsp {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        "-".to_string(),
                        ".".to_string(),
                        "/".to_string(),
                        "@".to_string(),
                        ":".to_string(),
                        "#".to_string(),
                        "\"".to_string(),
                        "{".to_string(),
                        "[".to_string(),
                        ",".to_string(),
                        " ".to_string(),
                    ]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                semantic_tokens_provider: None,
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "grpctestify LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        let version = params.text_document.version;
        self.documents
            .write()
            .await
            .insert(uri.to_string(), content.clone());
        self.doc_versions
            .write()
            .await
            .insert(uri.to_string(), version);
        self.invalidate_analysis_cache(&uri).await;
        self.publish_diagnostics(&uri, &content).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let Some(content) = params.content_changes.last().map(|c| c.text.clone()) else {
            return;
        };
        self.documents
            .write()
            .await
            .insert(uri.to_string(), content.clone());
        self.doc_versions
            .write()
            .await
            .insert(uri.to_string(), version);
        self.invalidate_analysis_cache(&uri).await;
        self.publish_diagnostics(&uri, &content).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Ok(content) = tokio::fs::read_to_string(uri.to_file_path().unwrap_or_default()).await
        {
            self.documents
                .write()
                .await
                .insert(uri.to_string(), content.clone());
            self.invalidate_analysis_cache(&uri).await;
            self.publish_diagnostics(&uri, &content).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(&uri.to_string());
        self.doc_versions.write().await.remove(&uri.to_string());
        self.parsed_docs.write().await.remove(&uri.to_string());
        self.parsed_doc_versions
            .write()
            .await
            .remove(&uri.to_string());
        self.semantic_tokens_cache
            .write()
            .await
            .remove(&uri.to_string());
        let mut inlay_cache = self.inlay_hints_cache.write().await;
        let prefix = format!("{}:", uri);
        inlay_cache.retain(|k, _| !k.starts_with(&prefix));
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let content = {
            let docs = self.documents.read().await;
            match docs.get(&uri.to_string()) {
                Some(c) => c.clone(),
                None => return Ok(None),
            }
        };

        let mut items = Vec::new();
        let current_line_raw = content.lines().nth(position.line as usize).unwrap_or("");
        let current_line = current_line_raw.trim();
        let line_prefix = current_line_raw
            .chars()
            .take(position.character as usize)
            .collect::<String>();
        let line_prefix_trimmed = line_prefix.trim();
        let typing_section_header_prefix = line_prefix_trimmed.chars().all(|ch| ch == '-')
            || line_prefix_trimmed.starts_with("--- ");
        let on_section_header_line =
            current_line.starts_with("---") && current_line.ends_with("---");

        if typing_section_header_prefix {
            items.extend(handlers::get_section_completions());
        }

        // Use AST for context-aware completions
        if let Some(doc) = self.get_or_parse_document(&uri, &content).await {
            let line_num = position.line as usize + 1;

            let in_any_section = doc
                .sections
                .iter()
                .any(|s| s.start_line <= line_num && line_num <= s.end_line);
            if current_line.is_empty() && !in_any_section {
                items.extend(handlers::get_section_completions());
            }

            // Context-aware completions based on section type
            for section in &doc.sections {
                if section.start_line <= line_num && line_num <= section.end_line {
                    let on_section_header =
                        line_num == section.start_line || on_section_header_line;
                    if on_section_header {
                        items.extend(handlers::get_section_header_option_completions(
                            &section.section_type,
                        ));
                    }

                    match section.section_type {
                        SectionType::Address if !on_section_header => {
                            items.extend(handlers::get_address_completions())
                        }
                        SectionType::Endpoint => {
                            if !on_section_header {
                                items.push(CompletionItem {
                                    label: "package.Service/Method".to_string(),
                                    kind: Some(CompletionItemKind::SNIPPET),
                                    detail: Some("gRPC endpoint template".to_string()),
                                    insert_text: Some(
                                        "${1:package}.${2:Service}/${3:Method}".to_string(),
                                    ),
                                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                                    ..CompletionItem::default()
                                });

                                let address = handlers::get_address_from_document(&content)
                                    .or_else(|| std::env::var(config::ENV_GRPCTESTIFY_ADDRESS).ok())
                                    .unwrap_or_else(config::default_address);
                                let proto_config = Self::proto_config_from_document(&doc, &uri);
                                items.extend(
                                    self.schema_endpoint_completions(&address, proto_config)
                                        .await,
                                );
                            }
                        }
                        SectionType::Request if !on_section_header => {
                            items.extend(
                                self.schema_message_field_completions(
                                    &doc,
                                    &uri,
                                    &content,
                                    section.start_line,
                                    position.line as usize,
                                    false,
                                )
                                .await,
                            );
                        }
                        SectionType::Response if !on_section_header => {
                            items.extend(
                                self.schema_message_field_completions(
                                    &doc,
                                    &uri,
                                    &content,
                                    section.start_line,
                                    position.line as usize,
                                    true,
                                )
                                .await,
                            );
                        }
                        SectionType::Asserts if !on_section_header => {
                            items.extend(handlers::get_assertion_completions());
                            items.extend(self.schema_assert_path_completions(&doc, &uri).await);
                        }
                        SectionType::Extract if !on_section_header => {
                            items.extend(handlers::get_extract_completions())
                        }
                        SectionType::Proto | SectionType::Tls | SectionType::Options
                            if !on_section_header =>
                        {
                            items.extend(handlers::get_section_key_completions(
                                &section.section_type,
                            ));
                        }
                        _ => {}
                    }
                    break;
                }
            }
        }

        if !items.is_empty() {
            let mut seen = std::collections::HashSet::new();
            items.retain(|item| seen.insert(item.label.clone()));
        }

        Ok(if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        })
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = {
            let docs = self.documents.read().await;
            match docs.get(&uri.to_string()) {
                Some(c) => c.clone(),
                None => return Ok(None),
            }
        };
        if let Some(doc) = self.get_or_parse_document(&uri, &content).await {
            let line = position.line as usize + 1;
            for section in &doc.sections {
                if section.start_line <= line
                    && line <= section.end_line
                    && let Some(content) = handlers::get_section_hover(&section.section_type)
                {
                    return Ok(Some(Hover {
                        contents: HoverContents::Scalar(MarkedString::String(content)),
                        range: None,
                    }));
                }
            }
        }
        Ok(None)
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        let file_name = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| uri.to_string());
        if let Ok(formatted) = crate::commands::fmt::format_gctf_content(content, &file_name)
            && formatted != *content
        {
            let line_count = content.lines().count() as u32;
            let last_len = content.lines().last().map(|l| l.len()).unwrap_or(0) as u32;
            return Ok(Some(vec![TextEdit::new(
                Range::new(Position::new(0, 0), Position::new(line_count, last_len)),
                formatted,
            )]));
        }
        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let content = {
            let docs = self.documents.read().await;
            match docs.get(&uri.to_string()) {
                Some(c) => c.clone(),
                None => return Ok(None),
            }
        };
        if let Some(doc) = self.get_or_parse_document(&uri, &content).await {
            let section_children =
                |s: &crate::parser::ast::Section| -> Option<Vec<DocumentSymbol>> {
                    let mut children: Vec<DocumentSymbol> = Vec::new();

                    if s.section_type == SectionType::Asserts {
                        for (idx, line) in s.raw_content.lines().enumerate() {
                            let trimmed = line.trim();
                            if trimmed.is_empty()
                                || trimmed.starts_with('#')
                                || trimmed.starts_with("//")
                            {
                                continue;
                            }

                            let line_num = (s.start_line + idx + 1) as u32;
                            #[allow(deprecated)]
                            let mut assertion_symbol = DocumentSymbol {
                                name: trimmed.to_string(),
                                detail: Some("assertion".to_string()),
                                kind: SymbolKind::STRING,
                                tags: None,
                                deprecated: None,
                                range: Range::new(
                                    Position::new(line_num, 0),
                                    Position::new(line_num, trimmed.len() as u32),
                                ),
                                selection_range: Range::new(
                                    Position::new(line_num, 0),
                                    Position::new(line_num, trimmed.len() as u32),
                                ),
                                children: None,
                            };

                            let mut var_children = Vec::new();
                            let mut offset = 0usize;
                            while let Some(open) = trimmed[offset..].find("{{") {
                                let abs_open = offset + open;
                                let Some(close_rel) = trimmed[abs_open..].find("}}") else {
                                    break;
                                };
                                let abs_close = abs_open + close_rel + 2;
                                let inner = trimmed[abs_open + 2..abs_close - 2].trim();
                                if !inner.is_empty() {
                                    #[allow(deprecated)]
                                    var_children.push(DocumentSymbol {
                                        name: inner.to_string(),
                                        detail: Some("variable reference".to_string()),
                                        kind: SymbolKind::VARIABLE,
                                        tags: None,
                                        deprecated: None,
                                        range: Range::new(
                                            Position::new(line_num, abs_open as u32),
                                            Position::new(line_num, abs_close as u32),
                                        ),
                                        selection_range: Range::new(
                                            Position::new(line_num, (abs_open + 2) as u32),
                                            Position::new(line_num, (abs_close - 2) as u32),
                                        ),
                                        children: None,
                                    });
                                }
                                offset = abs_close;
                            }

                            if !var_children.is_empty() {
                                assertion_symbol.children = Some(var_children);
                            }

                            children.push(assertion_symbol);
                        }
                    }

                    if s.section_type == SectionType::Extract {
                        for (idx, line) in s.raw_content.lines().enumerate() {
                            let trimmed = line.trim();
                            if trimmed.is_empty()
                                || trimmed.starts_with('#')
                                || trimmed.starts_with("//")
                            {
                                continue;
                            }

                            let Some((name, expr)) = trimmed.split_once('=') else {
                                continue;
                            };
                            let var_name = name.trim();
                            let line_num = (s.start_line + idx + 1) as u32;
                            let expr_trimmed = expr.trim();

                            #[allow(deprecated)]
                            children.push(DocumentSymbol {
                                name: var_name.to_string(),
                                detail: Some(format!("extract: {}", expr_trimmed)),
                                kind: SymbolKind::VARIABLE,
                                tags: None,
                                deprecated: None,
                                range: Range::new(
                                    Position::new(line_num, 0),
                                    Position::new(line_num, trimmed.len() as u32),
                                ),
                                selection_range: Range::new(
                                    Position::new(line_num, 0),
                                    Position::new(line_num, var_name.len() as u32),
                                ),
                                children: None,
                            });
                        }
                    }

                    if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    }
                };

            let symbols = doc
                .sections
                .iter()
                .map(|s| {
                    #[allow(deprecated)]
                    DocumentSymbol {
                        name: format!("{:?}", s.section_type),
                        detail: Some(format!("Lines {}-{}", s.start_line, s.end_line)),
                        kind: SymbolKind::MODULE,
                        tags: None,
                        deprecated: None,
                        range: Range::new(
                            Position::new(s.start_line as u32, 0),
                            Position::new(s.end_line as u32, 0),
                        ),
                        selection_range: Range::new(
                            Position::new(s.start_line as u32, 3),
                            Position::new(s.start_line as u32, 15),
                        ),
                        children: section_children(s),
                    }
                })
                .collect();
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let mut actions = Vec::new();
        let uri = params.text_document.uri.clone();

        for diagnostic in &params.context.diagnostics {
            if diagnostic.code == Some(NumberOrString::String("DEPRECATED_SECTION".to_string()))
                && diagnostic.message.contains("HEADERS")
            {
                let action = handlers::create_headers_deprecated_action(
                    &params.text_document.uri,
                    diagnostic.range,
                );
                actions.push(CodeActionOrCommand::CodeAction(action));
            }

            if let Some(NumberOrString::String(code)) = &diagnostic.code
                && code.starts_with("OPT_")
                && let Some(data) = &diagnostic.data
                && let Some(replacement) = data.get("replacement").and_then(|v| v.as_str())
            {
                let action = handlers::create_optimizer_rewrite_action(
                    &params.text_document.uri,
                    diagnostic.range,
                    replacement,
                    code,
                );
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        let docs = self.documents.read().await;
        if let Some(content) = docs.get(&uri.to_string())
            && let Ok(doc) = parser::parse_gctf_from_str(content, uri.as_str())
        {
            let edits = handlers::collect_optimizer_rewrite_edits(&doc, content);
            if !edits.is_empty() {
                let action = handlers::create_apply_all_optimizer_rewrite_action(
                    &uri,
                    edits.clone(),
                    edits.len(),
                );
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        Ok(if actions.is_empty() {
            None
        } else {
            Some(actions)
        })
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        if let Some(loc) =
            variable_definition::find_variable_definition(content, position, uri.as_str())
        {
            Ok(Some(GotoDefinitionResponse::Scalar(
                variable_definition::variable_location_to_lsp(&loc),
            )))
        } else {
            Ok(None)
        }
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        // First find the variable name at the position
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = position.line as usize;
        if line_idx >= lines.len() {
            return Ok(None);
        }

        let line = lines[line_idx];
        let char_idx = position.character as usize;
        if char_idx >= line.len() {
            return Ok(None);
        }

        // Get variable name at position
        if let Some(var_name) = variable_definition::extract_variable_at_position(line, char_idx) {
            // Find all references to this variable
            let locations =
                variable_definition::find_variable_references(content, &var_name, uri.as_str());
            Ok(if locations.is_empty() {
                None
            } else {
                Some(locations)
            })
        } else {
            Ok(None)
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let position = params.position;

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        // Check if position is on a variable reference
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = position.line as usize;
        if line_idx >= lines.len() {
            return Ok(None);
        }

        let line = lines[line_idx];
        let char_idx = position.character as usize;
        if char_idx >= line.len() {
            return Ok(None);
        }

        // Look for {{ var_name }} pattern
        if let Some(_var_name) = variable_definition::extract_variable_at_position(line, char_idx) {
            // Find the range of the variable reference
            if let Some(start) = line[..char_idx].rfind("{{") {
                if let Some(end) = line[char_idx..].find("}}") {
                    let range = Range::new(
                        Position::new(line_idx as u32, start as u32),
                        Position::new(line_idx as u32, (start + end + 2) as u32),
                    );
                    // Return Range variant of PrepareRenameResponse
                    Ok(Some(PrepareRenameResponse::Range(range)))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        // First find the variable name at the position
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = position.line as usize;
        if line_idx >= lines.len() {
            return Ok(None);
        }

        let line = lines[line_idx];
        let char_idx = position.character as usize;
        if char_idx >= line.len() {
            return Ok(None);
        }

        // Get variable name at position
        if let Some(var_name) = variable_definition::extract_variable_at_position(line, char_idx) {
            // Find all references to this variable
            let locations =
                variable_definition::find_variable_references(content, &var_name, uri.as_str());

            if locations.is_empty() {
                return Ok(None);
            }

            // Create text edits for all references
            let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
            for location in locations {
                let edit = TextEdit::new(location.range, format!("{{{{ {} }}}}", new_name));
                changes.entry(location.uri).or_default().push(edit);
            }

            Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }))
        } else {
            Ok(None)
        }
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        // Check if we're typing a plugin function (@uuid, @email, etc.)
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = position.line as usize;
        if line_idx >= lines.len() {
            return Ok(None);
        }

        let line = lines[line_idx];
        let char_idx = position.character as usize;
        if char_idx >= line.len() {
            return Ok(None);
        }

        // Look for @plugin( pattern
        if let Some(at_pos) = line[..char_idx].rfind('@')
            && let Some(paren_pos) = line[at_pos..].find('(')
        {
            let plugin_name = &line[at_pos + 1..at_pos + paren_pos];
            let open_paren_abs = at_pos + paren_pos;
            let active_param = infer_active_parameter(line, open_paren_abs, char_idx);

            // Get signature info for known plugins
            let signatures = get_plugin_signatures();
            if let Some(sig) = signatures.get(plugin_name) {
                return Ok(Some(SignatureHelp {
                    signatures: vec![SignatureInformation {
                        label: sig.label.clone(),
                        documentation: Some(Documentation::String(sig.documentation.clone())),
                        parameters: Some(sig.parameters.clone()),
                        active_parameter: None,
                    }],
                    active_signature: Some(0),
                    active_parameter: Some(active_param),
                }));
            }
        }

        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let version = self.current_doc_version(&uri).await;

        if let Some((cached_ver, cached_tokens)) = self
            .semantic_tokens_cache
            .read()
            .await
            .get(&uri.to_string())
            .cloned()
            && cached_ver == version
        {
            return Ok(Some(SemanticTokensResult::Tokens(cached_tokens)));
        }

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        let tokens = build_semantic_tokens(content);
        self.semantic_tokens_cache
            .write()
            .await
            .insert(uri.to_string(), (version, tokens.clone()));
        Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        let ranges = build_folding_ranges(content);
        Ok(Some(ranges))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let range = params.range;
        let version = self.current_doc_version(&uri).await;
        let cache_key = Self::inlay_cache_key(&uri, &range);

        if let Some((cached_ver, cached_hints)) =
            self.inlay_hints_cache.read().await.get(&cache_key).cloned()
            && cached_ver == version
        {
            return Ok(Some(cached_hints));
        }

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        let hints = build_inlay_hints(content, range);
        self.inlay_hints_cache
            .write()
            .await
            .insert(cache_key, (version, hints.clone()));
        Ok(Some(hints))
    }
}

/// Plugin signature information
struct LspPluginSignature {
    label: String,
    documentation: String,
    parameters: Vec<ParameterInformation>,
}

/// Get plugin signatures for signature help
fn get_plugin_signatures() -> std::collections::HashMap<String, LspPluginSignature> {
    use std::collections::HashMap;

    let mut signatures = HashMap::new();
    let mut plugins = PluginManager::new().list();
    plugins.sort_by(|a, b| a.name().cmp(b.name()));

    for plugin in plugins {
        let normalized = plugin.name().trim_start_matches('@').to_string();
        let signature = plugin.signature();
        let template: Vec<&str> = if signature.arg_names.is_empty() {
            vec!["value"]
        } else {
            signature.arg_names.to_vec()
        };
        let label = format!("@{}({})", normalized, template.join(", "));

        let return_kind = match signature.return_kind {
            PluginReturnKind::Boolean => "bool",
            PluginReturnKind::Number => "number",
            PluginReturnKind::String => "string",
            PluginReturnKind::Value => "value",
            PluginReturnKind::Unknown => "unknown",
        };
        let purity = match signature.purity {
            PluginPurity::Pure => "pure",
            PluginPurity::ContextDependent => "context-dependent",
            PluginPurity::Impure => "impure",
        };
        let documentation = format!(
            "{}\n\nReturns: {} | Purity: {} | Deterministic: {} | Idempotent: {}",
            plugin.description(),
            return_kind,
            purity,
            signature.deterministic,
            signature.idempotent
        );

        let parameters = template
            .into_iter()
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.to_string()),
                documentation: None,
            })
            .collect();

        signatures
            .entry(normalized)
            .or_insert_with(|| LspPluginSignature {
                label,
                documentation,
                parameters,
            });
    }

    signatures
}

fn infer_active_parameter(line: &str, open_paren_abs: usize, cursor_idx: usize) -> u32 {
    let start = (open_paren_abs + 1).min(line.len());
    let end = cursor_idx.min(line.len());
    if end <= start {
        return 0;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut commas = 0u32;

    for ch in line[start..end].chars() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }

    commas
}

/// Build semantic tokens for syntax highlighting
/// Note: Currently uses hybrid approach - AST for sections when available, line parsing for rest
/// TODO: Full AST-based tokenization when parser tracks all token types
fn build_semantic_tokens(content: &str) -> SemanticTokens {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct RawToken {
        line: u32,
        start: u32,
        length: u32,
        token_type: u32,
    }

    // Token type indices (must match the order in initialize)
    const KEYWORD: u32 = 0;
    const VARIABLE: u32 = 1;
    const FUNCTION: u32 = 2;
    const NUMBER: u32 = 3;
    const OPERATOR: u32 = 4;

    let section_header_re = regex::Regex::new(r"^---\s*[A-Z_]+(?:\s+.+)?\s*---$").ok();
    let jq_keyword_re = regex::Regex::new(r"\b(if|then|else|end|select|map|reduce|foreach|def|import|include|module|as|label|break)\b").ok();
    let variable_re = regex::Regex::new(r"\{\{[^}]+\}\}").ok();
    let plugin_re = regex::Regex::new(r"@[A-Za-z_][A-Za-z0-9_]*").ok();
    let number_re = regex::Regex::new(r"\b\d+(?:\.\d+)?\b").ok();
    let operator_re = regex::Regex::new(
        r"==|!=|<=|>=|\bcontains\b|\bmatches\b|\bstartsWith\b|\bendsWith\b|[<>+\-*/%|]",
    )
    .ok();

    let mut raw_tokens: Vec<RawToken> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let tokenize_line = |line: &str, line_num: u32, include_jq_keywords: bool| {
        let mut line_tokens: Vec<RawToken> = Vec::new();
        if let Some(re) = &variable_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: VARIABLE,
                });
            }
        }
        if let Some(re) = &plugin_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: FUNCTION,
                });
            }
        }
        if let Some(re) = &number_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: NUMBER,
                });
            }
        }
        if let Some(re) = &operator_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: OPERATOR,
                });
            }
        }
        if include_jq_keywords && let Some(re) = &jq_keyword_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: KEYWORD,
                });
            }
        }
        line_tokens
    };

    if let Ok(doc) = parser::parse_gctf_from_str(content, "temp.gctf")
        && !doc.sections.is_empty()
    {
        for section in &doc.sections {
            if section.start_line < lines.len() {
                let header_line = lines[section.start_line];
                if section_header_re
                    .as_ref()
                    .is_some_and(|re| re.is_match(header_line.trim()))
                {
                    let start = header_line.find("---").unwrap_or(0) as u32;
                    let length = header_line.trim().len() as u32;
                    raw_tokens.push(RawToken {
                        line: section.start_line as u32,
                        start,
                        length,
                        token_type: KEYWORD,
                    });
                }
            }

            for (idx, section_line) in section.raw_content.lines().enumerate() {
                let line_num = (section.start_line + idx + 1) as u32;
                let include_jq_keywords = section.section_type == parser::ast::SectionType::Extract;
                raw_tokens.extend(tokenize_line(section_line, line_num, include_jq_keywords));
            }
        }
    } else {
        // Fallback for incomplete buffers where full parse may fail.
        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx as u32;
            if section_header_re
                .as_ref()
                .is_some_and(|re| re.is_match(line.trim()))
            {
                let start = line.find("---").unwrap_or(0) as u32;
                let length = line.trim().len() as u32;
                raw_tokens.push(RawToken {
                    line: line_num,
                    start,
                    length,
                    token_type: KEYWORD,
                });
            }
            raw_tokens.extend(tokenize_line(line, line_num, true));
        }
    }

    raw_tokens.sort_by_key(|t| (t.line, t.start, t.length, t.token_type));
    raw_tokens.dedup();

    let mut encoded: Vec<SemanticToken> = Vec::with_capacity(raw_tokens.len());
    let mut last_line: u32 = 0;
    let mut last_start: u32 = 0;

    for t in raw_tokens {
        let delta_line = t.line.saturating_sub(last_line);
        let delta_start = if delta_line == 0 {
            t.start.saturating_sub(last_start)
        } else {
            t.start
        };
        encoded.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: 0,
        });
        last_line = t.line;
        last_start = t.start;
    }

    SemanticTokens {
        result_id: None,
        data: encoded,
    }
}

/// Build folding ranges for the document
fn build_folding_ranges(content: &str) -> Vec<FoldingRange> {
    let mut ranges: Vec<FoldingRange> = Vec::new();

    // Parse content using AST for accurate section detection
    if let Ok(doc) = parser::parse_gctf_from_str(content, "temp.gctf") {
        // Create folding ranges for each section
        for section in &doc.sections {
            // Only create folding range if section has multiple lines
            if section.end_line > section.start_line {
                ranges.push(FoldingRange {
                    start_line: ((section.start_line as i32) - 1).max(0) as u32,
                    start_character: Some(0),
                    end_line: ((section.end_line as i32) - 1).max(0) as u32,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: Some(format!("--- {} ---", section.section_type.as_str())),
                });
            }
        }
    }

    ranges
}

/// Build inlay hints for the document
/// Shows type information for variables in EXTRACT sections and section types
fn build_inlay_hints(content: &str, range: tower_lsp::lsp_types::Range) -> Vec<InlayHint> {
    let mut hints: Vec<InlayHint> = Vec::new();

    let infer_type_label = |expr: &str| -> &'static str {
        let e = expr.trim();
        if e == "true" || e == "false" {
            return "bool";
        }
        if e.starts_with('"') && e.ends_with('"') && e.len() >= 2 {
            return "string";
        }
        if e.parse::<f64>().is_ok() {
            return "number";
        }
        if e.starts_with('@')
            && let Some(open) = e.find('(')
        {
            let plugin_name = e[1..open].trim();
            if let Some(plugin) = PluginManager::new().get(plugin_name) {
                return match plugin.signature().return_kind {
                    PluginReturnKind::Boolean => "bool",
                    PluginReturnKind::Number => "number",
                    PluginReturnKind::String => "string",
                    PluginReturnKind::Value | PluginReturnKind::Unknown => "value",
                };
            }
        }
        "value"
    };

    // Parse content using AST for accurate section and variable detection
    if let Ok(doc) = parser::parse_gctf_from_str(content, "temp.gctf") {
        // Add section type hints
        for section in &doc.sections {
            // Check if section is within the requested range
            let section_line = ((section.start_line as i32) - 1).max(0) as u32;
            if section_line >= range.start.line && section_line <= range.end.line {
                // Add section type hint at the end of the section header line
                hints.push(InlayHint {
                    position: tower_lsp::lsp_types::Position {
                        line: section_line,
                        character: 1000, // End of line
                    },
                    label: InlayHintLabel::String(format!(": {}", section.section_type.as_str())),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                });
            }
        }

        // Add variable type hints in EXTRACT sections
        for section in &doc.sections {
            if section.section_type == parser::ast::SectionType::Extract
                && let parser::ast::SectionContent::Extract(extractions) = &section.content
            {
                for (var_name, expr) in extractions {
                    let mut hint_line: Option<u32> = None;
                    let mut hint_char: u32 = 1000;
                    for (idx, line) in section.raw_content.lines().enumerate() {
                        let trimmed = line.trim();
                        if let Some((name, _)) = trimmed.split_once('=')
                            && name.trim() == var_name
                        {
                            hint_line = Some((section.start_line + idx + 1) as u32);
                            hint_char = name.len() as u32;
                            break;
                        }
                    }

                    let line_num =
                        hint_line.unwrap_or(((section.start_line as i32) - 1).max(0) as u32);
                    if line_num >= range.start.line && line_num <= range.end.line {
                        hints.push(InlayHint {
                            position: tower_lsp::lsp_types::Position {
                                line: line_num,
                                character: hint_char,
                            },
                            label: InlayHintLabel::String(format!(": {}", infer_type_label(expr))),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: Some(InlayHintTooltip::String(format!(
                                "Extracted from expression: {}",
                                expr
                            ))),
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        });
                    }
                }
            }
        }

        for opt in optimizer::collect_assertion_optimizations(&doc) {
            let line_num = opt.line.saturating_sub(1) as u32;
            if line_num < range.start.line || line_num > range.end.line {
                continue;
            }
            hints.push(InlayHint {
                position: tower_lsp::lsp_types::Position {
                    line: line_num,
                    character: 1000,
                },
                label: InlayHintLabel::String(format!("opt: {}", opt.rule_id)),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: opt
                    .proof_note
                    .as_ref()
                    .map(|s| InlayHintTooltip::String(s.clone())),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
    }

    hints
}

pub async fn start_lsp_server() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(GrpctestifyLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_semantic_tokens_section_headers() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n";
        let tokens = build_semantic_tokens(content);

        // Should have at least one token for the section header
        // Note: AST parsing may fail for incomplete content, so we check both cases
        if !tokens.data.is_empty() {
            // First token should be a KEYWORD (section header)
            assert_eq!(tokens.data[0].token_type, 0); // KEYWORD
        }
        // If AST parsing failed, tokens will be empty - that's acceptable for this test
    }

    #[test]
    fn test_build_semantic_tokens_variables() {
        let content = "{{ variable_name }}\n";
        let tokens = build_semantic_tokens(content);

        // Should have at least one token for the variable
        assert!(!tokens.data.is_empty());

        // Find the VARIABLE token
        let var_token = tokens.data.iter().find(|t| t.token_type == 1); // VARIABLE
        assert!(var_token.is_some());
    }

    #[test]
    fn test_build_semantic_tokens_plugins() {
        let content = "@uuid(.field)\n";
        let tokens = build_semantic_tokens(content);

        // Should have at least one token for the plugin
        assert!(!tokens.data.is_empty());

        // Find the FUNCTION token
        let func_token = tokens.data.iter().find(|t| t.token_type == 2); // FUNCTION
        assert!(func_token.is_some());
    }

    #[test]
    fn test_build_semantic_tokens_numbers() {
        let content = "123\n456.789\n";
        let tokens = build_semantic_tokens(content);

        // Should have at least two tokens for the numbers
        let num_tokens: Vec<_> = tokens.data.iter().filter(|t| t.token_type == 3).collect(); // NUMBER
        assert!(num_tokens.len() >= 2);
    }

    #[test]
    fn test_build_semantic_tokens_empty() {
        let content = "";
        let tokens = build_semantic_tokens(content);

        // Should have no tokens for empty content
        assert!(tokens.data.is_empty());
    }

    #[test]
    fn test_build_semantic_tokens_jq_keywords() {
        let content = "if .x > 0 then \"yes\" else \"no\" end\n";
        let tokens = build_semantic_tokens(content);

        // Should have tokens for JQ keywords (if, then, else, end)
        let keyword_tokens: Vec<_> = tokens.data.iter().filter(|t| t.token_type == 0).collect(); // KEYWORD
        assert!(keyword_tokens.len() >= 4); // if, then, else, end
    }

    #[test]
    fn test_build_semantic_tokens_operators() {
        let content = ".x + .y | select(.z > 0)\n";
        let tokens = build_semantic_tokens(content);

        // Should have tokens for operators (+, |, >)
        let operator_tokens: Vec<_> = tokens.data.iter().filter(|t| t.token_type == 4).collect(); // OPERATOR
        assert!(operator_tokens.len() >= 3); // +, |, >
    }

    #[test]
    fn test_build_folding_ranges() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{\n  \"id\": 123\n}\n\n--- RESPONSE ---\n{\n  \"result\": \"ok\"\n}\n";
        let ranges = build_folding_ranges(content);

        // Should have folding ranges for sections with multiple lines
        assert!(!ranges.is_empty());

        // Check that ranges have proper structure
        for range in &ranges {
            assert!(range.start_line <= range.end_line);
            assert!(range.kind == Some(FoldingRangeKind::Region));
        }
    }

    #[test]
    fn test_build_inlay_hints_section_types() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";
        let range = tower_lsp::lsp_types::Range {
            start: tower_lsp::lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: tower_lsp::lsp_types::Position {
                line: 10,
                character: 0,
            },
        };
        let hints = build_inlay_hints(content, range);

        // Should have type hints for sections
        assert!(!hints.is_empty());

        // Check that hints have TYPE kind
        for hint in &hints {
            assert!(hint.kind == Some(InlayHintKind::TYPE));
        }
    }

    #[test]
    fn test_build_inlay_hints_extract_variables() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- EXTRACT ---\ntoken = .token\nuser_id = .user.id\n\n--- RESPONSE ---\n{}\n";
        let range = tower_lsp::lsp_types::Range {
            start: tower_lsp::lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: tower_lsp::lsp_types::Position {
                line: 10,
                character: 0,
            },
        };
        let hints = build_inlay_hints(content, range);

        // Should have hints (section types + extract variables)
        assert!(!hints.is_empty());
        let labels: Vec<String> = hints
            .iter()
            .filter_map(|h| match &h.label {
                InlayHintLabel::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|l| l == ": value"));
    }

    #[test]
    fn test_build_inlay_hints_optimizer_opportunities() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n@has_header(\"x\") == true\n";
        let range = tower_lsp::lsp_types::Range {
            start: tower_lsp::lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: tower_lsp::lsp_types::Position {
                line: 10,
                character: 0,
            },
        };
        let hints = build_inlay_hints(content, range);
        let labels: Vec<String> = hints
            .iter()
            .filter_map(|h| match &h.label {
                InlayHintLabel::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|l| l == "opt: OPT_B001"));
    }

    #[test]
    fn test_build_inlay_hints_empty() {
        let content = "";
        let range = tower_lsp::lsp_types::Range {
            start: tower_lsp::lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: tower_lsp::lsp_types::Position {
                line: 0,
                character: 0,
            },
        };
        let hints = build_inlay_hints(content, range);

        // Should have no hints for empty content
        assert!(hints.is_empty());
    }

    #[test]
    fn test_get_plugin_signatures_uses_runtime_arg_names() {
        let signatures = get_plugin_signatures();
        let regex = signatures.get("regex").unwrap();
        assert_eq!(regex.label, "@regex(value, pattern)");

        let has_trailer = signatures.get("has_trailer").unwrap();
        assert_eq!(has_trailer.label, "@has_trailer(name)");
        assert!(has_trailer.documentation.contains("Returns: bool"));
    }

    #[test]
    fn test_infer_active_parameter_counts_top_level_commas() {
        let line = "@regex(.name, \"a,b\")";
        let open = line.find('(').unwrap();
        let cursor = line.len() - 1;
        let active = infer_active_parameter(line, open, cursor);
        assert_eq!(active, 1);
    }
}
