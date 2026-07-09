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
pub mod tsv;

pub use analyzer::{
    IndexReason, IndexRequirement, SourceUsageAnalyzer, SourceUsagePlan, effective_source_name,
};
pub use csv::CsvReader;
pub use definition::{IndexMode, JoinType, SourceDefinition};
pub use detect::{SourceFormat, detect_format};
pub use driven::{
    FallbackReason, FallbackType, RuntimeFallbackPolicy, SourceDrivenConfig, SourceFallbackEvent,
};
pub use filter::{FilterCondition, matches_all as matches_filter_all};
pub use index::{BloomFilter, IndexEntry, IndexEntryV4, SourceIndex, XorFilter};
pub use memory::InMemorySource;
pub use ndjson::NdjsonReader;
pub use source_error::SourceError;
pub use source_row::SourceRow;
pub use tsv::TsvReader;
pub use twoq_cache::TwoQCache;

use crate::utils::file::FileUtils;
use anyhow::Result;
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
            Ok(Box::new(CsvReader::new(reader, delimiter)?))
        }
        SourceFormat::Tsv => Ok(Box::new(TsvReader::new(reader)?)),
        SourceFormat::Ndjson => Ok(Box::new(NdjsonReader::new(reader))),
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
    use super::*;

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
