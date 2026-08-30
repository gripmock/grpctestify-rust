use serde_json::json;
use std::collections::HashMap;
use tower_lsp::lsp_types::*;

use crate::bench::schema::{
    BENCH_ASSERT_MODE_VALUES, BENCH_CACHE_VALUES, BENCH_DURATION_STOP_VALUES,
    BENCH_LOAD_SCHEDULE_VALUES, BENCH_MODE_VALUES, allowed_values_message, bench_key_detail,
    bench_keys_canonical_order,
};
use crate::config;
use crate::optimizer;
use crate::parser::{self, ast::SectionType};
use crate::plugins::PluginPurity;

pub fn get_section_hover(section_type: &SectionType) -> Option<String> {
    section_hover_for(crate::parser::ast::Family::Gctf, section_type)
}

pub fn section_hover_for(
    family: crate::parser::ast::Family,
    section_type: &SectionType,
) -> Option<String> {
    if family == crate::parser::ast::Family::Httf {
        match section_type {
            SectionType::Address => {
                return Some("**ADDRESS**\n\nWhere the calls go: `https://api.example.com`, or a `host:port` dialled over `http://`.\n\nA path in ENDPOINT is joined to it; an absolute url ignores it.".to_string());
            }
            SectionType::Endpoint => {
                return Some("**ENDPOINT**\n\nThe call, as a method and a path: `POST /v1/users`.\n\nAny method is accepted. `{{variables}}` are substituted in the path.".to_string());
            }
            SectionType::Request => {
                return Some("**REQUEST**\n\nThe body, sent as written — JSON, a form, XML, or plain text.\n\nThe `content-type` is the one REQUEST_HEADERS names, or one inferred from the body. A file with no REQUEST sends no body.".to_string());
            }
            SectionType::Response => {
                return Some("**RESPONSE**\n\nThe body that must come back. JSON is compared field by field; anything else is compared as the text it is.\n\nOptions: `partial`, `tolerance`, `redact`, `unordered_arrays`.".to_string());
            }
            SectionType::Asserts => {
                return Some("**ASSERTS**\n\nWhat must be true of the answer.\n\n`@status()` is the code it came back with, `@header(\"…\")` reads a response header, and a jq path reads the body.\n\nExample:\n```\n@status() == 200\n.name == \"Ada\"\n```".to_string());
            }
            _ => {}
        }
    }
    match section_type {
        SectionType::Address => Some("**ADDRESS**\n\nServer address in `host:port` format.".to_string()),
        SectionType::Endpoint => Some("**ENDPOINT**\n\ngRPC endpoint in `package.Service/Method` format.".to_string()),
        SectionType::Request => Some("**REQUEST**\n\nRequest payload in JSON/JSON5 format.\n\nSupports:\n- Comments (`//`, `/* */`, `#`)\n- Trailing commas\n- Unquoted keys\n- Single-quoted strings".to_string()),
        SectionType::Response => Some("**RESPONSE**\n\nExpected response with inline options.\n\nOptions:\n- `with_asserts` - Run ASSERTS\n- `partial` - Subset comparison\n- `tolerance` - Numeric tolerance\n- `redact` - Redact fields\n- `unordered_arrays` - Order-independent".to_string()),
        SectionType::Error => Some("**ERROR**\n\nExpected error response.\n\nUse when you expect the gRPC call to fail.".to_string()),
        SectionType::RequestHeaders => Some("**REQUEST_HEADERS**\n\nRequest headers in `key: value` format.".to_string()),
        SectionType::Tls => Some("**TLS**\n\nTLS/mTLS configuration.\n\nKeys:\n- `ca_cert` - CA certificate path\n- `client_cert` - Client certificate\n- `client_key` - Client key\n- `server_name` - SNI server name\n- `insecure` - Skip verification".to_string()),
        SectionType::Proto => Some("**PROTO**\n\nProto file configuration.\n\nKeys:\n- `descriptor` - Path to .desc file\n- `files` - Comma-separated proto files\n- `import_paths` - Import paths".to_string()),
        SectionType::Options => Some(
            "**OPTIONS**\n\nPer-test runtime overrides.\n\n             `timeout` seconds · `retry` count · `retry_delay` seconds · `no_retry` bool ·              `compression` none|gzip · `protocol` grpc|grpc-web|connectrpc\n\n             Precedence: section attributes > OPTIONS > CLI."
                .to_string(),
        ),
        SectionType::Extract => Some("**EXTRACT**\n\nVariable extraction using JQ paths, carried to later documents in the chain.\n\nExample:\n```\nuser_id = .id\ntoken = .auth.token\n```\n\nUse in later documents: `{{user_id}}` in REQUEST/headers, `$user_id` in ASSERTS.".to_string()),
        SectionType::Asserts => Some("**ASSERTS**\n\nAssertion expressions.\n\nOperators: `==`, `!=`, `>`, `<`, `>=`, `<=`, `contains`, `matches`, `startsWith`, `endsWith`\nValidators: `@is_uuid`, `@is_email`, `@is_ip`, `@is_url`, `@is_timestamp`, `@is_base64`, `@is_json`\nState: `@is_empty`, `@has_value`, `@len`\nScope: `@scope.index`, `@scope.message_count`\nTiming: `@elapsed_ms`, `@total_elapsed_ms`\nMetadata: `@header`, `@has_header`, `@trailer`, `@has_trailer`, `@env`\nType methods: `@url.*`, `@email.*`, `@ip.version`, `@uuid.version`, `@json.key`\nJQ: `select`, `length`, `startswith`".to_string()),
        SectionType::Meta => Some("**META**\n\nFile-level metadata (YAML).\n\nMust be first section in file.\n\nOnly 0 or 1 per file.".to_string()),
        SectionType::Bench => Some(bench_hover_doc()),
        SectionType::Dataset => Some("**DATASET**\n\nInline data-driven test rows (YAML list of objects).\n\nEach row's fields become `{{dataset.field}}` template variables, expanding this file into one test case per row — the same mechanism as `run --data`, but self-contained in the file.\n\nMutually exclusive with `--data`. Only 0 or 1 per file.".to_string()),
    }
}

fn bench_hover_doc() -> String {
    let keys_preview = bench_keys_canonical_order()
        .into_iter()
        .take(12)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "**BENCH**\n\nFile-level benchmark profile/options in `key: value` format.\n\nRecommended placement: first section, or immediately after META.\n\nOnly 0 or 1 per file.\n\nKey examples: `{}`\n\nCanonical values:\n- `mode`: {}\n- `load_schedule`: {}\n- `duration_stop`: {}\n- `assert_mode`: {}\n- `cache`: {}",
        keys_preview,
        allowed_values_message(BENCH_MODE_VALUES),
        allowed_values_message(BENCH_LOAD_SCHEDULE_VALUES),
        allowed_values_message(BENCH_DURATION_STOP_VALUES),
        allowed_values_message(BENCH_ASSERT_MODE_VALUES),
        allowed_values_message(BENCH_CACHE_VALUES),
    )
}

pub fn get_section_completions() -> Vec<CompletionItem> {
    section_completions_for(crate::parser::ast::Family::Gctf)
}

pub fn section_completions_for(family: crate::parser::ast::Family) -> Vec<CompletionItem> {
    let gctf_only = ["TLS", "PROTO", "BENCH", "ERROR"];
    [
        "ADDRESS",
        "ENDPOINT",
        "REQUEST",
        "RESPONSE",
        "ERROR",
        "REQUEST_HEADERS",
        "TLS",
        "PROTO",
        "OPTIONS",
        "BENCH",
        "EXTRACT",
        "ASSERTS",
        "DATASET",
    ]
    .into_iter()
    .filter(|s| family.allows_grpc() || !gctf_only.contains(s))
    .map(|s| CompletionItem {
        label: format!("--- {} ---", s),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(format!("{} section", s)),
        insert_text: Some(format!("--- {} ---", s)),
        ..CompletionItem::default()
    })
    .collect()
}

pub fn get_address_completions() -> Vec<CompletionItem> {
    address_completions_for(crate::parser::ast::Family::Gctf)
}

pub fn address_completions_for(family: crate::parser::ast::Family) -> Vec<CompletionItem> {
    let entries: Vec<(&str, &str)> = if family == crate::parser::ast::Family::Httf {
        vec![
            ("http://localhost:8080", "A local HTTP server"),
            ("https://api.example.com", "A host over TLS"),
        ]
    } else {
        vec![
            ("localhost:4770", "Default gripmock port"),
            ("localhost:50051", "Common gRPC port"),
            ("localhost:9000", "Alternative gRPC port"),
        ]
    };
    entries
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

pub fn get_assertion_completions() -> Vec<CompletionItem> {
    assertion_completions_for(crate::parser::ast::Family::Gctf)
}

pub fn assertion_completions_for(family: crate::parser::ast::Family) -> Vec<CompletionItem> {
    let says_nothing: &[&str] = match family {
        crate::parser::ast::Family::Httf => &["trailer", "has_trailer"],
        crate::parser::ast::Family::Gctf => &["status"],
        crate::parser::ast::Family::Apif => &[],
    };
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

    let mut plugins = crate::execution::plugin_dir::build_plugin_manager().list();
    plugins.sort_by(|a, b| a.name().cmp(b.name()));

    for plugin in plugins {
        let signature = plugin.signature();
        let name = plugin.name().trim_start_matches('@');
        if says_nothing.contains(&name) {
            continue;
        }
        let purity = match signature.purity {
            PluginPurity::Pure => "pure",
            PluginPurity::ContextDependent => "context",
            PluginPurity::Impure => "impure",
        };
        let label = format!("@{}(...)", name);
        let detail = format!("{} [{}]", plugin.description(), purity);
        let detail = if let Some(repl) = signature.replacement {
            format!(
                "{} (deprecated, use @{}) [{}]",
                plugin.description(),
                repl,
                purity
            )
        } else {
            detail
        };
        let deprecated = signature.replacement.map(|_| true);
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail),
            deprecated,
            ..CompletionItem::default()
        });
    }

    items
}

