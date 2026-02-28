//! LSP request handlers with full test coverage
//!
//! This module contains all LSP request handlers with comprehensive test coverage.
//! Each handler is tested in isolation.

use serde_json::json;
use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::parser::{self, ast::SectionType};
use crate::plugins::{PluginManager, PluginPurity};

/// Get hover documentation for a section type
pub fn get_section_hover(section_type: &SectionType) -> Option<String> {
    match section_type {
        SectionType::Address => Some("**ADDRESS**\n\nServer address in `host:port` format.".to_string()),
        SectionType::Endpoint => Some("**ENDPOINT**\n\ngRPC endpoint in `package.Service/Method` format.".to_string()),
        SectionType::Request => Some("**REQUEST**\n\nRequest payload in JSON/JSON5 format.\n\nSupports:\n- Comments (`//`, `/* */`, `#`)\n- Trailing commas\n- Unquoted keys\n- Single-quoted strings".to_string()),
        SectionType::Response => Some("**RESPONSE**\n\nExpected response with inline options.\n\nOptions:\n- `with_asserts` - Run ASSERTS\n- `partial` - Subset comparison\n- `tolerance` - Numeric tolerance\n- `redact` - Redact fields\n- `unordered_arrays` - Order-independent".to_string()),
        SectionType::Error => Some("**ERROR**\n\nExpected error response.\n\nUse when you expect the gRPC call to fail.".to_string()),
        SectionType::RequestHeaders => Some("**REQUEST_HEADERS**\n\nRequest headers in `key: value` format.".to_string()),
        SectionType::Tls => Some("**TLS**\n\nTLS/mTLS configuration.\n\nKeys:\n- `ca_cert` - CA certificate path\n- `client_cert` - Client certificate\n- `client_key` - Client key\n- `server_name` - SNI server name\n- `insecure` - Skip verification".to_string()),
        SectionType::Proto => Some("**PROTO**\n\nProto file configuration.\n\nKeys:\n- `descriptor` - Path to .desc file\n- `files` - Comma-separated proto files\n- `import_paths` - Import paths".to_string()),
        SectionType::Options => Some("**OPTIONS**\n\nTest execution options.".to_string()),
        SectionType::Extract => Some("**EXTRACT**\n\nVariable extraction using JQ paths.\n\nExample:\n```\nuser_id: .id\ntoken: .auth.token\n```\n\nUse in REQUEST: `${user_id}`".to_string()),
        SectionType::Asserts => Some("**ASSERTS**\n\nAssertion expressions.\n\nOperators: `==`, `!=`, `>`, `<`, `contains`, `matches`\nPlugins: `@uuid`, `@email`, `@ip`, `@url`, `@timestamp`\nJQ: `select`, `length`, `startswith`".to_string()),
    }
}

/// Get completions for section headers
pub fn get_section_completions() -> Vec<CompletionItem> {
    vec![
        "ADDRESS",
        "ENDPOINT",
        "REQUEST",
        "RESPONSE",
        "ERROR",
        "REQUEST_HEADERS",
        "TLS",
        "PROTO",
        "OPTIONS",
        "EXTRACT",
        "ASSERTS",
    ]
    .into_iter()
    .map(|s| CompletionItem {
        label: format!("--- {} ---", s),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(format!("{} section", s)),
        insert_text: Some(format!("--- {} ---", s)),
        ..CompletionItem::default()
    })
    .collect()
}

/// Get completions for known gRPC servers
pub fn get_address_completions() -> Vec<CompletionItem> {
    vec![
        ("localhost:4770", "Default gripmock port"),
        ("localhost:50051", "Common gRPC port"),
        ("localhost:9000", "Alternative gRPC port"),
    ]
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

/// Get completions for assertion operators and plugins
pub fn get_assertion_completions() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = vec![
        ("==", CompletionItemKind::OPERATOR, "Equality"),
        ("!=", CompletionItemKind::OPERATOR, "Inequality"),
        (">", CompletionItemKind::OPERATOR, "Greater than"),
        ("<", CompletionItemKind::OPERATOR, "Less than"),
        (">=", CompletionItemKind::OPERATOR, "Greater or equal"),
        ("<=", CompletionItemKind::OPERATOR, "Less or equal"),
        (
            "contains",
            CompletionItemKind::KEYWORD,
            "String/array contains",
        ),
        ("matches", CompletionItemKind::KEYWORD, "Regex match"),
    ]
    .into_iter()
    .map(|(label, kind, detail)| CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: Some(detail.to_string()),
        ..CompletionItem::default()
    })
    .collect();

    let mut plugins = PluginManager::new().list();
    plugins.sort_by(|a, b| a.name().cmp(b.name()));

    for plugin in plugins {
        let signature = plugin.signature();
        let name = plugin.name().trim_start_matches('@');
        let purity = match signature.purity {
            PluginPurity::Pure => "pure",
            PluginPurity::ContextDependent => "context",
            PluginPurity::Impure => "impure",
        };
        let label = format!("@{}(...)", name);
        let detail = format!("{} [{}]", plugin.description(), purity);
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail),
            ..CompletionItem::default()
        });
    }

    items
}

