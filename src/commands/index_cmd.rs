use crate::bench::sources::index::SourceIndex;
use crate::bench::sources::index_builder::{
    build_index_for_source_with_progress, index_path_for_source,
};
use crate::bench::sources::{SourceDefinition, SourceUsageAnalyzer, effective_source_name};
use crate::cli::args::IndexArgs;
use crate::parser::ast::{SectionContent, SectionType};
use anyhow::{Context, Result};
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub fn handle_index(args: &IndexArgs) -> Result<()> {
    // Stats mode: show index file metadata
    if args.stats {
        for path in &args.sources {
            if !path.exists() {
                eprintln!("File not found: {}", path.display());
                continue;
            }
            match SourceIndex::read_from_file(path) {
                Ok(index) => {
                    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    println!("File: {}", path.display());
                    println!("  Size: {} bytes", file_size);
                    println!("  Key column: {}", index.key_column());
                    println!("  Key type: {:?}", index.key_type());
                    println!("  Index version: {}", index.index_version());
                    println!("  Entry count: {}", index.entry_count());
                    println!();
                }
                Err(e) => {
                    eprintln!("Error reading {}: {e}", path.display());
                }
            }
        }
        return Ok(());
    }

    let files = resolve_bench_files(&args.sources)?;
    if files.is_empty() {
        anyhow::bail!("no .gctf files found in provided paths");
    }

    let started = Instant::now();
    let mp = Arc::new(MultiProgress::new());
    let overall = mp.add(ProgressBar::new(files.len() as u64));
    overall.set_style(progress_style(
        "{spinner:.green} indexing {bar:24.cyan/blue} {pos}/{len} elapsed:{elapsed_precise}",
    ));
    overall.enable_steady_tick(std::time::Duration::from_millis(120));

    let file_count = files.len();
    let args = Arc::new(args.clone());

    let results: Vec<(usize, IndexRunOutcome)> = std::thread::scope(|s| {
        let overall_ref = &overall;
        let mp_ref = &mp;
        let mut handles: Vec<std::thread::ScopedJoinHandle<(usize, IndexRunOutcome)>> =
            Vec::with_capacity(file_count);

        for (i, source_path) in files.iter().enumerate() {
            let args = Arc::clone(&args);
            let source_path = source_path.clone();
            let overall_pb = overall_ref.clone();
            let mp = mp_ref.clone();

            handles.push(s.spawn(move || {
                let current = mp.add(ProgressBar::new(100));
                current.set_style(progress_style(
                    "{spinner:.green} {msg:45} {bar:20.cyan/blue} {pos:>3}%",
                ));
                current.enable_steady_tick(std::time::Duration::from_millis(80));
                current.set_position(0);
                current.set_message(format!("analyze {}", compact_path(&source_path)));

                let outcome = handle_index_single(&args, &current, &source_path);
                overall_pb.inc(1);

                match outcome {
                    Ok(o) => {
                        current.finish_and_clear();
                        (i, o)
                    }
                    Err(e) => {
                        current.finish_and_clear();
                        (
                            i,
                            IndexRunOutcome::Error(format!(
                                "{}: {}",
                                compact_path(&source_path),
                                e
                            )),
                        )
                    }
                }
            }));
        }

        handles
            .into_iter()
            .map(|h| h.join().expect("index thread panicked"))
            .collect()
    });

    overall.finish_with_message("indexing complete");

    let mut processed = 0usize;
    let mut skipped = 0usize;
    let mut total_rebuilt = 0usize;
    let mut total_required = 0usize;
    let mut total_missing = 0usize;
    let mut file_reports: Vec<String> = Vec::new();
    let mut processed_stats: Vec<FileStats> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut keep_index_paths: BTreeSet<PathBuf> = BTreeSet::new();

    let mut sorted = results;
    sorted.sort_by_key(|(i, _)| *i);

    for (i, outcome) in sorted {
        let source_path = &files[i];
        match outcome {
            IndexRunOutcome::Processed(stats) => {
                processed += 1;
                total_rebuilt += stats.rebuilt;
                total_required += stats.required;
                total_missing += stats.missing;
                file_reports.push(format!(
                    "OK   {} | required={} rebuilt={} reused={} missing={}",
                    compact_path(source_path),
                    stats.required,
                    stats.rebuilt,
                    stats.required.saturating_sub(stats.rebuilt + stats.missing),
                    stats.missing
                ));
                for d in &stats.details {
                    keep_index_paths.insert(d.index_path.clone());
                }
                processed_stats.push(stats);
            }
            IndexRunOutcome::Skipped(reason) => {
                skipped += 1;
                file_reports.push(format!("SKIP {} | {}", compact_path(source_path), reason));
            }
            IndexRunOutcome::Error(msg) => {
                problems.push(msg);
            }
        }
    }

    for line in file_reports {
        eprintln!("{}", line);
    }

    eprintln!("\nIndex summary:");
    eprintln!("  Scanned files: {}", files.len());
    eprintln!("  Processed: {}", processed);
    eprintln!("  Skipped: {}", skipped);
    eprintln!("  Required indexes: {}", total_required);
    eprintln!("  Rebuilt: {}", total_rebuilt);
    eprintln!(
        "  Reused: {}",
        total_required.saturating_sub(total_rebuilt + total_missing)
    );
    eprintln!("  Missing after run: {}", total_missing);
    eprintln!("  Duration: {:.2?}", started.elapsed());

    if processed == 0 {
        eprintln!(
            "Note: no BENCH files with valid BENCH.sources list were found in provided paths."
        );
    }

    if !processed_stats.is_empty() {
        eprintln!("\nFile details:");
        for fs in processed_stats {
            let total_index_bytes: u64 = fs.details.iter().map(|d| d.index_size).sum();
            eprintln!(
                "  {} | indexes={} total_index_size={}",
                fs.file,
                fs.details.len(),
                HumanBytes(total_index_bytes)
            );
            for d in fs.details {
                eprintln!(
                    "    - {}.{} | source={} rows={} unique={} index={}",
                    d.source,
                    d.column,
                    HumanBytes(d.source_size),
                    d.entries,
                    d.unique,
                    HumanBytes(d.index_size)
                );
            }
        }
    }

    if !problems.is_empty() {
        eprintln!("\nProblems:");
        for p in &problems {
            eprintln!("  - {}", p);
        }
        anyhow::bail!("indexing finished with {} error(s)", problems.len());
    }
    Ok(())
}

