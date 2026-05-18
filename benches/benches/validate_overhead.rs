use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use stabstream_deserialize::{
    rle::encode_detector_events,
    stream::{QssfStream, StreamConfig},
    testutil::synthetic_surface_d5_stream,
};
use stabstream_validate::policy::ValidationPolicy;
use tokio::runtime::Runtime;

fn bench_validate_overhead(c: &mut Criterion) {
    let stream_bytes = synthetic_surface_d5_stream(1, 0.05);

    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("validate");
    group.throughput(Throughput::Elements(1));

    // Use a small ring buffer so each iteration avoids a 4 MiB mmap/munmap.
    group.bench_function("strict_parity_surface_d5", |b| {
        b.iter(|| {
            rt.block_on(async {
                let cursor = std::io::Cursor::new(&stream_bytes);
                let reader = tokio::io::BufReader::new(cursor);
                let config = StreamConfig {
                    validation: ValidationPolicy::StrictParity,
                    ring_buf_bytes: 4096,
                    ..Default::default()
                };
                let mut stream = QssfStream::new(reader, config);
                let frame = stream.next_frame().await.unwrap();
                criterion::black_box(frame.map(|f| f.header.frame_id))
            })
        });
    });

    group.bench_function("disabled_surface_d5", |b| {
        b.iter(|| {
            rt.block_on(async {
                let cursor = std::io::Cursor::new(&stream_bytes);
                let reader = tokio::io::BufReader::new(cursor);
                let config = StreamConfig {
                    validation: ValidationPolicy::Disabled,
                    ring_buf_bytes: 4096,
                    ..Default::default()
                };
                let mut stream = QssfStream::new(reader, config);
                let frame = stream.next_frame().await.unwrap();
                criterion::black_box(frame.map(|f| f.header.frame_id))
            })
        });
    });

    // Standalone RLE popcount micro-benchmark (no async overhead).
    let events: Vec<bool> = (0..24).map(|i| i % 5 == 0).collect();
    let encoded = encode_detector_events(&events);
    group.bench_function("rle_popcount_24_ancillas", |b| {
        b.iter(|| {
            criterion::black_box(stabstream_deserialize::rle::popcount_rle(
                criterion::black_box(&encoded),
            ))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_validate_overhead);
criterion_main!(benches);
