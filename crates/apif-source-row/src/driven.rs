#![allow(clippy::unwrap_used, clippy::expect_used)] // audited safe
use crate::SourceRow;
use crate::filter::{FilterCondition, matches_all as matches_filter_all};
use crate::index::SourceIndex;
use crate::index_builder::index_path_for_source;
use crate::memory::InMemorySource;
use crate::{SourceDefinition, SourceReader, open_source_reader};
use anyhow::{Context, Result};
use apif_twoq_cache::TwoQCache;
use apif_utils::FileUtils;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::warn;

/// Default capacities for the dimension row cache.
/// The hot queue holds frequently-referenced rows; the cold queue absorbs
/// one-time lookups to prevent cache pollution.
const DIMENSION_CACHE_HOT: usize = 2048;
const DIMENSION_CACHE_COLD: usize = 8192;

const ENV_DIMENSION_MEMORY_BUDGET: &str = "GRPCTESTIFY_DIMENSION_MEMORY_BUDGET";
const MAX_DIMENSION_MEMORY_BUDGET: u64 = 512 * 1024 * 1024;
const MIN_DIMENSION_MEMORY_BUDGET: u64 = 32 * 1024 * 1024;

/// RAM per byte of dimension file once loaded; measured 12.02x on a 5-column
/// CSV. The budget is a RAM budget, so the file size must be scaled by it.
pub(crate) const DIMENSION_MEMORY_EXPANSION: u64 = 12;

fn resolve_dimension_budget() -> u64 {
    if let Ok(val) = std::env::var(ENV_DIMENSION_MEMORY_BUDGET)
        && !val.is_empty()
        && let Ok(bytes) = parse_bytes(&val)
    {
        return bytes;
    }

    let mut sys = sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing());
    sys.refresh_memory();
    let available = sys.available_memory();

    if available == 0 {
        return MIN_DIMENSION_MEMORY_BUDGET;
    }

    (available / 2).clamp(MIN_DIMENSION_MEMORY_BUDGET, MAX_DIMENSION_MEMORY_BUDGET)
}

/// A source's own `memory_budget`, else the run-wide one.
fn task_budget(def: &SourceDefinition, default_budget: u64) -> u64 {
    def.memory_budget
        .as_deref()
        .and_then(|v| parse_bytes(v).ok())
        .unwrap_or(default_budget)
}

/// Whether a dimension of this file size can be held in memory within `budget`.
fn fits_in_memory(file_size: u64, budget: u64) -> bool {
    file_size
        .checked_mul(DIMENSION_MEMORY_EXPANSION)
        .is_some_and(|needed| needed <= budget)
}

fn parse_bytes(s: &str) -> Result<u64> {
    let s = s.trim_ascii().to_ascii_lowercase();
    let split_pos = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && *c != '.')
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let num_str = &s[..split_pos];
    let unit = s[split_pos..].trim_ascii();
    let num: f64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid number: {num_str}"))?;
    let bytes = match unit {
        "" | "b" => num,
        "kb" | "k" => num * 1024.0,
        "mb" | "m" => num * 1024.0 * 1024.0,
        "gb" | "g" => num * 1024.0 * 1024.0 * 1024.0,
        other => anyhow::bail!("unknown unit: {other} (use kb, mb, gb)"),
    };
    Ok(bytes as u64)
}

pub enum DimensionSource {
    Memory(Arc<InMemorySource>),
    Indexed(Box<IndexedDimension>),
}

/// How to turn one raw line of an indexed dimension back into fields.
#[derive(Debug, Clone, Copy)]
pub enum RowDecoder {
    Delimited(u8),
    Ndjson,
}

pub struct IndexedDimension {
    pub index: Arc<SourceIndex>,
    pub mmap: memmap2::Mmap,
    pub cache: Mutex<TwoQCache<String, SourceRow>>,
    /// Column names of the source, so rows read from the mmap carry the same
    /// field names an in-memory dimension would (`dim.name`, not `dim.col_0`).
    pub headers: Vec<String>,
    pub decoder: RowDecoder,
    /// Applied on lookup. An in-memory dimension filters at load; without this
    /// the same YAML filtered or not depending on free RAM.
    pub filter: Vec<FilterCondition>,
}

impl IndexedDimension {
    fn row_from_line(&self, line: &str) -> SourceRow {
        let values = match self.decoder {
            RowDecoder::Ndjson => self.ndjson_values(line),
            RowDecoder::Delimited(delimiter) => delimited_values(line, delimiter),
        };
        match values {
            Some(values) if !self.headers.is_empty() => SourceRow::new(&self.headers, values),
            Some(values) => SourceRow::new(
                &(0..values.len())
                    .map(|i| format!("col_{i}"))
                    .collect::<Vec<_>>(),
                values,
            ),
            None => SourceRow::from_csv_line(line),
        }
    }

    fn ndjson_values(&self, line: &str) -> Option<Vec<String>> {
        let serde_json::Value::Object(obj) = serde_json::from_str(line).ok()? else {
            return None;
        };
        Some(
            self.headers
                .iter()
                .map(|k| match obj.get(k) {
                    None | Some(serde_json::Value::Null) => String::new(),
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(v) => v.to_string(),
                })
                .collect(),
        )
    }
}

fn delimited_values(line: &str, delimiter: u8) -> Option<Vec<String>> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(line.as_bytes());
    let mut record = csv::StringRecord::new();
    if reader.read_record(&mut record).ok()? {
        Some(record.iter().map(|f| f.trim_ascii().to_string()).collect())
    } else {
        None
    }
}

impl DimensionSource {
    fn lookup_row(&self, key: &str) -> Result<Option<SourceRow>> {
        match self {
            DimensionSource::Memory(mem) => Ok(mem.lookup(key).cloned()),
            DimensionSource::Indexed(idx) => {
                let mut cache = idx.cache.lock().expect("cache mutex poisoned");
                if let Some(row) = cache.get(&key.to_string()) {
                    return Ok(Some(row.clone()));
                }
                let Some(line) = idx.index.lookup_row_from_mmap(idx.mmap.as_ref(), key)? else {
                    return Ok(None);
                };
                let row = idx.row_from_line(&line);
                if !idx.filter.is_empty() && !matches_filter_all(&row, &idx.filter) {
                    return Ok(None);
                }
                cache.insert(key.to_string(), row.clone());
                Ok(Some(row))
            }
        }
    }

    /// Look up ALL rows matching the given key.
    fn lookup_all(&self, key: &str) -> Vec<SourceRow> {
        match self {
            DimensionSource::Memory(mem) => mem
                .iter()
                .filter(|(k, _)| k.as_str() == key)
                .map(|(_, row)| row.clone())
                .collect(),
            DimensionSource::Indexed(idx) => {
                let Some(entries) = idx.index.lookup_all(key) else {
                    return Vec::new();
                };
                let data = idx.mmap.as_ref();
                entries
                    .iter()
                    .filter_map(|entry| {
                        let start = entry.offset as usize;
                        let end = start.checked_add(entry.row_length as usize)?;
                        let bytes = crate::index::trim_row_terminator(data.get(start..end)?);
                        std::str::from_utf8(bytes)
                            .ok()
                            .map(|line| idx.row_from_line(line))
                    })
                    .filter(|row| idx.filter.is_empty() || matches_filter_all(row, &idx.filter))
                    .collect()
            }
        }
    }
}

