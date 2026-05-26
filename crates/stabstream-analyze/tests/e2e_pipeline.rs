//! End-to-end pipeline integration test.
//!
//! Exercises the full stabstream pipeline in one test:
//!   QSSF bytes → deserialize → SyndromeWindow → UnionFindDecoder → LogicalErrorAccumulator
//!
//! Fire rate is chosen so that only frame 0 / ancilla 0 ever fires (the only
//! (i + j) that is divisible by the modulus when i ∈ 0..5 and j ∈ 0..23).
//! That keeps the maximum active-detector node ID at 0, which is safely within
//! the single-detector graph used here.

use std::sync::Arc;

use stabstream_core::window::SyndromeWindow;
use stabstream_decoder::{union_find::UnionFindDecoder, Decoder};
use stabstream_dem::{graph::SpacetimeGraph, DetectorErrorModel};
use stabstream_deserialize::{
    stream::{QssfStream, StreamConfig},
    testutil::synthetic_surface_d5_stream,
};
use stabstream_metrics::LogicalErrorAccumulator;
use stabstream_validate::policy::ValidationPolicy;

/// One-detector / one-observable DEM. The graph has two nodes: D0 (index 0)
/// and the boundary (index 1). Active detector 0 is the only node ever
/// addressed by the controlled synthetic stream below.
const MINIMAL_DEM: &str = "error(0.1) D0 ^ L0\ndetector D0\nlogical_observable L0\n";

#[tokio::test]
async fn e2e_pipeline_parse_decode_accumulate() {
    const FRAMES: u64 = 5;

    // Build a synthetic 5-frame QSSF byte stream.
    // fire_rate=0.03 → internal modulus = floor(1/0.03)+1 = 34.
    // For 5 frames (i=0..4) × 24 ancillas (j=0..23): max i+j = 27 < 34,
    // so the only firing position is i=0, j=0 → node ID 0.
    let bytes = synthetic_surface_d5_stream(FRAMES, 0.03);

    // Stage 1 – deserialize.
    let cursor = std::io::Cursor::new(&bytes);
    let reader = tokio::io::BufReader::new(cursor);
    let mut stream = QssfStream::new(
        reader,
        StreamConfig {
            validation: ValidationPolicy::Disabled,
            ..Default::default()
        },
    );

    // Stage 2 – build decoder from DEM.
    let dem = DetectorErrorModel::parse(MINIMAL_DEM).unwrap();
    let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
    let decoder = UnionFindDecoder::new(graph);

    // Stage 3 – accumulate metrics.
    let acc = LogicalErrorAccumulator::new(1);
    let mut window = SyndromeWindow::new(24, 5);
    let mut frame_count = 0u64;

    while let Some(frame) = stream.next_frame().await.unwrap() {
        window.push(&frame);
        let result = decoder.decode_window(&window);
        acc.record(&result, 0 /* ground truth: no observable flipped */);
        frame_count += 1;
    }

    // Every frame was parsed and decoded exactly once.
    assert_eq!(frame_count, FRAMES, "all frames parsed");
    assert_eq!(acc.total_shots(), FRAMES, "one shot recorded per frame");

    // The pipeline produced a valid logical error rate in [0, 1].
    let p_l = acc.logical_error_rate(0);
    assert!(
        (0.0..=1.0).contains(&p_l),
        "p_L must be in [0, 1], got {p_l}"
    );
}
