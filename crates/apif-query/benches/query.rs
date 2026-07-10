use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use std::collections::HashMap;

fn benchmark_parse_query(c: &mut Criterion) {
    let queries = [
        "users status=active",
        "users name~glob\"*John*\"",
        "users msg~re:\"error|warn\"",
        "users age>=18 age<=30 status=active",
        "users id=1,2,3,4,5",
        "orders total>100 status=pending",
    ];

    let mut group = c.benchmark_group("parse_query");
    for query in &queries {
        group.bench_with_input(format!("{:.20}", query.replace('"', "'")), query, |b, q| {
            b.iter(|| apif_query::parse_query(black_box(q)));
        });
    }
    group.finish();
}

fn benchmark_filter_matches(c: &mut Criterion) {
    let row: HashMap<String, String> = HashMap::from([
        ("status".into(), "active".into()),
        ("age".into(), "25".into()),
        ("name".into(), "Alice Johnson".into()),
        ("msg".into(), "error: timeout occurred".into()),
        ("id".into(), "42".into()),
    ]);

    let queries = [
        ("eq", "users status=active"),
        ("like_glob", r#"users name~glob"*John*""#),
        ("regex", r#"users msg~re:"error|warn""#),
        ("numeric_range", "users age>=18 age<=30"),
        ("mixed", "users status=active age>=18"),
    ];

    let mut group = c.benchmark_group("filter_matches");
    for (name, query_str) in &queries {
        let query = apif_query::parse_query(query_str).unwrap();
        group.bench_with_input(*name, &query, |b, q| {
            b.iter(|| q.matches_all(black_box(&row)));
        });
    }
    group.finish();
}

fn benchmark_glob_match(c: &mut Criterion) {
    let patterns = [
        ("*John*", "Alice Johnson"),
        ("*.gctf", "test.gctf"),
        ("prefix*suffix", "prefix_middle_suffix"),
        ("hello", "hello"),
    ];

    let mut group = c.benchmark_group("glob_match");
    for (pattern, value) in &patterns {
        group.bench_with_input(
            format!("{}/{}", pattern, value),
            &(*pattern, *value),
            |b, (pat, val)| {
                b.iter(|| apif_query::glob_match(black_box(pat), black_box(val)));
            },
        );
    }
    group.finish();
}

fn benchmark_regex_cache_effect(c: &mut Criterion) {
    let value = "test_value_123";
    let pattern = r"test_.*\d+";

    let mut group = c.benchmark_group("regex_cache_effect");
    group.bench_function("first_call", |b| {
        b.iter(|| apif_query::regex_match(black_box(pattern), black_box(value)));
    });
    group.bench_function("cached_call", |b| {
        b.iter(|| apif_query::regex_match(black_box(pattern), black_box(value)));
    });
    group.finish();
}

fn benchmark_like_optimizations(c: &mut Criterion) {
    let value = "this is a long test value with numbers 12345";

    let patterns = [
        ("starts_with", "this*"),
        ("ends_with", "*12345"),
        ("contains", "*test*"),
        ("exact", "this is a long test value with numbers 12345"),
        ("glob_regex", "test*123*value"),
    ];

    let mut group = c.benchmark_group("like_optimizations");
    for (name, pattern) in &patterns {
        group.bench_with_input(*name, pattern, |b, pat| {
            b.iter(|| apif_query::glob_match(black_box(pat), black_box(value)));
        });
    }
    group.finish();
}

fn benchmark_numeric_comparison(c: &mut Criterion) {
    let ops = [
        ("gte_numeric", "500", ">=", "100", true),
        ("gte_string", "xyz", ">=", "abc", true),
        ("gte_mixed", "100", ">=", "50", true),
    ];

    let mut group = c.benchmark_group("numeric_comparison");
    for (name, left, op, right, _expected) in &ops {
        group.bench_with_input(*name, &(*left, *op, *right), |b, (l, o, r)| {
            b.iter(|| apif_query::compare_values(black_box(l), black_box(o), black_box(r)));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    benchmark_parse_query,
    benchmark_filter_matches,
    benchmark_glob_match,
    benchmark_regex_cache_effect,
    benchmark_like_optimizations,
    benchmark_numeric_comparison,
);
criterion_main!(benches);