struct DimensionJoin {
    source_name: String,
    foreign_key: String,
    join_type: super::definition::JoinType,
}

/// Tracks cross-product iteration state for a single primary row.
/// Stores the row and pre-computed dimension lookup results.
struct CrossProductState {
    row: SourceRow,
    cross_matches: Vec<Vec<SourceRow>>,
    cross_indices: Vec<usize>,
}

#[derive(Clone)]
struct DimTask {
    name: String,
    resolved_path: PathBuf,
    key_col: String,
    file_size: u64,
    def: SourceDefinition,
}

fn load_dimension_source(
    def: &SourceDefinition,
    document_path: &Path,
    resolved_path: &Path,
    key_col: &str,
) -> Result<DimensionSource> {
    let mut reader = open_source_reader(def, document_path)
        .with_context(|| format!("failed to open dimension source '{}'", def.file))?;
    if reader.headers().is_empty() {
        // NDJSON has no header line: its columns come from the first record.
        reader.next_row()?;
    }
    let headers = reader.headers().to_vec();
    drop(reader);
    let effective_key = if key_col.is_empty() {
        headers.first().cloned().unwrap_or_default()
    } else {
        key_col.to_string()
    };

    let index =
        load_or_build_index_with_key(def, document_path, &effective_key).with_context(|| {
            format!(
                "failed to build/load index for dimension '{}'",
                resolved_path.display()
            )
        })?;
    let file = std::fs::File::open(resolved_path)
        .with_context(|| format!("failed to open dimension file: {}", resolved_path.display()))?;
    // SAFETY: no safe std mmap API; sound while the read-only, run-owned dimension file isn't truncated/mutated concurrently.
    let mmap = unsafe { memmap2::Mmap::map(&file) }
        .with_context(|| format!("failed to mmap dimension file: {}", resolved_path.display()))?;
    Ok(DimensionSource::Indexed(Box::new(IndexedDimension {
        index: Arc::new(index),
        mmap,
        cache: Mutex::new(TwoQCache::new(DIMENSION_CACHE_HOT, DIMENSION_CACHE_COLD)),
        headers,
        decoder: row_decoder_for(def, resolved_path),
        filter: def.filter.clone().unwrap_or_default(),
    })))
}

/// The line decoder a dimension's format implies.
fn row_decoder_for(def: &SourceDefinition, resolved_path: &Path) -> RowDecoder {
    let format = def
        .format
        .clone()
        .or_else(|| crate::detect::detect_format(resolved_path).ok());
    match format {
        Some(crate::detect::SourceFormat::Ndjson) => RowDecoder::Ndjson,
        Some(crate::detect::SourceFormat::Tsv) => RowDecoder::Delimited(b'\t'),
        _ => RowDecoder::Delimited(def.delimiter.unwrap_or(b',')),
    }
}

fn load_dimension_in_memory(
    def: &SourceDefinition,
    document_path: &Path,
    resolved_path: &Path,
    key_col: &str,
) -> Result<DimensionSource> {
    let mut reader = open_source_reader(def, document_path)
        .with_context(|| format!("failed to open dimension source '{}'", def.file))?;
    let effective_key = if key_col.is_empty() {
        reader.headers().first().cloned().unwrap_or_default()
    } else {
        key_col.to_string()
    };
    let mem = InMemorySource::load(&mut *reader, &effective_key)
        .with_context(|| format!("failed to load dimension '{}'", resolved_path.display()))?;
    let mem = if let Some(ref filter) = def.filter {
        let filtered = mem.filter(filter);
        Arc::new(filtered)
    } else {
        Arc::new(mem)
    };
    Ok(DimensionSource::Memory(mem))
}

/// The dimension key a primary row carries for a join, composite or not.
fn join_key(row: &SourceRow, spec: &str) -> Option<String> {
    crate::index::composite_value(spec, |c| row.get(c).map(str::to_string))
}

/// Rows read from the primary source per refill. Large enough that the reader
/// lock and the CSV parse amortise across many requests, small enough that a
/// short run does not read far past what it uses.
const PRIMARY_BATCH_ROWS: usize = 256;

pub struct SourceDrivenConfig {
    pub primary: Arc<Mutex<Box<dyn SourceReader>>>,
    /// Rows pulled from `primary` ahead of demand. Parsing a CSV record is far
    /// more expensive than handing one out, and every bench worker pulls from
    /// the same reader — doing the parse under the reader lock serialised all
    /// of them behind it. Refilling in batches keeps the parse off the
    /// per-request critical section.
    primary_batch: Mutex<std::collections::VecDeque<SourceRow>>,
    pub primary_name: String,
    pub dimensions: HashMap<String, DimensionSource>,
    pub resolved_paths: HashMap<String, PathBuf>,
    dim_joins: Vec<DimensionJoin>,
    primary_filter: Vec<FilterCondition>,
    pub load_stats: DimLoadStats,
    pub runtime_stats: SourceRuntimeStats,
    /// Cross-products awaiting emission, one per primary row that produced
    /// them. A single `Option` slot let two workers that each pulled a
    /// distinct row both install state, the second silently discarding the
    /// first row's entire product.
    cross_product_state: std::sync::Mutex<std::collections::VecDeque<CrossProductState>>,
    /// Whether any join is a CROSS. Without it the cross-product mutex was
    /// acquired on every single request even when no join could ever populate
    /// it.
    has_cross_join: bool,
    pub loaded_at: std::time::Instant,
    pub current_row: std::sync::atomic::AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct DimLoadStats {
    pub in_memory_count: usize,
    pub indexed_count: usize,
    pub total_file_bytes: u64,
    pub index_build_ms: u64,
}

/// Runtime statistics for dimension source lookups.
/// All counters use `Relaxed` atomic ordering — values are approximate
/// and intended for observability only, not for decision-making.
#[derive(Debug)]
pub struct SourceRuntimeStats {
    pub dimension_lookups: std::sync::atomic::AtomicU64,
    pub dimension_hits: std::sync::atomic::AtomicU64,
    pub dimension_misses: std::sync::atomic::AtomicU64,
    pub in_memory_lookups: std::sync::atomic::AtomicU64,
    pub indexed_lookups: std::sync::atomic::AtomicU64,
}

/// Consistent snapshot of runtime stats at a point in time.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatsSnapshot {
    pub dimension_lookups: u64,
    pub dimension_hits: u64,
    pub dimension_misses: u64,
    pub in_memory_lookups: u64,
    pub indexed_lookups: u64,
}

impl SourceRuntimeStats {
    /// Take a consistent snapshot of all counters.
    pub fn snapshot(&self) -> RuntimeStatsSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        RuntimeStatsSnapshot {
            dimension_lookups: self.dimension_lookups.load(Relaxed),
            dimension_hits: self.dimension_hits.load(Relaxed),
            dimension_misses: self.dimension_misses.load(Relaxed),
            in_memory_lookups: self.in_memory_lookups.load(Relaxed),
            indexed_lookups: self.indexed_lookups.load(Relaxed),
        }
    }
}