/// Get completions for EXTRACT JQ functions
pub fn get_extract_completions() -> Vec<CompletionItem> {
    vec![
        // String functions
        (
            "upper",
            CompletionItemKind::FUNCTION,
            "Convert to uppercase",
        ),
        (
            "lower",
            CompletionItemKind::FUNCTION,
            "Convert to lowercase",
        ),
        ("trim", CompletionItemKind::FUNCTION, "Trim whitespace"),
        ("split(\",\")", CompletionItemKind::FUNCTION, "Split string"),
        ("join(\"-\")", CompletionItemKind::FUNCTION, "Join array"),
        (
            "gsub(\"old\"; \"new\")",
            CompletionItemKind::FUNCTION,
            "Global substitution",
        ),
        // Numeric functions
        ("avg", CompletionItemKind::FUNCTION, "Average of array"),
        ("min", CompletionItemKind::FUNCTION, "Minimum value"),
        ("max", CompletionItemKind::FUNCTION, "Maximum value"),
        ("add", CompletionItemKind::FUNCTION, "Sum of array"),
        (
            "length",
            CompletionItemKind::FUNCTION,
            "Length of array/string",
        ),
        // Array functions
        (
            "[.[] | select(.active)]",
            CompletionItemKind::SNIPPET,
            "Filter array",
        ),
        ("[.[] | .name]", CompletionItemKind::SNIPPET, "Map array"),
        (
            "sort_by(.field)",
            CompletionItemKind::FUNCTION,
            "Sort by field",
        ),
        ("reverse", CompletionItemKind::FUNCTION, "Reverse array"),
        ("unique", CompletionItemKind::FUNCTION, "Unique values"),
        (
            "group_by(.field)",
            CompletionItemKind::FUNCTION,
            "Group by field",
        ),
        // Object functions
        ("keys", CompletionItemKind::FUNCTION, "Get keys"),
        ("values", CompletionItemKind::FUNCTION, "Get values"),
        ("del(.field)", CompletionItemKind::FUNCTION, "Delete field"),
        // Type conversion
        (
            "tostring",
            CompletionItemKind::FUNCTION,
            "Convert to string",
        ),
        (
            "tonumber",
            CompletionItemKind::FUNCTION,
            "Convert to number",
        ),
        ("type", CompletionItemKind::FUNCTION, "Get type"),
        // Conditional
        (
            "if .field == \"x\" then \"y\" else \"z\" end",
            CompletionItemKind::SNIPPET,
            "Conditional",
        ),
        (
            "// \"default\"",
            CompletionItemKind::SNIPPET,
            "Default value",
        ),
        // Date/Time
        (
            "fromdateiso8601",
            CompletionItemKind::FUNCTION,
            "Parse ISO8601",
        ),
        (
            "todateiso8601",
            CompletionItemKind::FUNCTION,
            "Format ISO8601",
        ),
        (
            "strftime(\"%Y-%m-%d\")",
            CompletionItemKind::FUNCTION,
            "Format date",
        ),
        // Encoding
        ("@base64", CompletionItemKind::FUNCTION, "Base64 encode"),
        ("@base64d", CompletionItemKind::FUNCTION, "Base64 decode"),
        ("@uri", CompletionItemKind::FUNCTION, "URI encode"),
        // JSON
        ("tojson", CompletionItemKind::FUNCTION, "Stringify JSON"),
        ("fromjson", CompletionItemKind::FUNCTION, "Parse JSON"),
    ]
    .into_iter()
    .map(|(label, kind, detail)| CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: Some(detail.to_string()),
        insert_text: Some(label.to_string()),
        ..CompletionItem::default()
    })
    .collect()
}

/// Extract address from document using AST
pub fn get_address_from_document(content: &str) -> Option<String> {
    let doc = parser::parse_gctf_from_str(content, "temp.gctf").ok()?;
    for section in &doc.sections {
        if section.section_type == SectionType::Address
            && let parser::ast::SectionContent::Single(addr) = &section.content
        {
            return Some(addr.trim().to_string());
        }
    }
    std::env::var("GRPCTESTIFY_ADDRESS").ok()
}

