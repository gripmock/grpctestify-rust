pub mod assert;
pub mod cli;
pub mod config;
pub mod execution;
pub mod grpc;
pub mod logging;
pub mod lsp;
pub mod parser;
pub mod plugins;
pub mod report;
pub mod state;
pub mod utils;

pub use parser::parse_gctf;
pub use parser::validate_document;

/// Format/serialize a GCTF document to string
pub fn serialize_gctf(doc: &parser::GctfDocument) -> String {
    use std::fmt::Write;
    let mut output = String::new();

    for section in &doc.sections {
        write!(output, "--- {} ---", section.section_type.as_str()).unwrap();
        output.push('\n');

        match &section.content {
            parser::ast::SectionContent::Single(s) => {
                writeln!(output, "{}", s.trim()).unwrap();
            }
            parser::ast::SectionContent::Json(val) => {
                // Try to format as pretty JSON, fall back to raw if it fails (JSON5/comments)
                if let Ok(pretty) = serde_json::to_string_pretty(val) {
                    writeln!(output, "{}", pretty).unwrap();
                } else {
                    // Preserve raw content for JSON5 with comments
                    let raw = section.raw_content.trim();
                    writeln!(output, "{}", raw).unwrap();
                }
            }
            parser::ast::SectionContent::JsonLines(lines) => {
                // Each line is a separate JSON object - keep on single line for idempotency
                for val in lines {
                    if let Ok(compact) = serde_json::to_string(val) {
                        writeln!(output, "{}", compact).unwrap();
                    }
                }
            }
            parser::ast::SectionContent::KeyValues(kv) => {
                // Sort keys for deterministic output
                let mut sorted: Vec<_> = kv.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                for (k, v) in sorted {
                    writeln!(output, "{}: {}", k, v).unwrap();
                }
            }
            parser::ast::SectionContent::Assertions(lines) => {
                for line in lines {
                    writeln!(output, "{}", line.trim()).unwrap();
                }
            }
            parser::ast::SectionContent::Empty => {}
            parser::ast::SectionContent::Extract(vars) => {
                // Sort keys for deterministic output
                let mut sorted: Vec<_> = vars.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                for (k, v) in sorted {
                    writeln!(output, "{}: {}", k, v).unwrap();
                }
            }
        }
        output.push('\n');
    }

    output.trim_end().to_string() + "\n"
}
