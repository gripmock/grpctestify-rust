//! Inlay hints for GCTF documents.
//!
//! Shows type information for variables in EXTRACT sections, section types,
//! optimizer hints, and unused variable warnings.

use crate::lsp::handlers;
use crate::optimizer;
use crate::parser;
use crate::parser::ast::SectionType;
use crate::plugins::{PluginManager, extract_plugin_call_name};
use tower_lsp::lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, InlayHintTooltip, Position, Range,
};

fn infer_type_label(expr: &str) -> &'static str {
    if matches!(expr.trim(), "true" | "false") {
        return "bool";
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(expr.trim()) {
        return match v {
            serde_json::Value::String(_) => "string",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::Bool(_) => "bool",
            serde_json::Value::Null => "null",
            _ => "value",
        };
    }
    if let Some(name) = extract_plugin_call_name(expr.trim())
        && let Some(plugin) = PluginManager::new().get(&name)
    {
        return plugin.signature().return_type.display_name();
    }
    "value"
}

pub fn build_inlay_hints(content: &str, range: Range) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    let Ok(head) = parser::parse_gctf_from_str(content, "temp.gctf") else {
        return hints;
    };
    let total_docs = head.document_count();
    for (doc_idx, d) in head.iter_chain().enumerate() {
        for section in &d.sections {
            // start_line is already 0-based (the section header line).
            let section_line = section.start_line as u32;
            if section_line < range.start.line || section_line > range.end.line {
                continue;
            }
            if section.section_type == SectionType::Endpoint && total_docs > 1 {
                hints.push(InlayHint {
                    position: Position {
                        line: section_line,
                        character: 1000,
                    },
                    label: InlayHintLabel::String(format!(
                        "document {} of {}",
                        doc_idx + 1,
                        total_docs
                    )),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: Some(InlayHintTooltip::String(format!(
                        "Document {} of {} in this file",
                        doc_idx + 1,
                        total_docs
                    ))),
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                });
            } else {
                hints.push(InlayHint {
                    position: Position {
                        line: section_line,
                        character: 1000,
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
        for section in &d.sections {
            if section.section_type == SectionType::Extract
                && let parser::ast::SectionContent::Extract(extractions) = &section.content
            {
                for (var_name, expr) in extractions {
                    let mut hint_line: Option<u32> = None;
                    let mut hint_char: u32 = 1000;
                    for (idx, line) in section.raw_content.lines().enumerate() {
                        if let Some((name, _)) = line.trim().split_once('=')
                            && name.trim() == var_name
                        {
                            hint_line = Some((section.start_line + idx + 1) as u32);
                            hint_char = name.len() as u32;
                            break;
                        }
                    }
                    let line_num = hint_line.unwrap_or(section.start_line as u32);
                    if line_num >= range.start.line && line_num <= range.end.line {
                        hints.push(InlayHint {
                            position: Position {
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
        for opt in optimizer::collect_assertion_optimizations(d, optimizer::OptimizeLevel::Advisory)
        {
            let line_num = opt.line.saturating_sub(1) as u32;
            if line_num < range.start.line || line_num > range.end.line {
                continue;
            }
            hints.push(InlayHint {
                position: Position {
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
    for unused_var in handlers::collect_unused_variables(&head) {
        // unused_var.line is already 0-based.
        let line_num = unused_var.line as u32;
        if line_num < range.start.line || line_num > range.end.line {
            continue;
        }
        hints.push(InlayHint {
            position: Position {
                line: line_num,
                character: (unused_var.character + unused_var.name.len()) as u32,
            },
            label: InlayHintLabel::String("unused".to_string()),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: Some(InlayHintTooltip::String(format!(
                "'{}' is extracted but never used in subsequent documents",
                unused_var.name
            ))),
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }
    hints
}
