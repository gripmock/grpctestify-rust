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

pub struct IndexedDimension {
    pub index: Arc<SourceIndex>,
    pub mmap: memmap2::Mmap,
    pub cache: Mutex<TwoQCache<String, SourceRow>>,
    /// Column names of the source, so rows read from the mmap carry the same
    /// field names an in-memory dimension would (`dim.name`, not `dim.col_0`).
    pub headers: Vec<String>,
}

impl IndexedDimension {
    fn row_from_line(&self, line: &str) -> SourceRow {
        if self.headers.is_empty() {
            SourceRow::from_csv_line(line)
        } else {
            let values: Vec<String> = line
                .split(',')
                .map(|p| p.trim_ascii().to_string())
                .collect();
            SourceRow::new(&self.headers, values)
        }
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
                        let bytes = data.get(start..end)?;
                        std::str::from_utf8(bytes)
                            .ok()
                            .map(|line| idx.row_from_line(line))
                    })
                    .collect()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeFallbackPolicy {
    #[default]
    Skip,
    ScanSource,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct SourceFallbackEvent {
    pub source_name: String,
    pub key: String,
    pub reason: FallbackReason,
    pub fallback_type: FallbackType,
}

#[derive(Debug, Clone, Default)]
pub enum FallbackReason {
    #[default]
    IndexLookupMiss,
    IndexCorrupted,
    IndexOutOfSync,
    TypeMismatch,
}

#[derive(Debug, Clone, Default)]
pub enum FallbackType {
    #[default]
    None,
    ScanSource,
    Error,
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
    let reader = open_source_reader(def, document_path)
        .with_context(|| format!("failed to open dimension source '{}'", def.file))?;
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
    let mmap = unsafe { memmap2::Mmap::map(&file) }
        .with_context(|| format!("failed to mmap dimension file: {}", resolved_path.display()))?;
    Ok(DimensionSource::Indexed(Box::new(IndexedDimension {
        index: Arc::new(index),
        mmap,
        cache: Mutex::new(TwoQCache::new(DIMENSION_CACHE_HOT, DIMENSION_CACHE_COLD)),
        headers,
    })))
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

pub struct SourceDrivenConfig {
    pub primary: Arc<Mutex<Box<dyn SourceReader>>>,
    pub primary_name: String,
    pub dimensions: HashMap<String, DimensionSource>,
    pub resolved_paths: HashMap<String, PathBuf>,
    dim_joins: Vec<DimensionJoin>,
    primary_filter: Vec<FilterCondition>,
    pub load_stats: DimLoadStats,
    pub runtime_stats: SourceRuntimeStats,
    pub fallback_policy: RuntimeFallbackPolicy,
    cross_product_state: std::sync::Mutex<Option<CrossProductState>>,
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
    pub index_fallbacks: std::sync::atomic::AtomicU64,
}

/// Consistent snapshot of runtime stats at a point in time.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatsSnapshot {
    pub dimension_lookups: u64,
    pub dimension_hits: u64,
    pub dimension_misses: u64,
    pub in_memory_lookups: u64,
    pub indexed_lookups: u64,
    pub index_fallbacks: u64,
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
            index_fallbacks: self.index_fallbacks.load(Relaxed),
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
            index_fallbacks: std::sync::atomic::AtomicU64::new(0),
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
            index_fallbacks: std::sync::atomic::AtomicU64::new(
                self.index_fallbacks
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

    pub fn record_fallback(&self) {
        use std::sync::atomic::Ordering;
        self.index_fallbacks.fetch_add(1, Ordering::Relaxed);
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
            let file_size = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);

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
            total_file_bytes += task.file_size;
            if task.file_size <= memory_bb {
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
                    let src = if t.file_size <= memory_bb {
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
                    if t.file_size <= memory_bb {
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

        Ok(Some(Self {
            primary: Arc::new(Mutex::new(primary_reader)),
            primary_name,
            dimensions,
            resolved_paths,
            dim_joins,
            primary_filter,
            cross_product_state: std::sync::Mutex::new(None),
            loaded_at: std::time::Instant::now(),
            current_row: std::sync::atomic::AtomicU64::new(0),
            load_stats: DimLoadStats {
                in_memory_count,
                indexed_count,
                total_file_bytes,
                index_build_ms,
            },
            runtime_stats: SourceRuntimeStats::default(),
            fallback_policy: RuntimeFallbackPolicy::default(),
        }))
    }

    pub fn next_row_variables(&self) -> Result<Option<HashMap<String, Value>>> {
        // If we're in the middle of a cross-product iteration, yield the next combination
        {
            let state_guard = self
                .cross_product_state
                .lock()
                .expect("cross_product_state mutex should not be poisoned");
            if state_guard.is_some() {
                drop(state_guard);
                return self.next_cross_product_combination();
            }
        }

        // Loop (rather than recurse) so a long run of INNER-join misses can't
        // blow the stack.
        let row = loop {
            let candidate = {
                let mut reader = self.primary.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                loop {
                    match reader.next_row()? {
                        Some(r) => {
                            if self.primary_filter.is_empty()
                                || matches_filter_all(&r, &self.primary_filter)
                            {
                                break r;
                            }
                        }
                        None => return Ok(None),
                    }
                }
            };

            // Check INNER join constraints — skip row if the FK column is
            // absent or its value has no match in the dimension.
            let inner_missing = self.dim_joins.iter().any(|j| {
                j.join_type == super::definition::JoinType::Inner
                    && candidate
                        .get(&j.foreign_key)
                        .is_none_or(|fk| self.dimension_lookup(&j.source_name, fk).is_none())
            });
            if inner_missing {
                continue;
            }
            break candidate;
        };

        let mut vars = self.build_primary_vars(&row);

        // Process LEFT joins (standard: add dimension fields when FK matches)
        for join in &self.dim_joins {
            if join.join_type != super::definition::JoinType::Left {
                continue;
            }
            if let Some(fk_val) = row.get(&join.foreign_key)
                && let Some(dim_row) = self.dimension_lookup(&join.source_name, fk_val)
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
                if let Some(fk_val) = row.get(&join.foreign_key) {
                    if let Some(all_rows) = self.dimension_lookup_all(&join.source_name, fk_val) {
                        cross_matches.push(all_rows);
                    } else {
                        cross_matches.push(Vec::new());
                    }
                } else {
                    cross_matches.push(Vec::new());
                }
            }

            *self
                .cross_product_state
                .lock()
                .expect("cross_product_state mutex should not be poisoned") =
                Some(CrossProductState {
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
            .expect("cross_product_state mutex should not be poisoned");
        let state = state_guard.as_ref().unwrap();
        let row = &state.row;
        let mut vars = self.build_primary_vars(row);

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
        let mut new_indices = state.cross_indices.clone();
        let mut advanced = false;

        for i in (0..cross_count).rev() {
            let max = state.cross_matches[i].len();
            if max == 0 {
                continue;
            }
            if new_indices[i] + 1 < max {
                new_indices[i] += 1;
                advanced = true;
                break;
            } else {
                new_indices[i] = 0;
            }
        }

        if advanced {
            *state_guard = Some(CrossProductState {
                row: row.clone(),
                cross_matches: state.cross_matches.clone(),
                cross_indices: new_indices,
            });
        } else {
            *state_guard = None;
        }

        self.current_row
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Some(vars))
    }

    fn build_primary_vars(&self, row: &SourceRow) -> HashMap<String, Value> {
        let mut vars = HashMap::new();
        for col in row.columns() {
            if let Some(val) = row.get(col) {
                vars.insert(
                    format!("{}.{}", self.primary_name, col),
                    Value::String(val.to_string()),
                );
            }
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

    pub fn build_dimension_variables(
        &self,
        row: &SourceRow,
        joins: &[(String, String, String)],
    ) -> HashMap<String, Value> {
        let mut vars = HashMap::new();

        for (dim_name, local_key_col, _remote_key_col) in joins {
            if let Some(key_val) = row.get(local_key_col)
                && let Some(dim_row) = self.dimension_lookup(dim_name, key_val)
            {
                for col in dim_row.columns() {
                    if let Some(val) = dim_row.get(col) {
                        vars.insert(format!("{dim_name}.{col}"), Value::String(val.to_string()));
                    }
                }
            }
        }

        vars
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
    // `read_first_line` keeps the trailing newline, so its length already
    // covers the header line plus its line terminator: the first data row
    // begins at exactly `header_line.len()`.
    let mut byte_offset = header_line.len() as u64;
    let mut row_count = 0u64;

    while let Some(row) = reader.next_row()? {
        let key_val = row.get(key_col).ok_or_else(|| {
            anyhow::anyhow!("column '{}' not found in row {}", key_col, row_count)
        })?;
        let row_bytes = estimate_row_size(&row);
        index
            .insert(key_val.to_string(), byte_offset, row_bytes)
            .with_context(|| format!("failed to insert key '{}' at row {}", key_val, row_count))?;
        byte_offset += row_bytes as u64 + 1;
        row_count += 1;
    }

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
        let dir = std::env::temp_dir().join("gctf_driven_indexed_match_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(
            &dir,
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

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression (BUG 1): a CROSS join over an indexed dimension must expand
    /// the primary row across all matching dimension rows, injecting real field
    /// names. Previously `dimension_lookup_all` returned `None` for indexed
    /// dimensions, so the cross product collapsed to the primary row alone.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn cross_join_indexed_dimension_expands_rows() {
        let dir = std::env::temp_dir().join("gctf_driven_cross_indexed_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "orders.csv", "order_id,region_id\nO1,R01\n");
        create_temp_csv(
            &dir,
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
            fallback_policy: RuntimeFallbackPolicy::default(),
            cross_product_state: std::sync::Mutex::new(None),
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

        std::fs::remove_dir_all(&dir).ok();
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
        let dir = std::env::temp_dir().join("gctf_driven_inner_missing_col_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(
            &dir,
            "orders.jsonl",
            "{\"order_id\":\"O1\"}\n{\"order_id\":\"O2\"}\n",
        );
        create_temp_csv(&dir, "regions.csv", "region_id,region_name\nR01,Moscow\n");

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

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression (BUG 1): a run of INNER-join misses is drained iteratively
    /// (not by recursion), and the first matching row after the misses is
    /// still returned correctly.
    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn inner_join_drains_leading_misses_then_matches() {
        let dir = std::env::temp_dir().join("gctf_driven_inner_drain_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(
            &dir,
            "orders.csv",
            "order_id,region_id\nO1,NOPE\nO2,NOPE\nO3,R01\n",
        );
        create_temp_csv(&dir, "regions.csv", "region_id,region_name\nR01,Moscow\n");

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

        std::fs::remove_dir_all(&dir).ok();
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
        let dir = std::env::temp_dir().join("gctf_driven_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "users.csv", "id,name,age\n1,Alice,30\n2,Bob,25\n");

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

        std::fs::remove_dir_all(&dir).ok();
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
        let dir = std::env::temp_dir().join("gctf_driven_reset_replay_test");
        std::fs::create_dir_all(&dir).unwrap();
        create_temp_csv(&dir, "users.csv", "id,name\n1,Alice\n2,Bob\n");

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

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn primary_with_dimension_join() {
        let dir = std::env::temp_dir().join("gctf_driven_join_test");
        std::fs::create_dir_all(&dir).unwrap();

        create_temp_csv(
            &dir,
            "pvz.csv",
            "pvz_id,region_id,name\n1,R01,PVZ Alpha\n2,R02,PVZ Beta\n",
        );
        create_temp_csv(
            &dir,
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

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn dimension_missing_fk_still_injects_primary() {
        let dir = std::env::temp_dir().join("gctf_driven_fk_test");
        std::fs::create_dir_all(&dir).unwrap();

        create_temp_csv(&dir, "data.csv", "id,ref_id,val\n1,MISSING,hello\n");
        create_temp_csv(&dir, "ref.csv", "ref_id,label\nOK,Found\n");

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

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn primary_filter_skips_non_matching_rows() {
        let dir = std::env::temp_dir().join("gctf_driven_filter_test");
        std::fs::create_dir_all(&dir).unwrap();

        create_temp_csv(
            &dir,
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

        std::fs::remove_dir_all(&dir).ok();
    }
}
