pub mod analyzer;
pub mod csv;
pub mod definition;
pub mod detect;
pub mod driven;
pub mod filter;
pub mod index;
pub mod index_builder;
pub mod memory;
pub mod ndjson;
pub mod schema;
pub mod tsv;

pub use analyzer::{
    IndexReason, IndexRequirement, SourceUsageAnalyzer, SourceUsagePlan, effective_source_name,
};
pub use apif_source_error::SourceError;
pub use apif_twoq_cache::TwoQCache;
pub use csv::CsvReader;
pub use definition::{IndexMode, JoinType, SourceDefinition};
pub use detect::{SourceFormat, detect_format};
pub use driven::{
    FallbackReason, FallbackType, RuntimeFallbackPolicy, SourceDrivenConfig, SourceFallbackEvent,
};
pub use filter::{FilterCondition, matches_all as matches_filter_all};
pub use index::{IndexEntry, IndexEntryV4, SourceIndex};
pub use memory::InMemorySource;
pub use ndjson::NdjsonReader;
pub use tsv::TsvReader;

use anyhow::Result;
use apif_utils::FileUtils;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

pub trait SourceReader: Send {
    fn next_row(&mut self) -> Result<Option<SourceRow>>;
    fn headers(&self) -> &[String];

    /// Attempt to reset the reader to the beginning.
    /// Returns `Ok(())` even if unsupported — check `supports_reset()` first.
    fn reset(&mut self) -> Result<()>;

    /// Whether `reset()` is actually supported by this reader.
    /// Readers wrapping non-seekable streams (stdin, network) return false.
    fn supports_reset(&self) -> bool {
        false
    }
}

pub fn open_source_reader(
    definition: &SourceDefinition,
    document_path: &Path,
) -> Result<Box<dyn SourceReader>> {
    let resolved = resolve_source_path(definition, document_path);
    let file = std::fs::File::open(&resolved)
        .map_err(|e| SourceError::FileOpenFailed(resolved.display().to_string(), e))?;

    let format = if let Some(fmt) = &definition.format {
        fmt.clone()
    } else {
        detect_format(&resolved)?
    };

    let reader = BufReader::new(file);
    match format {
        SourceFormat::Csv => {
            let delimiter = definition.delimiter.unwrap_or(b',');
            Ok(Box::new(CsvReader::new_seekable(reader, delimiter)?))
        }
        SourceFormat::Tsv => Ok(Box::new(TsvReader::new_seekable(reader)?)),
        SourceFormat::Ndjson => Ok(Box::new(NdjsonReader::new_seekable(reader))),
    }
}

pub fn resolve_source_path(
    definition: &SourceDefinition,
    document_path: &Path,
) -> std::path::PathBuf {
    FileUtils::resolve_relative_path(document_path, &definition.file)
}

pub fn peek_format(reader: &mut BufReader<impl Read>) -> Result<SourceFormat> {
    let n = reader.fill_buf()?;
    let text = String::from_utf8_lossy(n);
    Ok(detect::detect_format_from_content(&text))
}

pub fn row_to_template_variables(
    source_name: &str,
    row: &SourceRow,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut vars = std::collections::HashMap::with_capacity(row.len() * 2);

    for col in row.columns() {
        if let Some(val) = row.get(col) {
            let namespaced = format!("{source_name}.{col}");
            vars.insert(namespaced, serde_json::Value::String(val.to_string()));
        }
    }

    vars
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn row_to_template_variables_namespaced() {
        let row = SourceRow::from_pairs(vec![
            ("id".into(), "42".into()),
            ("name".into(), "Alice".into()),
        ]);
        let vars = row_to_template_variables("users", &row);

        assert_eq!(
            vars.get("users.id"),
            Some(&serde_json::Value::String("42".into()))
        );
        assert_eq!(
            vars.get("users.name"),
            Some(&serde_json::Value::String("Alice".into()))
        );
        assert!(!vars.contains_key("id"));
    }

    #[test]
    fn row_to_template_variables_empty_row() {
        let row = SourceRow::from_pairs(vec![]);
        let vars = row_to_template_variables("src", &row);
        assert!(vars.is_empty());
    }
}