impl Default for SourceRuntimeStats {
    fn default() -> Self {
        Self {
            dimension_lookups: std::sync::atomic::AtomicU64::new(0),
            dimension_hits: std::sync::atomic::AtomicU64::new(0),
            dimension_misses: std::sync::atomic::AtomicU64::new(0),
            in_memory_lookups: std::sync::atomic::AtomicU64::new(0),
            indexed_lookups: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Clone for SourceRuntimeStats {
    fn clone(&self) -> Self {
        Self {
            dimension_lookups: std::sync::atomic::AtomicU64::new(
                self.dimension_lookups
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            dimension_hits: std::sync::atomic::AtomicU64::new(
                self.dimension_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            dimension_misses: std::sync::atomic::AtomicU64::new(
                self.dimension_misses
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            in_memory_lookups: std::sync::atomic::AtomicU64::new(
                self.in_memory_lookups
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            indexed_lookups: std::sync::atomic::AtomicU64::new(
                self.indexed_lookups
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl SourceRuntimeStats {
    pub fn record_lookup(&self, _source_name: &str, found: bool, is_indexed: bool) {
        use std::sync::atomic::Ordering;
        self.dimension_lookups.fetch_add(1, Ordering::Relaxed);
        if found {
            self.dimension_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.dimension_misses.fetch_add(1, Ordering::Relaxed);
        }
        if is_indexed {
            self.indexed_lookups.fetch_add(1, Ordering::Relaxed);
        } else {
            self.in_memory_lookups.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl SourceDrivenConfig {
    pub fn prepare(definitions: &[SourceDefinition], document_path: &Path) -> Result<Option<Self>> {
        if definitions.is_empty() {
            return Ok(None);
        }

        let primary_def = &definitions[0];
        let primary_name = primary_def
            .name
            .clone()
            .unwrap_or_else(|| "primary".to_string());

        let primary_reader = open_source_reader(primary_def, document_path)
            .with_context(|| format!("failed to open primary source '{}'", primary_def.file))?;
        let primary_filter = primary_def.filter.clone().unwrap_or_default();

        let mut dimensions = HashMap::new();
        let mut resolved_paths = HashMap::new();
        let mut dim_joins = Vec::new();
        let mut dim_tasks: Vec<DimTask> = Vec::new();

        for def in &definitions[1..] {
            let dim_name = def.name.clone().unwrap_or_else(|| "dim".to_string());

            let resolved = FileUtils::resolve_relative_path(document_path, &def.file);
            // A file we cannot stat is treated as too large: indexing it is
            // slower but bounded, while assuming zero loads it whole.
            let file_size = std::fs::metadata(&resolved)
                .map(|m| m.len())
                .unwrap_or(u64::MAX);

            let key_col = def
                .indexed_by
                .as_ref()
                .map(|v| v.join(super::index::COMPOSITE_KEY_SEPARATOR))
                .unwrap_or_default();

            dim_joins.push(DimensionJoin {
                source_name: dim_name.clone(),
                foreign_key: key_col.clone(),
                join_type: def.join_type_or_default(),
            });

            resolved_paths.insert(dim_name.clone(), resolved.clone());
            dim_tasks.push(DimTask {
                name: dim_name,
                resolved_path: resolved,
                key_col,
                file_size,
                def: def.clone(),
            });
        }

        let memory_bb = resolve_dimension_budget();
        let mut in_memory: Vec<DimTask> = Vec::new();
        let mut too_large: Vec<DimTask> = Vec::new();
        let mut total_file_bytes = 0u64;
        for task in dim_tasks {
            total_file_bytes = total_file_bytes.saturating_add(task.file_size);
            if fits_in_memory(task.file_size, task_budget(&task.def, memory_bb)) {
                in_memory.push(task);
            } else {
                too_large.push(task);
            }
        }
        in_memory.sort_by_key(|t| t.file_size);

        let task_count = in_memory.len() + too_large.len();
        let all_tasks: Vec<DimTask> = in_memory.iter().chain(too_large.iter()).cloned().collect();
        let stats = Arc::new(std::sync::Mutex::new((0usize, 0usize, 0u64)));

        let results: Vec<(String, Result<DimensionSource>)> = if task_count <= 1 {
            all_tasks
                .into_iter()
                .map(|t| {
                    let start = std::time::Instant::now();
                    let src = if fits_in_memory(t.file_size, task_budget(&t.def, memory_bb)) {
                        load_dimension_in_memory(
                            &t.def,
                            document_path,
                            &t.resolved_path,
                            &t.key_col,
                        )
                    } else {
                        load_dimension_source(&t.def, document_path, &t.resolved_path, &t.key_col)
                    };
                    let elapsed = start.elapsed().as_millis() as u64;
                    let mut s = stats.lock().expect("stats mutex should not be poisoned");
                    if fits_in_memory(t.file_size, task_budget(&t.def, memory_bb)) {
                        s.0 += 1;
                    } else {
                        s.1 += 1;
                    }
                    s.2 += elapsed;
                    (t.name, src)
                })
                .collect()
        } else {
            std::thread::scope(|s| {
                all_tasks
                    .into_iter()
                    .map(|t| {
                        let doc_path = document_path.to_path_buf();
                        let mem_budget = memory_bb;
                        let stats = Arc::clone(&stats);
                        s.spawn(move || {
                            let start = std::time::Instant::now();
                            let src = if t.file_size <= mem_budget {
                                load_dimension_in_memory(
                                    &t.def,
                                    &doc_path,
                                    &t.resolved_path,
                                    &t.key_col,
                                )
                            } else {
                                load_dimension_source(
                                    &t.def,
                                    &doc_path,
                                    &t.resolved_path,
                                    &t.key_col,
                                )
                            };
                            let elapsed = start.elapsed().as_millis() as u64;
                            let mut ss = stats.lock().expect("stats mutex should not be poisoned");
                            if t.file_size <= mem_budget {
                                ss.0 += 1;
                            } else {
                                ss.1 += 1;
                            }
                            ss.2 += elapsed;
                            (t.name, src)
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|h| h.join().expect("dimension load thread panicked"))
                    .collect()
            })
        };

        let (in_memory_count, indexed_count, index_build_ms) =
            *stats.lock().expect("stats mutex should not be poisoned");

        for (name, result) in results {
            match result {
                Ok(ds) => {
                    dimensions.insert(name, ds);
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("failed to load dimension '{}'", name));
                }
            }
        }

        let has_cross_join = dim_joins
            .iter()
            .any(|j| j.join_type == super::definition::JoinType::Cross);

        Ok(Some(Self {
            primary: Arc::new(Mutex::new(primary_reader)),
            primary_batch: Mutex::new(std::collections::VecDeque::new()),
            primary_name,
            dimensions,
            resolved_paths,
            has_cross_join,
            dim_joins,
            primary_filter,
            cross_product_state: std::sync::Mutex::new(std::collections::VecDeque::new()),
            loaded_at: std::time::Instant::now(),
            current_row: std::sync::atomic::AtomicU64::new(0),
            load_stats: DimLoadStats {
                in_memory_count,
                indexed_count,
                total_file_bytes,
                index_build_ms,
            },
            runtime_stats: SourceRuntimeStats::default(),
        }))
    }

    /// Rewind the primary source to the top, for a duration run that outlives
    /// its data. Clears the read-ahead batch and any half-emitted cross-product
    /// so the first row after the wrap is the first row of the file — replaying
    /// the leftover cross-product of the pre-rewind row was a real defect.
    pub fn rewind(&self) -> Result<()> {
        // Lock order is reader -> batch everywhere, including here. Taking the
        // batch first deadlocked against `next_primary_row`, which holds the
        // reader across its refill.
        let mut reader = self.primary.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        self.primary_batch
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .clear();
        if let Ok(mut state) = self.cross_product_state.lock() {
            state.clear();
        }
        reader.reset()
    }

    /// Next filtered row from the primary source, refilling from the reader in
    /// batches.
    fn next_primary_row(&self) -> Result<Option<SourceRow>> {
        if let Some(row) = self
            .primary_batch
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .pop_front()
        {
            return Ok(Some(row));
        }

        // Refill into a local buffer with only the reader lock held. Holding
        // the batch lock across the parse too made every other worker wait out
        // a whole 256-row parse rather than just a pop. Lock order is
        // reader -> batch, and the fast path above takes batch alone, so there
        // is no cycle.
        let mut reader = self.primary.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(row) = self
            .primary_batch
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .pop_front()
        {
            // Someone refilled while we waited for the reader.
            return Ok(Some(row));
        }

        let mut refill = Vec::with_capacity(PRIMARY_BATCH_ROWS);
        while refill.len() < PRIMARY_BATCH_ROWS {
            match reader.next_row()? {
                Some(r) => {
                    if self.primary_filter.is_empty()
                        || matches_filter_all(&r, &self.primary_filter)
                    {
                        refill.push(r);
                    }
                }
                None => break,
            }
        }
        drop(reader);

        let mut batch = self
            .primary_batch
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        batch.extend(refill);
        Ok(batch.pop_front())
    }

    pub fn next_row_variables(&self) -> Result<Option<HashMap<String, Value>>> {
        // An in-flight cross-product is observed and consumed under one lock
        // acquisition. Checking `is_some()`, dropping the guard and re-locking
        // let two workers both see the state, the first drain the last
        // combination, and the second unwrap `None`.
        if self.has_cross_join
            && let Some(vars) = self.next_cross_product_combination()?
        {
            return Ok(Some(vars));
        }

        // Loop (rather than recurse) so a long run of INNER-join misses can't
        // blow the stack.
        let row = loop {
            let candidate = match self.next_primary_row()? {
                Some(r) => r,
                None => return Ok(None),
            };

            // Check INNER join constraints — skip row if the FK column is
            // absent or its value has no match in the dimension.
            let inner_missing = self.dim_joins.iter().any(|j| {
                j.join_type == super::definition::JoinType::Inner
                    && join_key(&candidate, &j.foreign_key)
                        .is_none_or(|fk| self.dimension_lookup(&j.source_name, &fk).is_none())
            });
            if inner_missing {
                continue;
            }
            break candidate;
        };

        let mut vars = self.build_primary_vars(&row);

        // LEFT and INNER both contribute the dimension's fields; INNER only
        // differs in dropping the primary row when there is no match, which the
        // loop above has already done. Restricting this to LEFT meant an INNER
        // join filtered rows and then injected nothing.
        for join in &self.dim_joins {
            if !matches!(
                join.join_type,
                super::definition::JoinType::Left | super::definition::JoinType::Inner
            ) {
                continue;
            }
            if let Some(fk_val) = join_key(&row, &join.foreign_key)
                && let Some(dim_row) = self.dimension_lookup(&join.source_name, &fk_val)
            {
                for col in dim_row.columns() {
                    if let Some(val) = dim_row.get(col) {
                        vars.insert(
                            format!("{}.{}", join.source_name, col),
                            Value::String(val.to_string()),
                        );
                    }
                }
            }
        }

        // Check for CROSS joins — build cross-product state
        let has_cross = self
            .dim_joins
            .iter()
            .any(|j| j.join_type == super::definition::JoinType::Cross);
        if has_cross {
            let mut cross_matches: Vec<Vec<SourceRow>> = Vec::new();
            for join in &self.dim_joins {
                if join.join_type != super::definition::JoinType::Cross {
                    continue;
                }
                if let Some(fk_val) = join_key(&row, &join.foreign_key) {
                    if let Some(all_rows) = self.dimension_lookup_all(&join.source_name, &fk_val) {
                        cross_matches.push(all_rows);
                    } else {
                        cross_matches.push(Vec::new());
                    }
                } else {
                    cross_matches.push(Vec::new());
                }
            }

            self.cross_product_state
                .lock()
                .map_err(|e| anyhow::anyhow!("cross_product_state mutex poisoned: {e}"))?
                .push_back(CrossProductState {
                    row,
                    cross_matches,
                    cross_indices: vec![
                        0;
                        self.dim_joins
                            .iter()
                            .filter(|j| j.join_type == super::definition::JoinType::Cross)
                            .count()
                    ],
                });
            return self.next_cross_product_combination();
        }

        self.current_row
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Some(vars))
    }

    /// Yield the next combination from a cross-product state.
    fn next_cross_product_combination(&self) -> Result<Option<HashMap<String, Value>>> {
        let mut state_guard = self
            .cross_product_state
            .lock()
            .map_err(|e| anyhow::anyhow!("cross_product_state mutex poisoned: {e}"))?;
        // `None` means "no cross-product in flight", not "source exhausted" —
        // the caller falls through to pulling a fresh primary row.
        let Some(state) = state_guard.front_mut() else {
            return Ok(None);
        };
        let mut vars = self.build_primary_vars(&state.row);

        // Inject dimension fields for each cross join at the current index
        let mut cross_idx = 0;
        for join in &self.dim_joins {
            if join.join_type != super::definition::JoinType::Cross {
                continue;
            }
            if let Some(matches) = state.cross_matches.get(cross_idx) {
                let idx = state.cross_indices[cross_idx];
                if idx < matches.len() {
                    let dim_row = &matches[idx];
                    for col in dim_row.columns() {
                        if let Some(val) = dim_row.get(col) {
                            vars.insert(
                                format!("{}.{}", join.source_name, col),
                                Value::String(val.to_string()),
                            );
                        }
                    }
                }
            }
            cross_idx += 1;
        }

        // Advance the cross-product indices (lexicographic order)
        let cross_count = self
            .dim_joins
            .iter()
            .filter(|j| j.join_type == super::definition::JoinType::Cross)
            .count();
        // Advance in place. Rebuilding the state cloned the whole match set on
        // every emitted combination — O(M^2) row clones for one primary row.
        let mut advanced = false;
        for i in (0..cross_count).rev() {
            let max = state.cross_matches[i].len();
            if max == 0 {
                continue;
            }
            if state.cross_indices[i] + 1 < max {
                state.cross_indices[i] += 1;
                advanced = true;
                break;
            }
            state.cross_indices[i] = 0;
        }

        if !advanced {
            state_guard.pop_front();
        }

        self.current_row
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Some(vars))
    }

    fn build_primary_vars(&self, row: &SourceRow) -> HashMap<String, Value> {
        // `zip`, not `row.get(col)` per column: `get` is a linear scan with a
        // string compare, so building an already index-aligned row cost O(C^2)
        // comparisons.
        let mut vars = HashMap::with_capacity(row.columns().len());
        for (col, val) in row.columns().iter().zip(row.values()) {
            let mut key = String::with_capacity(self.primary_name.len() + 1 + col.len());
            key.push_str(&self.primary_name);
            key.push('.');
            key.push_str(col);
            vars.insert(key, Value::String(val.clone()));
        }
        vars
    }

    pub fn dimension_lookup(&self, source_name: &str, key: &str) -> Option<SourceRow> {
        let dim = self.dimensions.get(source_name)?;
        let is_indexed = matches!(dim, DimensionSource::Indexed(_));
        let result = dim.lookup_row(key).ok().flatten();
        self.runtime_stats
            .record_lookup(source_name, result.is_some(), is_indexed);
        result
    }

    pub fn dimension_lookup_all(&self, source_name: &str, key: &str) -> Option<Vec<SourceRow>> {
        let dim = self.dimensions.get(source_name)?;
        let rows = dim.lookup_all(key);
        if rows.is_empty() { None } else { Some(rows) }
    }

    pub fn primary_headers(&self) -> Vec<String> {
        let reader = self.primary.lock().ok();
        match reader {
            Some(r) => r.headers().to_vec(),
            None => Vec::new(),
        }
    }
}

fn load_or_build_index_with_key(
    def: &SourceDefinition,
    document_path: &Path,
    key_col: &str,
) -> Result<SourceIndex> {
    let source_path = FileUtils::resolve_relative_path(document_path, &def.file);
    let idx_path = index_path_for_source(&source_path, key_col);

    if idx_path.exists() {
        match SourceIndex::read_from_file(&idx_path) {
            Ok(index) => {
                let idx_meta = std::fs::metadata(&idx_path);
                let src_meta = std::fs::metadata(&source_path);
                if let (Ok(im), Ok(sm)) = (idx_meta, src_meta)
                    && let (Ok(it), Ok(st)) = (im.modified(), sm.modified())
                    && it >= st
                {
                    return Ok(index);
                }
            }
            Err(e) => {
                if is_corruption_error(&e) {
                    warn!(
                        "Index corrupted (checksum mismatch), rebuilding: {}. Error: {}",
                        idx_path.display(),
                        e
                    );
                    let _ = std::fs::remove_file(&idx_path);
                }
            }
        }
    }

    let mut reader = open_source_reader(def, document_path)
        .with_context(|| format!("failed to open source for indexing: {}", def.file))?;

    let mut index = SourceIndex::new(key_col);
    let header_line = read_first_line(&source_path)?;
    // Fallback only; see `SourceReader::last_row_span`.
    let mut byte_offset = header_line.len() as u64;
    let mut row_count = 0u64;

    let mut batch: Vec<(String, u64, u32)> = Vec::new();
    while let Some(row) = reader.next_row()? {
        let key_val = crate::index::composite_value(key_col, |c| row.get(c).map(str::to_string))
            .ok_or_else(|| {
                anyhow::anyhow!("column '{}' not found in row {}", key_col, row_count)
            })?;
        let (offset, row_bytes) = match reader.last_row_span() {
            Some(span) => span,
            None => (byte_offset, estimate_row_size(&row)),
        };
        batch.push((key_val, offset, row_bytes));
        byte_offset = offset + row_bytes as u64 + 1;
        row_count += 1;
    }
    index
        .batch_insert(batch)
        .with_context(|| format!("failed to index {row_count} rows of '{}'", def.file))?;

    let parent = idx_path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).ok();
    let mut index_mut = index;
    index_mut
        .write_to_file(&idx_path)
        .with_context(|| format!("failed to write index to {}", idx_path.display()))?;

    SourceIndex::read_from_file(&idx_path)
}

fn is_corruption_error(e: &anyhow::Error) -> bool {
    let msg = e.to_string();
    msg.contains("corrupted")
        || msg.contains("checksum mismatch")
        || msg.contains("invalid index file")
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

    /// A filter used to apply only when the dimension fit in memory, so the
    /// same YAML filtered or not depending on free RAM.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn an_indexed_dimension_applies_its_filter_like_an_in_memory_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "pvz.csv",
            "id,status\nP1,active\nP2,closed\nP3,active\n",
        );
        let def: SourceDefinition = serde_yaml_ng::from_str(
            "file: pvz.csv\nname: pvz\nindexed_by: [id]\nfilter:\n  - field: status\n    equals: active\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &def.file);

        for dim in [
            load_dimension_source(&def, &doc_path, &resolved, "id").unwrap(),
            load_dimension_in_memory(&def, &doc_path, &resolved, "id").unwrap(),
        ] {
            assert!(dim.lookup_row("P1").unwrap().is_some(), "active kept");
            assert!(dim.lookup_row("P2").unwrap().is_none(), "closed dropped");
            assert!(dim.lookup_all("P2").is_empty(), "closed dropped from all");
            assert_eq!(dim.lookup_all("P3").len(), 1);
        }
    }

    /// The mmap line was split on `,` whatever the format, so an over-budget
    /// TSV decoded into one garbage column.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn an_indexed_tsv_dimension_decodes_its_own_format() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "regions.tsv", "id\tname\nR01\tMoscow, RU\n");
        let def: SourceDefinition =
            serde_yaml_ng::from_str("file: regions.tsv\nname: regions\nindexed_by: [id]\n")
                .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &def.file);

        let dim = load_dimension_source(&def, &doc_path, &resolved, "id").unwrap();
        let row = dim.lookup_row("R01").unwrap().expect("row must be found");
        assert_eq!(row.get("id"), Some("R01"));
        assert_eq!(
            row.get("name"),
            Some("Moscow, RU"),
            "a comma inside a TSV field is data, not a separator"
        );
    }

    /// Same for NDJSON: a JSON object split on commas yields nothing usable.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn an_indexed_ndjson_dimension_decodes_its_own_format() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "regions.ndjson",
            "{\"id\": \"R01\", \"name\": \"Moscow\", \"pop\": 13}\n",
        );
        let def: SourceDefinition =
            serde_yaml_ng::from_str("file: regions.ndjson\nname: regions\nindexed_by: [id]\n")
                .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &def.file);

        let dim = load_dimension_source(&def, &doc_path, &resolved, "id").unwrap();
        let row = dim.lookup_row("R01").unwrap().expect("row must be found");
        assert_eq!(row.get("name"), Some("Moscow"));
        assert_eq!(row.get("pop"), Some("13"));
    }

    /// The indexed path must build the same composite key the in-memory one
    /// does, or the two disagree above the memory budget.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn an_indexed_composite_key_matches_the_in_memory_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "prices.csv",
            "order_id,product_id,price\nO1,P1,10\nO1,P2,99\n",
        );
        let def: SourceDefinition = serde_yaml_ng::from_str(
            "file: prices.csv\nname: prices\nindexed_by: [order_id, product_id]\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &def.file);
        let key = def
            .indexed_by
            .as_ref()
            .unwrap()
            .join(crate::index::COMPOSITE_KEY_SEPARATOR);

        let sep = crate::index::COMPOSITE_KEY_SEPARATOR;
        for dim in [
            load_dimension_source(&def, &doc_path, &resolved, &key).unwrap(),
            load_dimension_in_memory(&def, &doc_path, &resolved, &key).unwrap(),
        ] {
            let row = dim
                .lookup_row(&format!("O1{sep}P2"))
                .unwrap()
                .expect("composite key must resolve");
            assert_eq!(row.get("price"), Some("99"));
            assert!(dim.lookup_row(&format!("O1{sep}P9")).unwrap().is_none());
        }

        // The unit separator must never reach a file name.
        let idx = crate::index_builder::index_path_for_source(&resolved, &key);
        let name = idx.file_name().unwrap().to_string_lossy();
        assert!(!name.contains('\u{1f}'), "index file named {name}");
        assert!(
            name.contains("order_id+product_id"),
            "index file named {name}"
        );
    }

    /// A quoted delimiter inside a CSV field is data.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn an_indexed_csv_dimension_honours_quoting() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "q.csv", "id,name\nR01,\"Moscow, RU\"\n");
        let def: SourceDefinition =
            serde_yaml_ng::from_str("file: q.csv\nname: q\nindexed_by: [id]\n").unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &def.file);

        let dim = load_dimension_source(&def, &doc_path, &resolved, "id").unwrap();
        let row = dim.lookup_row("R01").unwrap().expect("row must be found");
        assert_eq!(row.get("name"), Some("Moscow, RU"));
    }

    /// Regression (BUG 1): index-backed dimensions must yield the same rows —
    /// including real column names and values — that a full-scan in-memory
    /// dimension does. Before the fix, `lookup_all` returned nothing for indexed
    /// dimensions and the mmap offset/length math read out-of-bounds garbage for
    /// `lookup_row`. (Unique keys only: the in-memory path is a map keyed by the
    /// join key and cannot hold multiple rows per key, so the two paths are only
    /// comparable when keys are unique.)
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn indexed_dimension_matches_in_memory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "regions.csv",
            "region_id,region_name\nR01,Moscow\nR02,Saint Petersburg\n",
        );
        let def: SourceDefinition =
            serde_yaml_ng::from_str("file: regions.csv\nname: regions\nindexed_by: [region_id]\n")
                .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &def.file);

        let indexed = load_dimension_source(&def, &doc_path, &resolved, "region_id").unwrap();
        assert!(matches!(indexed, DimensionSource::Indexed(_)));
        let memory = load_dimension_in_memory(&def, &doc_path, &resolved, "region_id").unwrap();
        assert!(matches!(memory, DimensionSource::Memory(_)));

        let fingerprint = |row: &SourceRow| {
            row.columns()
                .iter()
                .map(|c| format!("{c}={}", row.get(c).unwrap_or("")))
                .collect::<Vec<_>>()
                .join(",")
        };

        for key in ["R01", "R02"] {
            // Single-row lookup: identical column names AND values across paths.
            let idx_row = indexed.lookup_row(key).unwrap().unwrap();
            let mem_row = memory.lookup_row(key).unwrap().unwrap();
            assert_eq!(idx_row.columns(), mem_row.columns());
            assert_eq!(fingerprint(&idx_row), fingerprint(&mem_row));

            // `lookup_all` on the indexed path must no longer come back empty.
            let idx_all = indexed.lookup_all(key);
            let mem_all = memory.lookup_all(key);
            assert_eq!(idx_all.len(), 1);
            assert_eq!(fingerprint(&idx_all[0]), fingerprint(&mem_all[0]));
        }
        assert_eq!(
            indexed
                .lookup_row("R01")
                .unwrap()
                .unwrap()
                .get("region_name"),
            Some("Moscow")
        );

        // Missing key: both paths yield an empty set.
        assert!(indexed.lookup_all("NOPE").is_empty());
        assert!(memory.lookup_all("NOPE").is_empty());
        assert!(indexed.lookup_row("NOPE").unwrap().is_none());
    }

    /// Regression (BUG 1): a CROSS join over an indexed dimension must expand
    /// the primary row across all matching dimension rows, injecting real field
    /// names. Previously `dimension_lookup_all` returned `None` for indexed
    /// dimensions, so the cross product collapsed to the primary row alone.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn cross_join_indexed_dimension_expands_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "orders.csv", "order_id,region_id\nO1,R01\n");
        create_temp_csv(
            dir,
            "regions.csv",
            "region_id,region_name\nR01,Moscow\nR01,Kazan\nR02,Perm\n",
        );
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        // Force the dimension onto the indexed (mmap) path regardless of the
        // machine's memory budget.
        let dim_def: SourceDefinition =
            serde_yaml_ng::from_str("file: regions.csv\nname: regions\nindexed_by: [region_id]\n")
                .unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &dim_def.file);
        let indexed = load_dimension_source(&dim_def, &doc_path, &resolved, "region_id").unwrap();
        assert!(matches!(indexed, DimensionSource::Indexed(_)));

