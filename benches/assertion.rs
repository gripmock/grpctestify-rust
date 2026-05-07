use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn bench_assertion(c: &mut Criterion) {
    let simple = ".id == 42 and .active == true";
    let complex = "if @len(.items) > 0 then (@regex(.name, /foo.*/i) and .meta.version >= 2) else (.status == \"empty\" or {{ feature_flag }} == true) end";

    c.bench_function("assertion/tokenize_simple", |b| {
        b.iter(|| {
            let tokens = grpctestify::parser::tokenize_assertion(black_box(simple));
            black_box(tokens.len());
        });
    });

    c.bench_function("assertion/tokenize_complex", |b| {
        b.iter(|| {
            let tokens = grpctestify::parser::tokenize_assertion(black_box(complex));
            black_box(tokens.len());
        });
    });

    c.bench_function("assertion/parse_simple", |b| {
        b.iter(|| {
            let expr = grpctestify::parser::parse_assertion(black_box(simple));
            black_box(expr);
        });
    });

    c.bench_function("assertion/parse_complex_serialize", |b| {
        b.iter(|| {
            let expr = grpctestify::parser::parse_assertion(black_box(complex));
            let s = grpctestify::parser::assertion_to_string(&expr);
            black_box(s.len());
        });
    });
}

criterion_group!(assertion_benches, bench_assertion);
criterion_main!(assertion_benches);