enum IndexRunOutcome {
    Processed(FileStats),
    Skipped(String),
    Error(String),
}

#[derive(Default)]
struct FileStats {
    file: String,
    required: usize,
    rebuilt: usize,
    missing: usize,
    details: Vec<IndexDetail>,
}

struct IndexDetail {
    source: String,
    column: String,
    index_path: PathBuf,
    source_size: u64,
    index_size: u64,
    entries: usize,
    unique: usize,
}

struct IndexTask {
    source_name: String,
    column: String,
    def: SourceDefinition,
    source_file: PathBuf,
    idx_path: PathBuf,
    needs_rebuild: bool,
}

enum IndexTaskResult {
    Ok { rebuilt: bool, detail: IndexDetail },
    Cached(IndexDetail),
    Failed(String),
}

fn execute_index_task(task: &IndexTask, source_path: &Path) -> IndexTaskResult {
    if task.needs_rebuild {
        match build_index_for_source_with_progress(&task.def, source_path, |_, _, _| {}) {
            Ok(_rebuilt_path) => {}
            Err(e) => {
                return IndexTaskResult::Failed(format!(
                    "failed to build index for {}.{}: {e}",
                    task.source_name, task.column
                ));
            }
        }
    }

    if !task.idx_path.exists() {
        return IndexTaskResult::Failed(format!(
            "index file missing after build: {}",
            task.idx_path.display()
        ));
    }

    let index = match SourceIndex::read_from_file(&task.idx_path) {
        Ok(idx) => idx,
        Err(e) => {
            return IndexTaskResult::Failed(format!(
                "failed to read index {}.{}: {e}",
                task.source_name, task.column
            ));
        }
    };
    let idx_meta = match std::fs::metadata(&task.idx_path) {
        Ok(m) => m,
        Err(e) => {
            return IndexTaskResult::Failed(format!(
                "failed to stat index {}: {e}",
                task.idx_path.display()
            ));
        }
    };
    let src_meta = match std::fs::metadata(&task.source_file) {
        Ok(m) => m,
        Err(e) => {
            return IndexTaskResult::Failed(format!(
                "failed to stat source {}: {e}",
                task.source_file.display()
            ));
        }
    };

    let detail = IndexDetail {
        source: task.source_name.clone(),
        column: task.column.clone(),
        index_path: task.idx_path.clone(),
        source_size: src_meta.len(),
        index_size: idx_meta.len(),
        entries: index.len(),
        unique: index.unique_keys_len(),
    };

    if task.needs_rebuild {
        IndexTaskResult::Ok {
            rebuilt: true,
            detail,
        }
    } else {
        IndexTaskResult::Cached(detail)
    }
}

