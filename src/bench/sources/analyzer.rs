use crate::parser::ast::{GctfDocument, SectionContent, SectionType};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

use super::SourceDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRef {
    pub source: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IndexReason {
    TemplateLookup,
    DimensionJoin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRequirement {
    pub source: String,
    pub column: String,
    pub reason: IndexReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceUsagePlan {
    pub template_refs: Vec<TemplateRef>,
    pub required_indexes: Vec<IndexRequirement>,
}

pub struct SourceUsageAnalyzer;

impl SourceUsageAnalyzer {
    pub fn analyze(document: &GctfDocument, sources: &[SourceDefinition]) -> SourceUsagePlan {
        let template_refs = extract_template_refs(document);
        let mut required: BTreeSet<(String, String, IndexReason)> = BTreeSet::new();

        let mut source_name_to_indexed: HashMap<String, Vec<String>> = HashMap::new();
        for (i, s) in sources.iter().enumerate() {
            let name = effective_source_name(s, i);
            let indexed = s
                .indexed_columns()
                .into_iter()
                .map(|x| x.to_string())
                .collect();
            source_name_to_indexed.insert(name, indexed);
        }

        let used_sources: BTreeSet<String> =
            template_refs.iter().map(|r| r.source.clone()).collect();

        for tr in &template_refs {
            if let Some(indexed_cols) = source_name_to_indexed.get(&tr.source)
                && indexed_cols.iter().any(|c| c == &tr.column)
            {
                required.insert((
                    tr.source.clone(),
                    tr.column.clone(),
                    IndexReason::TemplateLookup,
                ));
            }
        }

        if let Some(primary) = sources.first() {
            let primary_name = effective_source_name(primary, 0);
            for (i, dim) in sources.iter().enumerate().skip(1) {
                let dim_name = effective_source_name(dim, i);
                if !used_sources.contains(&dim_name) {
                    continue;
                }
                for key in dim.indexed_columns() {
                    let key = key.to_string();
                    let primary_fk_used = template_refs
                        .iter()
                        .any(|r| r.source == primary_name && r.column == key);
                    if primary_fk_used {
                        required.insert((dim_name.clone(), key, IndexReason::DimensionJoin));
                    }
                }
            }
        }

        let required_indexes = required
            .into_iter()
            .map(|(source, column, reason)| IndexRequirement {
                source,
                column,
                reason,
            })
            .collect();

        SourceUsagePlan {
            template_refs,
            required_indexes,
        }
    }
}

pub fn effective_source_name(def: &SourceDefinition, index: usize) -> String {
    if let Some(name) = &def.name
        && !name.trim().is_empty()
    {
        return name.trim().to_string();
    }
    let stem = std::path::Path::new(&def.file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if stem.is_empty() {
        format!("source_{index}")
    } else {
        stem
    }
}

fn extract_template_refs(document: &GctfDocument) -> Vec<TemplateRef> {
    let mut refs = Vec::new();
    for section in &document.sections {
        if !section.raw_content.is_empty() {
            collect_from_string(&section.raw_content, &mut refs);
        }
        match (&section.section_type, &section.content) {
            (SectionType::Request, SectionContent::Json(v))
            | (SectionType::Response, SectionContent::Json(v))
            | (SectionType::Error, SectionContent::Json(v)) => {
                collect_from_json(v, &mut refs);
            }
            (SectionType::Request, SectionContent::JsonLines(lines))
            | (SectionType::Response, SectionContent::JsonLines(lines)) => {
                for v in lines {
                    collect_from_json(v, &mut refs);
                }
            }
            (SectionType::Asserts, SectionContent::Assertions(lines)) => {
                for line in lines {
                    collect_from_string(line, &mut refs);
                }
            }
            (SectionType::Extract, SectionContent::Extract(map)) => {
                for v in map.values() {
                    collect_from_string(v, &mut refs);
                }
            }
            _ => {}
        }
    }
    refs
}

fn collect_from_json(v: &Value, out: &mut Vec<TemplateRef>) {
    match v {
        Value::String(s) => collect_from_string(s, out),
        Value::Array(a) => {
            for x in a {
                collect_from_json(x, out);
            }
        }
        Value::Object(m) => {
            for x in m.values() {
                collect_from_json(x, out);
            }
        }
        _ => {}
    }
}

fn collect_from_string(s: &str, out: &mut Vec<TemplateRef>) {
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let inner = after[..end].trim();
        if let Some((source, col)) = parse_source_column(inner) {
            out.push(TemplateRef {
                source: source.to_string(),
                column: col.to_string(),
            });
        }
        rest = &after[end + 2..];
    }
}

fn parse_source_column(inner: &str) -> Option<(&str, &str)> {
    let mut parts = inner.split('.');
    let source = parts.next()?.trim();
    let column = parts.next()?.trim();
    if source.is_empty() || column.is_empty() {
        return None;
    }
    if parts.next().is_some() {
        return None;
    }
    Some((source, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_doc(content: &str) -> GctfDocument {
        crate::parser::parse_gctf_from_str(content, "test.gctf").unwrap()
    }

    #[test]
    fn extract_request_template_refs() {
        let doc = parse_doc(
            r#"--- REQUEST ---
{"user_id":"{{users.user_id}}","r":"{{regions.region_id}}"}
"#,
        );
        let refs = extract_template_refs(&doc);
        assert!(
            refs.iter()
                .any(|r| r.source == "users" && r.column == "user_id")
        );
        assert!(
            refs.iter()
                .any(|r| r.source == "regions" && r.column == "region_id")
        );
    }

    #[test]
    fn derive_required_indexes_for_dimension_join() {
        let doc = parse_doc(
            r#"--- REQUEST ---
{"region":"{{regions.name}}","rid":"{{pvz.region_id}}"}
"#,
        );
        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            r#"
- name: pvz
  file: data/pvz.csv
- name: regions
  file: data/regions.csv
  indexed_by: [region_id]
"#,
        )
        .unwrap();

        let plan = SourceUsageAnalyzer::analyze(&doc, &defs);
        assert!(
            plan.required_indexes
                .iter()
                .any(|r| r.source == "regions" && r.column == "region_id")
        );
    }
}
