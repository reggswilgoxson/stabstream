use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_parse_surface_d5(c: &mut Criterion) {
    // TODO: generate synthetic QSSF frames for surface_d5 in memory,
    // then benchmark QssfStream parsing in a tokio runtime.
    // Report throughput in frames/second and bytes/second.
    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Elements(1));
    group.bench_function("surface_d5_parse_only", |b| {
        b.iter(|| {
            // TODO: parse one frame from pre-built byte slice
        })
    });
    group.finish();
}

criterion_group!(benches, bench_parse_surface_d5);
criterion_main!(benches);
