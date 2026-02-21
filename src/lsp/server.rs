use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::{lsp_types::*, Client, LanguageServer, LspService, Server};

use crate::parser::{self, GctfDocument};
use crate::parser::ast::SectionType;
use crate::parser::validator::{self, ValidationError, ErrorSeverity};

type ServiceCache = Arc<RwLock<HashMap<String, Vec<String>>>>;

#[allow(dead_code)]
pub struct GrpctestifyLsp {
    client: Client,
    default_address: Mutex<Option<String>>,
    service_cache: ServiceCache,
}

#[allow(dead_code)]
impl GrpctestifyLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            default_address: Mutex::new(None),
            service_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_service_cache(client: Client, cache: ServiceCache) -> Self {
        Self {
            client,
            default_address: Mutex::new(None),
            service_cache: cache,
        }
    }

    fn get_known_servers() -> Vec<(&'static str, &'static str)> {
        vec![
            ("localhost:4770", "Default gripmock port"),
            ("localhost:50051", "Common gRPC port"),
            ("localhost:9000", "Alternative gRPC port"),
        ]
    }

    async fn get_address_from_document(content: &str) -> Option<String> {
        let doc = parser::parse_gctf_from_str(content, "temp.gctf").ok()?;
        for section in &doc.sections {
            if section.section_type == SectionType::Address {
                if let parser::ast::SectionContent::Single(addr) = &section.content {
                    return Some(addr.trim().to_string());
                }
            }
        }
        std::env::var("GRPCTESTIFY_ADDRESS").ok()
    }

    fn detect_section_at_position(content: &str, position: Position) -> Option<(SectionType, usize)> {
        let doc = parser::parse_gctf_from_str(content, "temp.gctf").ok()?;
        let line = position.line as usize + 1;
        
        for section in &doc.sections {
            if section.start_line <= line && line <= section.end_line {
                return Some((section.section_type.clone(), section.start_line));
            }
        }
        None
    }

    fn get_address_completions() -> Vec<CompletionItem> {
        Self::get_known_servers()
            .into_iter()
            .map(|(addr, desc)| CompletionItem {
                label: addr.to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some(desc.to_string()),
                insert_text: Some(addr.to_string()),
                ..CompletionItem::default()
            })
            .collect()
    }

    async fn get_endpoint_completions(&self, content: &str) -> Vec<CompletionItem> {
        if let Some(address) = Self::get_address_from_document(content).await {
            let cache = self.service_cache.read().await;
            if let Some(services) = cache.get(&address) {
                return services.iter().map(|s| CompletionItem {
                    label: s.clone(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some("gRPC endpoint".to_string()),
                    insert_text: Some(s.clone()),
                    ..CompletionItem::default()
                }).collect();
            }
        }
        vec![CompletionItem {
            label: "Set ADDRESS first".to_string(),
            kind: Some(CompletionItemKind::TEXT),
            detail: Some("Define ADDRESS section to enable endpoint discovery".to_string()),
            ..CompletionItem::default()
        }]
    }
}

impl GrpctestifyLsp {
    fn file_path_from_uri(uri: &Url) -> Option<PathBuf> {
        uri.to_file_path().ok()
    }

