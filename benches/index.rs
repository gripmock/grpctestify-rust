use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use grpctestify::bench::sources::definition::SourceDefinition;
use grpctestify::bench::sources::index::{BloomFilter, XorFilter};
use grpctestify::bench::sources::index_builder::{build_index_for_source, load_or_build_index};
use grpctestify::bench::sources::{SourceFormat, SourceIndex};
use std::hint::black_box;
use std::io::{BufWriter, Write};

fn write_csv(path: &std::path::Path, rows: usize) {
    let mut w = BufWriter::new(std::fs::File::create(path).expect("create csv"));
    writeln!(w, "id,zone_id,payload").expect("csv header");
    for i in 0..rows {
        writeln!(w, "u{i},z{},payload_{i}", i % 1024).expect("csv row");
    }
}

fn write_tsv(path: &std::path::Path, rows: usize) {
    let mut w = BufWriter::new(std::fs::File::create(path).expect("create tsv"));
    writeln!(w, "id\tzone_id\tpayload").expect("tsv header");
    for i in 0..rows {
        writeln!(w, "u{i}\tz{}\tpayload_{i}", i % 1024).expect("tsv row");
    }
}

fn write_ndjson(path: &std::path::Path, rows: usize) {
    let mut w = BufWriter::new(std::fs::File::create(path).expect("create ndjson"));
    for i in 0..rows {
        writeln!(
            w,
            "{{\"id\":\"u{i}\",\"zone_id\":\"z{}\",\"payload\":\"payload_{i}\"}}",
            i % 1024
        )
        .expect("ndjson row");
    }
}

fn bench_fixture(
    rows: usize,
    format: SourceFormat,
) -> (tempfile::TempDir, SourceDefinition, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let ext = match format {
        SourceFormat::Csv => "csv",
        SourceFormat::Tsv => "tsv",
        SourceFormat::Ndjson => "ndjson",
    };
    let src = dir.path().join(format!("source.{ext}"));
    match format {
        SourceFormat::Csv => write_csv(&src, rows),
        SourceFormat::Tsv => write_tsv(&src, rows),
        SourceFormat::Ndjson => write_ndjson(&src, rows),
    }
    let doc = dir.path().join("bench.gctf");
    std::fs::write(&doc, "").expect("doc file");

    let def = SourceDefinition::from_file_raw(&format!("source.{ext}"), "id", Some(&format));

    (dir, def, doc)
}

fn bench_index_build_by_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("index/build_by_format");
    let rows = 100_000usize;

    for format in [SourceFormat::Csv, SourceFormat::Tsv, SourceFormat::Ndjson] {
        let label = match format {
            SourceFormat::Csv => "csv",
            SourceFormat::Tsv => "tsv",
            SourceFormat::Ndjson => "ndjson",
        };
        group.throughput(Throughput::Elements(rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &format, |b, fmt| {
            b.iter(|| {
                let (_dir, def, doc) = bench_fixture(rows, fmt.clone());
                let idx_path = build_index_for_source(&def, &doc).expect("build index");
                let loaded = SourceIndex::read_from_file(&idx_path).expect("read index");
                black_box(loaded.len());
                black_box(loaded.unique_keys_len());
            });
        });
    }

    group.finish();
}

fn bench_index_load_reuse(c: &mut Criterion) {
    let mut group = c.benchmark_group("index/load_reuse");
    let rows = 150_000usize;

    for format in [SourceFormat::Csv, SourceFormat::Tsv, SourceFormat::Ndjson] {
        let label = match format {
            SourceFormat::Csv => "csv",
            SourceFormat::Tsv => "tsv",
            SourceFormat::Ndjson => "ndjson",
        };

        let (_dir, def, doc) = bench_fixture(rows, format);
        let _ = build_index_for_source(&def, &doc).expect("warm index");

        group.throughput(Throughput::Elements(rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &def, |b, def_in| {
            b.iter(|| {
                let loaded = load_or_build_index(def_in, &doc).expect("load or build");
                black_box(loaded.lookup("u42").is_some());
                black_box(loaded.lookup_all("u42").map(|v| v.len()).unwrap_or(0));
            });
        });
    }

    group.finish();
}

fn bench_index_lookup_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("index/lookup_density");
    let mut idx = SourceIndex::new("zone_id");
    let rows = 500_000usize;
    for i in 0..rows {
        let _ = idx.insert(format!("z{}", i % 1024), i as u64 * 16, 15);
    }

    group.throughput(Throughput::Elements(rows as u64));
    group.bench_function("lookup_first_and_all", |b| {
        b.iter(|| {
            black_box(idx.lookup("z7").is_some());
            black_box(idx.lookup_all("z7").map(|v| v.len()).unwrap_or(0));
        });
    });

    group.finish();
}

