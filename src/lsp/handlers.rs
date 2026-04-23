//! LSP request handlers with full test coverage
//!
//! This module contains all LSP request handlers with comprehensive test coverage.
//! Each handler is tested in isolation.

use serde_json::json;
use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::config;
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
        SectionType::Asserts => Some("**ASSERTS**\n\nAssertion expressions.\n\nOperators: `==`, `!=`, `>`, `<`, `contains`, `matches`\nPlugins: `@uuid`, `@email`, `@ip`, `@url`, `@timestamp`, `@elapsed_ms`, `@total_elapsed_ms`\nJQ: `select`, `length`, `startswith`".to_string()),
        SectionType::Meta => Some("**META**\n\nFile-level metadata (YAML).\n\nMust be first section in file.\n\nOnly 0 or 1 per file.".to_string()),
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

pub fn get_section_key_completions(section_type: &SectionType) -> Vec<CompletionItem> {
    let entries: Vec<(&str, &str)> = match section_type {
        SectionType::Proto => vec![
            ("descriptor", "Path to descriptor set (.desc/.binpb)"),
            ("files", "List of .proto files"),
            ("import_paths", "List of import search paths"),
        ],
        SectionType::Tls => vec![
            ("ca_file", "CA certificate path"),
            ("cert_file", "Client certificate path"),
            ("key_file", "Client private key path"),
            ("server_name", "TLS SNI server name"),
            ("insecure", "Disable certificate verification"),
        ],
        SectionType::Options => vec![
            ("timeout", "Request timeout (e.g. 5s)"),
            ("retries", "Retry count"),
            ("parallel", "Run test execution in parallel"),
            ("sort", "Execution order (path, random)"),
            ("dry_run", "Parse/validate only without gRPC call"),
        ],
        _ => vec![],
    };

    entries
        .into_iter()
        .map(|(label, detail)| CompletionItem {
            label: format!("{}:", label),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(detail.to_string()),
            insert_text: Some(format!("{}: ", label)),
            ..CompletionItem::default()
        })
        .collect()
}

pub fn get_section_header_option_completions(section_type: &SectionType) -> Vec<CompletionItem> {
    let entries: Vec<(&str, &str)> = match section_type {
        SectionType::Response => vec![
            ("partial=true", "Enable partial response matching"),
            ("with_asserts=true", "Run ASSERTS after RESPONSE comparison"),
            ("tolerance=0.001", "Numeric tolerance for float comparisons"),
            (
                "unordered_arrays=true",
                "Ignore array order while comparing",
            ),
            ("redact=$.token", "Redact field path in comparisons"),
        ],
        _ => vec![],
    };

    entries
        .into_iter()
        .map(|(label, detail)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
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
    std::env::var(config::ENV_GRPCTESTIFY_ADDRESS).ok()
}

/// Get variable completions from EXTRACT sections in all preceding documents.
/// Returns completions for `{{var}}` syntax in REQUEST, REQUEST_HEADERS, etc.
pub fn get_variable_completions(
    doc: &crate::parser::GctfDocument,
    current_line_0based: usize,
) -> Vec<CompletionItem> {
    use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

    let mut items = Vec::new();
    let current_doc_idx = find_document_index_at_line(doc, current_line_0based);

    // Collect variables from all documents before the current one
    for (doc_idx, d) in doc.iter_chain().enumerate() {
        // Only variables from documents BEFORE the current document
        if doc_idx >= current_doc_idx {
            continue;
        }

        for section in &d.sections {
            if section.section_type != SectionType::Extract {
                continue;
            }
            if let parser::ast::SectionContent::Extract(extractions) = &section.content {
                for (name, expr) in extractions {
                    let detail = format!("from Document {}, EXTRACT: {}", doc_idx + 1, expr);
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some(detail),
                        insert_text: Some(format!("{{{{ {} }}}}", name)),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
                            tower_lsp::lsp_types::MarkupContent {
                                kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                                value: format!(
                                    "**{}**\n\nExtracted: `{}`\nSource: Document {}, line {}",
                                    name,
                                    expr,
                                    doc_idx + 1,
                                    section.start_line + 1
                                ),
                            },
                        )),
                        ..CompletionItem::default()
                    });
                }
            }
        }
    }

    items
}

