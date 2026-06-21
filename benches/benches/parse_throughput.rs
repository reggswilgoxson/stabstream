use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use stabstream_deserialize::{
    parser::{parse_file_header, parse_frame_header},
    stream::{QssfStream, StreamConfig},
    testutil::synthetic_surface_d5_stream,
};
use stabstream_validate::policy::ValidationPolicy;
use tokio::runtime::Runtime;

fn bench_parse_surface_d5(c: &mut Criterion) {
    let stream_bytes = synthetic_surface_d5_stream(1, 0.05);

    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Elements(1));

    // -- Synchronous benchmarks: pure deserialization cost, no async overhead --

    // Frame header parse: 36-byte LE field extraction + CRC32 verification.
    // This is the primary number against the "frame deserialization: 200 ns" budget.
    group.bench_function("frame_header_sync", |b| {
        // File header is 26 bytes; frame header immediately follows.
        let hdr_bytes = &stream_bytes[26..62];
        b.iter(|| {
            criterion::black_box(parse_frame_header(criterion::black_box(hdr_bytes)).unwrap())
        });
    });

    // Full per-frame synchronous parse: file header + frame header + payload slice offsets.
    // Represents the complete deserialization work for one frame without any I/O.
    group.bench_function("full_frame_sync", |b| {
        b.iter(|| {
            let bytes = criterion::black_box(stream_bytes.as_slice());
            let (_, file_end) = parse_file_header(bytes).unwrap();
            let (frame_hdr, _) = parse_frame_header(&bytes[file_end..]).unwrap();
            let payload_start = file_end + 36;
            let de_len =
                u16::from_le_bytes([bytes[payload_start], bytes[payload_start + 1]]) as usize;
            let ancilla = frame_hdr.ancilla_count as usize;
            let de_slice = &bytes[payload_start + 2..payload_start + 2 + de_len];
            let meas_slice = &bytes[payload_start + 2 + de_len..][..ancilla];
            criterion::black_box((frame_hdr.frame_id, de_slice, meas_slice))
        });
    });

    // CRC32 over the 32-byte frame header body — isolates the hash cost that sits
    // inside parse_frame_header. This is the "CRC validation: 70 ns" budget component.
    group.bench_function("crc32_frame_header_32b", |b| {
        // Frame header starts at byte 26; CRC covers the first 32 of its 36 bytes.
        let hdr_body = &stream_bytes[26..58];
        b.iter(|| criterion::black_box(crc32fast::hash(criterion::black_box(hdr_body))));
    });

    // -- Async streaming path: shows end-to-end overhead vs the sync baselines --
    // Runtime is created once; a fresh Cursor is supplied per iteration via
    // iter_batched so QssfStream setup cost (ring buffer alloc) is included
    // but not amortised across multiple frames.
    let rt = Runtime::new().unwrap();
    group.bench_function("stream_async", |b| {
        b.iter_batched(
            || std::io::Cursor::new(stream_bytes.as_slice()),
            |cursor| {
                rt.block_on(async {
                    let reader = tokio::io::BufReader::new(cursor);
                    let config = StreamConfig {
                        validation: ValidationPolicy::Disabled,
                        ring_buf_bytes: 4096,
                        ..Default::default()
                    };
                    let mut stream = QssfStream::new(reader, config);
                    criterion::black_box(
                        stream
                            .next_frame()
                            .await
                            .unwrap()
                            .map(|f| f.header.frame_id),
                    )
                })
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_parse_surface_d5);
criterion_main!(benches);