        let primary_def: SourceDefinition =
            serde_yaml_ng::from_str("file: orders.csv\nname: orders\n").unwrap();
        let primary_reader = open_source_reader(&primary_def, &doc_path).unwrap();

        let mut dimensions = HashMap::new();
        dimensions.insert("regions".to_string(), indexed);

        let config = SourceDrivenConfig {
            primary: Arc::new(Mutex::new(primary_reader)),
            primary_batch: Mutex::new(std::collections::VecDeque::new()),
            primary_name: "orders".to_string(),
            dimensions,
            resolved_paths: HashMap::new(),
            dim_joins: vec![DimensionJoin {
                source_name: "regions".to_string(),
                foreign_key: "region_id".to_string(),
                join_type: crate::definition::JoinType::Cross,
            }],
            primary_filter: Vec::new(),
            load_stats: DimLoadStats::default(),
            runtime_stats: SourceRuntimeStats::default(),
            has_cross_join: true,
            cross_product_state: std::sync::Mutex::new(std::collections::VecDeque::new()),
            loaded_at: std::time::Instant::now(),
            current_row: std::sync::atomic::AtomicU64::new(0),
        };

        let mut region_names = Vec::new();
        while let Some(vars) = config.next_row_variables().unwrap() {
            assert_eq!(
                vars.get("orders.order_id"),
                Some(&Value::String("O1".into()))
            );
            if let Some(Value::String(name)) = vars.get("regions.region_name") {
                region_names.push(name.clone());
            }
        }
        region_names.sort();
        assert_eq!(
            region_names,
            vec!["Kazan".to_string(), "Moscow".to_string()]
        );
    }

    /// Regression: `next_primary_row` refills with the reader lock held and
    /// then takes the batch lock, while `rewind` took the batch lock first and
    /// the reader second — an order inversion that deadlocked a duration run
    /// the moment one worker wrapped the source while another was refilling.
    /// Compilation cannot catch this, so the test bounds itself in wall time.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn concurrent_rewind_and_refill_do_not_deadlock() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Deliberately fewer rows than the refill batch, so the source is
        // exhausted and rewound constantly while workers pull.
        let mut rows = String::from("id,name\n");
        for i in 0..50 {
            rows.push_str(&format!("{i},user-{i}\n"));
        }
        create_temp_csv(dir, "rows.csv", &rows);
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let def: SourceDefinition =
            serde_yaml_ng::from_str("file: rows.csv\nname: rows\n").unwrap();
        let config = Arc::new(
            SourceDrivenConfig::prepare(&[def], &doc_path)
                .unwrap()
                .unwrap(),
        );

        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let served = Arc::new(std::sync::atomic::AtomicU64::new(0));
        {
            let config = Arc::clone(&config);
            let done = Arc::clone(&done);
            let served = Arc::clone(&served);
            std::thread::spawn(move || {
                std::thread::scope(|scope| {
                    for _ in 0..8 {
                        let config = &config;
                        let served = &served;
                        scope.spawn(move || {
                            for _ in 0..2_000 {
                                match config.next_row_variables() {
                                    Ok(Some(_)) => {
                                        served.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    }
                                    // Exhausted: wrap, exactly as the duration
                                    // executor does.
                                    Ok(None) => {
                                        let _ = config.rewind();
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }
                });
                done.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if done.load(std::sync::atomic::Ordering::SeqCst) {
                assert!(served.load(std::sync::atomic::Ordering::Relaxed) > 0);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!(
            "deadlocked: {} rows served before the workers stopped making progress",
            served.load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    /// Regression: `next_row_variables` checked `cross_product_state` under the
    /// lock, dropped the guard, then re-locked inside
    /// `next_cross_product_combination` and unwrapped. Two workers could both
    /// observe `Some`, the first drain the last combination and clear the slot,
    /// and the second unwrap `None` — a panic on any CROSS join driven by more
    /// than one worker. A second race let two workers each installing a
    /// different row's product clobber one another, silently dropping a row.
    ///
    /// Every emitted combination is collected here and checked for completeness,
    /// so a lost product fails the assertion rather than passing quietly.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn concurrent_cross_join_neither_panics_nor_drops_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // 40 primary rows, each matching 3 dimension rows -> 120 combinations.
        let mut orders = String::from("order_id,region_id\n");
        for i in 0..40 {
            orders.push_str(&format!("O{i},R{}\n", i % 4));
        }
        create_temp_csv(dir, "orders.csv", &orders);

        let mut regions = String::from("region_id,region_name\n");
        for r in 0..4 {
            for n in 0..3 {
                regions.push_str(&format!("R{r},name-{r}-{n}\n"));
            }
        }
        create_temp_csv(dir, "regions.csv", &regions);

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let dim_def: SourceDefinition =
            serde_yaml_ng::from_str("file: regions.csv\nname: regions\nindexed_by: [region_id]\n")
                .unwrap();
        let resolved = FileUtils::resolve_relative_path(&doc_path, &dim_def.file);
        let dimension = load_dimension_source(&dim_def, &doc_path, &resolved, "region_id").unwrap();

        let primary_def: SourceDefinition =
            serde_yaml_ng::from_str("file: orders.csv\nname: orders\n").unwrap();
        let primary_reader = open_source_reader(&primary_def, &doc_path).unwrap();

        let mut dimensions = HashMap::new();
        dimensions.insert("regions".to_string(), dimension);

        let config = Arc::new(SourceDrivenConfig {
            primary: Arc::new(Mutex::new(primary_reader)),
            primary_batch: Mutex::new(std::collections::VecDeque::new()),
            primary_name: "orders".to_string(),
            dimensions,
            resolved_paths: HashMap::new(),
            dim_joins: vec![DimensionJoin {
                source_name: "regions".to_string(),
                foreign_key: "region_id".to_string(),
                join_type: crate::definition::JoinType::Cross,
            }],
            primary_filter: Vec::new(),
            load_stats: DimLoadStats::default(),
            runtime_stats: SourceRuntimeStats::default(),
            has_cross_join: true,
            cross_product_state: std::sync::Mutex::new(std::collections::VecDeque::new()),
            loaded_at: std::time::Instant::now(),
            current_row: std::sync::atomic::AtomicU64::new(0),
        });

        let emitted = Arc::new(Mutex::new(Vec::new()));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let config = Arc::clone(&config);
                let emitted = Arc::clone(&emitted);
                scope.spawn(move || {
                    while let Some(vars) = config.next_row_variables().expect("no panic, no error")
                    {
                        let order = match vars.get("orders.order_id") {
                            Some(Value::String(s)) => s.clone(),
                            _ => panic!("primary field missing"),
                        };
                        let region = match vars.get("regions.region_name") {
                            Some(Value::String(s)) => s.clone(),
                            _ => panic!("cross-joined field missing"),
                        };
                        emitted.lock().unwrap().push((order, region));
                    }
                });
            }
        });

        let mut got = emitted.lock().unwrap().clone();
        got.sort();
        got.dedup();
        assert_eq!(
            got.len(),
            120,
            "40 rows x 3 matches must yield 120 distinct combinations, got {}",
            got.len()
        );
    }

    /// Regression (BUG 1): an INNER join must skip a primary row whose FK
    /// column is entirely absent, not just present-but-unmatched. The prior
    /// `is_some_and` check treated an absent FK column as "no constraint" and
    /// wrongly emitted the row; `is_none_or` skips it. The primary here is
    /// NDJSON with no `region_id` field at all, so an INNER join on it must
    /// yield zero rows.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn inner_join_skips_rows_missing_fk_column() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "orders.jsonl",
            "{\"order_id\":\"O1\"}\n{\"order_id\":\"O2\"}\n",
        );
        create_temp_csv(dir, "regions.csv", "region_id,region_name\nR01,Moscow\n");

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: orders.jsonl\n  name: orders\n- file: regions.csv\n  name: regions\n  indexed_by: [region_id]\n  join_type: inner\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        // No primary row carries the FK column, so the INNER join drops them all.
        assert!(config.next_row_variables().unwrap().is_none());
    }

    /// Regression (BUG 1): a run of INNER-join misses is drained iteratively
    /// (not by recursion), and the first matching row after the misses is
    /// still returned correctly.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn inner_join_drains_leading_misses_then_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "orders.csv",
            "order_id,region_id\nO1,NOPE\nO2,NOPE\nO3,R01\n",
        );
        create_temp_csv(dir, "regions.csv", "region_id,region_name\nR01,Moscow\n");

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: orders.csv\n  name: orders\n- file: regions.csv\n  name: regions\n  indexed_by: [region_id]\n  join_type: inner\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        // O1 and O2 have unmatched FKs and are skipped; O3's FK matches, so it
        // is the only row that survives the INNER join.
        let vars = config.next_row_variables().unwrap().unwrap();
        assert_eq!(
            vars.get("orders.order_id"),
            Some(&Value::String("O3".into()))
        );
        assert!(config.next_row_variables().unwrap().is_none());
    }

    /// An INNER join used to filter the primary rows and then contribute
    /// nothing: only LEFT injected the dimension's fields.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn an_inner_join_injects_the_dimension_fields_it_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "orders.csv", "order_id,region_id\nO1,R01\n");
        create_temp_csv(dir, "regions.csv", "region_id,region_name\nR01,Moscow\n");

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: orders.csv\n  name: orders\n- file: regions.csv\n  name: regions\n  indexed_by: [region_id]\n  join_type: inner\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();
        let vars = config.next_row_variables().unwrap().unwrap();
        assert_eq!(
            vars.get("orders.order_id"),
            Some(&Value::String("O1".into()))
        );
        assert_eq!(
            vars.get("regions.region_name"),
            Some(&Value::String("Moscow".into())),
            "the matched dimension row must be available to the template: {vars:?}"
        );
    }

    /// `indexed_by: [a, b]` was passed around as the literal column name
    /// `"a\x1Fb"`, which no row has: the in-memory path failed the lookup
    /// outright and the CLI builder quietly indexed on `a` alone.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn a_composite_key_joins_on_every_named_column() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(
            dir,
            "lines.csv",
            "order_id,product_id,qty\nO1,P1,2\nO1,P2,5\n",
        );
        create_temp_csv(
            dir,
            "prices.csv",
            "order_id,product_id,price\nO1,P1,10\nO1,P2,99\n",
        );

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: lines.csv\n  name: lines\n- file: prices.csv\n  name: prices\n  indexed_by: [order_id, product_id]\n",
        )
        .unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        // Both rows share `order_id`, so indexing on the first column alone
        // would give them the same price.
        let first = config.next_row_variables().unwrap().unwrap();
        assert_eq!(first.get("lines.qty"), Some(&Value::String("2".into())));
        assert_eq!(
            first.get("prices.price"),
            Some(&Value::String("10".into())),
            "{first:?}"
        );

        let second = config.next_row_variables().unwrap().unwrap();
        assert_eq!(second.get("lines.qty"), Some(&Value::String("5".into())));
        assert_eq!(
            second.get("prices.price"),
            Some(&Value::String("99".into())),
            "{second:?}"
        );
    }

    // The gate compared a raw file size with a RAM budget, so a dimension
    // needing 12x its file size was loaded whole whenever the file alone fit.
    // `memory_budget` was parsed and never read.
    #[test]
    fn a_source_may_override_the_run_wide_memory_budget() {
        let mut def: SourceDefinition =
            serde_yaml_ng::from_str("file: pvz.csv\nname: pvz\n").unwrap();
        assert_eq!(task_budget(&def, 4096), 4096);

        def.memory_budget = Some("1kb".into());
        assert_eq!(task_budget(&def, 4096), 1024);

        // An unparseable value falls back rather than dropping to zero and
        // pushing every dimension onto the index path.
        def.memory_budget = Some("not a size".into());
        assert_eq!(task_budget(&def, 4096), 4096);
    }

    #[test]
    fn the_memory_gate_accounts_for_in_memory_expansion() {
        let budget = 120;
        assert!(
            fits_in_memory(10, budget),
            "10 bytes needs 120, exactly the budget"
        );
        assert!(!fits_in_memory(11, budget), "11 bytes needs 132");
        assert!(fits_in_memory(0, budget));
    }

    // A file we cannot stat must not be treated as empty.
    #[test]
    fn an_unmeasurable_file_never_counts_as_fitting() {
        assert!(!fits_in_memory(u64::MAX, u64::MAX));
    }

    #[test]
    fn no_definitions_returns_none() {
        let result = SourceDrivenConfig::prepare(&[], Path::new("test.gctf")).unwrap();
        assert!(result.is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn primary_only_no_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "users.csv", "id,name,age\n1,Alice,30\n2,Bob,25\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: users.csv\n  name: users\n").unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        assert_eq!(config.primary_name, "users");
        assert!(config.dimensions.is_empty());

        let vars = config.next_row_variables().unwrap().unwrap();
        assert_eq!(vars.get("users.id"), Some(&Value::String("1".into())));
        assert_eq!(vars.get("users.name"), Some(&Value::String("Alice".into())));

        let vars2 = config.next_row_variables().unwrap().unwrap();
        assert_eq!(vars2.get("users.name"), Some(&Value::String("Bob".into())));

        let vars3 = config.next_row_variables().unwrap();
        assert!(vars3.is_none());
    }

    /// Regression: in duration/soak bench mode, once the primary source is
    /// exhausted the engine calls `reset()` on the primary reader to keep
    /// feeding parameterized rows. Before the fix `reset()` was a no-op while
    /// `supports_reset()` claimed success, so every row after exhaustion came
    /// back with empty variables — silently destroying the parameterization.
    /// After the fix, resetting rewinds the reader so the same rows repeat.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn primary_reset_replays_rows_for_duration_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_temp_csv(dir, "users.csv", "id,name\n1,Alice\n2,Bob\n");

        let defs: Vec<SourceDefinition> =
            serde_yaml_ng::from_str("- file: users.csv\n  name: users\n").unwrap();
        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        let collect_pass = |config: &SourceDrivenConfig| {
            let mut names = Vec::new();
            while let Some(vars) = config.next_row_variables().unwrap() {
                if let Some(Value::String(n)) = vars.get("users.name") {
                    names.push(n.clone());
                }
            }
            names
        };

        // First pass drains the source.
        assert_eq!(collect_pass(&config), vec!["Alice", "Bob"]);

        // The bench duration loop rewinds the exhausted primary source.
        {
            let mut reader = config.primary.lock().unwrap();
            assert!(reader.supports_reset());
            reader.reset().unwrap();
        }

        // The next read must yield the original first row, not empty vars.
        let vars = config.next_row_variables().unwrap().unwrap();
        assert_eq!(vars.get("users.id"), Some(&Value::String("1".into())));
        assert_eq!(vars.get("users.name"), Some(&Value::String("Alice".into())));

        // And the whole pass replays identically.
        let vars2 = config.next_row_variables().unwrap().unwrap();
        assert_eq!(vars2.get("users.name"), Some(&Value::String("Bob".into())));
        assert!(config.next_row_variables().unwrap().is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn primary_with_dimension_join() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        create_temp_csv(
            dir,
            "pvz.csv",
            "pvz_id,region_id,name\n1,R01,PVZ Alpha\n2,R02,PVZ Beta\n",
        );
        create_temp_csv(
            dir,
            "regions.csv",
            "region_id,region_name\nR01,Moscow\nR02,Saint Petersburg\n",
        );

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: pvz.csv\n  name: pvz\n- file: regions.csv\n  name: regions\n  indexed_by: [region_id]\n"
        ).unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        assert_eq!(config.dimensions.len(), 1);

        let vars = config.next_row_variables().unwrap().unwrap();

        assert_eq!(vars.get("pvz.pvz_id"), Some(&Value::String("1".into())));
        assert_eq!(
            vars.get("pvz.region_id"),
            Some(&Value::String("R01".into()))
        );
        assert_eq!(
            vars.get("pvz.name"),
            Some(&Value::String("PVZ Alpha".into()))
        );

        assert_eq!(
            vars.get("regions.region_id"),
            Some(&Value::String("R01".into()))
        );
        assert_eq!(
            vars.get("regions.region_name"),
            Some(&Value::String("Moscow".into()))
        );

        let vars2 = config.next_row_variables().unwrap().unwrap();
        assert_eq!(
            vars2.get("pvz.name"),
            Some(&Value::String("PVZ Beta".into()))
        );
        assert_eq!(
            vars2.get("regions.region_name"),
            Some(&Value::String("Saint Petersburg".into()))
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn dimension_missing_fk_still_injects_primary() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        create_temp_csv(dir, "data.csv", "id,ref_id,val\n1,MISSING,hello\n");
        create_temp_csv(dir, "ref.csv", "ref_id,label\nOK,Found\n");

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: data.csv\n  name: data\n- file: ref.csv\n  name: ref\n  indexed_by: [ref_id]\n",
        )
        .unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();

        let vars = config.next_row_variables().unwrap().unwrap();
        assert_eq!(vars.get("data.val"), Some(&Value::String("hello".into())));
        assert!(!vars.contains_key("ref.label"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn primary_filter_skips_non_matching_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        create_temp_csv(
            dir,
            "pvz.csv",
            "pvz_id,status,name\n1,inactive,Old\n2,active,New\n",
        );

        let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(
            "- file: pvz.csv\n  name: pvz\n  filter:\n    - field: status\n      equals: active\n",
        )
        .unwrap();

        let doc_path = dir.join("test.gctf");
        std::fs::write(&doc_path, "").unwrap();

        let config = SourceDrivenConfig::prepare(&defs, &doc_path)
            .unwrap()
            .unwrap();
        let vars = config.next_row_variables().unwrap().unwrap();
        assert_eq!(vars.get("pvz.pvz_id"), Some(&Value::String("2".into())));
        assert_eq!(vars.get("pvz.name"), Some(&Value::String("New".into())));
        assert!(config.next_row_variables().unwrap().is_none());
    }
}
