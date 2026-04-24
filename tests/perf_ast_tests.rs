use std::hint::black_box;
use std::time::Instant;

fn bench(name: &str, iterations: u32, mut f: impl FnMut()) {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    eprintln!(
        "{}: {} iterations in {:?} ({:?}/call)",
        name, iterations, elapsed, per_call
    );
}

#[test]
fn perf_ast_tokenize_simple() {
    let assertion = ".id == 42 and .active == true";
    bench("ast_tokenize_simple", 200_000, || {
        let tokens = grpctestify::parser::tokenize_assertion(black_box(assertion));
        black_box(tokens.len());
    });
}

#[test]
fn perf_ast_tokenize_complex() {
    let assertion = "if @len(.items) > 0 then (@regex(.name, /foo.*/i) and .meta.version >= 2) else (.status == \"empty\" or {{ feature_flag }} == true) end";
    bench("ast_tokenize_complex", 100_000, || {
        let tokens = grpctestify::parser::tokenize_assertion(black_box(assertion));
        black_box(tokens.len());
    });
}

#[test]
fn perf_ast_parse_simple() {
    let assertion = ".id == 42 and .active == true";
    bench("ast_parse_simple", 100_000, || {
        let expr = grpctestify::parser::parse_assertion(black_box(assertion));
        black_box(matches!(expr, grpctestify::parser::AssertionExpr::Raw(_)));
    });
}

#[test]
fn perf_ast_parse_complex_with_serialize() {
    let assertion = "if @len(.items) > 0 then (@regex(.name, /foo.*/i) and .meta.version >= 2) else (.status == \"empty\" or {{ feature_flag }} == true) end";
    bench("ast_parse_complex_serialize", 50_000, || {
        let expr = grpctestify::parser::parse_assertion(black_box(assertion));
        let s = grpctestify::parser::assertion_to_string(&expr);
        black_box(s.len());
    });
}
