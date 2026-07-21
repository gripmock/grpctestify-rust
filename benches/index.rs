use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use grpctestify::bench::sources::definition::SourceDefinition;
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

criterion_group!(
    index_benches,
    bench_index_build_by_format,
    bench_index_load_reuse,
    bench_index_lookup_density
);
criterion_main!(index_benches);
