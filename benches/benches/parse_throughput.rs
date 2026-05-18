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

    group.bench_function("surface_d5_parse_only", |b| {
        b.iter(|| {
            rt.block_on(async {
                let cursor = std::io::Cursor::new(&stream_bytes);
                let reader = tokio::io::BufReader::new(cursor);
                let config = StreamConfig {
                    validation: ValidationPolicy::Disabled,
                    ..Default::default()
                };
                let mut stream = QssfStream::new(reader, config);
                criterion::black_box(stream.next_frame().await.unwrap())
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
                    ..Default::default()
                };
                let mut stream = QssfStream::new(reader, config);
                criterion::black_box(stream.next_frame().await.unwrap())
            })
        });
    });

    group.finish();
}

criterion_group!(benches, bench_parse_surface_d5);
criterion_main!(benches);