fn bench_bloom_filter_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom/construction");
    let elements = 100_000usize;
    let fpr = 0.01;

    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function("new_with_fp", |b| {
        b.iter(|| {
            let bf = BloomFilter::new(elements, fpr);
            black_box(bf.bit_count());
            black_box(bf.hash_count());
        });
    });

    group.bench_function("with_capacity", |b| {
        b.iter(|| {
            let bf = BloomFilter::with_capacity(1_000_000, 10);
            black_box(bf.bit_count());
            black_box(bf.hash_count());
        });
    });

    group.finish();
}

fn bench_bloom_filter_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom/insert");
    let elements = 100_000usize;
    let fpr = 0.01;
    let keys: Vec<String> = (0..elements).map(|i| format!("key_{}", i)).collect();

    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function("insert_100k", |b| {
        b.iter(|| {
            let mut bf = BloomFilter::new(elements, fpr);
            for key in &keys {
                bf.insert(key);
            }
            black_box(&bf);
        });
    });

    group.finish();
}

fn bench_bloom_filter_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom/lookup");
    let elements = 100_000usize;
    let fpr = 0.01;
    let keys: Vec<String> = (0..elements).map(|i| format!("key_{}", i)).collect();
    let mut bf = BloomFilter::new(elements, fpr);
    for key in &keys {
        bf.insert(key);
    }

    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function("contains_existing", |b| {
        b.iter(|| {
            for key in &keys[..1000] {
                black_box(bf.contains(key));
            }
        });
    });

    group.bench_function("contains_missing", |b| {
        let missing: Vec<String> = (0..1000).map(|i| format!("missing_{}", i)).collect();
        b.iter(|| {
            for key in &missing {
                black_box(bf.contains(key));
            }
        });
    });

    group.finish();
}

fn bench_bloom_filter_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom/memory");

    for elements in [10_000, 100_000, 500_000, 1_000_000] {
        let bf = BloomFilter::new(elements, 0.01);
        group.throughput(Throughput::Bytes(bf.memory_bits() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(elements), &elements, |b, n| {
            b.iter(|| {
                black_box(BloomFilter::new(*n, 0.01).memory_bits());
            });
        });
    }

    group.finish();
}

fn bench_bloom_filter_index_integration(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom/index_integration");

    for rows in [50_000, 100_000, 500_000] {
        let mut idx = SourceIndex::new("zone_id");
        let unique_keys = rows / 10;
        for i in 0..rows {
            let _ = idx.insert(format!("z{}", i % unique_keys), i as u64 * 16, 15);
        }

        let mut bf = BloomFilter::new(unique_keys, 0.01);
        for i in 0..unique_keys {
            bf.insert(&format!("z{}", i));
        }

        group.throughput(Throughput::Elements(rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(rows), &rows, |b, r| {
            b.iter(|| {
                let key = format!("z{}", r % unique_keys);
                if bf.contains(&key) {
                    black_box(idx.lookup(&key).is_some());
                }
            });
        });
    }

    group.finish();
}

fn bench_bloom_filter_false_positive_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom/false_positive_rate");

    for elements in [10_000, 100_000, 500_000] {
        let fpr = 0.01;
        let mut bf = BloomFilter::new(elements, fpr);
        let keys: Vec<String> = (0..elements).map(|i| format!("key_{}", i)).collect();
        for key in &keys {
            bf.insert(key);
        }

        let missing: Vec<String> = (0..10000).map(|i| format!("missing_{}", i)).collect();
        let _fp_count = missing.iter().filter(|k| bf.contains(k)).count();

        group.bench_with_input(BenchmarkId::from_parameter(elements), &elements, |b, n| {
            b.iter(|| {
                let mut bf2 = BloomFilter::new(*n, 0.01);
                for key in &keys[..*n] {
                    bf2.insert(key);
                }
                let fp = missing.iter().filter(|k| bf2.contains(k)).count();
                black_box(fp);
            });
        });
    }

    group.finish();
}

