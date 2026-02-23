//! LSP request handlers with full test coverage
//! 
//! This module contains all LSP request handlers with comprehensive test coverage.
//! Each handler is tested in isolation.

use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::parser::{self, ast::SectionType};

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
    vec!["ADDRESS", "ENDPOINT", "REQUEST", "RESPONSE", "ERROR", 
         "REQUEST_HEADERS", "TLS", "PROTO", "OPTIONS", "EXTRACT", "ASSERTS"]
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
    vec![
        // Operators
        ("==", CompletionItemKind::OPERATOR, "Equality"),
        ("!=", CompletionItemKind::OPERATOR, "Inequality"),
        (">", CompletionItemKind::OPERATOR, "Greater than"),
        ("<", CompletionItemKind::OPERATOR, "Less than"),
        (">=", CompletionItemKind::OPERATOR, "Greater or equal"),
        ("<=", CompletionItemKind::OPERATOR, "Less or equal"),
        ("contains", CompletionItemKind::KEYWORD, "String/array contains"),
        ("matches", CompletionItemKind::KEYWORD, "Regex match"),
        // Plugins
        ("@uuid(.field)", CompletionItemKind::FUNCTION, "UUID validation"),
        ("@email(.field)", CompletionItemKind::FUNCTION, "Email validation"),
        ("@ip(.field)", CompletionItemKind::FUNCTION, "IP validation"),
        ("@url(.field)", CompletionItemKind::FUNCTION, "URL validation"),
        ("@timestamp(.field)", CompletionItemKind::FUNCTION, "Timestamp validation"),
        ("@header(\"name\")", CompletionItemKind::FUNCTION, "Check header"),
        ("@len(.field)", CompletionItemKind::FUNCTION, "Get length"),
    ]
    .into_iter()
    .map(|(label, kind, detail)| CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: Some(detail.to_string()),
        ..CompletionItem::default()
    })
    .collect()
}

/// Extract address from document using AST
pub async fn get_address_from_document(content: &str) -> Option<String> {
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

/// Convert validation error to LSP diagnostic
pub fn validation_error_to_diagnostic(error: &crate::parser::validator::ValidationError, content: &str) -> Diagnostic {
    let severity = match error.severity {
        crate::parser::validator::ErrorSeverity::Error => DiagnosticSeverity::ERROR,
        crate::parser::validator::ErrorSeverity::Warning => DiagnosticSeverity::WARNING,
        crate::parser::validator::ErrorSeverity::Info => DiagnosticSeverity::INFORMATION,
    };
    
    // AST line is 1-based, LSP is 0-based
    let line_num = (error.line.unwrap_or(1) - 1) as u32;
    let line_len = content.lines().nth(line_num as usize).map(|l| l.len()).unwrap_or(0) as u32;
    
    Diagnostic::new(
        Range::new(Position::new(line_num, 0), Position::new(line_num, line_len)),
        Some(severity),
        None, None,
        error.message.clone(),
        None, None,
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
        assert!(labels.contains(&"@uuid(.field)"));
        assert!(labels.contains(&"@email(.field)"));
    }

    #[tokio::test]
    async fn test_get_address_from_document_with_address() {
        let content = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method
"#;
        let address = get_address_from_document(content).await;
        assert_eq!(address, Some("localhost:4770".to_string()));
    }

    #[tokio::test]
    async fn test_get_address_from_document_no_address() {
        let content = r#"--- ENDPOINT ---
test.Service/Method
"#;
        let address = get_address_from_document(content).await;
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
        
        assert_eq!(action.title, "Replace --- HEADERS --- with --- REQUEST_HEADERS ---");
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));
        
        let edit = action.edit.unwrap();
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "--- REQUEST_HEADERS ---");
    }
}
