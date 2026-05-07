mod common;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser/parse_gctf");

    let single = common::single_doc_content();
    group.throughput(Throughput::Bytes(single.len() as u64));
    group.bench_function("single_doc", |b| {
        b.iter(|| {
            let doc = grpctestify::parser::parse_gctf_from_str(black_box(single), "bench.gctf")
                .expect("parse single doc");
            black_box(doc.document_count());
        });
    });

    for docs in [10usize, 100, 1000] {
        let source = common::generate_multi_doc_content(docs);
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(docs), &source, |b, input| {
            b.iter(|| {
                let doc = grpctestify::parser::parse_gctf_from_str(black_box(input), "bench.gctf")
                    .expect("parse multi doc");
                black_box(doc.document_count());
            });
        });
    }

    group.finish();
}

fn bench_validate(c: &mut Criterion) {
    let source = common::generate_multi_doc_content(100);
    let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").expect("parse");

    c.bench_function("parser/validate_document_diagnostics_chain_100", |b| {
        b.iter(|| {
            for d in doc.iter_chain() {
                let out = grpctestify::parser::validate_document_diagnostics(d);
                black_box(out.len());
            }
        });
    });
}

criterion_group!(parser_benches, bench_parse, bench_validate);
criterion_main!(parser_benches);
