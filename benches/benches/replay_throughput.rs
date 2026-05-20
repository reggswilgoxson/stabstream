use std::io::Write;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use stabstream_decoder::NullDecoder;
use stabstream_deserialize::testutil::synthetic_surface_d5_stream;
use stabstream_replay::analyze::{analyze_file, AnalysisConfig};

fn bench_analyze_null_decoder(c: &mut Criterion) {
    const FRAMES: u64 = 10_000;

    // Pre-generate synthetic data and write to a temp file once.
    let bytes = synthetic_surface_d5_stream(FRAMES, 0.05);
    let tmp = std::env::temp_dir().join("stabstream_bench_replay.qssf");
    {
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(&bytes).unwrap();
    }

    let decoder = NullDecoder;

    let mut group = c.benchmark_group("replay_throughput");
    group.throughput(Throughput::Elements(FRAMES));
    group.sample_size(10); // full analysis is slow; fewer samples

    group.bench_function("analyze_10k_frames_null_decoder", |b| {
        b.iter(|| {
            let config = AnalysisConfig {
                window_depth: 5,
                observable_count: 1,
            };
            let report = analyze_file(&tmp, &decoder, config).unwrap();
            criterion::black_box(report.frames_processed)
        });
    });

    group.finish();

    let _ = std::fs::remove_file(&tmp);
}

criterion_group!(benches, bench_analyze_null_decoder);
criterion_main!(benches);
