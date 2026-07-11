use super::index::{KeyType, SourceIndex};
use super::{SourceDefinition, open_source_reader};
use crate::utils::file::FileUtils;
use anyhow::{Context, Result};
use apif_source_row::SourceRow;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_MEMORY_LIMIT: u64 = 256 * 1024 * 1024; // 256MB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPhase {
    Scan,
    Write,
}

#[derive(Debug, Default)]
pub struct IndexMetrics {
    pub builds_total: AtomicU64,
    pub builds_failed: AtomicU64,
}

impl IndexMetrics {
    pub fn record_build_success(&self) {
        self.builds_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_build_failure(&self) {
        self.builds_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn builds_total(&self) -> u64 {
        self.builds_total.load(Ordering::Relaxed)
    }

    pub fn builds_failed(&self) -> u64 {
        self.builds_failed.load(Ordering::Relaxed)
    }
}

pub static INDEX_METRICS: LazyLock<IndexMetrics, fn() -> IndexMetrics> =
    LazyLock::new(IndexMetrics::default);

pub fn index_path_for_source(source_path: &Path, key_column: &str) -> PathBuf {
    let dir = source_path.parent().unwrap_or(Path::new("."));
    let file_name = source_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .or_else(|| {
            source_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "source".to_string());
    dir.join(format!("{file_name}.{key_column}.gcti"))
}

pub fn build_index_for_source(
    definition: &SourceDefinition,
    document_path: &Path,
) -> Result<PathBuf> {
    build_index_for_source_with_progress(definition, document_path, |_phase, _done, _total| {})
}

pub fn build_index_for_source_with_progress<F>(
    definition: &SourceDefinition,
    document_path: &Path,
    mut on_progress: F,
) -> Result<PathBuf>
where
    F: FnMut(BuildPhase, u64, u64),
{
    let result =
        build_index_for_source_with_progress_impl(definition, document_path, &mut on_progress);
    match result {
        Ok(path) => {
            INDEX_METRICS.record_build_success();
            Ok(path)
        }
        Err(e) => {
            INDEX_METRICS.record_build_failure();
            Err(e)
        }
    }
}

fn build_index_for_source_with_progress_impl<F>(
    definition: &SourceDefinition,
    document_path: &Path,
    on_progress: &mut F,
) -> Result<PathBuf>
where
    F: FnMut(BuildPhase, u64, u64),
{
    let source_path = FileUtils::resolve_relative_path(document_path, &definition.file);
    let key_columns = definition.indexed_columns();

    if key_columns.is_empty() {
        anyhow::bail!(
            "no indexed_by column specified for source '{}'",
            definition.file
        );
    }

    let key_column = &key_columns[0];
    let idx_path = index_path_for_source(&source_path, key_column);
    let source_size = std::fs::metadata(&source_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let key_type = infer_key_type_for_column(&source_path, definition, key_column, source_size)?;

    let mut reader = open_source_reader(definition, document_path)
        .with_context(|| format!("failed to open source for indexing: {}", definition.file))?;

    let mut index = SourceIndex::with_key_type(key_column, key_type);
    let header_line = read_first_line(&source_path)?;
    let mut byte_offset = header_line.len() as u64 + 1;

    let mut row_count = 0u64;
    on_progress(BuildPhase::Scan, byte_offset.min(source_size), source_size);
    while let Some(row) = reader.next_row()? {
        let key_val = row.get(key_column).ok_or_else(|| {
            anyhow::anyhow!("column '{}' not found in row {}", key_column, row_count)
        })?;

        let row_bytes = estimate_row_size(&row);
        index
            .insert(key_val.to_string(), byte_offset, row_bytes)
            .with_context(|| format!("failed to insert key '{}' at row {}", key_val, row_count))?;
        byte_offset += row_bytes as u64 + 1;
        row_count += 1;
        if row_count.is_multiple_of(1024) {
            on_progress(BuildPhase::Scan, byte_offset.min(source_size), source_size);
        }
    }
    on_progress(BuildPhase::Scan, source_size, source_size);

    let mut index_mut = index;
    on_progress(BuildPhase::Write, 0, 1);
    index_mut
        .write_to_file(&idx_path)
        .with_context(|| format!("failed to write index to {}", idx_path.display()))?;
    on_progress(BuildPhase::Write, 1, 1);

    // Warn if index file exceeds memory limit
    if let Ok(meta) = std::fs::metadata(&idx_path) {
        let size = meta.len();
        if size > DEFAULT_MEMORY_LIMIT {
            tracing::warn!(
                "Index file {} is {} MB — exceeds {} MB limit. Consider increasing memory budget or reducing dataset size.",
                idx_path.display(),
                size / (1024 * 1024),
                DEFAULT_MEMORY_LIMIT / (1024 * 1024)
            );
        }
    }

    Ok(idx_path)
}

fn infer_key_type_for_column(
    source_path: &Path,
    definition: &SourceDefinition,
    key_column: &str,
    source_size: u64,
) -> Result<KeyType> {
    let file = std::fs::File::open(source_path).with_context(|| {
        format!(
            "failed to open source for type inference: {}",
            source_path.display()
        )
    })?;
    let mut reader = std::io::BufReader::new(file);

    let key_column_idx = if definition.format == Some(super::detect::SourceFormat::Ndjson) {
        infer_ndjson_column_index(&mut reader, key_column)?
    } else {
        find_column_index(&mut reader, key_column)?
    };

    let max_bytes_scan = source_size.min(1024 * 1024);
    let (key_type, _stats) = if definition.format == Some(super::detect::SourceFormat::Ndjson) {
        super::index::infer_key_type_from_ndjson_stream(
            &mut reader,
            key_column,
            1000,
            max_bytes_scan,
        )?
    } else {
        super::index::infer_key_type_from_stream(&mut reader, key_column_idx, 1000, max_bytes_scan)?
    };

    Ok(key_type)
}

fn infer_ndjson_column_index<R: std::io::BufRead>(
    reader: &mut R,
    target_column: &str,
) -> Result<usize> {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => anyhow::bail!("empty NDJSON file, cannot infer column index"),
            Ok(_) => {}
            Err(e) => anyhow::bail!("failed to read NDJSON for column inference: {}", e),
        }
        let trimmed = line.trim_ascii();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let obj: serde_json::Map<String, serde_json::Value> = serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("invalid JSON in NDJSON: {}", e))?;
        let mut keys: Vec<String> = obj.keys().cloned().collect();
        keys.sort();
        let idx = keys
            .iter()
            .position(|k| k == target_column)
            .with_context(|| format!("column '{}' not found in NDJSON object", target_column))?;
        return Ok(idx);
    }
}

pub fn find_column_index<R: std::io::BufRead + std::io::Seek>(
    reader: &mut R,
    target_column: &str,
) -> Result<usize> {
    reader.seek(std::io::SeekFrom::Start(0))?;
    let mut header = String::new();
    reader.read_line(&mut header)?;

    let delimiter = if header.contains('\t') { b'\t' } else { b',' };
    let columns: Vec<&str> = header.trim_ascii().split(delimiter as char).collect();

    let idx = columns
        .iter()
        .position(|&c| c == target_column)
        .with_context(|| format!("column '{}' not found in source header", target_column))?;

    Ok(idx)
}

pub fn load_or_build_index(
    definition: &SourceDefinition,
    document_path: &Path,
) -> Result<SourceIndex> {
    let source_path = FileUtils::resolve_relative_path(document_path, &definition.file);
    let key_columns = definition.indexed_columns();

    if key_columns.is_empty() {
        anyhow::bail!("no indexed_by column for source '{}'", definition.file);
    }

    let key_column = &key_columns[0];
    let idx_path = index_path_for_source(&source_path, key_column);

    if idx_path.exists()
        && let Ok(index) = SourceIndex::read_from_file(&idx_path)
        && is_index_fresh(&idx_path, &source_path)
    {
        return Ok(index);
    }

    build_index_for_source(definition, document_path)?;
    SourceIndex::read_from_file(&idx_path)
}

fn is_index_fresh(idx_path: &Path, source_path: &Path) -> bool {
    let idx_meta = match std::fs::metadata(idx_path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let src_meta = match std::fs::metadata(source_path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    if let (Ok(idx_time), Ok(src_time)) = (idx_meta.modified(), src_meta.modified()) {
        return idx_time >= src_time;
    }

    true
}

fn read_first_line(path: &Path) -> Result<String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line)
}

fn estimate_row_size(row: &SourceRow) -> u32 {
    let mut size = 0u32;
    for col in row.columns() {
        size += col.len() as u32 + 1;
    }
    for val in row.values() {
        size += val.len() as u32;
    }
    size + row.columns().len().saturating_sub(1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(miri))]
    use std::io::Write;