fn bench_xor_filter_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("xor_filter/construction");
    let elements = 100_000usize;

    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function("new", |b| {
        b.iter(|| {
            let xf = XorFilter::new(elements, 0.01);
            black_box(xf.array_size());
            black_box(xf.fingerprint_bits());
        });
    });

    group.finish();
}

fn bench_xor_filter_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("xor_filter/insert");
    let elements = 100_000usize;
    let keys: Vec<String> = (0..elements).map(|i| format!("key_{}", i)).collect();

    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function("insert_100k", |b| {
        b.iter(|| {
            let mut xf = XorFilter::new(elements, 0.01);
            for key in &keys {
                xf.insert(key);
            }
            black_box(&xf);
        });
    });

    group.finish();
}

fn bench_xor_filter_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("xor_filter/lookup");
    let elements = 100_000usize;
    let keys: Vec<String> = (0..elements).map(|i| format!("key_{}", i)).collect();
    let mut xf = XorFilter::new(elements, 0.01);
    for key in &keys {
        xf.insert(key);
    }

    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function("contains_existing", |b| {
        b.iter(|| {
            for key in &keys[..1000] {
                black_box(xf.contains(key));
            }
        });
    });

    group.bench_function("contains_missing", |b| {
        let missing: Vec<String> = (0..1000).map(|i| format!("missing_{}", i)).collect();
        b.iter(|| {
            for key in &missing {
                black_box(xf.contains(key));
            }
        });
    });

    group.finish();
}

fn bench_xor_filter_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("xor_filter/memory");

    for elements in [10_000, 100_000, 500_000, 1_000_000] {
        let xf = XorFilter::new(elements, 0.01);
        group.throughput(Throughput::Bytes(xf.memory_bits() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(elements), &elements, |b, n| {
            b.iter(|| {
                black_box(XorFilter::new(*n, 0.01).memory_bits());
            });
        });
    }

    group.finish();
}

fn bench_filter_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter/comparison");

    for elements in [10_000, 100_000, 500_000] {
        let keys: Vec<String> = (0..elements).map(|i| format!("key_{}", i)).collect();
        let missing: Vec<String> = (0..10000).map(|i| format!("missing_{}", i)).collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_bf", elements)),
            &elements,
            |b, n| {
                b.iter(|| {
                    let mut bf = BloomFilter::new(*n, 0.01);
                    for key in &keys {
                        bf.insert(key);
                    }
                    let fp = missing.iter().filter(|k| bf.contains(k)).count();
                    black_box(fp);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_xf", elements)),
            &elements,
            |b, n| {
                b.iter(|| {
                    let mut xf = XorFilter::new(*n, 0.01);
                    for key in &keys {
                        xf.insert(key);
                    }
                    let fp = missing.iter().filter(|k| xf.contains(k)).count();
                    black_box(fp);
                });
            },
        );
    }

    group.finish();
}

fn bench_filter_size_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter/size");

    for elements in [10_000, 100_000, 500_000, 1_000_000] {
        let bf = BloomFilter::new(elements, 0.01);
        let xf = XorFilter::new(elements, 0.01);

        group.bench_with_input(BenchmarkId::from_parameter(elements), &elements, |b, n| {
            b.iter(|| {
                let bf_size = BloomFilter::new(*n, 0.01).memory_bits();
                let xf_size = XorFilter::new(*n, 0.01).memory_bits();
                black_box((bf_size, xf_size));
            });
        });

        let ratio = (xf.memory_bits() as f64) / (bf.memory_bits() as f64);
        eprintln!(
            "elements={}, bloom={} bits, xor={} bits, ratio={:.2}",
            elements,
            bf.memory_bits(),
            xf.memory_bits(),
            ratio
        );
    }

    group.finish();
}

criterion_group!(
    index_benches,
    bench_index_build_by_format,
    bench_index_load_reuse,
    bench_index_lookup_density,
    bench_bloom_filter_construction,
    bench_bloom_filter_insert,
    bench_bloom_filter_lookup,
    bench_bloom_filter_memory,
    bench_bloom_filter_index_integration,
    bench_bloom_filter_false_positive_rate,
    bench_xor_filter_construction,
    bench_xor_filter_insert,
    bench_xor_filter_lookup,
    bench_xor_filter_memory,
    bench_filter_comparison,
    bench_filter_size_comparison
);
criterion_main!(index_benches);