pub fn get_extract_completions() -> Vec<CompletionItem> {
    vec![
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
        ("avg", CompletionItemKind::FUNCTION, "Average of array"),
        ("min", CompletionItemKind::FUNCTION, "Minimum value"),
        ("max", CompletionItemKind::FUNCTION, "Maximum value"),
        ("add", CompletionItemKind::FUNCTION, "Sum of array"),
        (
            "length",
            CompletionItemKind::FUNCTION,
            "Length of array/string",
        ),
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
        ("keys", CompletionItemKind::FUNCTION, "Get keys"),
        ("values", CompletionItemKind::FUNCTION, "Get values"),
        ("del(.field)", CompletionItemKind::FUNCTION, "Delete field"),
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
        ("@base64", CompletionItemKind::FUNCTION, "Base64 encode"),
        ("@base64d", CompletionItemKind::FUNCTION, "Base64 decode"),
        ("@uri", CompletionItemKind::FUNCTION, "URI encode"),
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
    section_key_completions_for(crate::parser::ast::Family::Gctf, section_type)
}

pub fn section_key_completions_for(
    family: crate::parser::ast::Family,
    section_type: &SectionType,
) -> Vec<CompletionItem> {
    if *section_type == SectionType::Bench {
        return bench_keys_canonical_order()
            .into_iter()
            .map(|k| CompletionItem {
                label: format!("{}:", k),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(bench_key_detail(k)),
                insert_text: Some(format!("{}: ", k)),
                ..CompletionItem::default()
            })
            .collect();
    }

    let entries: Vec<(&str, &str)> = match section_type {
        SectionType::Proto => vec![
            ("descriptor", "Path to descriptor set (.desc/.binpb)"),
            ("files", "List of .proto files"),
            ("import_paths", "List of import search paths"),
        ],
        SectionType::Tls => vec![
            ("ca_cert", "CA certificate path"),
            ("client_cert", "Client certificate path"),
            ("client_key", "Client private key path"),
            ("server_name", "TLS SNI server name"),
            ("insecure", "Skip certificate verification"),
        ],
        SectionType::Options => vec![
            ("timeout", "Per-test timeout in seconds"),
            ("retry", "Retry count on failure"),
            ("retry_delay", "Seconds between retries"),
            ("no_retry", "Disable retries for this test"),
            ("compression", "none | gzip"),
            ("protocol", "grpc | grpc-web | connectrpc"),
        ],
        _ => vec![],
    };

    let grpc_only = ["compression", "protocol"];
    entries
        .into_iter()
        .filter(|(label, _)| family.allows_grpc() || !grpc_only.contains(label))
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

pub fn get_variable_completions(
    doc: &crate::parser::GctfDocument,
    current_line_0based: usize,
) -> Vec<CompletionItem> {
    use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

    let mut items = Vec::new();
    let current_doc_idx = find_document_index_at_line(doc, current_line_0based);

    for (doc_idx, d) in doc.iter_chain().enumerate() {
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

pub fn section_index_at_line(sections: &[parser::ast::Section], line: usize) -> Option<usize> {
    sections
        .iter()
        .position(|s| s.start_line <= line && line < s.end_line)
}

fn find_document_index_at_line(doc: &crate::parser::GctfDocument, line_0based: usize) -> usize {
    for (idx, d) in doc.iter_chain().enumerate() {
        if let (Some(first), Some(last)) = (d.sections.first(), d.sections.last())
            && line_0based >= first.start_line
            && line_0based < last.end_line
        {
            return idx;
        }
    }
    0
}

pub fn get_var_hover(
    doc: &crate::parser::GctfDocument,
    line_0based: usize,
    character: u32,
) -> Option<tower_lsp::lsp_types::Hover> {
    use tower_lsp::lsp_types::{Hover, HoverContents, MarkedString};

    let line_str = doc.metadata.source.as_deref()?.lines().nth(line_0based)?;

    let char_pos = crate::lsp::position::utf16_col_to_byte(line_str, character as usize);
    if char_pos >= line_str.len() {
        return None;
    }

    let before = &line_str[..char_pos];
    let open_brace = before.rfind("{{")?;

    let after = &line_str[char_pos..];
    let close_brace = after.find("}}")?;

    let var_content = line_str[open_brace + 2..char_pos + close_brace].trim();
    let var_name = var_content.split_whitespace().next()?;

    let current_doc_idx = find_document_index_at_line(doc, line_0based);

    for (doc_idx, d) in doc.iter_chain().enumerate() {
        if doc_idx >= current_doc_idx {
            break;
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

    let hover_text = format!(
        "**Unknown variable: `{}`**\n\nNo EXTRACT definition found in preceding documents.",
        var_name
    );
    Some(tower_lsp::lsp_types::Hover {
        contents: HoverContents::Scalar(MarkedString::String(hover_text)),
        range: None,
    })
}

pub fn get_plugin_hover(
    doc: &crate::parser::GctfDocument,
    line_0based: usize,
    character: u32,
) -> Option<tower_lsp::lsp_types::Hover> {
    use tower_lsp::lsp_types::{Hover, HoverContents, MarkedString};

    let line_str = doc.metadata.source.as_deref()?.lines().nth(line_0based)?;
    if !line_str.contains('@') {
        return None;
    }

    let col = crate::lsp::position::utf16_col_to_byte(line_str, character as usize);
    let end = line_str[col..]
        .chars()
        .next()
        .map(|c| col + c.len_utf8())
        .unwrap_or(col);
    let at_pos = line_str[..end].rfind('@')?;

    let rest = &line_str[at_pos..];
    let name_end = rest[1..]
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.')
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    let plugin_name = &rest[1..name_end];
    if plugin_name.is_empty() {
        return None;
    }

    let manager = crate::execution::plugin_dir::build_plugin_manager();
    let plugin = manager.get(plugin_name)?;

    let sig = plugin.signature();
    let purity_str = match sig.purity {
        PluginPurity::Pure => "pure",
        PluginPurity::ContextDependent => "context-dependent",
        PluginPurity::Impure => "impure (side effects)",
    };

    let return_type_str = format!("**Return type:** `{}`", sig.return_type.display_name());
    let args_str = if sig.arg_names.is_empty() {
        String::new()
    } else {
        let args: Vec<String> = sig
            .arg_types
            .iter()
            .zip(sig.arg_names.iter())
            .map(|(t, n)| {
                format!(
                    "`{}`: `{}`{}",
                    n,
                    t.expected.display_name(),
                    if t.required { "" } else { " (optional)" }
                )
            })
            .collect();
        format!("\n\n**Arguments:**\n{}", args.join("\n"))
    };

    let deprecation = if let Some(replacement) = &sig.replacement {
        format!("\n\n⚠️ **Deprecated:** Use `@{}` instead.", replacement)
    } else {
        String::new()
    };

    let hover_text = format!(
        "**`@{}(...)`** — {}\n\n{}| **Purity:** {}| **Deterministic:** {}| **Idempotent:** {}{}{}",
        plugin_name,
        plugin.description(),
        return_type_str,
        purity_str,
        sig.deterministic,
        sig.idempotent,
        args_str,
        deprecation,
    );

    Some(Hover {
        contents: HoverContents::Scalar(MarkedString::String(hover_text)),
        range: None,
    })
}

fn token_range_on_line(line_text: &str, line: u32, token: &str) -> Range {
    match line_text.find(token) {
        Some(byte_start) => {
            let start = crate::lsp::position::byte_to_utf16_col(line_text, byte_start) as u32;
            let end =
                crate::lsp::position::byte_to_utf16_col(line_text, byte_start + token.len()) as u32;
            Range::new(Position::new(line, start), Position::new(line, end))
        }
        None => Range::new(
            Position::new(line, 0),
            Position::new(line, line_text.len() as u32),
        ),
    }
}

fn unknown_key_name(message: &str) -> Option<&str> {
    let start = message.find(" key '")? + " key '".len();
    let rest = &message[start..];
    let end = rest.find('\'')?;
    Some(&rest[..end])
}

fn whole_line_range(line: u32, content: &str) -> Range {
    let text = content.lines().nth(line as usize).unwrap_or("");
    Range::new(
        Position::new(line, 0),
        Position::new(line, text.len().max(1) as u32),
    )
}

fn recovered_to_diagnostic(diagnostic: &apif_diagnostics::Diagnostic, content: &str) -> Diagnostic {
    use apif_diagnostics::DiagnosticSeverity as Own;
    let severity = match diagnostic.severity {
        Own::Error => DiagnosticSeverity::ERROR,
        Own::Warning => DiagnosticSeverity::WARNING,
        Own::Information => DiagnosticSeverity::INFORMATION,
        Own::Hint => DiagnosticSeverity::HINT,
    };
    let line = diagnostic.range.start.line as u32;
    let width = content.lines().nth(line as usize).unwrap_or("").len() as u32;
    let start = diagnostic.range.start.column as u32;
    let end = diagnostic.range.end.column as u32;
    let range = if diagnostic.range.end.line as u32 == line && end > start && end <= width {
        Range::new(Position::new(line, start), Position::new(line, end))
    } else {
        whole_line_range(line, content)
    };
    let mut out = Diagnostic {
        range,
        severity: Some(severity),
        message: diagnostic.message.clone(),
        ..Default::default()
    };
    if let Some(replacement) = parse_meta_list_hint(&out.message) {
        out.code = Some(NumberOrString::String("META_LIST_EXPECTED".to_string()));
        out.data = Some(json!({ "replacement": replacement }));
    }
    out
}

fn say_unverified_in_the_editors_voice(diag: &mut Diagnostic, http: bool) {
    if !diag
        .message
        .starts_with("At least one verification section")
    {
        return;
    }
    diag.severity = Some(DiagnosticSeverity::WARNING);
    diag.message = if http {
        "Nothing verifies the answer yet — the run passes as long as the call succeeds. Add RESPONSE or ASSERTS."
    } else {
        "Nothing verifies the answer yet — the run passes as long as the call succeeds. Add RESPONSE, ERROR or ASSERTS."
    }
    .to_string();
}

fn say_the_keys_in_the_editors_voice(diag: &mut Diagnostic) {
    let Some(cut) = diag.message.find(". Supported keys:") else {
        return;
    };
    let section = diag
        .message
        .strip_prefix("Unknown ")
        .and_then(|rest| rest.split_once(" key "))
        .map(|(name, _)| name.to_string())
        .unwrap_or_default();
    let form = if section.is_empty() {
        "the form beside this row".to_string()
    } else {
        format!("the {section} form")
    };
    let hint = diag
        .message
        .find(" Hint: ")
        .map(|at| diag.message[at..].to_string())
        .unwrap_or_default();
    diag.message = format!(
        "{} — {form} lists what it takes.{hint}",
        &diag.message[..cut]
    );
}

pub fn validation_error_to_diagnostic(
    error: &crate::parser::validator::ValidationError,
    content: &str,
) -> Diagnostic {
    let severity = match error.severity {
        crate::parser::validator::ErrorSeverity::Error => DiagnosticSeverity::ERROR,
        crate::parser::validator::ErrorSeverity::Warning => DiagnosticSeverity::WARNING,
        crate::parser::validator::ErrorSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    let line_num = error.line.unwrap_or(0) as u32;
    let line_text = content.lines().nth(line_num as usize).unwrap_or("");

    let range = match unknown_key_name(&error.message) {
        Some(key) => token_range_on_line(line_text, line_num, key),
        None => Range::new(
            Position::new(line_num, 0),
            Position::new(line_num, line_text.len() as u32),
        ),
    };

    let mut diagnostic = Diagnostic::new(
        range,
        Some(severity),
        None,
        None,
        error.message.clone(),
        None,
        None,
    );

    if let Some((unknown_key, suggested_key)) = parse_unknown_bench_key_hint(&error.message) {
        diagnostic.code = Some(NumberOrString::String("BENCH_UNKNOWN_KEY".to_string()));
        if suggested_key.contains(':') {
            diagnostic.range = whole_line_range(line_num, content);
            diagnostic.data = Some(json!({
                "unknown_key": unknown_key,
                "replacement": suggested_key,
            }));
        } else {
            diagnostic.data = Some(json!({
                "unknown_key": unknown_key,
                "suggested_key": suggested_key,
            }));
        }
    } else if let Some(replacement) = parse_meta_list_hint(&error.message) {
        diagnostic.code = Some(NumberOrString::String("META_LIST_EXPECTED".to_string()));
        diagnostic.data = Some(json!({ "replacement": replacement }));
    } else if let Some((unknown_key, suggested_key)) = parse_deprecated_key_hint(&error.message) {
        diagnostic.code = Some(NumberOrString::String(
            "DEPRECATED_KEY_SPELLING".to_string(),
        ));
        diagnostic.data = Some(json!({
            "unknown_key": unknown_key,
            "suggested_key": suggested_key,
        }));
    }

    if error.line.is_none() {
        let mut data = diagnostic.data.take().unwrap_or_else(|| json!({}));
        if let Some(map) = data.as_object_mut() {
            map.insert("scope".to_string(), json!("file"));
        }
        diagnostic.data = Some(data);
    }

    diagnostic
}

fn parse_meta_list_hint(message: &str) -> Option<String> {
    if !message.starts_with("META ") || !message.contains("is a list, not a line") {
        return None;
    }
    let start = message.find("write `")? + "write `".len();
    let rest = &message[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn parse_unknown_bench_key_hint(message: &str) -> Option<(String, String)> {
    let prefix = "Unknown BENCH key '";
    let start = message.find(prefix)? + prefix.len();
    let tail = &message[start..];
    let end = tail.find('\'')?;
    let unknown = tail[..end].to_string();

    let hint_prefix = "did you mean '";
    let hint_start = message.find(hint_prefix)? + hint_prefix.len();
    let hint_tail = &message[hint_start..];
    let hint_end = hint_tail.find('\'')?;
    let suggested = hint_tail[..hint_end].to_string();

    Some((unknown, suggested))
}

fn parse_deprecated_key_hint(message: &str) -> Option<(String, String)> {
    let (lhs, rhs) = message.split_once(" is deprecated; prefer ")?;
    let strip = |s: &str| {
        s.trim_start_matches("OPTIONS.")
            .trim_start_matches("Attribute #[")
            .trim_start_matches("#[")
            .trim_end_matches(']')
            .to_string()
    };
    Some((strip(lhs), strip(rhs)))
}

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

pub fn create_bench_key_fix_action(
    uri: &Url,
    range: Range,
    unknown_key: &str,
    suggested_key: &str,
    content: &str,
) -> Option<CodeAction> {
    let line_idx = range.start.line as usize;
    let line = content.lines().nth(line_idx)?;
    let byte_col = line.find(unknown_key)?;
    let start_char = line[..byte_col].chars().count() as u32;
    let end_char = start_char + unknown_key.chars().count() as u32;
    let start = Position::new(range.start.line, start_char);
    let end = Position::new(range.start.line, end_char);

    Some(CodeAction {
        title: format!("Replace '{}' with '{}'", unknown_key, suggested_key),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri.clone(),
                vec![TextEdit::new(
                    Range::new(start, end),
                    suggested_key.to_string(),
                )],
            )])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    })
}

fn check_optimizer_sentence(editor_message: &str) -> String {
    editor_message
        .strip_prefix("Optimizer hint: ")
        .unwrap_or(editor_message)
        .replace(" -> ", " → ")
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
    let hints =
        crate::optimizer::collect_assertion_optimizations(doc, optimizer::OptimizeLevel::Safe);
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
            hint.rule_id.to_string(),
            hint.before,
        ));
    }

    rewrites
}

#[derive(Debug, Clone)]
pub struct UnusedVariable {
    pub name: String,
    pub has_later_steps: bool,
    pub line: usize,
    pub character: usize,
    pub doc_index: usize,
}

pub fn collect_unused_variables(doc: &crate::parser::GctfDocument) -> Vec<UnusedVariable> {
    let defined_vars = extract_all_vars(doc);

    defined_vars
        .into_iter()
        .filter(|(def_doc_idx, var_name, _, _)| !is_var_read(doc, *def_doc_idx, var_name))
        .map(|(doc_idx, name, line, character)| UnusedVariable {
            name,
            has_later_steps: doc_idx + 1 < doc.document_count(),
            line,
            character,
            doc_index: doc_idx,
        })
        .collect()
}

fn bound_name(written: &str) -> String {
    let trimmed = written.trim();
    match trimmed.rsplit_once(':') {
        Some((name, kind))
            if !kind.is_empty() && kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            name.trim().to_string()
        }
        _ => trimmed.to_string(),
    }
}

fn extract_all_vars(doc: &crate::parser::GctfDocument) -> Vec<(usize, String, usize, usize)> {
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
                        let name = bound_name(&extract_var.name);
                        let global_line = section.start_line + local_line + 1;
                        let char_pos = raw_line.find(&name).unwrap_or(0);
                        vars.push((doc_idx, name, global_line, char_pos));
                    }
                }

                for var_name in extractions.keys() {
                    let already_present = vars.iter().any(|(_, n, _, _)| n == var_name);
                    if !already_present {
                        for (local_line, raw_line) in section.raw_content.lines().enumerate() {
                            if raw_line.trim().starts_with(var_name) {
                                let global_line = section.start_line + local_line + 1;
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

fn is_var_read(doc: &crate::parser::GctfDocument, def_doc_idx: usize, var_name: &str) -> bool {
    for (doc_idx, curr_doc) in doc.iter_chain().enumerate() {
        if doc_idx == def_doc_idx {
            if doc_contains_var_reference_excluding_extract(curr_doc, var_name) {
                return true;
            }
            continue;
        }
        if doc_idx > def_doc_idx && doc_contains_var_reference(curr_doc, var_name) {
            return true;
        }
    }
    false
}

fn doc_contains_var_reference(doc: &crate::parser::GctfDocument, var_name: &str) -> bool {
    for section in &doc.sections {
        if section_contains_var_reference(section, var_name) {
            return true;
        }
    }
    false
}

fn section_contains_var_reference(section: &crate::parser::ast::Section, var_name: &str) -> bool {
    match &section.content {
        parser::ast::SectionContent::Json(value) => json_contains_var(value, var_name),
        parser::ast::SectionContent::JsonLines(values) => {
            values.iter().any(|v| json_contains_var(v, var_name))
        }
        parser::ast::SectionContent::KeyValues(kv) => {
            kv.values().any(|v| contains_var_pattern(v, var_name))
        }
        parser::ast::SectionContent::Extract(_) => false,
        parser::ast::SectionContent::Assertions(asserts) => {
            asserts.iter().any(|a| contains_assert_var_ref(a, var_name))
        }
        parser::ast::SectionContent::Single(s) => contains_var_pattern(s, var_name),
        parser::ast::SectionContent::Rows(rows) => {
            rows.iter().any(|v| json_contains_var(v, var_name))
        }
        parser::ast::SectionContent::Empty => false,
        parser::ast::SectionContent::Meta(_) => false,
    }
}

fn json_contains_var(value: &serde_json::Value, var_name: &str) -> bool {
    match value {
        serde_json::Value::String(s) => contains_var_pattern(s, var_name),
        serde_json::Value::Object(map) => map.values().any(|v| json_contains_var(v, var_name)),
        serde_json::Value::Array(arr) => arr.iter().any(|v| json_contains_var(v, var_name)),
        _ => false,
    }
}

fn contains_var_pattern(s: &str, var_name: &str) -> bool {
    let patterns = [
        format!("{{{{ {} }}}}", var_name),
        format!("{{{{{} }}}}", var_name),
        format!("{{{{ {}}}}}", var_name),
        format!("{{{{{}}}}}", var_name),
    ];
    patterns.iter().any(|p| s.contains(p))
}

fn contains_assert_var_ref(assertion: &str, var_name: &str) -> bool {
    let pattern = format!("${}", var_name);
    assertion.contains(&pattern)
}

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

pub fn unused_variable_message(var: &UnusedVariable) -> String {
    if var.has_later_steps {
        format!(
            "Variable '{}' is extracted but no later step reads it",
            var.name
        )
    } else {
        format!(
            "Variable '{}' is extracted but nothing reads it — this step's ASSERTS can, or a step after it",
            var.name
        )
    }
}

pub fn unused_variable_to_diagnostic(var: &UnusedVariable) -> Diagnostic {
    let lsp_line = var.line as u32;
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
        message: unused_variable_message(var),
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

    let workflow = crate::execution::Workflow::from_document_with_analysis(doc);

    for event in &workflow.events {
        if let crate::execution::WorkflowEvent::SemanticAnalysis {
            type_mismatches,
            unknown_plugins,
        } = event
        {
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

fn collect_assertion_style_diagnostics(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    use apif_semantics as semantics;

    let lines: Vec<&str> = content.lines().collect();
    let at = |line: usize, expression: &str| {
        let lsp_line = line.saturating_sub(1) as u32;
        let text = lines.get(lsp_line as usize).copied().unwrap_or("");
        if expression.is_empty() || !text.contains(expression) {
            whole_line_range(lsp_line, content)
        } else {
            token_range_on_line(text, lsp_line, expression)
        }
    };
    let warn = |range: Range, rule_id: &str, message: String| Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(rule_id.to_string())),
        source: Some("grpctestify-semantics".to_string()),
        message,
        ..Diagnostic::default()
    };

    let mut out = Vec::new();
    for found in semantics::collect_deprecated_plugin_calls(doc) {
        out.push(warn(
            at(found.line, &found.expression),
            &found.rule_id,
            format!(
                "{} — `fmt --write` rewrites it to `{}`",
                found.message, found.replacement
            ),
        ));
    }
    for found in semantics::collect_constant_assertions(doc) {
        out.push(warn(
            at(found.line, &found.expression),
            &found.rule_id,
            found.message,
        ));
    }
    for found in semantics::collect_duplicate_assertions(doc) {
        out.push(warn(
            at(found.line, &found.expression),
            &found.rule_id,
            found.message,
        ));
    }
    for found in semantics::collect_redundant_response_assertions(doc) {
        out.push(warn(
            at(found.line, &found.expression),
            &found.rule_id,
            found.message,
        ));
    }
    out
}

pub fn line_of_cause(content: &str, cause: &str) -> Option<usize> {
    let after = cause.split("inline option: ").nth(1)?;
    let key = after
        .split([' ', '\n'])
        .next()
        .map(|k| k.trim_end_matches(['—', ':', ',']))
        .filter(|k| !k.is_empty())?;
    content.lines().position(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("---")
            && trimmed.ends_with("---")
            && trimmed.split_whitespace().any(|token| token == key)
    })
}

pub fn collect_insecure_tls_diagnostics(
    document: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    use crate::parser::ast::{SectionContent, SectionType};

    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    for doc in document.iter_chain() {
        for section in doc.sections_by_type(SectionType::Tls) {
            let SectionContent::KeyValues(pairs) = &section.content else {
                continue;
            };
            let skips = pairs
                .get("insecure")
                .is_some_and(|v| crate::execution::runner_helpers::parse_bool_flag(v));
            if !skips {
                continue;
            }
            let at = lines
                .iter()
                .position(|line| {
                    let trimmed = line.trim_start().to_ascii_lowercase();
                    trimmed.starts_with("insecure") && trimmed.contains(':')
                })
                .unwrap_or(0) as u32;
            out.push(Diagnostic::new(
                whole_line_range(at, content),
                Some(DiagnosticSeverity::WARNING),
                Some(NumberOrString::String("TLS_VERIFICATION_SKIPPED".into())),
                Some("grpctestify".to_string()),
                "TLS certificate verification is skipped for this file — every run of it, here and in CI"
                    .to_string(),
                None,
                None,
            ));
        }
    }
    out
}

pub fn collect_half_identity_diagnostics(
    document: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    use crate::parser::ast::{SectionContent, SectionType};

    const CERT: [&str; 3] = ["client_cert", "cert", "cert_file"];
    const KEY: [&str; 3] = ["client_key", "key", "key_file"];

    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    for doc in document.iter_chain() {
        for section in doc.sections_by_type(SectionType::Tls) {
            let SectionContent::KeyValues(pairs) = &section.content else {
                continue;
            };
            let named = |keys: &[&str]| {
                keys.iter()
                    .find_map(|k| pairs.get(*k))
                    .map(|v| v.trim())
                    .is_some_and(|v| !v.is_empty())
            };
            let has_cert = named(&CERT);
            let has_key = named(&KEY);
            if has_cert == has_key {
                continue;
            }
            let written = if has_cert { CERT } else { KEY };
            let at = lines
                .iter()
                .position(|line| {
                    let trimmed = line.trim_start().to_ascii_lowercase();
                    written
                        .iter()
                        .any(|k| trimmed.starts_with(k) && trimmed.contains(':'))
                })
                .unwrap_or(0) as u32;
            let missing = if has_cert {
                "client_key"
            } else {
                "client_cert"
            };
            out.push(Diagnostic::new(
                whole_line_range(at, content),
                Some(DiagnosticSeverity::WARNING),
                Some(NumberOrString::String("TLS_CLIENT_IDENTITY_INCOMPLETE".into())),
                Some("grpctestify".to_string()),
                format!(
                    "A client identity needs both halves — `{missing}` is missing, so a gRPC call dials with no identity at all"
                ),
                None,
                None,
            ));
        }
    }
    out
}

pub fn collect_group_race_diagnostics(document: &crate::parser::GctfDocument) -> Vec<Diagnostic> {
    let steps: Vec<&crate::parser::GctfDocument> = document.iter_chain().collect();
    let mut out = Vec::new();
    let mut start = 0;

    while start < steps.len() {
        if !steps[start].runs_in_parallel() {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < steps.len() && steps[end].runs_in_parallel() {
            end += 1;
        }

        for reader in start..end {
            for binder in start..end {
                if binder == reader {
                    continue;
                }
                for name in bound_names(steps[binder]) {
                    if steps[..start]
                        .iter()
                        .any(|s| bound_names(s).contains(&name))
                    {
                        continue;
                    }
                    let Some((line, character)) = reference_at(steps[reader], &name) else {
                        continue;
                    };
                    let mut diag = Diagnostic::new(
                        Range::new(
                            Position::new(line as u32, character as u32),
                            Position::new(line as u32, (character + name.len() + 4) as u32),
                        ),
                        Some(DiagnosticSeverity::ERROR),
                        None,
                        Some("grpctestify".to_string()),
                        format!(
                            "{{{{{name}}}}} is bound by another step of this parallel group — the steps of a group go out together, so nothing they bind is there for each other. Move this step out of the group, or bind {name} before it."
                        ),
                        None,
                        None,
                    );
                    diag.code = Some(NumberOrString::String("PARALLEL_RACE".to_string()));
                    out.push(diag);
                }
            }
        }
        start = end;
    }

    out
}

fn bound_names(step: &crate::parser::GctfDocument) -> Vec<String> {
    step.sections
        .iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .filter_map(|s| match &s.content {
            parser::ast::SectionContent::Extract(extractions) => {
                Some(extractions.keys().cloned().collect::<Vec<_>>())
            }
            _ => None,
        })
        .flatten()
        .collect()
}

fn reference_at(step: &crate::parser::GctfDocument, name: &str) -> Option<(usize, usize)> {
    for section in &step.sections {
        if section.section_type == SectionType::Extract {
            continue;
        }
        for (offset, line) in section.raw_content.lines().enumerate() {
            let mut from = 0;
            while let Some(open) = line[from..].find("{{") {
                let at = from + open;
                let rest = &line[at + 2..];
                let Some(close) = rest.find("}}") else { break };
                if rest[..close].trim() == name {
                    return Some((section.start_line + offset + 1, at));
                }
                from = at + 2 + close;
            }
        }
    }
    None
}

pub fn collect_placeholder_diagnostics(content: &str) -> Vec<Diagnostic> {
    let extracted = extracted_names(content);
    let mut diagnostics = Vec::new();
    let mut section: Option<&str> = None;

    for (i, line) in content.lines().enumerate() {
        if let Some(name) = section_header(line) {
            let head = name.split_whitespace().next().unwrap_or(name);
            section = match head {
                "ASSERTS" => Some("ASSERTS"),
                "EXTRACT" => Some("EXTRACT"),
                "TLS" => Some("TLS"),
                "PROTO" => Some("PROTO"),
                _ => None,
            };
            continue;
        }
        let Some(where_) = section else {
            continue;
        };
        let Some(start) = line.find("{{") else {
            continue;
        };
        let Some(end) = line[start..].find("}}").map(|at| start + at + 2) else {
            continue;
        };
        let name = line[start + 2..end - 2].trim().to_string();
        let remedy = if where_ == "TLS" || where_ == "PROTO" {
            " — this section is read as paths, from the directory of the file that names them"
                .to_string()
        } else if extracted.contains(&name) {
            format!(" — the file extracts '{name}', which is read as ${name}")
        } else if name.starts_with("dataset.") {
            " — a row's value is written into REQUEST or RESPONSE, which are substituted"
                .to_string()
        } else {
            String::new()
        };
        diagnostics.push(Diagnostic {
            range: Range::new(
                Position::new(i as u32, start as u32),
                Position::new(i as u32, end as u32),
            ),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(
                "UNSUBSTITUTED_PLACEHOLDER".to_string(),
            )),
            source: Some("grpctestify".to_string()),
            message: format!(
                "{{{{{name}}}}} is not substituted in {where_}: it is used as written{remedy}"
            ),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

fn section_header(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    trimmed
        .strip_prefix("---")
        .and_then(|rest| rest.strip_suffix("---"))
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn extracted_names(content: &str) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut in_extract = false;
    for line in content.lines() {
        if let Some(name) = section_header(line) {
            in_extract = name == "EXTRACT";
            continue;
        }
        if !in_extract {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && !name.starts_with('#') {
                names.insert(name.to_string());
            }
        }
    }
    names
}

pub fn collect_sources_diagnostics(
    doc: &crate::parser::GctfDocument,
    content: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    use crate::parser::ast::{SectionContent, SectionType};

    for section in &doc.sections {
        if section.section_type != SectionType::Bench {
            continue;
        }

        let SectionContent::KeyValues(kv) = &section.content else {
            continue;
        };

        let Some(sources_yaml) = kv.get("sources") else {
            continue;
        };

        if sources_yaml.trim().is_empty() {
            continue;
        }

        match serde_yaml_ng::from_str::<Vec<crate::bench::sources::SourceDefinition>>(sources_yaml)
        {
            Ok(defs) => {
                for def in &defs {
                    if def.file.is_empty()
                        && let Some(line_idx) =
                            find_line_with_key("sources", &lines, section.start_line)
                    {
                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new(line_idx as u32, 0),
                                Position::new(line_idx as u32, sources_yaml.len() as u32),
                            ),
                            severity: Some(DiagnosticSeverity::ERROR),
                            code: Some(NumberOrString::String("SRC001".to_string())),
                            source: Some("grpctestify-sources".to_string()),
                            message: "source 'file' is required".to_string(),
                            ..Diagnostic::default()
                        });
                    }
                }
            }
            Err(e) => {
                if let Some(line_idx) = find_line_with_key("sources", &lines, section.start_line) {
                    diagnostics.push(Diagnostic {
                        range: Range::new(
                            Position::new(line_idx as u32, 0),
                            Position::new(line_idx as u32, sources_yaml.len().min(100) as u32),
                        ),
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: Some(NumberOrString::String("SRC000".to_string())),
                        source: Some("grpctestify-sources".to_string()),
                        message: format!("invalid sources: {}", e),
                        ..Diagnostic::default()
                    });
                }
            }
        }
    }

    diagnostics
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Editor,
    Check,
}

pub fn preamble_section_order(doc: &crate::parser::GctfDocument) -> Vec<(usize, String, String)> {
    let first_body_idx = doc
        .sections
        .iter()
        .position(|s| s.section_type.preamble_rank().is_none())
        .unwrap_or(doc.sections.len());

    let preamble: Vec<(usize, &'static str, usize)> = doc.sections[..first_body_idx]
        .iter()
        .filter_map(|s| {
            Some((
                s.start_line,
                s.section_type.as_str(),
                s.section_type.preamble_rank()?,
            ))
        })
        .collect();

    let mut out = Vec::new();
    for window in preamble.windows(2) {
        let [(_, prev_name, prev_rank), (curr_line, curr_name, curr_rank)] = window else {
            continue;
        };
        if curr_rank >= prev_rank {
            continue;
        }
        out.push((
            curr_line + 1,
            format!(
                "Section order: {curr_name} should come before {prev_name} (canonical: META→BENCH→DATASET→ADDRESS→ENDPOINT→TLS→PROTO→OPTIONS)",
            ),
            "run `fmt --write` to reorder preamble sections into canonical order".to_string(),
        ));
    }
    out
}

pub fn collect_all_diagnostics(content: &str, file_name: &str) -> Vec<Diagnostic> {
    collect_all_diagnostics_in(content, file_name, Voice::Editor)
}

pub fn collect_all_diagnostics_in(content: &str, file_name: &str, voice: Voice) -> Vec<Diagnostic> {
    let document = match parser::parse_gctf_from_str(content, file_name) {
        Ok(d) => d,
        Err(e) => {
            let recovered =
                crate::parser::error_recovery::parse_content_with_recovery(content, file_name);
            let cause = e.to_string();
            let positioned: Vec<Diagnostic> = recovered
                .diagnostics
                .diagnostics
                .iter()
                .map(|d| {
                    let mut diag = recovered_to_diagnostic(d, content);
                    if cause.contains(&d.message) {
                        diag.severity = Some(DiagnosticSeverity::ERROR);
                    }
                    diag
                })
                .collect();
            if !positioned.is_empty() {
                return positioned;
            }
            let line = line_of_cause(content, &cause).unwrap_or(0);
            return vec![Diagnostic::new_simple(
                whole_line_range(line as u32, content),
                format!("Parse error: {}", e),
            )];
        }
    };

    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut address_seen = false;
    let mut missing_said = false;
    for (doc_idx, d) in document.iter_chain().enumerate() {
        let doc_label = if document.is_single_document() {
            None
        } else {
            Some(doc_idx + 1)
        };

        let mut errors = crate::parser::validator::validate_document_diagnostics(d);
        let has_own_address = d.get_address(None).is_some();
        if address_seen || missing_said {
            errors.retain(|e| !e.message.starts_with("ADDRESS section missing"));
        } else {
            missing_said = errors
                .iter()
                .any(|e| e.message.starts_with("ADDRESS section missing"));
        }
        address_seen = address_seen || has_own_address;
        for e in &errors {
            let mut diag = validation_error_to_diagnostic(e, content);
            if voice != Voice::Check {
                say_unverified_in_the_editors_voice(
                    &mut diag,
                    d.transport() == crate::parser::ast::Transport::Http,
                );
            }
            say_the_keys_in_the_editors_voice(&mut diag);
            if let Some(n) = doc_label {
                diag.message = format!("Document {}: {}", n, diag.message);
            }
            diags.push(diag);
        }

        let single = d.detached();

        let mut semantic_diags = collect_semantic_diagnostics(&single, content);
        for diag in &mut semantic_diags {
            if let Some(n) = doc_label {
                diag.message = format!("Document {}: {}", n, diag.message);
            }
        }

        let mut opt_diags = collect_optimizer_diagnostics(&single, content);
        if voice == Voice::Check {
            for diag in &mut opt_diags {
                diag.severity = Some(DiagnosticSeverity::WARNING);
                diag.message = check_optimizer_sentence(&diag.message);
            }
        }

        let r001_lines: std::collections::HashSet<u32> = opt_diags
            .iter()
            .filter(|diag| diag.code == Some(NumberOrString::String("OPT_R001".into())))
            .map(|diag| diag.range.start.line)
            .collect();
        semantic_diags.retain(|diag| {
            !(diag.code == Some(NumberOrString::String("SEM_D001".into()))
                && r001_lines.contains(&diag.range.start.line))
        });

        diags.extend(semantic_diags);
        diags.extend(opt_diags);

        let mut sources_diags = collect_sources_diagnostics(d, content);
        for diag in &mut sources_diags {
            if let Some(n) = doc_label {
                diag.message = format!("Document {}: {}", n, diag.message);
            }
        }
        diags.extend(sources_diags);
    }

    for unused_var in collect_unused_variables(&document) {
        diags.push(unused_variable_to_diagnostic(&unused_var));
    }

    diags.extend(collect_placeholder_diagnostics(content));
    diags.extend(collect_group_race_diagnostics(&document));
    diags.extend(collect_insecure_tls_diagnostics(&document, content));
    diags.extend(collect_half_identity_diagnostics(&document, content));

    diags.extend(collect_deprecated_diagnostics(content));

    diags.extend(collect_wrong_family_plugin_diagnostics(content, file_name));
    diags.extend(collect_unhonoured_attribute_diagnostics(content, file_name));

    diags.extend(collect_assertion_style_diagnostics(&document, content));

    for (line, message, hint) in preamble_section_order(&document) {
        let at = line.saturating_sub(1) as u32;
        diags.push(Diagnostic::new(
            whole_line_range(at, content),
            Some(DiagnosticSeverity::WARNING),
            Some(NumberOrString::String("SECTION_ORDER".into())),
            Some("grpctestify".to_string()),
            format!("{message} — {hint}"),
            None,
            None,
        ));
    }

    diags
}

pub fn collect_unhonoured_attribute_diagnostics(content: &str, file_name: &str) -> Vec<Diagnostic> {
    if crate::parser::ast::Family::of(file_name) != crate::parser::ast::Family::Httf {
        return Vec::new();
    }

    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("#[") || !trimmed.contains("repeat") {
            continue;
        }
        let mut diag = Diagnostic::new(
            token_range_on_line(line, i as u32, "#[repeat"),
            Some(DiagnosticSeverity::WARNING),
            None,
            Some("grpctestify".to_string()),
            "#[repeat] is not honoured for an HTTP file — one document is one request".to_string(),
            None,
            None,
        );
        diag.code = Some(NumberOrString::String("ATTRIBUTE_NOT_HONOURED".to_string()));
        out.push(diag);
    }
    out
}

pub fn collect_wrong_family_plugin_diagnostics(content: &str, file_name: &str) -> Vec<Diagnostic> {
    let says_nothing: &[(&str, &str)] = match crate::parser::ast::Family::of(file_name) {
        crate::parser::ast::Family::Httf => &[
            ("@trailer(", "an HTTP answer has no trailers"),
            ("@has_trailer(", "an HTTP answer has no trailers"),
        ],
        crate::parser::ast::Family::Gctf => &[("@status(", "a gRPC answer carries no HTTP status")],
        crate::parser::ast::Family::Apif => &[],
    };

    let mut out = Vec::new();
    let mut in_asserts = false;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("---") {
            in_asserts = trimmed
                .trim_matches('-')
                .trim()
                .eq_ignore_ascii_case("ASSERTS");
            continue;
        }
        if !in_asserts || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        for (token, why) in says_nothing {
            if !line.contains(token) {
                continue;
            }
            let mut diag = Diagnostic::new(
                token_range_on_line(line, i as u32, token.trim_end_matches('(')),
                Some(DiagnosticSeverity::WARNING),
                None,
                Some("grpctestify".to_string()),
                format!(
                    "{}() reads nothing here — {why}, so this assertion compares against null",
                    token.trim_end_matches('(')
                ),
                None,
                None,
            );
            diag.code = Some(NumberOrString::String("PLUGIN_WRONG_FAMILY".to_string()));
            out.push(diag);
        }
    }
    out
}

fn collect_deprecated_diagnostics(content: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = content.lines().collect();
    parser::detect_deprecations(&parser::tokenize_gctf(content))
        .into_iter()
        .map(|dep| {
            let line = dep.range.start.line as u32;
            let line_text = lines.get(line as usize).copied().unwrap_or("");

            let kebab = parse_deprecated_key_hint(&dep.message);
            let highlight = kebab
                .as_ref()
                .map(|(unknown, _)| unknown.as_str())
                .unwrap_or("HEADERS");
            let range = token_range_on_line(line_text, line, highlight);

            let mut diag = Diagnostic::new(
                range,
                Some(DiagnosticSeverity::WARNING),
                None,
                Some("grpctestify".to_string()),
                dep.message.clone(),
                None,
                None,
            );
            if let Some((unknown_key, suggested_key)) = kebab {
                diag.code = Some(NumberOrString::String(
                    "DEPRECATED_KEY_SPELLING".to_string(),
                ));
                diag.data = Some(json!({
                    "unknown_key": unknown_key,
                    "suggested_key": suggested_key,
                }));
            } else {
                diag.code = Some(NumberOrString::String("DEPRECATED_SECTION".to_string()));
            }
            diag
        })
        .collect()
}

fn find_line_with_key(key: &str, lines: &[&str], start_line: usize) -> Option<usize> {
    for (i, line) in lines.iter().enumerate().skip(start_line.saturating_sub(1)) {
        if line.contains(key) && line.contains(':') {
            return Some(i);
        }
        if i > start_line + 20 {
            break;
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizer::rule_ids;

    fn parse(content: &str) -> crate::parser::GctfDocument {
        crate::parser::parse_gctf_from_str(content, "<test>").expect("parse")
    }

    #[test]
    fn a_file_that_skips_verification_is_said_so_once() {
        let src = "--- ADDRESS ---\napi.internal:443\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\ninsecure: true\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n";
        let doc = crate::parser::parse_gctf_from_str(src, "t.gctf").expect("parse");
        let found = collect_insecure_tls_diagnostics(&doc, src);
        assert_eq!(found.len(), 1, "one file, one word, one warning");
        assert_eq!(
            found[0].code,
            Some(NumberOrString::String("TLS_VERIFICATION_SKIPPED".into())),
        );
        assert_eq!(found[0].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(found[0].range.start.line, 7, "the line the word is on");
    }

    #[test]
    fn a_file_that_verifies_says_nothing() {
        for src in [
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\nca_cert: /etc/ca.pem\n\n--- ASSERTS ---\n.ok\n",
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\ninsecure: false\n\n--- ASSERTS ---\n.ok\n",
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- ASSERTS ---\n.ok\n",
        ] {
            let doc = crate::parser::parse_gctf_from_str(src, "t.gctf").expect("parse");
            assert!(
                collect_insecure_tls_diagnostics(&doc, src).is_empty(),
                "{src}"
            );
        }
    }

    #[test]
    fn the_form_named_is_the_one_the_message_is_about() {
        let mut diag = Diagnostic::new_simple(
            whole_line_range(0, "x"),
            "Unknown TLS key 'ca_cret'. Supported keys: ca_cert, client_cert. Hint: did you mean 'ca_cert'?".to_string(),
        );
        say_the_keys_in_the_editors_voice(&mut diag);
        assert_eq!(
            diag.message,
            "Unknown TLS key 'ca_cret' — the TLS form lists what it takes. Hint: did you mean 'ca_cert'?"
        );
    }

    #[test]
    fn the_editor_says_the_key_without_the_dictionary() {
        let mut diag = Diagnostic::new_simple(
            whole_line_range(0, "x"),
            "Unknown BENCH key 'p95_ms'. Supported keys: concurrency, duration, max_rps Hint: did you mean 'thresholds.latency_ms.p(95)' ?".to_string(),
        );
        say_the_keys_in_the_editors_voice(&mut diag);
        assert_eq!(
            diag.message,
            "Unknown BENCH key 'p95_ms' — the BENCH form lists what it takes. Hint: did you mean 'thresholds.latency_ms.p(95)' ?"
        );
    }

    #[test]
    fn nothing_else_is_reworded() {
        let mut diag = Diagnostic::new_simple(
            whole_line_range(0, "x"),
            "ADDRESS section missing".to_string(),
        );
        say_the_keys_in_the_editors_voice(&mut diag);
        assert_eq!(diag.message, "ADDRESS section missing");
    }

    #[test]
    fn a_meta_list_message_carries_the_line_it_names() {
        assert_eq!(
            parse_meta_list_hint(
                "META tags is a list, not a line — write `tags: [smoke, billing]`, or one `- smoke` per line"
            )
            .as_deref(),
            Some("tags: [smoke, billing]")
        );
        assert!(parse_meta_list_hint("Invalid META: something else").is_none());
    }

    #[test]
    fn half_a_client_identity_is_reported() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\nca_cert: /etc/ca.pem\nclient_cert: /etc/client.pem\n\n--- ASSERTS ---\n.ok\n";
        let doc = crate::parser::parse_gctf_from_str(src, "t.gctf").expect("parse");
        let found = collect_half_identity_diagnostics(&doc, src);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].code,
            Some(NumberOrString::String(
                "TLS_CLIENT_IDENTITY_INCOMPLETE".into()
            )),
        );
        assert!(
            found[0].message.contains("client_key"),
            "{}",
            found[0].message
        );
        assert_eq!(found[0].range.start.line, 5, "the half that is written");
    }

    #[test]
    fn the_missing_half_is_named_either_way() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\nkey_file: /etc/client.key\n\n--- ASSERTS ---\n.ok\n";
        let doc = crate::parser::parse_gctf_from_str(src, "t.gctf").expect("parse");
        let found = collect_half_identity_diagnostics(&doc, src);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].message.contains("client_cert"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn a_whole_identity_says_nothing() {
        for src in [
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\nclient_cert: /c.pem\nclient_key: /c.key\n\n--- ASSERTS ---\n.ok\n",
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\ncert_file: /c.pem\nkey_file: /c.key\n\n--- ASSERTS ---\n.ok\n",
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\nca_cert: /ca.pem\n\n--- ASSERTS ---\n.ok\n",
        ] {
            let doc = crate::parser::parse_gctf_from_str(src, "t.gctf").expect("parse");
            assert!(
                collect_half_identity_diagnostics(&doc, src).is_empty(),
                "{src}"
            );
        }
    }

    #[test]
    fn an_extraction_nobody_reads_is_reported_for_the_shape_of_the_file() {
        let doc = parse(
            "--- ENDPOINT ---\nsvc.S/M\n\n--- REQUEST ---\n{}\n\n\
             --- EXTRACT ---\ntoken = .token\n\n--- ASSERTS ---\n.ok == true\n",
        );
        let unused = collect_unused_variables(&doc);
        assert_eq!(unused.len(), 1, "{unused:?}");
        assert!(!unused[0].has_later_steps);
        let message = unused_variable_message(&unused[0]);
        assert!(message.contains("nothing reads it"), "{message}");
        assert!(message.contains("ASSERTS"), "{message}");
        assert!(!message.contains("subsequent"), "{message}");
    }

    #[test]
    fn a_step_that_reads_its_own_extraction_is_not_unused() {
        let doc = parse(
            "--- ENDPOINT ---\nsvc.S/First\n\n--- REQUEST ---\n{}\n\n\
             --- EXTRACT ---\ntoken = .token\n\n\
             --- ASSERTS ---\n.token == $token\n\n\
             --- ENDPOINT ---\nsvc.S/Second\n\n--- REQUEST ---\n{}\n\n\
             --- ASSERTS ---\n.ok == true\n",
        );
        assert!(
            collect_unused_variables(&doc).is_empty(),
            "read by its own step"
        );
    }

    #[test]
    fn a_chain_step_whose_extraction_nobody_reads_says_so_as_a_chain() {
        let doc = parse(
            "--- ENDPOINT ---\nsvc.S/First\n\n--- REQUEST ---\n{}\n\n\
             --- EXTRACT ---\ntoken = .token\n\n--- ASSERTS ---\n.ok == true\n\n\
             --- ENDPOINT ---\nsvc.S/Second\n\n--- REQUEST ---\n{}\n\n\
             --- ASSERTS ---\n.ok == true\n",
        );
        let unused = collect_unused_variables(&doc);
        assert_eq!(unused.len(), 1, "{unused:?}");
        assert!(unused[0].has_later_steps);
        assert!(
            unused_variable_message(&unused[0]).contains("no later step reads it"),
            "{}",
            unused_variable_message(&unused[0])
        );
    }

    #[test]
    fn an_extraction_a_later_step_reads_is_not_reported() {
        let doc = parse(
            "--- ENDPOINT ---\nsvc.S/First\n\n--- REQUEST ---\n{}\n\n\
             --- EXTRACT ---\ntoken = .token\n\n--- ASSERTS ---\n.ok == true\n\n\
             --- ENDPOINT ---\nsvc.S/Second\n\n--- REQUEST ---\n{\"t\": \"{{token}}\"}\n\n\
             --- ASSERTS ---\n.ok == true\n",
        );
        assert!(collect_unused_variables(&doc).is_empty());
    }

    #[test]
    fn a_placeholder_in_a_path_section_is_named() {
        let file = "--- TLS ---\nca_cert: {{CA}}/ca.pem\n\n--- PROTO ---\nfiles: {{SCHEMA}}\n";
        let diags = collect_placeholder_diagnostics(file);
        assert_eq!(diags.len(), 2, "{diags:#?}");
        assert!(
            diags[0].message.contains("read as paths"),
            "{}",
            diags[0].message
        );
        assert!(diags[1].message.contains("PROTO"), "{}", diags[1].message);
    }

    #[test]
    fn a_placeholder_where_it_does_work_is_left_alone() {
        let file = "--- ADDRESS ---\n{{HOST}}:4770\n\n--- REQUEST ---\n{\"a\": \"{{WHO}}\"}\n";
        assert!(collect_placeholder_diagnostics(file).is_empty());
    }

    #[test]
    fn a_section_with_attributes_is_still_that_section() {
        let file = "--- ASSERTS ---\n.a == \"{{WHO}}\"\n";
        assert_eq!(collect_placeholder_diagnostics(file).len(), 1);
    }

    #[test]
    fn a_plugin_that_reads_nothing_in_this_family_is_named() {
        let gctf =
            "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n@status() == 0\n";
        let diags = collect_wrong_family_plugin_diagnostics(gctf, "a.gctf");
        assert_eq!(diags.len(), 1, "{diags:#?}");
        assert!(
            diags[0].message.contains("no HTTP status"),
            "{}",
            diags[0].message
        );

        let httf = "--- ENDPOINT ---\nGET /a\n\n--- ASSERTS ---\n@trailer(\"x\") == \"y\"\n";
        let diags = collect_wrong_family_plugin_diagnostics(httf, "a.httf");
        assert_eq!(diags.len(), 1, "{diags:#?}");
        assert!(
            diags[0].message.contains("no trailers"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn a_plugin_that_belongs_to_this_family_is_left_alone() {
        let httf = "--- ENDPOINT ---\nGET /a\n\n--- ASSERTS ---\n@status() == 200\n";
        assert!(collect_wrong_family_plugin_diagnostics(httf, "a.httf").is_empty());

        let gctf = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n@trailer(\"x\") == \"y\"\n";
        assert!(collect_wrong_family_plugin_diagnostics(gctf, "a.gctf").is_empty());
    }

    #[test]
    fn an_attribute_this_family_never_reads_is_named() {
        let httf = "--- ENDPOINT ---\nGET /a\n\n#[repeat(3)]\n--- REQUEST ---\n{}\n";
        let diags = collect_unhonoured_attribute_diagnostics(httf, "a.httf");
        assert_eq!(diags.len(), 1, "{diags:#?}");
        assert!(
            diags[0].message.contains("one document is one request"),
            "{}",
            diags[0].message
        );

        let gctf = "--- ENDPOINT ---\na.B/C\n\n#[repeat(3)]\n--- REQUEST ---\n{}\n";
        assert!(collect_unhonoured_attribute_diagnostics(gctf, "a.gctf").is_empty());

        let skipped = "--- ENDPOINT ---\nGET /a\n\n#[skip]\n--- ASSERTS ---\n.a == 1\n";
        assert!(
            collect_unhonoured_attribute_diagnostics(skipped, "a.httf").is_empty(),
            "skip is honoured by both families"
        );
    }

    #[test]
    fn a_plugin_named_outside_asserts_is_not_a_warning() {
        let gctf =
            "--- ENDPOINT ---\na.B/C\n\n--- RESPONSE ---\n{\"note\": \"@status() is HTTP\"}\n";
        assert!(collect_wrong_family_plugin_diagnostics(gctf, "a.gctf").is_empty());
    }

    #[test]
    fn deprecated_diagnostic_underlines_just_the_offending_token() {
        let content =
            "--- OPTIONS ---\nretry-delay: 0.5\n\n--- ENDPOINT ---\nsvc/M\n\n--- REQUEST ---\n{}\n";
        let diags = collect_deprecated_diagnostics(content);
        let d = diags
            .iter()
            .find(|d| d.message.contains("retry-delay is deprecated"))
            .expect("kebab deprecation diagnostic");
        assert_eq!(d.range.start.line, 1);
        assert_eq!(d.range.start.character, 0);
        assert_eq!(
            d.range.end.character, 11,
            "range must span exactly `retry-delay` (11 chars), not the whole line"
        );
    }

    #[test]
    fn unknown_options_key_diagnostic_underlines_just_the_key() {
        let content =
            "--- OPTIONS ---\ndry_run: true\n\n--- ENDPOINT ---\nsvc/M\n\n--- REQUEST ---\n{}\n";
        let diags = collect_all_diagnostics(content, "t.gctf");
        let d = diags
            .iter()
            .find(|d| d.message.contains("Unknown OPTIONS key 'dry_run'"))
            .expect("unknown OPTIONS key diagnostic");
        assert_eq!(d.range.start.line, 1);
        assert_eq!(d.range.start.character, 0);
        assert_eq!(
            d.range.end.character, 7,
            "range must span exactly `dry_run` (7 chars)"
        );
    }

    #[test]
    fn deprecated_headers_diagnostic_underlines_the_name() {
        let content = "--- HEADERS ---\nx: 1\n\n--- ENDPOINT ---\nsvc/M\n\n--- REQUEST ---\n{}\n";
        let diags = collect_deprecated_diagnostics(content);
        let d = diags
            .iter()
            .find(|d| d.message.contains("HEADERS is deprecated"))
            .expect("HEADERS deprecation diagnostic");
        assert_eq!(d.range.start.line, 0);
        assert_eq!(d.range.start.character, 4);
        assert_eq!(d.range.end.character, 11);
    }

    #[test]
    fn get_section_hover_all_types() {
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
    fn get_section_hover_content() {
        let hover = get_section_hover(&SectionType::Address).unwrap();
        assert!(hover.contains("ADDRESS"));
        assert!(hover.contains("host:port"));

        let hover = get_section_hover(&SectionType::Request).unwrap();
        assert!(hover.contains("JSON/JSON5"));
        assert!(hover.contains("Comments"));

        let hover = get_section_hover(&SectionType::Bench).unwrap();
        assert!(hover.contains("load_schedule"));
        assert!(hover.contains("fixed, stepping, adaptive, closed, open"));
    }

    #[test]
    fn test_get_section_completions() {
        let completions = get_section_completions();
        assert_eq!(completions.len(), 13);

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"--- ADDRESS ---"));
        assert!(labels.contains(&"--- ENDPOINT ---"));
        assert!(labels.contains(&"--- REQUEST ---"));
        assert!(labels.contains(&"--- DATASET ---"));
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
    #[cfg_attr(miri, ignore)]
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
    fn get_address_from_document_with_address() {
        let content = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method
"#;
        let address = get_address_from_document(content);
        assert_eq!(address, Some("localhost:4770".to_string()));
    }

    #[test]
    fn get_address_from_document_no_address() {
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
        assert_eq!(diagnostic.range.start.line, 5);
        assert_eq!(diagnostic.message, "Test error");
    }

    #[test]
    fn a_refused_file_says_which_line_refused_it() {
        let content =
            "--- ADDRESS ---\nlocalhost:4770\n--- NONSENSE ---\n--- ENDPOINT ---\na.B/C\n";
        let diags = collect_all_diagnostics(content, "t.gctf");
        let broke = diags
            .iter()
            .find(|d| d.message.contains("NONSENSE"))
            .expect("names the section that broke");
        assert_eq!(broke.range.start.line, 2, "the line the section is on");
        assert!(
            broke.range.end.character > broke.range.start.character,
            "a range with width, or an editor draws nothing: {:?}",
            broke.range
        );
        assert_eq!(
            broke.severity,
            Some(DiagnosticSeverity::ERROR),
            "the file does not parse, so it is not a warning"
        );
    }

    #[test]
    fn validation_error_to_diagnostic_line_zero() {
        let error = crate::parser::validator::ValidationError {
            message: "Line zero error".to_string(),
            line: Some(0),
            severity: crate::parser::validator::ErrorSeverity::Error,
        };
        let content = "--- ENDPOINT ---\ntest.Service/Method\n";
        let diagnostic = validation_error_to_diagnostic(&error, content);
        assert_eq!(diagnostic.range.start.line, 0);
        assert_eq!(diagnostic.range.end.line, 0);
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
    fn test_create_bench_key_fix_action() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let content = "--- BENCH ---\nload-schdule: fixed\n";
        let range = Range::new(Position::new(1, 0), Position::new(1, 21));

        let action =
            create_bench_key_fix_action(&uri, range, "load-schdule", "load_schedule", content)
                .unwrap();

        assert_eq!(action.title, "Replace 'load-schdule' with 'load_schedule'");
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));

        let edit = action.edit.unwrap();
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "load_schedule");
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].range.end, Position::new(1, 12));
    }

    #[test]
    fn create_bench_key_fix_action_returns_none_when_prefix_mismatch() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let content = "--- BENCH ---\nretry: 2\n";
        let range = Range::new(Position::new(1, 0), Position::new(1, 8));

        let action =
            create_bench_key_fix_action(&uri, range, "load-schdule", "load_schedule", content);

        assert!(action.is_none());
    }

    #[test]
    fn parse_deprecated_key_hint_options_form() {
        let (unknown, suggested) = parse_deprecated_key_hint(
            "OPTIONS.retry-delay is deprecated; prefer OPTIONS.retry_delay",
        )
        .unwrap();
        assert_eq!(unknown, "retry-delay");
        assert_eq!(suggested, "retry_delay");
    }

    #[test]
    fn parse_deprecated_key_hint_attribute_form() {
        let (unknown, suggested) =
            parse_deprecated_key_hint("Attribute #[no-retry] is deprecated; prefer #[no_retry]")
                .unwrap();
        assert_eq!(unknown, "no-retry");
        assert_eq!(suggested, "no_retry");
    }

    #[test]
    fn create_bench_key_fix_action_on_attribute_syntax() {
        let uri = Url::parse("file:///test.gctf").unwrap();
        let content = "#[retry-delay(0.2)]\n--- ENDPOINT ---\n";
        let range = Range::new(Position::new(0, 0), Position::new(0, 20));

        let action =
            create_bench_key_fix_action(&uri, range, "retry-delay", "retry_delay", content)
                .unwrap();

        let edit = action.edit.unwrap();
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits[0].new_text, "retry_delay");
        assert_eq!(edits[0].range.start, Position::new(0, 2));
        assert_eq!(edits[0].range.end, Position::new(0, 13));
    }

    #[test]
    fn collect_optimizer_diagnostics_safe_level_rename() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@uuid(.id)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert!(!diagnostics.is_empty(), "R001 should fire at Safe level");
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String(rule_ids::R001.to_string()))
        );
    }

    #[test]
    fn snapshot_optimizer_diagnostic_hint() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@uuid(.id)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);

        let actual = serde_json::to_value(&diagnostics[0]).unwrap();
        let expected = json!({
            "range": {
                "start": {"line": 4, "character": 0},
                "end": {"line": 4, "character": 10}
            },
            "severity": 4,
            "code": rule_ids::R001.as_str(),
            "source": "grpctestify-optimizer",
            "message": "Optimizer hint: @uuid(.id) -> @is_uuid(.id)",
            "data": {"replacement": "@is_uuid(.id)"}
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
@uuid(.id)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let edits = collect_optimizer_rewrite_edits(&doc, content);
        assert!(!edits.is_empty(), "R001 should produce edits at Safe level");
        assert!(edits[0].new_text.contains("@is_uuid"));
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
    fn snapshot_optimizer_quickfix_action() {
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
    fn snapshot_apply_all_optimizer_action() {
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
    fn collect_optimizer_diagnostics_non_boolean_plugin() {
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
    fn collect_optimizer_diagnostics_double_negation_rule() {
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
            Some(NumberOrString::String(rule_ids::C001.as_str().to_string())),
            "the first step is the canonical spelling"
        );

        let canonical = content.replace("!!@has_header(\"x\")", "not not @has_header(\"x\")");
        let doc = parser::parse_gctf_from_str(&canonical, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, &canonical);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String(rule_ids::B017.as_str().to_string())),
            "applying it leaves the elimination to offer"
        );
    }

    #[test]
    fn a_placeholder_in_asserts_is_flagged_where_it_is_written() {
        let content =
            "--- ENDPOINT ---\na.B/C\n\n--- ASSERTS ---\n.message == \"Hello, {{who}}!\"\n";
        let diags = collect_placeholder_diagnostics(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].range.start.line, 4);
        assert!(diags[0].message.contains("{{who}}"), "{}", diags[0].message);
        assert!(diags[0].message.contains("ASSERTS"));
    }

    #[test]
    fn a_placeholder_naming_an_extraction_is_told_how_to_read_it() {
        let content = "--- ENDPOINT ---\na.B/C\n\n--- RESPONSE ---\n{}\n\n--- EXTRACT ---\ntoken = .token\n\n--- ASSERTS ---\n.id == \"{{token}}\"\n";
        let diags = collect_placeholder_diagnostics(content);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$token"), "{}", diags[0].message);
    }

    #[test]
    fn a_placeholder_in_the_request_is_left_alone() {
        let content = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer {{TOKEN}}\n\n--- REQUEST ---\n{\"who\": \"{{USER}}\"}\n";
        assert!(collect_placeholder_diagnostics(content).is_empty());
    }

    #[test]
    fn a_placeholder_in_extract_is_flagged_too() {
        let content = "--- ENDPOINT ---\na.B/C\n\n--- RESPONSE ---\n{}\n\n--- EXTRACT ---\nid = .users[{{index}}].id\n";
        let diags = collect_placeholder_diagnostics(content);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("EXTRACT"), "{}", diags[0].message);
    }

    #[test]
    fn a_step_of_a_chain_is_told_in_the_editors_voice_too() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.a == 1\n\n--- ENDPOINT ---\na.B/D\n\n--- REQUEST ---\n{}\n";
        let diags = collect_all_diagnostics(content, "t.gctf");
        let unverified = diags
            .iter()
            .find(|d| d.message.contains("Nothing verifies the answer"))
            .expect("the second step verifies nothing");
        assert!(
            unverified.message.starts_with("Document 2:"),
            "{}",
            unverified.message
        );
        assert_eq!(unverified.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn an_http_step_of_a_mixed_chain_is_not_offered_error() {
        let content = "--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ADDRESS ---\nhttp://localhost:8080\n\n--- ENDPOINT ---\nGET /health\n";
        let said: Vec<String> = collect_all_diagnostics(content, "t.apif")
            .iter()
            .filter(|d| d.message.contains("Nothing verifies the answer"))
            .map(|d| d.message.clone())
            .collect();
        assert!(
            said.iter()
                .any(|m| m.starts_with("Document 1:") && m.contains("RESPONSE, ERROR or ASSERTS")),
            "{said:?}"
        );
        assert!(
            said.iter()
                .any(|m| m.starts_with("Document 2:") && m.contains("RESPONSE or ASSERTS")),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_with_nothing_to_verify_is_not_an_error_while_it_is_being_written() {
        let content =
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n";
        let diags = collect_all_diagnostics(content, "t.gctf");
        let unverified = diags
            .iter()
            .find(|d| d.message.starts_with("Nothing verifies the answer"))
            .expect("the file verifies nothing");
        assert_eq!(unverified.severity, Some(DiagnosticSeverity::WARNING));
        assert!(
            !diags
                .iter()
                .any(|d| d.message.starts_with("At least one verification section")),
            "the check's own wording should not survive alongside it"
        );
    }

    #[test]
    fn a_whole_file_problem_says_it_has_no_line() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n";
        let diags = collect_all_diagnostics(content, "t.gctf");
        let unverified = diags
            .iter()
            .find(|d| d.message.starts_with("Nothing verifies the answer"))
            .expect("the file verifies nothing");
        assert_eq!(
            unverified.data.as_ref().and_then(|d| d.get("scope")),
            Some(&json!("file"))
        );
        let missing_address = diags
            .iter()
            .find(|d| d.message.starts_with("ADDRESS section missing"))
            .expect("the file has no address");
        assert_eq!(
            missing_address.data.as_ref().and_then(|d| d.get("scope")),
            Some(&json!("file"))
        );
    }

    #[test]
    fn a_problem_with_a_line_is_left_alone() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\ntest.Service/Method\n\n--- OPTIONS ---\nnonsense: 1\n\n--- ASSERTS ---\n.a == 1\n";
        let scoped = collect_all_diagnostics(content, "t.gctf")
            .iter()
            .filter(|d| {
                d.data
                    .as_ref()
                    .and_then(|x| x.get("scope"))
                    .is_some_and(|s| s == &json!("file"))
            })
            .count();
        assert_eq!(scoped, 0);
    }

    #[test]
    fn a_chain_step_is_not_told_it_has_no_address() {
        let chain = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /a\n\n--- ASSERTS ---\n@status() == 200\n\n--- ENDPOINT ---\nGET /b\n\n--- ASSERTS ---\n@status() == 200\n";
        assert!(
            !collect_all_diagnostics(chain, "t.httf")
                .iter()
                .any(|d| d.message.contains("ADDRESS section missing"))
        );
    }

    #[test]
    fn a_chain_with_no_address_is_told_once() {
        let chain = "--- ENDPOINT ---\nGET /a\n\n--- ASSERTS ---\n@status() == 200\n\n--- ENDPOINT ---\nGET /b\n\n--- ASSERTS ---\n@status() == 200\n";
        let said = collect_all_diagnostics(chain, "t.httf")
            .iter()
            .filter(|d| d.message.contains("ADDRESS section missing"))
            .count();
        assert_eq!(said, 1);
    }

    #[test]
    fn an_http_file_is_not_told_to_add_an_error_section() {
        let content = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /x\n";
        let message = collect_all_diagnostics(content, "t.httf")
            .into_iter()
            .find(|d| d.message.starts_with("Nothing verifies the answer"))
            .expect("the file verifies nothing")
            .message;
        assert!(message.ends_with("Add RESPONSE or ASSERTS."), "{message}");
    }

    #[test]
    fn a_rewrite_counts_where_check_counts_it() {
        let content = "--- ENDPOINT ---\na.B/C\n\n--- ASSERTS ---\n!!@has_header(\"x\")\n";
        let of = |voice| {
            collect_all_diagnostics_in(content, "opt.gctf", voice)
                .into_iter()
                .find(
                    |d| matches!(&d.code, Some(NumberOrString::String(c)) if c.starts_with("OPT_")),
                )
                .expect("the optimizer has something to say")
        };

        let editor = of(Voice::Editor);
        assert_eq!(editor.severity, Some(DiagnosticSeverity::HINT));
        assert!(
            editor.message.starts_with("Optimizer hint: "),
            "{}",
            editor.message
        );
        assert!(editor.data.is_some());

        let checked = of(Voice::Check);
        assert_eq!(checked.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            checked.message,
            "!!@has_header(\"x\") → not not @has_header(\"x\")"
        );
        assert!(checked.data.is_some());
    }

    #[test]
    fn the_workbench_sees_the_assertion_rules_check_runs() {
        let duplicated = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"ok\"\n.status == \"ok\"\n";
        let said = collect_all_diagnostics(duplicated, "dup.gctf")
            .into_iter()
            .find(|d| d.code == Some(NumberOrString::String("SEM_C002".into())))
            .expect("the duplicate is reported");
        assert!(
            said.message.contains("Duplicate assertion"),
            "{}",
            said.message
        );
        assert_eq!(said.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(said.range.start.line, 8);

        let constant =
            "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n1 == 1\n";
        assert!(
            collect_all_diagnostics(constant, "const.gctf")
                .iter()
                .any(|d| d.code == Some(NumberOrString::String("SEM_C001".into()))),
        );
    }

    #[test]
    fn assertion_rules_say_nothing_about_a_file_they_do_not_apply_to() {
        let clean = "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"ok\"\n.id == 7\n";
        let codes: Vec<String> = collect_all_diagnostics(clean, "clean.gctf")
            .iter()
            .filter_map(|d| match &d.code {
                Some(NumberOrString::String(code)) => Some(code.clone()),
                _ => None,
            })
            .collect();
        assert!(!codes.iter().any(|c| c.starts_with("SEM_C")), "{codes:?}",);
    }

    #[test]
    fn the_workbench_sees_a_preamble_out_of_order() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- DATASET ---\n- who: World\n\n--- ASSERTS ---\n.ok\n";

        let said = collect_all_diagnostics(content, "rows.gctf")
            .into_iter()
            .find(|d| d.code == Some(NumberOrString::String("SECTION_ORDER".into())))
            .expect("the workbench sees it");

        assert_eq!(said.severity, Some(DiagnosticSeverity::WARNING));
        assert!(
            said.message.contains("DATASET should come before"),
            "{}",
            said.message
        );
        assert!(said.message.contains("fmt"), "{}", said.message);
        assert_eq!(said.range.start.line, 6);
    }

    #[test]
    fn a_preamble_in_order_is_not_mentioned() {
        let content = "--- DATASET ---\n- who: World\n\n--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- ASSERTS ---\n.ok\n";
        assert!(
            !collect_all_diagnostics(content, "rows.gctf")
                .iter()
                .any(|d| d.code == Some(NumberOrString::String("SECTION_ORDER".into()))),
        );
    }

    #[test]
    fn check_speaks_with_the_severity_the_command_line_uses() {
        let content = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n";

        let editor = collect_all_diagnostics(content, "t.gctf");
        let softened = editor
            .iter()
            .find(|d| d.message.starts_with("Nothing verifies the answer"))
            .expect("the editor says it gently");
        assert_eq!(softened.severity, Some(DiagnosticSeverity::WARNING));

        let checked = collect_all_diagnostics_in(content, "t.gctf", Voice::Check);
        let said = checked
            .iter()
            .find(|d| d.message.starts_with("At least one verification section"))
            .expect("check says what the command line says");
        assert_eq!(said.severity, Some(DiagnosticSeverity::ERROR));
        assert!(
            !checked
                .iter()
                .any(|d| d.message.starts_with("Nothing verifies")),
            "one voice at a time",
        );
    }

    #[test]
    fn a_file_that_checks_something_is_not_told_it_checks_nothing() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n";
        assert!(
            !collect_all_diagnostics(content, "t.gctf")
                .iter()
                .any(|d| d.message.starts_with("Nothing verifies"))
        );
    }

    #[test]
    fn a_rows_value_is_told_where_it_can_be_compared() {
        let content = "--- DATASET ---\n- greeting: hi\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.message == \"{{dataset.greeting}}\"\n";
        let said = collect_placeholder_diagnostics(content);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].message.contains("not substituted in ASSERTS"),
            "{}",
            said[0].message
        );
        assert!(
            said[0].message.contains("REQUEST or RESPONSE"),
            "the way out is the section that is substituted: {}",
            said[0].message,
        );
        assert!(!said[0].message.contains("$dataset"), "{}", said[0].message);
    }

    #[test]
    fn an_extracted_value_is_still_told_about_the_dollar_form() {
        let content = "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- EXTRACT ---\ntoken = .token\n\n--- ASSERTS ---\n.t == \"{{token}}\"\n";
        let said = collect_placeholder_diagnostics(content);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].message.contains("read as $token"),
            "{}",
            said[0].message
        );
    }

    #[test]
    fn a_typed_binding_is_read_under_its_own_name() {
        let read = "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n$price >= 0\n\n--- EXTRACT ---\nprice:number = .price\n";
        let doc = parser::parse_gctf_from_str(read, "typed.gctf").expect("parses");
        assert!(collect_unused_variables(&doc).is_empty(), "$price reads it");

        let unread = "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- EXTRACT ---\nprice:number = .price\n";
        let doc = parser::parse_gctf_from_str(unread, "typed.gctf").expect("parses");
        let unused = collect_unused_variables(&doc);
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].name, "price", "named without the type it carries");
    }

    #[test]
    fn a_refusal_lands_on_the_line_that_caused_it() {
        let content = "--- ADDRESS ---\nhttp://api.test\n\n--- ENDPOINT ---\nGET /a\n\n--- RESPONSE 200 ---\n{}\n";
        assert_eq!(
            line_of_cause(
                content,
                "unknown inline option: 200 — a status is not written in the header"
            ),
            Some(6)
        );
    }

    #[test]
    fn a_refusal_about_nothing_the_file_names_is_left_where_it_was() {
        let content = "--- ENDPOINT ---\na.A/One\n";
        assert_eq!(line_of_cause(content, "Unknown section type: WOBBLE"), None);
        assert_eq!(line_of_cause(content, "unknown inline option: 404"), None);
    }

    #[test]
    fn a_group_that_reads_its_own_binding_is_reported() {
        let content = "--- ADDRESS ---\nhttp://api.test\n\n--- ENDPOINT parallel ---\nGET /v1/a\n\n--- ASSERTS ---\n@status() == 200\n\n--- EXTRACT ---\nid = .id\n\n--- ENDPOINT parallel ---\nGET /v1/b/{{id}}\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = parser::parse_gctf_from_str(content, "race.httf").expect("parses");
        assert!(doc.get_document(1).is_some_and(|d| d.runs_in_parallel()));

        let said = collect_group_race_diagnostics(&doc);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].message.contains("parallel group"),
            "{}",
            said[0].message
        );
    }

    #[test]
    fn steps_that_run_one_after_another_may_read_each_other() {
        let content = "--- ADDRESS ---\nhttp://api.test\n\n--- ENDPOINT ---\nGET /v1/a\n\n--- ASSERTS ---\n@status() == 200\n\n--- EXTRACT ---\nid = .id\n\n--- ENDPOINT ---\nGET /v1/b/{{id}}\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = parser::parse_gctf_from_str(content, "chain.httf").expect("parses");
        assert!(collect_group_race_diagnostics(&doc).is_empty());
    }

    #[test]
    fn a_name_bound_before_the_group_is_the_one_it_reads() {
        let content = "--- ADDRESS ---\nhttp://api.test\n\n--- ENDPOINT ---\nGET /v1/a\n\n--- ASSERTS ---\n@status() == 200\n\n--- EXTRACT ---\nid = .id\n\n--- ENDPOINT parallel ---\nGET /v1/b/{{id}}\n\n--- ASSERTS ---\n@status() == 200\n\n--- EXTRACT ---\nid = .id\n\n--- ENDPOINT parallel ---\nGET /v1/c/{{id}}\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = parser::parse_gctf_from_str(content, "chain.httf").expect("parses");
        assert!(collect_group_race_diagnostics(&doc).is_empty());
    }

    #[test]
    fn collect_all_diagnostics_includes_optimizer_pass() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
