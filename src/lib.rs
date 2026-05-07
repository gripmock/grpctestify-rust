pub mod assert;
pub mod bench;
pub mod cli;
pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod execution;
pub mod grpc;
pub mod logging;
pub mod lsp;
pub mod optimizer;
pub mod parser;
pub mod plugins;
pub mod polyfill;
pub mod report;
pub mod semantics;
pub mod state;
pub mod time;
pub mod utils;

pub use parser::parse_gctf;
pub use parser::validate_document;

/// Format/serialize a GCTF document to string
pub fn serialize_gctf(doc: &parser::GctfDocument) -> String {
    use std::fmt::Write;
    let mut output = String::new();

    let sections = sort_sections_for_fmt(&doc.sections);

    for section in &sections {
        for attr in &section.attributes {
            let _ = writeln!(output, "{}", attr.format_directive());
        }

        let _ = write!(output, "--- {} ---", section.section_type.as_str());
        output.push('\n');

        match &section.content {
            parser::ast::SectionContent::Single(s) => {
                let _ = writeln!(output, "{}", s.trim());
            }
            parser::ast::SectionContent::Json(val) => {
                if let Ok(pretty) = serde_json::to_string_pretty(val) {
                    let _ = writeln!(output, "{}", pretty);
                } else {
                    let raw = section.raw_content.trim();
                    let _ = writeln!(output, "{}", raw);
                }
            }
            parser::ast::SectionContent::JsonLines(lines) => {
                for val in lines {
                    if let Ok(compact) = serde_json::to_string(val) {
                        let _ = writeln!(output, "{}", compact);
                    }
                }
            }
            parser::ast::SectionContent::KeyValues(kv) => {
                let mut sorted: Vec<_> = kv.iter().collect();
                if section.section_type == parser::ast::SectionType::Bench {
                    sorted.sort_by(|a, b| {
                        crate::bench::schema::bench_key_rank(a.0)
                            .cmp(&crate::bench::schema::bench_key_rank(b.0))
                            .then_with(|| a.0.cmp(b.0))
                    });
                } else {
                    sorted.sort_by(|a, b| a.0.cmp(b.0));
                }
                for (k, v) in sorted {
                    let _ = writeln!(output, "{}: {}", k, v);
                }
            }
            parser::ast::SectionContent::Assertions(lines) => {
                for line in lines {
                    let _ = writeln!(output, "{}", line.trim());
                }
            }
            parser::ast::SectionContent::Empty => {}
            parser::ast::SectionContent::Extract(vars) => {
                let mut sorted: Vec<_> = vars.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                for (k, v) in sorted {
                    let _ = writeln!(output, "{}: {}", k, v);
                }
            }
            parser::ast::SectionContent::Meta(meta) => {
                if let Ok(yaml) = serde_yaml_ng::to_string(meta) {
                    output.push_str(yaml.trim_end());
                }
            }
        }
        output.push('\n');
    }

    output.trim_end().to_string() + "\n"
}

fn sort_sections_for_fmt(sections: &[parser::ast::Section]) -> Vec<parser::ast::Section> {
    if sections.len() <= 1 {
        return sections.to_vec();
    }

    let first_body_idx = sections
        .iter()
        .position(|s| s.section_type.preamble_rank().is_none())
        .unwrap_or(sections.len());

    let mut preamble: Vec<&parser::ast::Section> = sections[..first_body_idx].iter().collect();
    preamble.sort_by_key(|s| s.section_type.preamble_rank().unwrap());

    let mut result = Vec::with_capacity(sections.len());
    for s in &preamble {
        result.push((*s).clone());
    }
    for s in &sections[first_body_idx..] {
        result.push((*s).clone());
    }
    result
}