    fn run_check(content: &str, file_path: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        match parser::parse_gctf_from_str(content, file_path) {
            Ok(doc) => {
                let validation_errors = validator::validate_document_diagnostics(&doc);
                for error in validation_errors {
                    diagnostics.push(Self::validation_error_to_diagnostic(&error));
                }

                Self::check_deprecated_sections(&doc, &mut diagnostics);
            }
            Err(e) => {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position { line: 0, character: 0 },
                        end: Position { line: 0, character: u32::MAX },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("PARSE_ERROR".to_string())),
                    source: Some("grpctestify".to_string()),
                    message: format!("Failed to parse file: {}", e),
                    related_information: None,
                    tags: None,
                    code_description: None,
                    data: None,
                });
            }
        }

        diagnostics
    }

    fn validation_error_to_diagnostic(error: &ValidationError) -> Diagnostic {
        let line = error.line.unwrap_or(1).saturating_sub(1) as u32;
        Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position { line, character: u32::MAX },
            },
            severity: Some(match error.severity {
                ErrorSeverity::Error => DiagnosticSeverity::ERROR,
                ErrorSeverity::Warning => DiagnosticSeverity::WARNING,
                ErrorSeverity::Info => DiagnosticSeverity::INFORMATION,
            }),
            code: Some(NumberOrString::String("VALIDATION_ERROR".to_string())),
            source: Some("grpctestify".to_string()),
            message: error.message.clone(),
            related_information: None,
            tags: None,
            code_description: None,
            data: None,
        }
    }

    fn check_deprecated_sections(doc: &GctfDocument, diagnostics: &mut Vec<Diagnostic>) {
        if let Some(source) = &doc.metadata.source {
            for (line_num, line) in source.lines().enumerate() {
                if line.trim().to_uppercase() == "--- HEADERS ---" {
                    diagnostics.push(Diagnostic {
                        range: Range {
                            start: Position { line: line_num as u32, character: 0 },
                            end: Position { line: line_num as u32, character: line.len() as u32 },
                        },
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(NumberOrString::String("DEPRECATED_SECTION".to_string())),
                        source: Some("grpctestify".to_string()),
                        message: "HEADERS section is deprecated, use REQUEST_HEADERS instead".to_string(),
                        related_information: None,
                        tags: None,
                        code_description: None,
                        data: Some(serde_json::json!({
                            "hint": "Replace --- HEADERS --- with --- REQUEST_HEADERS ---"
                        })),
                    });
                }
            }
        }
    }

    fn get_section_completions() -> Vec<CompletionItem> {
        let sections = [
            ("ADDRESS", "Server address (host:port)"),
            ("ENDPOINT", "gRPC endpoint (package.Service/Method)"),
            ("REQUEST", "Request payload (JSON)"),
            ("RESPONSE", "Expected response (JSON)"),
            ("ERROR", "Expected error response (JSON with code and message)"),
            ("REQUEST_HEADERS", "Request headers (key: value)"),
            ("TLS", "TLS configuration"),
            ("OPTIONS", "Test options"),
            ("PROTO", "Proto file configuration"),
            ("EXTRACT", "Variable extraction from response"),
            ("ASSERTS", "Assertion expressions"),
        ];

        sections
            .iter()
            .map(|(name, detail)| CompletionItem {
                label: format!("--- {} ---", name),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(detail.to_string()),
                insert_text: Some(format!("--- {} ---\n", name)),
                ..CompletionItem::default()
            })
            .collect()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for GrpctestifyLsp {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        if let Some(workspace_folders) = &params.workspace_folders {
            if let Some(folder) = workspace_folders.first() {
                let _ = self
                    .default_address
                    .lock()
                    .await
                    .insert(folder.uri.to_string());
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        "-".to_string(),
                        "\"".to_string(),
                        "{".to_string(),
                        ".".to_string(),
                        "$".to_string(),
                        "/".to_string(),
                    ]),
                    all_commit_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: Some(false),
                    },
                    completion_item: None,
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "grpctestify.runTest".to_string(),
                        "grpctestify.runAllTests".to_string(),
                    ],
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: Some(false),
                    },
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "grpctestify".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Grpctestify LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let file_path = Self::file_path_from_uri(&params.text_document.uri)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let diagnostics = Self::run_check(&params.text_document.text, &file_path);
        self.client
            .publish_diagnostics(params.text_document.uri, diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.last() {
            let file_path = Self::file_path_from_uri(&params.text_document.uri)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let diagnostics = Self::run_check(&change.text, &file_path);
            self.client
                .publish_diagnostics(params.text_document.uri, diagnostics, None)
                .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            let file_path = Self::file_path_from_uri(&params.text_document.uri)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let diagnostics = Self::run_check(&text, &file_path);
            self.client
                .publish_diagnostics(params.text_document.uri, diagnostics, None)
                .await;
        }
    }

    async fn completion(&self, _params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let items = Self::get_section_completions();
        
        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn hover(&self, _params: HoverParams) -> LspResult<Option<Hover>> {
        Ok(None)
    }

    async fn formatting(&self, _params: DocumentFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let mut actions = Vec::new();

        for diagnostic in &params.context.diagnostics {
            if let Some(NumberOrString::String(code)) = &diagnostic.code {
                if code == "DEPRECATED_SECTION" {
                    let uri = params.text_document.uri.clone();
                    let range = diagnostic.range;
                    
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Replace HEADERS with REQUEST_HEADERS".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diagnostic.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(
                                [(
                                    uri,
                                    vec![TextEdit {
                                        range,
                                        new_text: "REQUEST_HEADERS".to_string(),
                                    }],
                                )]
                                .into_iter()
                                .collect(),
                            ),
                            document_changes: None,
                            change_annotations: None,
                        }),
                        command: None,
                        is_preferred: Some(true),
                        disabled: None,
                        data: None,
                    }));
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<serde_json::Value>> {
        match params.command.as_str() {
            "grpctestify.runTest" => {
                self.client
                    .log_message(MessageType::INFO, "Running test...")
                    .await;
            }
            "grpctestify.runAllTests" => {
                self.client
                    .log_message(MessageType::INFO, "Running all tests...")
                    .await;
            }
            _ => {}
        }
        Ok(None)
    }
}

pub async fn start_lsp_server() -> Result<()> {
    tracing::info!("Starting Grpctestify LSP server...");
    
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    let (service, socket) = LspService::new(|client| GrpctestifyLsp::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
    
    Ok(())
}