!!@has_header("x")
"#;
        let expected_code = rule_ids::C001.as_str().to_string();
        let actual = collect_all_diagnostics(content, "test.gctf");
        assert!(
            actual
                .iter()
                .any(|d| d.code == Some(NumberOrString::String(expected_code.clone()))),
            "expected {expected_code} among {actual:?}"
        );
    }

    #[test]
    fn collect_all_diagnostics_flags_missing_sections() {
        let diagnostics = collect_all_diagnostics("", "test.gctf");
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn collect_optimizer_diagnostics_canonical_operator_rule() {
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
            Some(NumberOrString::String(rule_ids::C001.as_str().to_string()))
        );
        assert!(
            diagnostics[0].message.contains(".name startsWith \"abc\""),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn collect_optimizer_diagnostics_deprecated_plugin_rename() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@uuid(.id)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String(rule_ids::R001.to_string()))
        );
    }

    #[test]
    fn collect_optimizer_diagnostics_empty_to_is_empty() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@empty(.x)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let diagnostics = collect_optimizer_diagnostics(&doc, content);
        assert_eq!(diagnostics.len(), 1, "should rewrite @empty to @is_empty");
        assert!(diagnostics[0].message.contains("@is_empty"));
    }

    #[test]
    fn collect_semantic_diagnostics_unknown_plugin() {
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
    fn get_section_key_completions_proto() {
        let completions = get_section_key_completions(&SectionType::Proto);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"descriptor:"));
        assert!(labels.contains(&"files:"));
    }

    #[test]
    fn get_section_key_completions_tls() {
        let completions = get_section_key_completions(&SectionType::Tls);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"ca_cert:"));
        assert!(labels.contains(&"client_cert:"));
        assert!(labels.contains(&"client_key:"));
        assert!(!labels.contains(&"ca_file:"));
    }

    #[test]
    fn get_section_key_completions_options() {
        let completions = get_section_key_completions(&SectionType::Options);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"timeout:"));
        assert!(labels.contains(&"protocol:"));
        assert!(!labels.contains(&"retries:"));
    }

    #[test]
    fn get_section_key_completions_bench() {
        let completions = get_section_key_completions(&SectionType::Bench);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"load_schedule:"));
        assert!(labels.contains(&"progress_interval:"));
        assert!(labels.contains(&"thresholds.*:"));

        let mode_detail = completions
            .iter()
            .find(|c| c.label == "mode:")
            .and_then(|c| c.detail.clone())
            .unwrap_or_default();
        assert!(mode_detail.contains("fixed, stepping, adaptive, closed, open"));
    }

    #[test]
    fn get_section_key_completions_others() {
        assert!(get_section_key_completions(&SectionType::Address).is_empty());
        assert!(get_section_key_completions(&SectionType::Response).is_empty());
    }

    #[test]
    fn get_section_header_option_completions_response() {
        let completions = get_section_header_option_completions(&SectionType::Response);
        assert!(!completions.is_empty());

        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"partial=true"));
        assert!(labels.contains(&"tolerance=0.001"));
    }

    #[test]
    fn get_section_header_option_completions_others() {
        assert!(get_section_header_option_completions(&SectionType::Address).is_empty());
        assert!(get_section_header_option_completions(&SectionType::Request).is_empty());
    }

    #[test]
    fn get_var_hover_multibyte_line_no_panic() {
        let content = "--- ENDPOINT ---\nsvc.M\n\n--- EXTRACT ---\nname = .n\n\n--- REQUEST ---\n{\"msg\": \"Привет {{ name }}\"}\n";
        let doc = parser::parse_gctf_from_str(content, "t.gctf").unwrap();
        let hover = get_var_hover(&doc, 7, 20);
        assert!(hover.is_some());
    }

    #[test]
    fn section_index_at_line_half_open() {
        let content = "# note\n\n--- ENDPOINT ---\nsvc.M\n\n--- ASSERTS ---\n.id == 1\n";
        let doc = parser::parse_gctf_from_str(content, "t.gctf").unwrap();
        let sections = &doc.sections;
        let ep = sections
            .iter()
            .position(|s| s.section_type == SectionType::Endpoint)
            .unwrap();
        let ep_start = sections[ep].start_line;
        assert!(
            ep_start >= 1,
            "leading lines should push ENDPOINT past line 0"
        );
        assert_eq!(section_index_at_line(sections, ep_start - 1), None);
        assert_eq!(section_index_at_line(sections, ep_start), Some(ep));
        assert_eq!(section_index_at_line(sections, ep_start + 1), Some(ep));
        assert_ne!(
            section_index_at_line(sections, sections[ep].end_line),
            Some(ep)
        );
    }

    #[test]
    fn find_document_index_at_line_multidoc_boundaries() {
        let content = "--- ENDPOINT ---\nsvc.A\n\n--- ASSERTS ---\n.a == 1\n\n\
--- ENDPOINT ---\nsvc.B\n\n--- ASSERTS ---\n.b == 2\n";
        let doc = parser::parse_gctf_from_str(content, "t.gctf").unwrap();
        assert!(!doc.is_single_document(), "expected two chained documents");
        assert_eq!(find_document_index_at_line(&doc, 0), 0);
        assert_eq!(find_document_index_at_line(&doc, 5), 0);
        assert_eq!(find_document_index_at_line(&doc, 6), 1);
        assert_eq!(find_document_index_at_line(&doc, 10), 1);
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
        let completions = get_variable_completions(&doc, 17);
        assert!(!completions.is_empty());
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"user_id"));
        assert!(labels.contains(&"token"));
    }
}
