use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::{Client, LanguageServer, LspService, Server, lsp_types::*};

use crate::config;
use crate::grpc::client::{GrpcClient, GrpcClientConfig, ProtoConfig, WireProtocol};
use crate::lsp::handlers::{self, get_var_hover, get_variable_completions};
use crate::lsp::variable_definition;
use crate::parser::ast::SectionType;
use crate::parser::{self, GctfDocument};
use crate::plugins::{PluginManager, PluginPurity};

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
            connection_id: 0,
            protocol: WireProtocol::Grpc,
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
                            while lookahead
                                .next_if_map(|next| {
                                    if next.is_whitespace() {
                                        Ok(())
                                    } else {
                                        Err(next)
                                    }
                                })
                                .is_some()
                            {}
                            if lookahead.next_if_eq(&':').is_some() {
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

                if ch == '/' && chars.next_if_eq(&'/').is_some() {
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

                // Validate all documents in the chain
                let mut lsp_diags: Vec<Diagnostic> = Vec::new();
                for (doc_idx, d) in document.iter_chain().enumerate() {
                    let doc_label = if document.is_single_document() {
                        None
                    } else {
                        Some(doc_idx + 1)
                    };

                    let errors = crate::parser::validator::validate_document_diagnostics(d);
                    for e in &errors {
                        let mut diag = handlers::validation_error_to_diagnostic(e, content);
                        if let Some(n) = doc_label {
                            diag.message = format!("Document {}: {}", n, diag.message);
                        }
                        lsp_diags.push(diag);
                    }

                    // Semantic diagnostics
                    // Semantic diagnostics
                    let mut semantic_diags = handlers::collect_semantic_diagnostics(d, content);
                    for diag in &mut semantic_diags {
                        if let Some(n) = doc_label {
                            diag.message = format!("Document {}: {}", n, diag.message);
                        }
                    }

                    // Optimizer diagnostics (Safe-level rewrites)
                    let opt_diags = handlers::collect_optimizer_diagnostics(d, content);

                    // Deduplicate: suppress SEM_D001 if R001 (auto-fix) exists on same line
                    let r001_lines: std::collections::HashSet<u32> = opt_diags
                        .iter()
                        .filter(|d| d.code == Some(NumberOrString::String("OPT_R001".into())))
                        .map(|d| d.range.start.line)
                        .collect();
                    semantic_diags.retain(|d| {
                        !(d.code == Some(NumberOrString::String("SEM_D001".into()))
                            && r001_lines.contains(&d.range.start.line))
                    });

                    for diag in semantic_diags {
                        lsp_diags.push(diag);
                    }
                    for diag in opt_diags {
                        lsp_diags.push(diag);
                    }

                    // Sources diagnostics
                    let sources_diags = handlers::collect_sources_diagnostics(d, content);
                    for mut diag in sources_diags {
                        if let Some(n) = doc_label {
                            diag.message = format!("Document {}: {}", n, diag.message);
                        }
                        lsp_diags.push(diag);
                    }
                }

                // Unused variable diagnostics (EXTRACT vars not used in subsequent docs)
                for unused_var in handlers::collect_unused_variables(&document) {
                    lsp_diags.push(handlers::unused_variable_to_diagnostic(&unused_var));
                }

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
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: Some(false),
                        },
                        legend: SemanticTokensLegend {
                            token_types: vec![
                                SemanticTokenType::KEYWORD,
                                SemanticTokenType::VARIABLE,
                                SemanticTokenType::FUNCTION,
                                SemanticTokenType::NUMBER,
                                SemanticTokenType::OPERATOR,
                                SemanticTokenType::STRING,
                                SemanticTokenType::REGEXP,
                            ],
                            token_modifiers: vec![],
                        },
                        range: None,
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    }
                    .into(),
                ),
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
                        SectionType::Endpoint if !on_section_header => {
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
                        SectionType::Request if !on_section_header => {
                            // Variable completions for {{var}} in JSON
                            items.extend(get_variable_completions(&doc, position.line as usize));
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
                        SectionType::RequestHeaders if !on_section_header => {
                            // Variable completions for {{var}} in header values
                            items.extend(get_variable_completions(&doc, position.line as usize));
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

            // First check if cursor is on a {{var}} reference
            if let Some(var_hover) = get_var_hover(&doc, position.line as usize, position.character)
            {
                return Ok(Some(var_hover));
            }

            // Check for plugin hover (cursor on @plugin or @type.method)
            if let Some(plugin_hover) =
                handlers::get_plugin_hover(&doc, position.line as usize, position.character)
            {
                return Ok(Some(plugin_hover));
            }

            // Fall back to section hover
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

    #[expect(deprecated)]
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
                    crate::lsp::build_section_children_for_doc(&doc)
                        .into_iter()
                        .find(|child| child.name == format!("{:?}", s.section_type))
                        .and_then(|sym| sym.children)
                };

            // Multi-document: documents as top-level nodes
            if !doc.is_single_document() {
                let mut doc_symbols: Vec<DocumentSymbol> = Vec::new();
                for (doc_idx, d) in doc.iter_chain().enumerate() {
                    let endpoint = d.get_endpoint().unwrap_or_else(|| "unknown".to_string());
                    let doc_name = format!("Document {}: {}", doc_idx + 1, endpoint);

                    let section_children = crate::lsp::build_section_children_for_doc(d);

                    let first_line = d.sections.first().map(|s| s.start_line).unwrap_or(0) as u32;
                    let last_line = d.sections.last().map(|s| s.end_line).unwrap_or(0) as u32;

                    doc_symbols.push(DocumentSymbol {
                        name: doc_name,
                        detail: Some(format!("Lines {}-{}", first_line, last_line)),
                        kind: SymbolKind::MODULE,
                        tags: None,
                        deprecated: None,
                        range: Range::new(
                            Position::new(first_line, 0),
                            Position::new(last_line, 0),
                        ),
                        selection_range: Range::new(
                            Position::new(first_line, 0),
                            Position::new(first_line, 15),
                        ),
                        children: Some(section_children),
                    });
                }
                return Ok(Some(DocumentSymbolResponse::Nested(doc_symbols)));
            }

            // Single document: flat sections
            let symbols = doc
                .sections
                .iter()
                .map(|s| {
                    #[expect(deprecated)]
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

            if diagnostic.code == Some(NumberOrString::String("BENCH_UNKNOWN_KEY".to_string()))
                && let Some(data) = &diagnostic.data
                && let (Some(unknown_key), Some(suggested_key)) = (
                    data.get("unknown_key").and_then(|v| v.as_str()),
                    data.get("suggested_key").and_then(|v| v.as_str()),
                )
                && let Some(content) = self.documents.read().await.get(&uri.to_string())
                && let Some(action) = handlers::create_bench_key_fix_action(
                    &params.text_document.uri,
                    diagnostic.range,
                    unknown_key,
                    suggested_key,
                    content,
                )
            {
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
            Ok(variable_definition::variable_location_to_lsp(&loc)
                .map(GotoDefinitionResponse::Scalar))
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

        let tokens = crate::lsp::build_semantic_tokens(content);
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

        let ranges = crate::lsp::build_folding_ranges(content);
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

        let hints = crate::lsp::build_inlay_hints(content, range);
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

/// Get plugin signatures for signature help.
/// Builds from the canonical PLUGIN_SIGNATURES map and live plugin descriptions
/// so that custom/override plugins are reflected immediately.
fn get_plugin_signatures() -> std::collections::HashMap<String, LspPluginSignature> {
    use crate::plugins::PLUGIN_SIGNATURES;
    use std::collections::HashMap;

    let mut signatures = HashMap::new();
    let manager = PluginManager::new();

    for (name, signature) in PLUGIN_SIGNATURES.iter() {
        let normalized = name.trim_start_matches('@').to_string();
        let template: Vec<&str> = signature.arg_names.to_vec();
        let label = if template.is_empty() {
            format!("@{}()", normalized)
        } else {
            format!("@{}({})", normalized, template.join(", "))
        };

        let return_type_name = signature.return_type.display_name();
        let purity = match signature.purity {
            PluginPurity::Pure => "pure",
            PluginPurity::ContextDependent => "context-dependent",
            PluginPurity::Impure => "impure",
        };

        // Get live description from the plugin instance
        let description = manager
            .get(name)
            .map(|p| p.description().to_string())
            .unwrap_or_else(|| normalized.clone());

        let documentation = format!(
            "{}\n\nReturns: {} | Purity: {} | Deterministic: {} | Idempotent: {}",
            description, return_type_name, purity, signature.deterministic, signature.idempotent
        );

        let parameters = template
            .into_iter()
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.to_string()),
                documentation: None,
            })
            .collect();

        signatures.insert(
            normalized,
            LspPluginSignature {
                label,
                documentation,
                parameters,
            },
        );
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
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }

    commas
}

pub async fn start_lsp_server() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(GrpctestifyLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