fn handle_index_single(
    args: &IndexArgs,
    current: &ProgressBar,
    source_path: &Path,
) -> Result<IndexRunOutcome> {
    if !source_path.exists() {
        anyhow::bail!("source file not found: {}", source_path.display());
    }

    if !is_bench_file(source_path) {
        anyhow::bail!("index command expects a .gctf file with BENCH.sources");
    }

    let Some(defs) = parse_sources_from_bench_file(source_path)? else {
        return Ok(IndexRunOutcome::Skipped(
            "BENCH.sources is missing or not a YAML list".to_string(),
        ));
    };
    if defs.is_empty() {
        return Ok(IndexRunOutcome::Skipped(
            "BENCH.sources is empty".to_string(),
        ));
    }

    let parse_result = crate::parser::parse_with_recovery(source_path);
    let usage_plan = SourceUsageAnalyzer::analyze(&parse_result.document, &defs);

    let mut defs_by_name: BTreeMap<String, SourceDefinition> = BTreeMap::new();
    for (i, def) in defs.iter().enumerate() {
        defs_by_name.insert(effective_source_name(def, i), def.clone());
    }

    let mut required: BTreeSet<(String, String)> = BTreeSet::new();
    for req in &usage_plan.required_indexes {
        required.insert((req.source.clone(), req.column.clone()));
    }

    let mut tasks: Vec<IndexTask> = Vec::with_capacity(usage_plan.required_indexes.len());
    for req in &usage_plan.required_indexes {
        let Some(def) = defs_by_name.get(&req.source) else {
            continue;
        };
        let source_file =
            crate::utils::file::FileUtils::resolve_relative_path(source_path, &def.file);
        let idx_path = index_path_for_source(&source_file, &req.column);
        let state = index_state(&idx_path, &source_file);
        let needs_rebuild = args.force || !matches!(state, IndexState::Fresh);
        tasks.push(IndexTask {
            source_name: req.source.clone(),
            column: req.column.clone(),
            def: def.clone(),
            source_file,
            idx_path,
            needs_rebuild,
        });
    }

    current.set_message(format!(
        "build {} indexes for {}",
        tasks.len(),
        compact_path(source_path)
    ));

    let task_count = tasks.len();
    let results: Vec<IndexTaskResult> = if task_count <= 1 {
        tasks
            .into_iter()
            .map(|t| execute_index_task(&t, source_path))
            .collect()
    } else {
        std::thread::scope(|s| {
            tasks
                .into_iter()
                .map(|t| {
                    let source_path = source_path.to_path_buf();
                    s.spawn(move || execute_index_task(&t, &source_path))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().expect("index task panicked"))
                .collect()
        })
    };

    current.set_position(100);
    current.set_message(format!("done {}", compact_path(source_path)));

    let mut stats = FileStats {
        file: compact_path(source_path),
        required: required.len(),
        rebuilt: 0,
        missing: 0,
        details: Vec::new(),
    };
    let mut present: BTreeSet<(String, String)> = BTreeSet::new();

    for r in results {
        match r {
            IndexTaskResult::Ok { rebuilt, detail } => {
                if rebuilt {
                    stats.rebuilt += 1;
                }
                present.insert((detail.source.clone(), detail.column.clone()));
                stats.details.push(detail);
            }
            IndexTaskResult::Cached(detail) => {
                present.insert((detail.source.clone(), detail.column.clone()));
                stats.details.push(detail);
            }
            IndexTaskResult::Failed(msg) => {
                return Err(anyhow::anyhow!("{msg}"));
            }
        }
    }

    let missing: BTreeSet<_> = required.difference(&present).cloned().collect();
    if !missing.is_empty() {
        stats.missing = missing.len();
    }
    Ok(IndexRunOutcome::Processed(stats))
}

fn progress_style(template: &str) -> ProgressStyle {
    ProgressStyle::with_template(template).unwrap_or_else(|_| ProgressStyle::default_spinner())
}

fn compact_path(path: &Path) -> String {
    path.display().to_string()
}

fn resolve_bench_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for input in inputs {
        if !input.exists() {
            anyhow::bail!("path not found: {}", input.display());
        }
        if input.is_file() {
            if is_bench_file(input) {
                out.push(input.clone());
            }
            continue;
        }

        for entry in walkdir::WalkDir::new(input)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let p = entry.path();
            if is_bench_file(p) {
                out.push(p.to_path_buf());
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexState {
    Missing,
    Fresh,
    Stale,
    Corrupted,
}

fn index_state(index_path: &Path, source_path: &Path) -> IndexState {
    if !index_path.exists() {
        return IndexState::Missing;
    }

    if SourceIndex::read_from_file(index_path).is_err() {
        return IndexState::Corrupted;
    }

    let idx_meta = match std::fs::metadata(index_path) {
        Ok(m) => m,
        Err(_) => return IndexState::Corrupted,
    };
    let src_meta = match std::fs::metadata(source_path) {
        Ok(m) => m,
        Err(_) => return IndexState::Corrupted,
    };
    match (idx_meta.modified(), src_meta.modified()) {
        (Ok(i), Ok(s)) if i < s => IndexState::Stale,
        (Ok(_), Ok(_)) => IndexState::Fresh,
        _ => IndexState::Fresh,
    }
}

fn is_bench_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gctf"))
}

fn parse_sources_from_bench_file(path: &Path) -> Result<Option<Vec<SourceDefinition>>> {
    let parse_result = crate::parser::parse_with_recovery(path);
    let document = parse_result.document;
    let Some(bench_section) = document
        .sections
        .iter()
        .find(|s| s.section_type == SectionType::Bench)
    else {
        return Ok(None);
    };

    let Some(bench) = (match &bench_section.content {
        SectionContent::KeyValues(kv) => Some(kv),
        _ => None,
    }) else {
        return Ok(None);
    };

    let Some(raw) = bench.get("sources") else {
        return Ok(None);
    };

    if raw.trim().is_empty() {
        return Ok(None);
    }

    let defs: Vec<SourceDefinition> = serde_yaml_ng::from_str(raw).with_context(|| {
        format!(
            "failed to parse BENCH.sources as YAML array in {}",
            path.display()
        )
    })?;
    Ok(Some(defs))
}