/// Find which document (0-based index) contains the given line.
fn find_document_index_at_line(doc: &crate::parser::GctfDocument, line_0based: usize) -> usize {
    let line_1based = line_0based + 1;
    for (idx, d) in doc.iter_chain().enumerate() {
        if let (Some(first), Some(last)) = (d.sections.first(), d.sections.last())
            && line_1based >= first.start_line
            && line_1based <= last.end_line
        {
            return idx;
        }
    }
    // If line is before first section, it's in the first document
    0
}

/// Generate hover content for {{var_name}} references.
/// Shows where the variable was extracted from.
pub fn get_var_hover(
    doc: &crate::parser::GctfDocument,
    line_0based: usize,
    character: u32,
) -> Option<tower_lsp::lsp_types::Hover> {
    use tower_lsp::lsp_types::{Hover, HoverContents, MarkedString};

    let line_str = doc.metadata.source.as_deref()?.lines().nth(line_0based)?;

    let char_pos = character as usize;
    if char_pos >= line_str.len() {
        return None;
    }

    // Check if cursor is inside {{ ... }}
    let before = &line_str[..char_pos];
    let open_brace = before.rfind("{{")?;

    let after = &line_str[char_pos..];
    let close_brace = after.find("}}")?;

    let var_content = line_str[open_brace + 2..char_pos + close_brace].trim();
    let var_name = var_content.split_whitespace().next()?;

    // Find which document this line belongs to
    let current_doc_idx = find_document_index_at_line(doc, line_0based);

    // Find the variable definition in preceding documents
    for (doc_idx, d) in doc.iter_chain().enumerate() {
        if doc_idx >= current_doc_idx {
            break; // Only search documents before current
        }

        for section in &d.sections {
            if section.section_type != SectionType::Extract {
                continue;
            }
            if let parser::ast::SectionContent::Extract(extractions) = &section.content
                && let Some(expr) = extractions.get(var_name)
            {
                let hover_text = format!(
                    "**Variable: `{}`**\n\nExtracted: `{}`\nSource: Document {}, line {}",
                    var_name,
                    expr,
                    doc_idx + 1,
                    section.start_line + 1
                );
                return Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(hover_text)),
                    range: None,
                });
            }
        }
    }

    // Variable not found in preceding documents
    let hover_text = format!(
        "**Unknown variable: `{}`**\n\nNo EXTRACT definition found in preceding documents.",
        var_name
    );
    Some(tower_lsp::lsp_types::Hover {
        contents: HoverContents::Scalar(MarkedString::String(hover_text)),
        range: None,
    })
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
            hint.rule_id.as_str().to_string(),
            hint.before,
        ));
    }

    rewrites
}

// ─── Unused Variable Detection (AST-based) ───

/// Result of unused variable detection
#[derive(Debug, Clone)]
pub struct UnusedVariable {
    /// Variable name
    pub name: String,
    /// 1-based line number where the variable is defined (EXTRACT section)
    pub line: usize,
    /// 0-based character where the variable name starts on that line
    pub character: usize,
    /// Document index (0-based) where the variable was defined
    pub doc_index: usize,
}

/// Collect unused EXTRACT variables across the document chain (pure AST traversal).
///
/// A variable is "unused" if it was defined in an EXTRACT section but never
/// referenced via `{{ var_name }}` in any subsequent document.
pub fn collect_unused_variables(doc: &crate::parser::GctfDocument) -> Vec<UnusedVariable> {
    // Step 1: Extract all variables from EXTRACT sections via AST
    let defined_vars = extract_all_vars(doc);

    // Step 2: For each variable, check if it's used in any subsequent document
    defined_vars
        .into_iter()
        .filter(|(def_doc_idx, var_name, _, _)| {
            !is_var_used_in_subsequent_docs(doc, *def_doc_idx, var_name)
        })
        .map(|(doc_idx, name, line, character)| UnusedVariable {
            name,
            line,
            character,
            doc_index: doc_idx,
        })
        .collect()
}

