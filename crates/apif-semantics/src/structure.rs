use apif_parser as parser;
use apif_parser::ast::SectionType;

#[derive(Debug, Clone)]
pub struct UnusedVariable {
    pub name: String,
    pub has_later_steps: bool,
    pub line: usize,
    pub character: usize,
    pub doc_index: usize,
}

pub fn collect_unused_variables(doc: &parser::GctfDocument) -> Vec<UnusedVariable> {
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

fn extract_all_vars(doc: &parser::GctfDocument) -> Vec<(usize, String, usize, usize)> {
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

fn is_var_read(doc: &parser::GctfDocument, def_doc_idx: usize, var_name: &str) -> bool {
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

fn doc_contains_var_reference(doc: &parser::GctfDocument, var_name: &str) -> bool {
    for section in &doc.sections {
        if section_contains_var_reference(section, var_name) {
            return true;
        }
    }
    false
}

fn section_contains_var_reference(section: &parser::ast::Section, var_name: &str) -> bool {
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
    doc: &parser::GctfDocument,
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

pub fn preamble_section_order(doc: &parser::GctfDocument) -> Vec<(usize, String, String)> {
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
