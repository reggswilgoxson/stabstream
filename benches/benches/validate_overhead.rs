use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_validate_overhead(c: &mut Criterion) {
    // Compares ValidationPolicy::StrictParity vs ValidationPolicy::Disabled
    // to isolate the cost of parity checking against the parse-only baseline.
    let mut group = c.benchmark_group("validate");
    group.throughput(Throughput::Elements(1));

    group.bench_function("strict_parity_surface_d5", |b| {
        b.iter(|| {
            // TODO: parse one pre-built surface_d5 frame, then run
            // stabstream_validate::parity::check_parity
        })
    });

    group.bench_function("disabled_surface_d5", |b| {
        b.iter(|| {
            // TODO: parse one pre-built frame with ValidationPolicy::Disabled
        })
    });

    group.finish();
}

criterion_group!(benches, bench_validate_overhead);
criterion_main!(benches);