/// Extract all EXTRACT variables from the document chain with their AST source locations.
fn extract_all_vars(doc: &crate::parser::GctfDocument) -> Vec<(usize, String, usize, usize)> {
    // (doc_index, var_name, 1-based line, 0-based char)
    let mut vars = Vec::new();

    for (doc_idx, curr_doc) in doc.iter_chain().enumerate() {
        for section in &curr_doc.sections {
            if section.section_type != SectionType::Extract {
                continue;
            }
            if let parser::ast::SectionContent::Extract(extractions) = &section.content {
                for (local_line, raw_line) in section.raw_content.lines().enumerate() {
                    let trimmed = raw_line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                        continue;
                    }
                    if let Some(extract_var) = parser::ExtractVar::parse(trimmed) {
                        let global_line = section.start_line + local_line;
                        let char_pos = raw_line.find(&extract_var.name).unwrap_or(0);
                        vars.push((doc_idx, extract_var.name, global_line, char_pos));
                    }
                }

                // Also check for vars defined only in the parsed Extract map
                // (covers the case where raw_content parsing differs)
                for var_name in extractions.keys() {
                    // Check if already added from raw_content
                    let already_present = vars.iter().any(|(_, n, _, _)| n == var_name);
                    if !already_present {
                        // Find in raw_content to get line info
                        for (local_line, raw_line) in section.raw_content.lines().enumerate() {
                            if raw_line.trim().starts_with(var_name) {
                                let global_line = section.start_line + local_line;
                                let char_pos = raw_line.find(var_name).unwrap_or(0);
                                vars.push((doc_idx, var_name.clone(), global_line, char_pos));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    vars
}

/// Check if a variable is referenced via `{{ var_name }}` in any document after `def_doc_idx`.
fn is_var_used_in_subsequent_docs(
    doc: &crate::parser::GctfDocument,
    def_doc_idx: usize,
    var_name: &str,
) -> bool {
    // Check all documents AFTER the defining document
    for (doc_idx, curr_doc) in doc.iter_chain().enumerate() {
        if doc_idx > def_doc_idx && doc_contains_var_reference(curr_doc, var_name) {
            return true;
        }
    }

    // No subsequent docs (this is the last/only document) — check usage within
    // the same document via non-EXTRACT sections (AST traversal)
    if def_doc_idx == doc.document_count() - 1 {
        return doc_contains_var_reference_excluding_extract(doc, var_name);
    }

    false
}

/// Check if a document contains `{{ var_name }}` via AST traversal.
fn doc_contains_var_reference(doc: &crate::parser::GctfDocument, var_name: &str) -> bool {
    for section in &doc.sections {
        if section_contains_var_reference(section, var_name) {
            return true;
        }
    }
    false
}

/// Check if an AST section contains a variable reference.
fn section_contains_var_reference(section: &crate::parser::ast::Section, var_name: &str) -> bool {
    match &section.content {
        // JSON sections may contain {{ var }} in string values
        parser::ast::SectionContent::Json(value) => json_contains_var(value, var_name),
        // Multiple JSON values
        parser::ast::SectionContent::JsonLines(values) => {
            values.iter().any(|v| json_contains_var(v, var_name))
        }
        // Key-value sections (REQUEST_HEADERS, TLS, OPTIONS, PROTO)
        parser::ast::SectionContent::KeyValues(kv) => {
            kv.values().any(|v| contains_var_pattern(v, var_name))
        }
        // Extract sections: don't check — a var definition is not a usage
        parser::ast::SectionContent::Extract(_) => false,
        // Assertions: check for $var_name references (EXTRACT uses $var syntax)
        parser::ast::SectionContent::Assertions(asserts) => {
            // In ASSERTS, variables from EXTRACT are referenced as $var_name
            asserts.iter().any(|a| contains_assert_var_ref(a, var_name))
        }
        // Single value sections (ADDRESS, ENDPOINT)
        parser::ast::SectionContent::Single(s) => contains_var_pattern(s, var_name),
        parser::ast::SectionContent::Empty => false,
        parser::ast::SectionContent::Meta(_) => false,
    }
}

/// Recursively check if a JSON value contains `{{ var_name }}` in any string.
fn json_contains_var(value: &serde_json::Value, var_name: &str) -> bool {
    match value {
        serde_json::Value::String(s) => contains_var_pattern(s, var_name),
        serde_json::Value::Object(map) => map.values().any(|v| json_contains_var(v, var_name)),
        serde_json::Value::Array(arr) => arr.iter().any(|v| json_contains_var(v, var_name)),
        _ => false,
    }
}

/// Check if a string contains `{{ var_name }}` (with flexible whitespace around name).
fn contains_var_pattern(s: &str, var_name: &str) -> bool {
    // Patterns: {{ var_name }}, {{var_name }}, {{ var_name}}, {{var_name}}
    let patterns = [
        format!("{{{{ {} }}}}", var_name),
        format!("{{{{{} }}}}", var_name),
        format!("{{{{ {}}}}}", var_name),
        format!("{{{{{}}}}}", var_name),
    ];
    patterns.iter().any(|p| s.contains(p))
}

/// Check if an assertion references a variable via `$var_name`.
fn contains_assert_var_ref(assertion: &str, var_name: &str) -> bool {
    // EXTRACT variables are referenced as $var_name in assertions
    let pattern = format!("${}", var_name);
    assertion.contains(&pattern)
}

/// Check if a document contains variable references outside EXTRACT sections (AST-based).
fn doc_contains_var_reference_excluding_extract(
    doc: &crate::parser::GctfDocument,
    var_name: &str,
) -> bool {
    for section in &doc.sections {
        if section.section_type == SectionType::Extract {
            continue;
        }
        if section_contains_var_reference(section, var_name) {
            return true;
        }
    }
    false
}

/// Convert unused variables to LSP diagnostics
pub fn unused_variable_to_diagnostic(var: &UnusedVariable) -> Diagnostic {
    let lsp_line = var.line.saturating_sub(1) as u32;
    let char_start = var.character as u32;
    let char_end = (var.character + var.name.len()) as u32;

    Diagnostic {
        range: Range::new(
            Position::new(lsp_line, char_start),
            Position::new(lsp_line, char_end),
        ),
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String("UNUSED_VARIABLE".to_string())),
        source: Some("grpctestify".to_string()),
        message: format!(
            "Variable '{}' is extracted but never used in subsequent documents",
            var.name
        ),
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        ..Diagnostic::default()
    }
}

pub fn collect_semantic_diagnostics(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Use Workflow for semantic analysis
    let workflow = crate::execution::Workflow::from_document_with_analysis(doc);

    for event in &workflow.events {
        if let crate::execution::WorkflowEvent::SemanticAnalysis {
            type_mismatches,
            unknown_plugins,
        } = event
        {
            // Process type mismatches
            for mismatch in type_mismatches {
                let lsp_line = mismatch.line.saturating_sub(1) as u32;
                let full_line = lines.get(lsp_line as usize).copied().unwrap_or("");
                let empty_str = "".to_string();
                let expr = mismatch.expression.as_ref().unwrap_or(&empty_str);
                let start_char = full_line.find(expr).unwrap_or(0) as u32;
                let end_char = (start_char as usize + expr.len()) as u32;

                diagnostics.push(Diagnostic {
                    range: Range::new(
                        Position::new(lsp_line, start_char),
                        Position::new(lsp_line, end_char),
                    ),
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String(mismatch.rule_id.clone())),
                    source: Some("grpctestify-semantics".to_string()),
                    message: mismatch.message.clone(),
                    ..Diagnostic::default()
                });
            }

            // Process unknown plugins
            for unknown in unknown_plugins {
                let lsp_line = unknown.line.saturating_sub(1) as u32;
                let full_line = lines.get(lsp_line as usize).copied().unwrap_or("");
                let empty_str = "".to_string();
                let plugin_name = unknown.plugin_name.as_ref().unwrap_or(&empty_str);
                let needle = format!("@{}(", plugin_name);
                let start_char = full_line.find(&needle).unwrap_or(0) as u32;
                let end_char = start_char + needle.len() as u32;

                diagnostics.push(Diagnostic {
                    range: Range::new(
                        Position::new(lsp_line, start_char),
                        Position::new(lsp_line, end_char),
                    ),
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String(unknown.rule_id.clone())),
                    source: Some("grpctestify-semantics".to_string()),
                    message: unknown.message.clone(),
                    ..Diagnostic::default()
                });
            }
        }
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
    use crate::optimizer::rule_ids;

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
            Some(NumberOrString::String(rule_ids::B001.as_str().to_string()))
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
            "code": rule_ids::B001.as_str(),
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

        let action = create_optimizer_rewrite_action(
            &uri,
            range,
            "@has_header(\"x\")",
            rule_ids::B001.as_str(),
        );
        assert!(action.title.contains(rule_ids::B001.as_str()));
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
        let action = create_optimizer_rewrite_action(
            &uri,
            range,
            "@has_header(\"x\")",
            rule_ids::B001.as_str(),
        );

        let actual = serde_json::to_value(&action).unwrap();
        let expected = json!({
            "title": format!("Apply safe optimization ({})", rule_ids::B001.as_str()),
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
        let expected = rule_ids::B017.as_str();
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String(expected.to_string()))
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
        assert!(diagnostics.is_empty());
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
            Some(NumberOrString::String(rule_ids::B006.as_str().to_string()))
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

    #[test]
    fn test_get_extract_completions() {
        let completions = get_extract_completions();
        assert!(completions.len() > 10);

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"upper"));
        assert!(labels.contains(&"lower"));
        assert!(labels.contains(&"trim"));
    }

    #[test]
    fn test_get_section_key_completions_proto() {
        let completions = get_section_key_completions(&SectionType::Proto);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"descriptor:"));
        assert!(labels.contains(&"files:"));
    }

    #[test]
    fn test_get_section_key_completions_tls() {
        let completions = get_section_key_completions(&SectionType::Tls);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"ca_file:"));
        assert!(labels.contains(&"cert_file:"));
    }

    #[test]
    fn test_get_section_key_completions_options() {
        let completions = get_section_key_completions(&SectionType::Options);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"timeout:"));
        assert!(labels.contains(&"retries:"));
    }

    #[test]
    fn test_get_section_key_completions_others() {
        assert!(get_section_key_completions(&SectionType::Address).is_empty());
        assert!(get_section_key_completions(&SectionType::Response).is_empty());
    }

    #[test]
    fn test_get_section_header_option_completions_response() {
        let completions = get_section_header_option_completions(&SectionType::Response);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"partial=true"));
        assert!(labels.contains(&"tolerance=0.001"));
    }

    #[test]
    fn test_get_section_header_option_completions_others() {
        assert!(get_section_header_option_completions(&SectionType::Address).is_empty());
        assert!(get_section_header_option_completions(&SectionType::Request).is_empty());
    }

    #[test]
    fn test_get_variable_completions() {
        let source = r#"--- ENDPOINT ---
svc.Create

--- REQUEST ---
{}

--- RESPONSE ---
{"id": "123"}

--- EXTRACT ---
user_id = .id
token = .token

--- ENDPOINT ---
svc.Read

--- REQUEST ---
{"id": "{{}}"}

--- RESPONSE ---
{}
"#;
        let doc = crate::parser::parse_gctf_from_str(source, "test.gctf").unwrap();
        // Completions should be available in the second document
        let completions = get_variable_completions(&doc, 17);
        assert!(!completions.is_empty());
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"user_id"));
        assert!(labels.contains(&"token"));
    }
}
