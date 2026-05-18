use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use stabstream_deserialize::{
    stream::{QssfStream, StreamConfig},
    testutil::synthetic_surface_d5_stream,
};
use stabstream_validate::policy::ValidationPolicy;
use tokio::runtime::Runtime;

fn bench_parse_surface_d5(c: &mut Criterion) {
    // Pre-generate a single-frame QSSF byte stream; reuse it for every iteration.
    let stream_bytes = synthetic_surface_d5_stream(1, 0.05);

    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Elements(1));

    // Use a small ring buffer so each iteration avoids a 4 MiB mmap/munmap.
    // The synthetic single-frame payload is ~135 bytes; 4 KiB is ample.
    group.bench_function("surface_d5_parse_only", |b| {
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

    group.bench_function("surface_d5_crc_only", |b| {
        b.iter(|| {
            rt.block_on(async {
                let cursor = std::io::Cursor::new(&stream_bytes);
                let reader = tokio::io::BufReader::new(cursor);
                let config = StreamConfig {
                    validation: ValidationPolicy::CrcOnly,
                    ring_buf_bytes: 4096,
                    ..Default::default()
                };
                let mut stream = QssfStream::new(reader, config);
                let frame = stream.next_frame().await.unwrap();
                criterion::black_box(frame.map(|f| f.header.frame_id))
            })
        });
    });

    group.finish();
}

criterion_group!(benches, bench_parse_surface_d5);
criterion_main!(benches);
