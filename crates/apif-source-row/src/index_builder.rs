use crate::SourceRow;
use crate::index::{KeyType, SourceIndex};
use crate::{SourceDefinition, open_source_reader};
use anyhow::{Context, Result};
use apif_utils::FileUtils;
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
    dir.join(format!(
        "{file_name}.{}.gcti",
        crate::index::key_spec_slug(key_column)
    ))
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
    // Fallback only, for a reader that cannot report its position. Summing
    // decoded field lengths mis-locates every row after the first quoted field,
    // CRLF, comment, blank line or BOM.
    let mut byte_offset = header_line.len() as u64;

    let mut row_count = 0u64;
    // Collected and inserted in one pass: inserting numeric keys one at a time
    // shifts a `Vec` per row, which is O(n^2) over the file.
    let mut batch: Vec<(String, u64, u32)> = Vec::new();
    on_progress(BuildPhase::Scan, byte_offset.min(source_size), source_size);
    while let Some(row) = reader.next_row()? {
        let key_val = crate::index::composite_value(key_column, |c| row.get(c).map(str::to_string))
            .ok_or_else(|| {
                anyhow::anyhow!("column '{}' not found in row {}", key_column, row_count)
            })?;

        let (offset, row_bytes) = match reader.last_row_span() {
            Some(span) => span,
            None => (byte_offset, estimate_row_size(&row)),
        };
        batch.push((key_val, offset, row_bytes));
        byte_offset = offset + row_bytes as u64 + 1;
        row_count += 1;
        if row_count.is_multiple_of(1024) {
            on_progress(BuildPhase::Scan, byte_offset.min(source_size), source_size);
        }
    }
    index
        .batch_insert(batch)
        .with_context(|| format!("failed to index {row_count} rows of '{}'", definition.file))?;
    on_progress(BuildPhase::Scan, source_size, source_size);

    let mut index_mut = index;
    on_progress(BuildPhase::Write, 0, 1);
    index_mut
        .write_to_file(&idx_path)
        .with_context(|| format!("failed to write index to {}", idx_path.display()))?;
    on_progress(BuildPhase::Write, 1, 1);

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
    let header = header.trim_ascii().trim_start_matches('\u{feff}');
    let columns: Vec<&str> = header.split(delimiter as char).collect();

    let idx = columns
        .iter()
        .map(|c| c.trim_matches('"'))
        .position(|c| c == target_column)
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

