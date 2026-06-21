use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use stabstream_core::{
    frame::{FrameHeader, SyndromeFrame, SyndromePayload},
    window::{OwnedSyndromeData, SyndromeWindow},
};

const ANCILLA_COUNT: usize = 24;
const WINDOW_DEPTH: usize = 5;

fn make_owned_data(frame_id: u64) -> OwnedSyndromeData {
    OwnedSyndromeData {
        frame_id,
        round: frame_id as u32,
        timestamp_ns: frame_id * 1_100_000,
        detector_events: (0..ANCILLA_COUNT).map(|i| i % 20 == 3).collect(),
        meas_results: vec![1i8; ANCILLA_COUNT],
    }
}

fn prefilled_window() -> SyndromeWindow {
    let mut w = SyndromeWindow::new(ANCILLA_COUNT, WINDOW_DEPTH);
    for i in 0..WINDOW_DEPTH {
        w.push_owned(make_owned_data(i as u64));
    }
    w
}

fn bench_window_slide(c: &mut Criterion) {
    let mut group = c.benchmark_group("window_slide");
    group.throughput(Throughput::Elements(1));

    // Steady-state slide: window pre-filled to depth=5 so every push evicts the
    // oldest entry. Measures VecDeque rotation + detector-matrix rebuild only —
    // no RLE decode. This is the "window slide: 20 ns" budget component.
    let new_frame = make_owned_data(WINDOW_DEPTH as u64);
    let mut window = prefilled_window();
    group.bench_function("push_owned_steady_state_d5", |b| {
        b.iter_batched(
            || new_frame.clone(),
            |data| criterion::black_box(window.push_owned(data)),
            BatchSize::SmallInput,
        );
    });

    // Push from a borrowed SyndromeFrame: adds RLE decode on top of the slide.
    // RLE: 0x81 = 1 one, 0x17 = 23 zeros → 24 ancillas at ~4% fire rate.
    static RLE: &[u8] = &[0x81, 0x17];
    static MEAS: [i8; ANCILLA_COUNT] = [1; ANCILLA_COUNT];
    let frame = SyndromeFrame {
        header: FrameHeader {
            frame_id: 0,
            round: 0,
            timestamp_ns: 0,
            qubit_count: 25,
            ancilla_count: ANCILLA_COUNT as u16,
            payload_len: 0,
            code_type: 0x01,
            distance: 5,
            flags: 0,
            crc32: 0,
        },
        payload: SyndromePayload {
            detector_events: RLE,
            meas_results: &MEAS,
            timing_offsets: &[],
            parity_checks: &[],
        },
        metadata: None,
        annotations: None,
    };
    let mut window2 = prefilled_window();
    group.bench_function("push_with_rle_decode_d5", |b| {
        b.iter(|| criterion::black_box(window2.push(&frame)));
    });

    group.finish();
}

criterion_group!(benches, bench_window_slide);
criterion_main!(benches);
