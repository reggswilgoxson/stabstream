use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use stabstream_deserialize::rle::{decode_detector_events, encode_detector_events, popcount_rle};

fn bench_rle_encode_d5(c: &mut Criterion) {
    // d=5 surface code: 24 ancillas, ~5% firing rate
    let events: Vec<bool> = (0..24).map(|i| i % 20 == 3).collect();

    let mut group = c.benchmark_group("noise_sampler");
    group.throughput(Throughput::Elements(24)); // 24 events per call

    group.bench_function("rle_encode_24_ancillas", |b| {
        b.iter(|| criterion::black_box(encode_detector_events(criterion::black_box(&events))));
    });

    let encoded = encode_detector_events(&events);
    group.bench_function("rle_decode_24_ancillas", |b| {
        b.iter(|| criterion::black_box(decode_detector_events(criterion::black_box(&encoded))));
    });

    group.bench_function("popcount_rle_24_ancillas", |b| {
        b.iter(|| criterion::black_box(popcount_rle(criterion::black_box(&encoded))));
    });

    group.finish();
}

fn bench_rle_encode_batch(c: &mut Criterion) {
    // Simulate 1000-shot batch: encode 1000 independent detector-event vectors.
    let shots: Vec<Vec<bool>> = (0..1000u32)
        .map(|s| (0..24).map(|i| (s + i as u32) % 17 == 0).collect())
        .collect();

    let mut group = c.benchmark_group("noise_sampler_batch");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("rle_encode_batch_1k", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for events in &shots {
                total += encode_detector_events(events).len();
            }
            criterion::black_box(total)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_rle_encode_d5, bench_rle_encode_batch);
criterion_main!(benches);
