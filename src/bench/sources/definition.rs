use super::detect::SourceFormat;
use super::filter::FilterCondition;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
pub struct SourceDefinition {
    pub file: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_format_opt")]
    pub format: Option<SourceFormat>,
    #[serde(default)]
    pub delimiter: Option<u8>,
    #[serde(default)]
    pub header: Option<bool>,
    #[serde(default)]
    pub indexed_by: Option<IndexedBy>,
    #[serde(default)]
    pub index_mode: Option<IndexMode>,
    #[serde(default)]
    pub memory_budget: Option<String>,
    #[serde(default)]
    pub filter: Option<Vec<FilterCondition>>,
    #[serde(default)]
    pub join_type: Option<JoinType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinType {
    Inner,
    Left,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IndexedBy {
    Single(String),
    Multi(Vec<String>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexMode {
    #[default]
    OnDemand,
    BuildOnce,
    Memory,
}

fn deserialize_format_opt<'de, D>(de: D) -> Result<Option<SourceFormat>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(de)?;
    match s {
        None => Ok(None),
        Some(ref val) => SourceFormat::from_str(val)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

impl SourceDefinition {
    pub fn from_file_raw(file: &str, key_column: &str, format: Option<&SourceFormat>) -> Self {
        Self {
            file: file.to_string(),
            name: None,
            format: format.cloned(),
            delimiter: None,
            header: None,
            indexed_by: Some(IndexedBy::Single(key_column.to_string())),
            index_mode: Some(IndexMode::BuildOnce),
            memory_budget: None,
            filter: None,
            join_type: None,
        }
    }

    pub fn join_type_or_default(&self) -> JoinType {
        self.join_type.unwrap_or(JoinType::Left)
    }

    pub fn effective_index_mode(&self) -> IndexMode {
        self.index_mode.unwrap_or_default()
    }

    pub fn indexed_columns(&self) -> Vec<&str> {
        match &self.indexed_by {
            None => Vec::new(),
            Some(IndexedBy::Single(s)) => vec![s.as_str()],
            Some(IndexedBy::Multi(v)) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal() {
        let def: SourceDefinition = serde_yaml_ng::from_str("file: data/users.csv").unwrap();
        assert_eq!(def.file, "data/users.csv");
        assert!(def.name.is_none());
        assert!(def.format.is_none());
        assert!(def.indexed_by.is_none());
    }

    #[test]
    fn deserialize_full() {
        let yaml = "\
file: data/pvz.csv
name: pvz
format: csv
indexed_by: pvz_id
index_mode: build_once
memory_budget: 256mb
";
        let def: SourceDefinition = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(def.file, "data/pvz.csv");
        assert_eq!(def.name.as_deref(), Some("pvz"));
        assert_eq!(def.format, Some(SourceFormat::Csv));
        assert_eq!(def.effective_index_mode(), IndexMode::BuildOnce);
        assert_eq!(def.indexed_columns(), vec!["pvz_id"]);
        assert_eq!(def.memory_budget.as_deref(), Some("256mb"));
    }

    #[test]
    fn deserialize_multi_indexed_by() {
        let yaml = "\
file: data/pvz.csv
indexed_by:
  - pvz_id
  - region_id
";
        let def: SourceDefinition = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(def.indexed_columns(), vec!["pvz_id", "region_id"]);
    }

    #[test]
    fn index_mode_default_is_on_demand() {
        let def: SourceDefinition = serde_yaml_ng::from_str("file: x.csv").unwrap();
        assert_eq!(def.effective_index_mode(), IndexMode::OnDemand);
    }

    #[test]
    fn index_mode_all_variants() {
        assert_eq!(IndexMode::default(), IndexMode::OnDemand);
        assert_ne!(IndexMode::OnDemand, IndexMode::BuildOnce);
        assert_ne!(IndexMode::BuildOnce, IndexMode::Memory);
    }

    #[test]
    fn deserialize_format_aliases() {
        for fmt in &["csv", "tsv", "ndjson", "json", "jsonl"] {
            let yaml = format!("file: x\nformat: {fmt}");
            let def: SourceDefinition = serde_yaml_ng::from_str(&yaml).unwrap();
            assert!(def.format.is_some());
        }
    }

    #[test]
    fn deserialize_invalid_format_errors() {
        let yaml = "file: x\nformat: excel";
        let result: Result<SourceDefinition, _> = serde_yaml_ng::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_filter_conditions() {
        let yaml = "\
file: data/pvz.csv
filter:
  - field: status
    in: [active, suspended]
  - field: created_at
    gte: \"2024-01-01\"
";
        let def: SourceDefinition = serde_yaml_ng::from_str(yaml).unwrap();
        let filter = def.filter.expect("filter should exist");
        assert_eq!(filter.len(), 2);
        assert_eq!(filter[0].field, "status");
        assert_eq!(filter[0].in_values.as_ref().map(Vec::len), Some(2));
        assert_eq!(filter[1].field, "created_at");
        assert_eq!(filter[1].gte.as_deref(), Some("2024-01-01"));
    }
}
