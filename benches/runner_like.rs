mod common;

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn bench_workflow_and_plan(c: &mut Criterion) {
    let source = common::generate_multi_doc_content(100);
    let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").expect("parse");

    c.bench_function("runner_like/workflow_from_document_chain_100", |b| {
        b.iter(|| {
            for d in doc.iter_chain() {
                let w = grpctestify::execution::Workflow::from_document_with_analysis(d);
                black_box(w.summary.total_requests);
            }
        });
    });

    c.bench_function("runner_like/execution_plan_from_document_chain_100", |b| {
        b.iter(|| {
            for d in doc.iter_chain() {
                let plan = grpctestify::execution::ExecutionPlan::from_document(d);
                black_box(plan.summary.total_requests);
            }
        });
    });
}

criterion_group!(runner_like_benches, bench_workflow_and_plan);
criterion_main!(runner_like_benches);
