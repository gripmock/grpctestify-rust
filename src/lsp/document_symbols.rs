//! Document symbols for GCTF documents.
//!
//! Builds a tree of symbols for assertions and extracted variables
//! that appears in the LSP document symbol response.

use crate::parser;
use crate::parser::ast::SectionType;
#[allow(deprecated)]
use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

/// Build document symbols (assertions and extracted variables) for a GCTF document.
#[allow(deprecated)]
pub fn build_section_children_for_doc(doc: &parser::GctfDocument) -> Vec<DocumentSymbol> {
    let mut all_children: Vec<DocumentSymbol> = Vec::new();

    for s in &doc.sections {
        let mut children: Vec<DocumentSymbol> = Vec::new();

        if s.section_type == SectionType::Asserts {
            for (idx, line) in s.raw_content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
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
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
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

        if !children.is_empty() {
            all_children.extend(children);
        }
    }

    all_children
}
