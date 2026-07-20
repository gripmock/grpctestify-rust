//! Document symbols for GCTF documents.
//!
//! Builds a tree of symbols for assertions and extracted variables
//! that appears in the LSP document symbol response.

use crate::lsp::position::byte_to_utf16_col;
use crate::parser;
use crate::parser::ast::SectionType;
use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

/// Build document symbols (assertions and extracted variables) for a GCTF document.
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
                // Offsets below are byte indices into `trimmed`; convert them to
                // UTF-16 columns in the original `line` (which may be indented
                // and/or contain non-ASCII characters) as LSP requires.
                let lead = line.len() - line.trim_start().len();
                let off16 =
                    |byte_in_trimmed: usize| byte_to_utf16_col(line, lead + byte_in_trimmed) as u32;
                #[expect(deprecated)]
                let mut assertion_symbol = DocumentSymbol {
                    name: trimmed.to_string(),
                    detail: Some("assertion".to_string()),
                    kind: SymbolKind::STRING,
                    tags: None,
                    deprecated: None,
                    range: Range::new(
                        Position::new(line_num, off16(0)),
                        Position::new(line_num, off16(trimmed.len())),
                    ),
                    selection_range: Range::new(
                        Position::new(line_num, off16(0)),
                        Position::new(line_num, off16(trimmed.len())),
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
                        #[expect(deprecated)]
                        var_children.push(DocumentSymbol {
                            name: inner.to_string(),
                            detail: Some("variable reference".to_string()),
                            kind: SymbolKind::VARIABLE,
                            tags: None,
                            deprecated: None,
                            range: Range::new(
                                Position::new(line_num, off16(abs_open)),
                                Position::new(line_num, off16(abs_close)),
                            ),
                            selection_range: Range::new(
                                Position::new(line_num, off16(abs_open + 2)),
                                Position::new(line_num, off16(abs_close - 2)),
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
                let lead = line.len() - line.trim_start().len();
                let off16 =
                    |byte_in_trimmed: usize| byte_to_utf16_col(line, lead + byte_in_trimmed) as u32;
                // `var_name` is the leading token of `trimmed`, so it starts at
                // trimmed byte offset 0.
                let name_off = trimmed.len() - trimmed.trim_start().len();

                #[expect(deprecated)]
                children.push(DocumentSymbol {
                    name: var_name.to_string(),
                    detail: Some(format!("extract: {}", expr_trimmed)),
                    kind: SymbolKind::VARIABLE,
                    tags: None,
                    deprecated: None,
                    range: Range::new(
                        Position::new(line_num, off16(0)),
                        Position::new(line_num, off16(trimmed.len())),
                    ),
                    selection_range: Range::new(
                        Position::new(line_num, off16(name_off)),
                        Position::new(line_num, off16(name_off + var_name.len())),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_child_range_is_utf16() {
        // Cyrillic inside `{{ }}`: each char is 2 UTF-8 bytes but 1 UTF-16 unit.
        let content = "--- ENDPOINT ---\nsvc.M\n\n--- ASSERTS ---\n{{ имя }} == 1\n";
        let doc = parser::parse_gctf_from_str(content, "t.gctf").unwrap();
        let symbols = build_section_children_for_doc(&doc);
        let assertion = symbols
            .iter()
            .find(|s| s.children.is_some())
            .expect("assertion with a variable child");
        let child = &assertion.children.as_ref().unwrap()[0];
        assert_eq!(child.name, "имя");
        // `{{ имя }}` spans 9 UTF-16 columns; a raw byte offset would report 12.
        assert_eq!(child.range.start.character, 0);
        assert_eq!(child.range.end.character, 9);
        // The inner-name selection covers `имя` at UTF-16 columns 2..7.
        assert_eq!(child.selection_range.start.character, 2);
        assert_eq!(child.selection_range.end.character, 7);
    }

    #[test]
    fn test_extract_symbol_range_is_utf16() {
        // Non-ASCII in the extract expression must not push the range end past
        // the real UTF-16 width of the line.
        let content = "--- EXTRACT ---\nname = .поле\n";
        let doc = parser::parse_gctf_from_str(content, "t.gctf").unwrap();
        let symbols = build_section_children_for_doc(&doc);
        let sym = symbols
            .iter()
            .find(|s| s.name == "name")
            .expect("extract variable symbol");
        // `name = .поле` is 12 UTF-16 columns (4 Cyrillic chars = 4 units).
        assert_eq!(sym.range.end.character, 12);
        assert_eq!(sym.selection_range.start.character, 0);
        assert_eq!(sym.selection_range.end.character, 4);
    }
}