/// Convert validation error to LSP diagnostic
pub fn validation_error_to_diagnostic(
    error: &crate::parser::validator::ValidationError,
    content: &str,
) -> Diagnostic {
    let severity = match error.severity {
        crate::parser::validator::ErrorSeverity::Error => DiagnosticSeverity::ERROR,
        crate::parser::validator::ErrorSeverity::Warning => DiagnosticSeverity::WARNING,
        crate::parser::validator::ErrorSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    // AST line is 1-based, LSP is 0-based
    let line_num = (error.line.unwrap_or(1) - 1) as u32;
    let line_len = content
        .lines()
        .nth(line_num as usize)
        .map(|l| l.len())
        .unwrap_or(0) as u32;

    Diagnostic::new(
        Range::new(
            Position::new(line_num, 0),
            Position::new(line_num, line_len),
        ),
        Some(severity),
        None,
        None,
        error.message.clone(),
        None,
        None,
    )
}

/// Create code action for deprecated HEADERS section
pub fn create_headers_deprecated_action(uri: &Url, range: Range) -> CodeAction {
    CodeAction {
        title: "Replace --- HEADERS --- with --- REQUEST_HEADERS ---".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.clone(),
                vec![TextEdit::new(range, "--- REQUEST_HEADERS ---".to_string())],
            )])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    }
}

pub fn collect_optimizer_diagnostics(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (range, replacement, rule_id, before) in
        collect_optimizer_rewrites_with_ranges(doc, content)
    {
        diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::HINT),
            code: Some(NumberOrString::String(rule_id)),
            source: Some("grpctestify-optimizer".to_string()),
            message: format!("Optimizer hint: {} -> {}", before, replacement),
            data: Some(json!({"replacement": replacement})),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

pub fn collect_optimizer_rewrite_edits(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<TextEdit> {
    collect_optimizer_rewrites_with_ranges(doc, content)
        .into_iter()
        .map(|(range, replacement, _, _)| TextEdit::new(range, replacement))
        .collect()
}

fn collect_optimizer_rewrites_with_ranges(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<(Range, String, String, String)> {
    let hints = crate::optimizer::collect_assertion_optimizations(doc);
    let lines: Vec<&str> = content.lines().collect();
    let mut rewrites = Vec::new();

    for hint in hints {
        let lsp_line = hint.line.saturating_sub(1) as u32;
        let full_line = lines.get(lsp_line as usize).copied().unwrap_or("");
        let start_char = full_line.find(&hint.before).unwrap_or(0) as u32;
        let end_char = (start_char as usize + hint.before.len()) as u32;

        rewrites.push((
            Range::new(
                Position::new(lsp_line, start_char),
                Position::new(lsp_line, end_char),
            ),
            hint.after,
            hint.rule_id,
            hint.before,
        ));
    }

    rewrites
}

pub fn collect_semantic_diagnostics(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for mismatch in crate::semantics::collect_assertion_type_mismatches(doc) {
        let lsp_line = mismatch.line.saturating_sub(1) as u32;
        let full_line = lines.get(lsp_line as usize).copied().unwrap_or("");
        let start_char = full_line.find(&mismatch.expression).unwrap_or(0) as u32;
        let end_char = (start_char as usize + mismatch.expression.len()) as u32;

        diagnostics.push(Diagnostic {
            range: Range::new(
                Position::new(lsp_line, start_char),
                Position::new(lsp_line, end_char),
            ),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(mismatch.rule_id)),
            source: Some("grpctestify-semantics".to_string()),
            message: mismatch.message,
            ..Diagnostic::default()
        });
    }

    for unknown in crate::semantics::collect_unknown_plugin_calls(doc) {
        let lsp_line = unknown.line.saturating_sub(1) as u32;
        let full_line = lines.get(lsp_line as usize).copied().unwrap_or("");
        let needle = format!("@{}(", unknown.plugin_name);
        let start_char = full_line.find(&needle).unwrap_or(0) as u32;
        let end_char = start_char + needle.len() as u32;

        diagnostics.push(Diagnostic {
            range: Range::new(
                Position::new(lsp_line, start_char),
                Position::new(lsp_line, end_char),
            ),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(unknown.rule_id)),
            source: Some("grpctestify-semantics".to_string()),
            message: unknown.message,
            ..Diagnostic::default()
        });
    }

    diagnostics
}

pub fn create_optimizer_rewrite_action(
    uri: &Url,
    range: Range,
    replacement: &str,
    rule_id: &str,
) -> CodeAction {
    CodeAction {
        title: format!("Apply safe optimization ({})", rule_id),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.clone(),
                vec![TextEdit::new(range, replacement.to_string())],
            )])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    }
}

