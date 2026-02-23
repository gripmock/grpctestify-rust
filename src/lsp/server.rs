use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::{lsp_types::*, Client, LanguageServer, LspService, Server};

use crate::parser::{self, GctfDocument};
use crate::parser::ast::SectionType;
use crate::lsp::handlers;

type ServiceCache = Arc<RwLock<HashMap<String, Vec<String>>>>;

#[allow(dead_code)]
pub struct GrpctestifyLsp {
    client: Client,
    default_address: RwLock<Option<String>>,
    service_cache: ServiceCache,
    documents: Arc<RwLock<HashMap<String, String>>>,
    parsed_docs: Arc<RwLock<HashMap<String, GctfDocument>>>,
}

#[allow(dead_code)]
impl GrpctestifyLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            default_address: RwLock::new(None),
            service_cache: Arc::new(RwLock::new(HashMap::new())),
            documents: Arc::new(RwLock::new(HashMap::new())),
            parsed_docs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn publish_diagnostics(&self, uri: &Url, content: &str) {
        let path = PathBuf::from(uri.to_file_path().unwrap_or_default());
        
        match parser::parse_gctf(&path) {
            Ok(document) => {
                self.parsed_docs.write().await.insert(uri.to_string(), document.clone());
                
                let errors = crate::parser::validator::validate_document_diagnostics(&document);
                let lsp_diags: Vec<Diagnostic> = errors.iter()
                    .map(|e| handlers::validation_error_to_diagnostic(e, content))
                    .collect();
                
                self.client.publish_diagnostics(uri.clone(), lsp_diags, None).await;
            }
            Err(e) => {
                let diag = Diagnostic::new_simple(
                    Range::new(Position::new(0, 0), Position::new(0, 0)),
                    format!("Parse error: {}", e),
                );
                self.client.publish_diagnostics(uri.clone(), vec![diag], None).await;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for GrpctestifyLsp {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec!["-".to_string(), ".".to_string(), "/".to_string(), "@".to_string(), ":".to_string(), "#".to_string()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client.log_message(MessageType::INFO, "grpctestify LSP initialized").await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        self.documents.write().await.insert(uri.to_string(), content.clone());
        self.publish_diagnostics(&uri, &content).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.content_changes[0].text.clone();
        self.documents.write().await.insert(uri.to_string(), content.clone());
        self.publish_diagnostics(&uri, &content).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Ok(content) = tokio::fs::read_to_string(uri.to_file_path().unwrap_or_default()).await {
            self.documents.write().await.insert(uri.to_string(), content.clone());
            self.publish_diagnostics(&uri, &content).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(&uri.to_string());
        self.parsed_docs.write().await.remove(&uri.to_string());
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        
        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };
        
        let mut items = Vec::new();
        
        // Use AST for context-aware completions
        if let Ok(doc) = parser::parse_gctf_from_str(content, "temp.gctf") {
            let line_num = position.line as usize + 1;
            
            // Check if we're on an empty line or section header line
            let on_empty_or_header = doc.sections.iter().any(|s| {
                s.start_line == line_num || 
                (s.start_line > 0 && line_num == s.start_line - 1) ||
                (s.end_line < doc.sections.len() && line_num == s.end_line + 1)
            }) || content.lines().nth(position.line as usize).map(|l| l.trim().is_empty()).unwrap_or(true);
            
            if on_empty_or_header {
                items.extend(handlers::get_section_completions());
            }
            
            // Context-aware completions based on section type
            for section in &doc.sections {
                if section.start_line <= line_num && line_num <= section.end_line {
                    match section.section_type {
                        SectionType::Address => items.extend(handlers::get_address_completions()),
                        SectionType::Endpoint => items.extend(handlers::get_address_from_document(content).await.map(|_| vec![]).unwrap_or_else(|| vec![CompletionItem {
                            label: "package.Service/Method".to_string(),
                            kind: Some(CompletionItemKind::SNIPPET),
                            detail: Some("gRPC endpoint template".to_string()),
                            insert_text: Some("${1:package}.${2:Service}/${3:Method}".to_string()),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            ..CompletionItem::default()
                        }])),
                        SectionType::Asserts => items.extend(handlers::get_assertion_completions()),
                        _ => {}
                    }
                    break;
                }
            }
        }
        
        Ok(if items.is_empty() { None } else { Some(CompletionResponse::Array(items)) })
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        
        let docs = self.parsed_docs.read().await;
        if let Some(doc) = docs.get(&uri.to_string()) {
            let line = position.line as usize + 1;
            for section in &doc.sections {
                if section.start_line <= line && line <= section.end_line {
                    if let Some(content) = handlers::get_section_hover(&section.section_type) {
                        return Ok(Some(Hover {
                            contents: HoverContents::Scalar(MarkedString::String(content)),
                            range: None,
                        }));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };
        
        let path = PathBuf::from(uri.to_file_path().unwrap_or_default());
        if let Ok(document) = parser::parse_gctf(&path) {
            let formatted = crate::serialize_gctf(&document);
            if formatted != *content {
                let line_count = content.lines().count() as u32;
                let last_len = content.lines().last().map(|l| l.len()).unwrap_or(0) as u32;
                return Ok(Some(vec![TextEdit::new(
                    Range::new(Position::new(0, 0), Position::new(line_count, last_len)),
                    formatted,
                )]));
            }
        }
        Ok(None)
    }

    async fn document_symbol(&self, params: DocumentSymbolParams) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let docs = self.parsed_docs.read().await;
        if let Some(doc) = docs.get(&uri.to_string()) {
            let symbols = doc.sections.iter().map(|s| {
                DocumentSymbol {
                    name: format!("{:?}", s.section_type),
                    detail: Some(format!("Lines {}-{}", s.start_line, s.end_line)),
                    kind: SymbolKind::MODULE,
                    tags: None,
                    deprecated: None,
                    range: Range::new(Position::new(s.start_line as u32, 0), Position::new(s.end_line as u32, 0)),
                    selection_range: Range::new(Position::new(s.start_line as u32, 3), Position::new(s.start_line as u32, 15)),
                    children: None,
                }
            }).collect();
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let mut actions = Vec::new();
        
        for diagnostic in &params.context.diagnostics {
            if diagnostic.code == Some(NumberOrString::String("DEPRECATED_SECTION".to_string())) 
                && diagnostic.message.contains("HEADERS") {
                
                let action = handlers::create_headers_deprecated_action(&params.text_document.uri, diagnostic.range);
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
        
        Ok(if actions.is_empty() { None } else { Some(actions) })
    }
}

pub async fn start_lsp_server() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    let (service, socket) = LspService::new(GrpctestifyLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;
    
    Ok(())
}
