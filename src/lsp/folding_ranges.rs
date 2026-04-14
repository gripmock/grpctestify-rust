//! Folding ranges for GCTF documents.
//!
//! Provides folding ranges for documents and sections in `.gctf` files.

use crate::parser;
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};

/// Build folding ranges for the document content.
/// Returns region-level folding ranges for documents and sections.
pub fn build_folding_ranges(content: &str) -> Vec<FoldingRange> {
    let mut ranges: Vec<FoldingRange> = Vec::new();

    if let Ok(head) = parser::parse_gctf_from_str(content, "temp.gctf") {
        // Document-level folding
        for (doc_idx, d) in head.iter_chain().enumerate() {
            if let (Some(first), Some(last)) = (d.sections.first(), d.sections.last()) {
                let start = ((first.start_line as i32) - 1).max(0) as u32;
                let end = ((last.end_line as i32) - 1).max(0) as u32;
                if end > start {
                    let label = if head.is_single_document() {
                        d.get_endpoint().unwrap_or_else(|| "document".to_string())
                    } else {
                        format!(
                            "Doc {}: {}",
                            doc_idx + 1,
                            d.get_endpoint().unwrap_or_else(|| "unknown".to_string())
                        )
                    };
                    ranges.push(FoldingRange {
                        start_line: start,
                        start_character: Some(0),
                        end_line: end,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some(label),
                    });
                }
            }
        }

        // Section-level folding
        for d in head.iter_chain() {
            for section in &d.sections {
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
    }

    ranges
}