pub fn create_apply_all_optimizer_rewrite_action(
    uri: &Url,
    edits: Vec<TextEdit>,
    count: usize,
) -> CodeAction {
    CodeAction {
        title: format!("Apply all safe optimizations in file ({})", count),
        kind: Some(CodeActionKind::SOURCE),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(uri.clone(), edits)])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(false),
        ..CodeAction::default()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_section_hover_all_types() {
        assert!(get_section_hover(&SectionType::Address).is_some());
        assert!(get_section_hover(&SectionType::Endpoint).is_some());
        assert!(get_section_hover(&SectionType::Request).is_some());
        assert!(get_section_hover(&SectionType::Response).is_some());
        assert!(get_section_hover(&SectionType::Error).is_some());
        assert!(get_section_hover(&SectionType::RequestHeaders).is_some());
        assert!(get_section_hover(&SectionType::Tls).is_some());
        assert!(get_section_hover(&SectionType::Proto).is_some());
        assert!(get_section_hover(&SectionType::Options).is_some());
        assert!(get_section_hover(&SectionType::Extract).is_some());
        assert!(get_section_hover(&SectionType::Asserts).is_some());
    }

    #[test]
    fn test_get_section_hover_content() {
        let hover = get_section_hover(&SectionType::Address).unwrap();
        assert!(hover.contains("ADDRESS"));
        assert!(hover.contains("host:port"));

        let hover = get_section_hover(&SectionType::Request).unwrap();
        assert!(hover.contains("JSON/JSON5"));
        assert!(hover.contains("Comments"));
    }

    #[test]
    fn test_get_section_completions() {
        let completions = get_section_completions();
        assert_eq!(completions.len(), 11); // 11 section types (no RESPONSE_HEADERS)

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"--- ADDRESS ---"));
        assert!(labels.contains(&"--- ENDPOINT ---"));
        assert!(labels.contains(&"--- REQUEST ---"));
        assert!(labels.contains(&"--- RESPONSE ---"));
    }

    #[test]
    fn test_get_address_completions() {
        let completions = get_address_completions();
        assert_eq!(completions.len(), 3);

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"localhost:4770"));
        assert!(labels.contains(&"localhost:50051"));
        assert!(labels.contains(&"localhost:9000"));
    }

    #[test]
    fn test_get_assertion_completions() {
        let completions = get_assertion_completions();
        assert!(completions.len() >= 15);

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"=="));
        assert!(labels.contains(&"!="));
        assert!(labels.contains(&"@uuid(...)"));
        assert!(labels.contains(&"@email(...)"));
        assert!(labels.contains(&"@has_trailer(...)"));
    }

    #[test]
    fn test_get_address_from_document_with_address() {
        let content = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method
"#;
        let address = get_address_from_document(content);
        assert_eq!(address, Some("localhost:4770".to_string()));
    }

    #[test]
    fn test_get_address_from_document_no_address() {
        let content = r#"--- ENDPOINT ---
test.Service/Method
"#;
        let address = get_address_from_document(content);
        assert!(address.is_none());
    }

    #[test]
    fn test_validation_error_to_diagnostic() {
        let error = crate::parser::validator::ValidationError {
            message: "Test error".to_string(),
            line: Some(5),
            severity: crate::parser::validator::ErrorSeverity::Error,
        };

        let content = "line1\nline2\nline3\nline4\nline5\nline6";
        let diagnostic = validation_error_to_diagnostic(&error, content);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.range.start.line, 4); // 0-based
        assert_eq!(diagnostic.message, "Test error");
    }

    #[test]
    fn test_create_headers_deprecated_action() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let range = Range::new(Position::new(0, 0), Position::new(0, 10));

        let action = create_headers_deprecated_action(&uri, range);

        assert_eq!(
            action.title,
            "Replace --- HEADERS --- with --- REQUEST_HEADERS ---"
        );
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));

        let edit = action.edit.unwrap();
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "--- REQUEST_HEADERS ---");
    }

    #[test]
    fn test_collect_optimizer_diagnostics_has_header_true() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("OPT_B001".to_string()))
        );
    }

    #[test]
    fn test_snapshot_optimizer_diagnostic_hint() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);

        let actual = serde_json::to_value(&diagnostics[0]).unwrap();
        let expected = json!({
            "range": {
                "start": {"line": 4, "character": 0},
                "end": {"line": 4, "character": 24}
            },
            "severity": 4,
            "code": "OPT_B001",
            "source": "grpctestify-optimizer",
            "message": "Optimizer hint: @has_header(\"x\") == true -> @has_header(\"x\")",
            "data": {"replacement": "@has_header(\"x\")"}
        });
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_create_optimizer_rewrite_action() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let range = Range::new(Position::new(2, 0), Position::new(2, 10));

        let action = create_optimizer_rewrite_action(&uri, range, "@has_header(\"x\")", "OPT_B001");
        assert!(action.title.contains("OPT_B001"));
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    }

    #[test]
    fn test_collect_optimizer_rewrite_edits() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let edits = collect_optimizer_rewrite_edits(&doc, content);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "@has_header(\"x\")");
    }

    #[test]
    fn test_create_apply_all_optimizer_rewrite_action() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let edits = vec![
            TextEdit::new(
                Range::new(Position::new(4, 0), Position::new(4, 24)),
                "@has_header(\"x\")".to_string(),
            ),
            TextEdit::new(
                Range::new(Position::new(5, 0), Position::new(5, 25)),
                "!@has_header(\"y\")".to_string(),
            ),
        ];

        let action = create_apply_all_optimizer_rewrite_action(&uri, edits, 2);
        assert!(
            action
                .title
                .contains("Apply all safe optimizations in file")
        );
        assert!(action.title.contains("2"));
        assert_eq!(action.kind, Some(CodeActionKind::SOURCE));

        let changes = action
            .edit
            .unwrap()
            .changes
            .unwrap()
            .get(&uri)
            .unwrap()
            .clone();
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn test_snapshot_optimizer_quickfix_action() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let range = Range::new(Position::new(4, 0), Position::new(4, 24));
        let action = create_optimizer_rewrite_action(&uri, range, "@has_header(\"x\")", "OPT_B001");

        let actual = serde_json::to_value(&action).unwrap();
        let expected = json!({
            "title": "Apply safe optimization (OPT_B001)",
            "kind": "quickfix",
            "edit": {
                "changes": {
                    "file:///test.gctf": [
                        {
                            "range": {
                                "start": {"line": 4, "character": 0},
                                "end": {"line": 4, "character": 24}
                            },
                            "newText": "@has_header(\"x\")"
                        }
                    ]
                }
            },
            "isPreferred": true
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_snapshot_apply_all_optimizer_action() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let edits = vec![
            TextEdit::new(
                Range::new(Position::new(4, 0), Position::new(4, 24)),
                "@has_header(\"x\")".to_string(),
            ),
            TextEdit::new(
                Range::new(Position::new(5, 0), Position::new(5, 25)),
                "!@has_header(\"y\")".to_string(),
            ),
        ];
        let action = create_apply_all_optimizer_rewrite_action(&uri, edits, 2);

        let actual = serde_json::to_value(&action).unwrap();
        let expected = json!({
            "title": "Apply all safe optimizations in file (2)",
            "kind": "source",
            "edit": {
                "changes": {
                    "file:///test.gctf": [
                        {
                            "range": {
                                "start": {"line": 4, "character": 0},
                                "end": {"line": 4, "character": 24}
                            },
                            "newText": "@has_header(\"x\")"
                        },
                        {
                            "range": {
                                "start": {"line": 5, "character": 0},
                                "end": {"line": 5, "character": 25}
                            },
                            "newText": "!@has_header(\"y\")"
                        }
                    ]
                }
            },
            "isPreferred": false
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_collect_optimizer_diagnostics_non_boolean_plugin() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items) == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_collect_optimizer_diagnostics_double_negation_rule() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
!!@has_header("x")
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("OPT_B005".to_string()))
        );
    }

    #[test]
    fn test_collect_optimizer_diagnostics_canonical_operator_rule() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.name startswith "abc"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("OPT_N001".to_string()))
        );
    }

    #[test]
    fn test_collect_optimizer_diagnostics_constant_fold_rule() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
3 > 2
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("OPT_B006".to_string()))
        );
    }

    #[test]
    fn test_collect_semantic_diagnostics_unknown_plugin() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@regexp(.name, "^a") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_semantic_diagnostics(&doc, content);

        assert!(diagnostics.iter().any(|d| {
            d.code == Some(NumberOrString::String("SEM_F001".to_string()))
                && d.severity == Some(DiagnosticSeverity::ERROR)
        }));
    }
}