#[derive(Debug, Clone)]
pub struct SourceRow {
    columns: Vec<String>,
    values: Vec<String>,
}

impl SourceRow {
    pub fn new(headers: &[String], values: Vec<String>) -> Self {
        Self {
            columns: headers.to_vec(),
            values,
        }
    }

    pub fn from_csv_line(line: &str) -> Self {
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for part in line.split(',') {
            let part = part.trim_ascii();
            values.push(part.to_string());
            if columns.len() < values.len() {
                columns.push(format!("col_{}", columns.len()));
            }
        }
        Self { columns, values }
    }

    pub fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        let mut columns = Vec::with_capacity(pairs.len());
        let mut values = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            columns.push(k);
            values.push(v);
        }
        Self { columns, values }
    }

    pub fn get(&self, column: &str) -> Option<&str> {
        let idx = self.columns.iter().position(|c| c == column)?;
        self.values.get(idx).map(|s| s.as_str())
    }

    pub fn get_or(&self, column: &str, default: &str) -> String {
        self.get(column)
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn values(&self) -> &[String] {
        &self.values
    }

    pub fn to_map(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::with_capacity(self.columns.len());
        for (i, col) in self.columns.iter().enumerate() {
            if let Some(v) = self.values.get(i) {
                map.insert(col.clone(), v.clone());
            }
        }
        map
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod source_row_tests {
    use super::*;

    #[test]
    fn row_from_csv_line() {
        let row = SourceRow::from_csv_line("1,Alice,engineer");
        assert_eq!(row.columns(), &["col_0", "col_1", "col_2"]);
        assert_eq!(row.values(), &["1", "Alice", "engineer"]);
        assert_eq!(row.len(), 3);
    }

    #[test]
    fn row_from_csv_empty_line() {
        let row = SourceRow::from_csv_line("");
        // split("") on empty string produces [""]
        assert_eq!(row.len(), 1);
        assert_eq!(row.get("col_0"), Some(""));
    }

    #[test]
    fn row_new_and_get() {
        let headers = vec!["id".into(), "name".into()];
        let row = SourceRow::new(&headers, vec!["42".into(), "Alice".into()]);
        assert_eq!(row.get("id"), Some("42"));
        assert_eq!(row.get("name"), Some("Alice"));
        assert_eq!(row.get("missing"), None);
    }

    #[test]
    fn row_from_pairs() {
        let row = SourceRow::from_pairs(vec![("x".into(), "1".into()), ("y".into(), "2".into())]);
        assert_eq!(row.get("x"), Some("1"));
        assert_eq!(row.get("y"), Some("2"));
    }

    #[test]
    fn row_get_or_default() {
        let headers = vec!["id".into()];
        let row = SourceRow::new(&headers, vec!["1".into()]);
        assert_eq!(row.get_or("id", "fallback"), "1");
        assert_eq!(row.get_or("missing", "fallback"), "fallback");
    }

    #[test]
    fn row_to_map() {
        let headers = vec!["a".into(), "b".into()];
        let row = SourceRow::new(&headers, vec!["1".into(), "2".into()]);
        let map = row.to_map();
        assert_eq!(map.get("a"), Some(&"1".to_string()));
        assert_eq!(map.get("b"), Some(&"2".to_string()));
    }

    #[test]
    fn row_len_and_empty() {
        let empty_row = SourceRow::new(&[], vec![]);
        assert!(empty_row.is_empty());
        assert_eq!(empty_row.len(), 0);

        let row = SourceRow::new(&["x".into()], vec!["1".into()]);
        assert!(!row.is_empty());
        assert_eq!(row.len(), 1);
    }
}
