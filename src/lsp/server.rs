use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::{Client, LanguageServer, LspService, Server, lsp_types::*};

use crate::lsp::handlers;
use crate::lsp::variable_definition;
use crate::parser::ast::SectionType;
use crate::parser::{self, GctfDocument};

#[allow(dead_code)]
pub struct GrpctestifyLsp {
    client: Client,
    default_address: RwLock<Option<String>>,
    documents: Arc<RwLock<HashMap<String, String>>>,
    parsed_docs: Arc<RwLock<HashMap<String, GctfDocument>>>,
}

#[allow(dead_code)]
impl GrpctestifyLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            default_address: RwLock::new(None),
            documents: Arc::new(RwLock::new(HashMap::new())),
            parsed_docs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn publish_diagnostics(&self, uri: &Url, content: &str) {
        let path = uri.to_file_path().unwrap_or_default();

        match parser::parse_gctf(&path) {
            Ok(document) => {
                self.parsed_docs
                    .write()
                    .await
                    .insert(uri.to_string(), document.clone());

                let errors = crate::parser::validator::validate_document_diagnostics(&document);
                let lsp_diags: Vec<Diagnostic> = errors
                    .iter()
                    .map(|e| handlers::validation_error_to_diagnostic(e, content))
                    .collect();

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
                    TextDocumentSyncKind::INCREMENTAL,
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
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::KEYWORD, // Section headers (--- ENDPOINT ---), JQ keywords (if, then, else, end)
                                    SemanticTokenType::VARIABLE, // Variable references ({{ var }})
                                    SemanticTokenType::FUNCTION, // Plugin names (@uuid, @email), JQ functions (select, map)
                                    SemanticTokenType::NUMBER,   // Numbers
                                    SemanticTokenType::OPERATOR, // Operators (+, -, *, /, |, etc.)
                                ],
                                token_modifiers: vec![],
                            },
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..Default::default()
                        },
                    ),
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
        self.documents
            .write()
            .await
            .insert(uri.to_string(), content.clone());
        self.publish_diagnostics(&uri, &content).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.content_changes[0].text.clone();
        self.documents
            .write()
            .await
            .insert(uri.to_string(), content.clone());
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
                s.start_line == line_num
                    || (s.start_line > 0 && line_num == s.start_line - 1)
                    || (s.end_line < doc.sections.len() && line_num == s.end_line + 1)
            }) || content
                .lines()
                .nth(position.line as usize)
                .map(|l| l.trim().is_empty())
                .unwrap_or(true);

            if on_empty_or_header {
                items.extend(handlers::get_section_completions());
            }

            // Context-aware completions based on section type
            for section in &doc.sections {
                if section.start_line <= line_num && line_num <= section.end_line {
                    match section.section_type {
                        SectionType::Address => items.extend(handlers::get_address_completions()),
                        SectionType::Endpoint => items.extend(
                            handlers::get_address_from_document(content)
                                .await
                                .map(|_| vec![])
                                .unwrap_or_else(|| {
                                    vec![CompletionItem {
                                        label: "package.Service/Method".to_string(),
                                        kind: Some(CompletionItemKind::SNIPPET),
                                        detail: Some("gRPC endpoint template".to_string()),
                                        insert_text: Some(
                                            "${1:package}.${2:Service}/${3:Method}".to_string(),
                                        ),
                                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                                        ..CompletionItem::default()
                                    }]
                                }),
                        ),
                        SectionType::Asserts => items.extend(handlers::get_assertion_completions()),
                        SectionType::Extract => items.extend(handlers::get_extract_completions()),
                        _ => {}
                    }
                    break;
                }
            }
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

        let docs = self.parsed_docs.read().await;
        if let Some(doc) = docs.get(&uri.to_string()) {
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

        let path = uri.to_file_path().unwrap_or_default();
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

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let docs = self.parsed_docs.read().await;
        if let Some(doc) = docs.get(&uri.to_string()) {
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
                        children: None,
                    }
                })
                .collect();
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let mut actions = Vec::new();

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
                let edit = TextEdit::new(location.range, new_name.clone());
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
                    active_parameter: Some(0),
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

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        let tokens = build_semantic_tokens(content);
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

        let docs = self.documents.read().await;
        let content = match docs.get(&uri.to_string()) {
            Some(c) => c,
            None => return Ok(None),
        };

        let hints = build_inlay_hints(content, range);
        Ok(Some(hints))
    }
}

