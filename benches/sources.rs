#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! Throughput of the primary source-row pipeline, single-threaded and under
//! contention. `bench` pulls one row per request from a shared reader, so the
//! contended figure is the one that bounds a data-driven load test.

use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use apif_source_row::{SourceDefinition, SourceDrivenConfig};

const ROWS: usize = 5_000;
/// Rows drained per measured iteration.
const ROWS_PER_ITER: usize = 200;

fn csv_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("names.csv");
    let mut out = String::from("id,name,region\n");
    for i in 0..ROWS {
        out.push_str(&format!("{i},user-{i:06},region-{}\n", i % 8));
    }
    std::fs::write(&path, out).unwrap();
    path
}

fn config_for(path: &std::path::Path) -> Arc<SourceDrivenConfig> {
    let def = SourceDefinition {
        file: path.to_string_lossy().into_owned(),
        name: Some("names".to_string()),
        format: None,
        delimiter: None,
        indexed_by: None,
        memory_budget: None,
        filter: None,
        join_type: None,
    };
    Arc::new(SourceDrivenConfig::prepare(&[def], path).unwrap().unwrap())
}

/// Drain rows until the source is exhausted, rewinding so a long run keeps
/// going — the shape a duration-bounded bench uses.
fn drain(config: &SourceDrivenConfig, n: usize) {
    for _ in 0..n {
        match config.next_row_variables() {
            Ok(Some(vars)) => {
                black_box(vars.len());
            }
            Ok(None) => {
                config.rewind().unwrap();
            }
            Err(_) => break,
        }
    }
}

fn bench_sources(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let path = csv_fixture(dir.path());

    let mut group = c.benchmark_group("sources/next_row");
    group.throughput(Throughput::Elements(ROWS_PER_ITER as u64));
    group.sample_size(20);
    // Flat sampling with a bounded window: the contended cases scale their work
    // with the iteration count, and criterion's default linear sampling asks
    // for a quadratic amount of it.
    group.sampling_mode(SamplingMode::Flat);
    group.measurement_time(Duration::from_secs(5));
    group.warm_up_time(Duration::from_secs(1));

    group.bench_function("serial", |b| {
        let config = config_for(&path);
        b.iter(|| drain(&config, ROWS_PER_ITER));
    });

    // Every worker pulls from the same reader; this is what a bench run does.
    for threads in [4usize, 16] {
        // Built once: rebuilding it per batch re-parsed the whole fixture and
        // dominated the measurement.
        let config = config_for(&path);
        group.bench_function(format!("contended/{threads}"), |b| {
            b.iter_custom(|iters| {
                let config = Arc::clone(&config);
                // Signed on purpose: `AtomicUsize::fetch_sub` wraps, so once the
                // counter reached zero every thread still in the loop saw
                // `usize::MAX` and spun forever.
                let remaining =
                    Arc::new(AtomicIsize::new((iters as isize) * ROWS_PER_ITER as isize));
                let start = std::time::Instant::now();
                std::thread::scope(|scope| {
                    for _ in 0..threads {
                        let config = Arc::clone(&config);
                        let remaining = Arc::clone(&remaining);
                        scope.spawn(move || {
                            while remaining.fetch_sub(1, Ordering::Relaxed) > 0 {
                                match config.next_row_variables() {
                                    Ok(Some(vars)) => {
                                        black_box(vars.len());
                                    }
                                    Ok(None) => {
                                        let _ = config.rewind();
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }
                });
                start.elapsed()
            });
        });
    }

    group.finish();
}

criterion_group!(sources_benches, bench_sources);
criterion_main!(sources_benches);
