use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use stabstream_core::window::{OwnedSyndromeData, SyndromeWindow};
use stabstream_decoder::{union_find::UnionFindDecoder, Decoder};
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};

const ANCILLA_COUNT: usize = 24;
const WINDOW_DEPTH: usize = 5;

/// Build a chain-of-chains DEM representing a d=5 surface-code-like graph with
/// `WINDOW_DEPTH` rounds × `ANCILLA_COUNT` ancillas = 120 detector nodes.
///
/// Edge structure (221 edges total):
///   - 5 × 23 = 115 within-round horizontal edges
///   - 4 × 24 =  96 cross-round temporal edges
///   -      10 boundary (first + last ancilla per round → ^ L0)
fn make_d5_dem_str() -> String {
    use std::fmt::Write;
    let mut dem = String::with_capacity(8192);

    // Within-round chain edges
    for r in 0..WINDOW_DEPTH {
        for i in 0..ANCILLA_COUNT - 1 {
            let d1 = r * ANCILLA_COUNT + i;
            let d2 = r * ANCILLA_COUNT + i + 1;
            writeln!(dem, "error(0.05) D{d1} D{d2}").unwrap();
        }
    }

    // Cross-round temporal edges
    for r in 0..WINDOW_DEPTH - 1 {
        for i in 0..ANCILLA_COUNT {
            let d1 = r * ANCILLA_COUNT + i;
            let d2 = (r + 1) * ANCILLA_COUNT + i;
            writeln!(dem, "error(0.05) D{d1} D{d2}").unwrap();
        }
    }

    // Boundary edges: first and last ancilla in each round
    for r in 0..WINDOW_DEPTH {
        let first = r * ANCILLA_COUNT;
        let last = r * ANCILLA_COUNT + ANCILLA_COUNT - 1;
        writeln!(dem, "error(0.05) D{first} ^ L0").unwrap();
        writeln!(dem, "error(0.05) D{last} ^ L0").unwrap();
    }

    // Detector declarations
    for d in 0..(WINDOW_DEPTH * ANCILLA_COUNT) {
        writeln!(dem, "detector D{d}").unwrap();
    }

    writeln!(dem, "logical_observable L0").unwrap();
    dem
}

/// Pre-fill a SyndromeWindow with `WINDOW_DEPTH` rounds at ~5% fire rate
/// (deterministic: ancilla i fires when i % 20 == 3).
fn make_window() -> SyndromeWindow {
    let mut w = SyndromeWindow::new(ANCILLA_COUNT, WINDOW_DEPTH);
    for i in 0..WINDOW_DEPTH {
        w.push_owned(OwnedSyndromeData {
            frame_id: i as u64,
            round: i as u32,
            timestamp_ns: i as u64 * 1_100_000,
            detector_events: (0..ANCILLA_COUNT).map(|j| j % 20 == 3).collect(),
            meas_results: vec![1i8; ANCILLA_COUNT],
        });
    }
    w
}

fn bench_uf_decode(c: &mut Criterion) {
    let dem_str = make_d5_dem_str();
    let dem = DetectorErrorModel::parse(&dem_str).expect("valid DEM");
    let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
    let decoder = UnionFindDecoder::new(Arc::clone(&graph));

    let mut group = c.benchmark_group("uf_decode");
    group.throughput(Throughput::Elements(1));

    // Hot-path decode: pre-filled 5-round window, ~5% fire rate (~6 active detectors).
    // Window is rebuilt in setup so the decode itself is the only measured work.
    group.bench_function("decode_window_d5_steady_state", |b| {
        b.iter_batched(
            make_window,
            |w| criterion::black_box(decoder.decode_window(&w)),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_uf_decode);
criterion_main!(benches);