    #[cfg(not(miri))]
    fn create_temp_csv(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn index_path_naming() {
        let path = Path::new("data/pvz.csv");
        let idx = index_path_for_source(path, "region_id");
        assert_eq!(idx, PathBuf::from("data/pvz.csv.region_id.gcti"));
    }

    #[cfg(not(miri))]
    #[test]
    fn build_and_load_index() {
        let dir = std::env::temp_dir().join("gctf_idx_build_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "data.csv", "id,name\n1,Alice\n2,Bob\n3,Charlie\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: data.csv\n  name: data\n  indexed_by: [id]\n")
                .unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let idx_path = build_index_for_source(&defs[0], &doc_path).unwrap();
        assert!(idx_path.exists());

        let index = SourceIndex::read_from_file(&idx_path).unwrap();
        assert_eq!(index.len(), 3);
        assert_eq!(index.key_column(), "id");
        assert!(index.contains("1"));
        assert!(index.contains("2"));
        assert!(index.contains("3"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(not(miri))]
    #[test]
    fn load_or_build_creates_on_first_call() {
        let dir = std::env::temp_dir().join("gctf_idx_auto_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "items.csv", "code,label\nA,Alpha\nB,Bravo\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: items.csv\n  name: items\n  indexed_by: [code]\n")
                .unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let expected_idx = dir.join("items.csv.code.gcti");
        assert!(
            !expected_idx.exists(),
            "stale index file should not exist: {}",
            expected_idx.display()
        );

        let index = load_or_build_index(&defs[0], &doc_path).unwrap();
        assert!(expected_idx.exists());
        assert_eq!(index.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(not(miri))]
    #[test]
    fn load_or_build_reuses_existing() {
        let dir = std::env::temp_dir().join("gctf_idx_reuse_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "data.csv", "id,val\n1,hello\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: data.csv\n  name: d\n  indexed_by: [id]\n").unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let _idx1 = load_or_build_index(&defs[0], &doc_path).unwrap();
        let _idx2 = load_or_build_index(&defs[0], &doc_path).unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(not(miri))]
    #[test]
    fn build_index_no_key_column_errors() {
        let dir = std::env::temp_dir().join("gctf_idx_nokey_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "data.csv", "id,val\n1,hello\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: data.csv\n  name: d\n").unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let result = build_index_for_source(&defs[0], &doc_path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
