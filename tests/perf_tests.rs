#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
// Performance tests (no nightly required)

use std::time::Instant;

#[path = "../benches/common.rs"]
mod common;
use common::generate_multi_doc_content;

#[test]
fn perf_parse_single_document() {
    let source = r#"--- ENDPOINT ---
svc.Method
--- REQUEST ---
{"id": 1}
--- RESPONSE ---
{"status": "ok"}
"#;
    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let doc = grpctestify::parser::parse_gctf_from_str(source, "bench.gctf").unwrap();
        assert!(doc.is_single_document());
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "parse_single: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
    // Should be well under 1ms per call
    assert!(
        per_call.as_micros() < 1000,
        "parse took too long: {:?}",
        per_call
    );
}

#[test]
fn perf_parse_10_documents() {
    let source = generate_multi_doc_content(10);
    let iterations = 1_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").unwrap();
        assert_eq!(doc.document_count(), 10);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "parse_10_docs: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
}

#[test]
fn perf_parse_100_documents() {
    let source = generate_multi_doc_content(100);
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").unwrap();
        assert_eq!(doc.document_count(), 100);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "parse_100_docs: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
}

#[test]
fn perf_parse_1000_documents() {
    let source = generate_multi_doc_content(1000);
    let iterations = 10;
    let start = Instant::now();
    for _ in 0..iterations {
        let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").unwrap();
        assert_eq!(doc.document_count(), 1000);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "parse_1000_docs: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
}

#[test]
fn perf_iter_chain_single() {
    let source = r#"--- ENDPOINT ---
svc.Method
--- REQUEST ---
{"id": 1}
--- RESPONSE ---
{"status": "ok"}
"#;
    let doc = grpctestify::parser::parse_gctf_from_str(source, "bench.gctf").unwrap();
    let iterations = 100_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let count = doc.iter_chain().count();
        assert_eq!(count, 1);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "iter_chain_single: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
}

#[test]
fn perf_iter_chain_100() {
    let source = generate_multi_doc_content(100);
    let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").unwrap();
    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let count = doc.iter_chain().count();
        assert_eq!(count, 100);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "iter_chain_100: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
}

#[test]
fn perf_validate_chain_100() {
    let source = generate_multi_doc_content(100);
    let doc = grpctestify::parser::parse_gctf_from_str(&source, "bench.gctf").unwrap();
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        for d in doc.iter_chain() {
            let _ = grpctestify::parser::validate_document(d);
        }
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "validate_chain_100: {} iterations in {:?} ({:?}/call)",
        iterations, elapsed, per_call
    );
}