/// Byte length of the CSV row as it appears in the source file: the field
/// values joined by commas. This must match how the row was serialized on
/// disk so that mmap offsets computed during indexing stay in sync — counting
/// header/column names here would over-count and corrupt every offset.
fn estimate_row_size(row: &SourceRow) -> u32 {
    let mut size = 0u32;
    for val in row.values() {
        size += val.len() as u32;
    }
    size + row.values().len().saturating_sub(1) as u32
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

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_and_load_index() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "data.csv", "id,name\n1,Alice\n2,Bob\n3,Charlie\n");

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
    }

    /// Regression (BUG 1): an index built on disk must store offsets/lengths
    /// that slice the exact source-row bytes back out via the mmap read path.
    /// The old math seeded the offset with `header.len() + 1` (double-counting
    /// the newline the reader already retains) and sized rows by summing
    /// column-NAME lengths, so every stored offset/length was wrong and this
    /// round-trip read out-of-bounds garbage (or bailed) instead of the row.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn index_roundtrip_reads_exact_source_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let src = create_temp_csv(
            dir,
            "regions.csv",
            "region_id,region_name\nR01,Moscow\nR02,Saint Petersburg\n",
        );

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: regions.csv\n  name: regions\n  indexed_by: [region_id]\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let idx_path = build_index_for_source(&defs[0], &doc_path).unwrap();
        let index = SourceIndex::read_from_file(&idx_path).unwrap();

        let file = std::fs::File::open(&src).unwrap();
        let mmap = unsafe { memmap2::Mmap::map(&file) }.unwrap();

        assert_eq!(
            index.lookup_row_from_mmap(mmap.as_ref(), "R01").unwrap(),
            Some("R01,Moscow".to_string())
        );
        assert_eq!(
            index.lookup_row_from_mmap(mmap.as_ref(), "R02").unwrap(),
            Some("R02,Saint Petersburg".to_string())
        );

        // A span runs to the end of the line terminator; consumers slicing raw
        // entries trim it themselves.
        let entries = index.lookup_all("R02").unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        let bytes = &mmap.as_ref()[e.offset as usize..e.offset as usize + e.row_length as usize];
        assert_eq!(
            std::str::from_utf8(crate::index::trim_row_terminator(bytes)).unwrap(),
            "R02,Saint Petersburg"
        );
    }

    /// One-at-a-time insertion of numeric keys shifts a `Vec` per row, so index
    /// build time grew quadratically with the file. `batch_insert` sorts once.
    /// The bound is generous — this asserts the *shape*, not a wall-clock
    /// figure: quadratic growth would put the larger file far past it.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn index_build_time_grows_linearly_with_row_count() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let build = |rows: usize, name: &str| {
            let mut data = String::from("id,name\n");
            // Descending ids are the worst case for insertion: every key lands
            // at the front of the vector.
            for i in (0..rows).rev() {
                data.push_str(&format!("{i},user-{i}\n"));
            }
            create_temp_csv(dir, name, &data);
            let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(&format!(
                "- file: {name}\n  name: s\n  indexed_by: [id]\n"
            ))
            .unwrap();
            let doc_path = dir.join("test.gctf");
            std::fs::write(&doc_path, "").unwrap();
            let start = std::time::Instant::now();
            let idx_path = build_index_for_source(&defs[0], &doc_path).unwrap();
            let elapsed = start.elapsed();
            let index = SourceIndex::read_from_file(&idx_path).unwrap();
            assert_eq!(index.len(), rows);
            elapsed
        };

        let small = build(4_000, "small.csv");
        let large = build(32_000, "large.csv");
        let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-6);
        println!("4k={small:?} 32k={large:?} ratio={ratio:.1}x");
        assert!(
            ratio < 24.0,
            "8x the rows took {ratio:.1}x the time ({small:?} -> {large:?}); \
             quadratic insertion would be ~64x"
        );
    }

    /// Offsets came from summing decoded field lengths, which assumes the
    /// encoding: a quoted field, a CRLF, a comment line or a BOM each shift
    /// every later row, and the lookup then lands mid-record and parses into a
    /// plausible but wrong row. Each of these rows exercises one of those.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn offsets_survive_quoting_crlf_comments_and_a_bom() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let src = dir.join("nasty.csv");
        let content = concat!(
            "\u{feff}id,name\r\n",
            "R01,\"Moscow, RU\"\r\n",
            "# a comment line the reader skips\r\n",
            "\r\n",
            "R02,\"He said \"\"hi\"\"\"\r\n",
            "R03,Kazan\r\n",
        );
        std::fs::write(&src, content).unwrap();

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: nasty.csv\n  name: nasty\n  indexed_by: [id]\n")
                .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let idx_path = build_index_for_source(&defs[0], &doc_path).unwrap();
        let index = SourceIndex::read_from_file(&idx_path).unwrap();
        let file = std::fs::File::open(&src).unwrap();
        let mmap = unsafe { memmap2::Mmap::map(&file) }.unwrap();

        for (key, expected) in [
            ("R01", "R01,\"Moscow, RU\""),
            ("R02", "R02,\"He said \"\"hi\"\"\""),
            ("R03", "R03,Kazan"),
        ] {
            let line = index
                .lookup_row_from_mmap(mmap.as_ref(), key)
                .unwrap()
                .unwrap_or_else(|| panic!("{key} must be indexed"));
            assert_eq!(line, expected, "row {key} is misaligned");
        }
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn load_or_build_creates_on_first_call() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "items.csv", "code,label\nA,Alpha\nB,Bravo\n");

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
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn load_or_build_reuses_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "data.csv", "id,val\n1,hello\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: data.csv\n  name: d\n  indexed_by: [id]\n").unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let _idx1 = load_or_build_index(&defs[0], &doc_path).unwrap();
        let _idx2 = load_or_build_index(&defs[0], &doc_path).unwrap();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_index_no_key_column_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "data.csv", "id,val\n1,hello\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: data.csv\n  name: d\n").unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let result = build_index_for_source(&defs[0], &doc_path);
        assert!(result.is_err());
    }
}