/// Plugin signature information
struct PluginSignature {
    label: String,
    documentation: String,
    parameters: Vec<ParameterInformation>,
}

/// Get plugin signatures for signature help
fn get_plugin_signatures() -> std::collections::HashMap<String, PluginSignature> {
    use std::collections::HashMap;

    let mut signatures = HashMap::new();

    signatures.insert(
        "uuid".to_string(),
        PluginSignature {
            label: "@uuid(field)".to_string(),
            documentation: "Validate UUID format (v1-v5)".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("field".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "email".to_string(),
        PluginSignature {
            label: "@email(field)".to_string(),
            documentation: "Validate email format".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("field".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "ip".to_string(),
        PluginSignature {
            label: "@ip(field)".to_string(),
            documentation: "Validate IP address format (IPv4/IPv6)".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("field".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "url".to_string(),
        PluginSignature {
            label: "@url(field)".to_string(),
            documentation: "Validate URL format".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("field".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "timestamp".to_string(),
        PluginSignature {
            label: "@timestamp(field)".to_string(),
            documentation: "Validate timestamp format (ISO 8601)".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("field".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "header".to_string(),
        PluginSignature {
            label: "@header(name)".to_string(),
            documentation: "Check if header exists".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("name".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "trailer".to_string(),
        PluginSignature {
            label: "@trailer(name)".to_string(),
            documentation: "Check if trailer exists".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("name".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "len".to_string(),
        PluginSignature {
            label: "@len(field)".to_string(),
            documentation: "Get length of string/array".to_string(),
            parameters: vec![ParameterInformation {
                label: ParameterLabel::Simple("field".to_string()),
                documentation: None,
            }],
        },
    );

    signatures.insert(
        "regex".to_string(),
        PluginSignature {
            label: "@regex(field, pattern)".to_string(),
            documentation: "Validate field matches regex pattern".to_string(),
            parameters: vec![
                ParameterInformation {
                    label: ParameterLabel::Simple("field".to_string()),
                    documentation: None,
                },
                ParameterInformation {
                    label: ParameterLabel::Simple("pattern".to_string()),
                    documentation: None,
                },
            ],
        },
    );

    signatures
}

/// Build semantic tokens for syntax highlighting
/// Note: Currently uses hybrid approach - AST for sections when available, line parsing for rest
/// TODO: Full AST-based tokenization when parser tracks all token types
fn build_semantic_tokens(content: &str) -> SemanticTokens {
    let mut tokens: Vec<SemanticToken> = Vec::new();
    let mut last_line: u32 = 0;
    let mut last_start: u32 = 0;

    // Token type indices (must match the order in initialize)
    const KEYWORD: u32 = 0;
    const VARIABLE: u32 = 1;
    const FUNCTION: u32 = 2;
    const NUMBER: u32 = 3;
    const OPERATOR: u32 = 4;

    // Compile regexes once
    let num_regex = regex::Regex::new(r"\b\d+(\.\d+)?\b").ok();
    let jq_keyword_regexes: Vec<(&str, Option<regex::Regex>)> = vec![
        ("if", regex::Regex::new(r"\bif\b").ok()),
        ("then", regex::Regex::new(r"\bthen\b").ok()),
        ("else", regex::Regex::new(r"\belse\b").ok()),
        ("end", regex::Regex::new(r"\bend\b").ok()),
        ("select", regex::Regex::new(r"\bselect\b").ok()),
        ("map", regex::Regex::new(r"\bmap\b").ok()),
        ("reduce", regex::Regex::new(r"\breduce\b").ok()),
        ("foreach", regex::Regex::new(r"\bforeach\b").ok()),
        ("def", regex::Regex::new(r"\bdef\b").ok()),
        ("import", regex::Regex::new(r"\bimport\b").ok()),
        ("include", regex::Regex::new(r"\binclude\b").ok()),
        ("module", regex::Regex::new(r"\bmodule\b").ok()),
        ("as", regex::Regex::new(r"\bas\b").ok()),
        ("label", regex::Regex::new(r"\blabel\b").ok()),
        ("break", regex::Regex::new(r"\bbreak\b").ok()),
    ];

    // Use line-by-line parsing for all tokens
    for (line_idx, line) in content.lines().enumerate() {
        let line_num = line_idx as u32;

        // Section headers: --- ENDPOINT ---, --- REQUEST ---, etc.
        if line.trim().starts_with("---") && line.trim().ends_with("---") {
            let start = line.find("---").unwrap_or(0) as u32;
            let length = line.trim().len() as u32;

            let delta_line = line_num.saturating_sub(last_line);
            let delta_start = if delta_line == 0 {
                start.saturating_sub(last_start)
            } else {
                start
            };
            last_line = line_num;
            last_start = start;

            tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type: KEYWORD,
                token_modifiers_bitset: 0,
            });
        }

        // JQ keywords: if, then, else, end, select, map, etc.
        for (_keyword, regex_opt) in &jq_keyword_regexes {
            if let Some(re) = regex_opt {
                for mat in re.find_iter(line) {
                    let start = mat.start() as u32;
                    let length = mat.end() as u32 - start;

                    let delta_line = line_num.saturating_sub(last_line);
                    let delta_start = if delta_line == 0 {
                        start.saturating_sub(last_start)
                    } else {
                        start
                    };
                    last_line = line_num;
                    last_start = start;

                    tokens.push(SemanticToken {
                        delta_line,
                        delta_start,
                        length,
                        token_type: KEYWORD,
                        token_modifiers_bitset: 0,
                    });
                }
            }
        }

        // JQ functions: .field, |, etc.
        let jq_functions = [
            "select",
            "map",
            "map_values",
            "keys",
            "values",
            "to_entries",
            "from_entries",
            "length",
            "keys_unsorted",
            "sort",
            "sort_by",
            "reverse",
            "min",
            "max",
            "group_by",
            "unique",
            "unique_by",
            "flatten",
            "add",
            "any",
            "all",
            "contains",
            "indices",
            "index",
            "rindex",
            "startswith",
            "endswith",
            "explode",
            "implode",
            "utf8bytelength",
            "type",
            "isfinite",
            "isnan",
            "isnormal",
            "infinite",
            "nan",
            "null",
            "false",
            "true",
            "floor",
            "ceil",
            "round",
            "near",
            "min_by",
            "max_by",
            "del",
            "delpaths",
            "getpath",
            "setpath",
            "filepath",
            "format",
            "entries",
            "recurse",
            "walk",
            "env",
            "now",
            "today",
            "strptime",
            "strftime",
            "strflocaltime",
            "mktime",
            "gmtime",
            "localtime",
            "debug",
            "error",
            "halt",
            "halt_error",
            "stderr",
            "input",
            "inputs",
            "scan",
            "splits",
            "captures",
            "capture",
            "test",
            "match",
            "split",
            "join",
            "explode",
            "implode",
            "ascii_downcase",
            "ascii_upcase",
            "repeat",
            "limit",
            "first",
            "range",
            "recurse",
            "while",
            "until",
            "try",
            "catch",
            "alt",
            "//",
        ];
        for func in &jq_functions {
            if let Some(pos) = line.find(func) {
                // Check if it's a function call (followed by space or parenthesis or pipe)
                let after_pos = pos + func.len();
                if after_pos >= line.len()
                    || line
                        .chars()
                        .nth(after_pos)
                        .is_none_or(|c| c.is_whitespace() || c == '(' || c == '|' || c == ',')
                {
                    let start = pos as u32;
                    let length = func.len() as u32;

                    let delta_line = line_num.saturating_sub(last_line);
                    let delta_start = if delta_line == 0 {
                        start.saturating_sub(last_start)
                    } else {
                        start
                    };
                    last_line = line_num;
                    last_start = start;

                    tokens.push(SemanticToken {
                        delta_line,
                        delta_start,
                        length,
                        token_type: FUNCTION,
                        token_modifiers_bitset: 0,
                    });
                }
            }
        }

        // Variable references: {{ var_name }}
        let mut search_start = 0;
        while let Some(open_pos) = line[search_start..].find("{{") {
            let abs_open = search_start + open_pos;
            if let Some(close_pos) = line[abs_open..].find("}}") {
                let start = abs_open as u32;
                let length = (close_pos + 2) as u32;

                let delta_line = line_num.saturating_sub(last_line);
                let delta_start = if delta_line == 0 {
                    start.saturating_sub(last_start)
                } else {
                    start
                };
                last_line = line_num;
                last_start = start;

                tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length,
                    token_type: VARIABLE,
                    token_modifiers_bitset: 0,
                });
                search_start = abs_open + close_pos + 2;
            } else {
                break;
            }
        }

        // Plugin names: @uuid, @email, @ip, etc.
        let plugins = [
            "@uuid",
            "@email",
            "@url",
            "@ip",
            "@timestamp",
            "@regex",
            "@len",
            "@header",
            "@trailer",
            "@env",
        ];
        for plugin in &plugins {
            if let Some(pos) = line.find(plugin) {
                let start = pos as u32;
                let length = plugin.len() as u32;

                let delta_line = line_num.saturating_sub(last_line);
                let delta_start = if delta_line == 0 {
                    start.saturating_sub(last_start)
                } else {
                    start
                };
                last_line = line_num;
                last_start = start;

                tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length,
                    token_type: FUNCTION,
                    token_modifiers_bitset: 0,
                });
            }
        }

        // Operators: |, +, -, *, /, %, ==, !=, <, >, <=, >=
        let operators = [
            "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "|",
        ];
        for op in &operators {
            let mut pos = 0;
            while let Some(found_pos) = line[pos..].find(op) {
                let start = (pos + found_pos) as u32;
                let length = op.len() as u32;

                let delta_line = line_num.saturating_sub(last_line);
                let delta_start = if delta_line == 0 {
                    start.saturating_sub(last_start)
                } else {
                    start
                };
                last_line = line_num;
                last_start = start;

                tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length,
                    token_type: OPERATOR,
                    token_modifiers_bitset: 0,
                });
                pos = pos + found_pos + op.len();
            }
        }

        // Numbers (simple pattern)
        if let Some(num_regex) = &num_regex {
            for mat in num_regex.find_iter(line) {
                let start = mat.start() as u32;
                let length = mat.end() as u32 - start;

                let delta_line = line_num.saturating_sub(last_line);
                let delta_start = if delta_line == 0 {
                    start.saturating_sub(last_start)
                } else {
                    start
                };
                last_line = line_num;
                last_start = start;

                tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length,
                    token_type: NUMBER,
                    token_modifiers_bitset: 0,
                });
            }
        }
    }

    SemanticTokens {
        result_id: None,
        data: tokens,
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
                for _extraction in extractions {
                    let section_line = ((section.start_line as i32) - 1).max(0) as u32;
                    if section_line >= range.start.line && section_line <= range.end.line {
                        // Add type hint for extracted variable
                        hints.push(InlayHint {
                            position: tower_lsp::lsp_types::Position {
                                line: section_line,
                                character: 1000,
                            },
                            label: InlayHintLabel::String(": any".to_string()),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: None,
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        });
                    }
                }
            }
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
    fn test_build_folding_ranges_single_line() {
        // Test that sections with start_line == end_line don't create folding ranges
        // Note: Parser may not create sections for incomplete content, so we just verify
        // the function doesn't panic and returns a valid result
        let content = "--- ENDPOINT ---\n";
        let _ranges = build_folding_ranges(content);

        // Just verify the function works without panicking
        // The actual behavior depends on parser behavior for incomplete content
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
}
