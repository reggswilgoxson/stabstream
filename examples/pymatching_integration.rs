//! Example: feed stabstream syndrome frames to PyMatching via the Python bridge.
//!
//! This Rust example demonstrates the end-to-end pipeline:
//!   1. Parse a Stim DEM → `DetectorErrorModel` + `SpacetimeGraph`
//!   2. Stream QSSF frames → `SyndromeWindow`
//!   3. Export the window's detector matrix to Python via `stabstream-py`
//!   4. Call `DetectorErrorModel.to_pymatching()` to build a `pymatching.Matching`
//!   5. Run MWPM decode and accumulate logical error rate
//!
//! The Python bridge (`stabstream-py`) exposes the Rust types with zero-copy
//! NumPy arrays.  All decoding happens in Python (PyMatching); stabstream
//! provides the deserializer and detector matrix.
//!
//! # Running
//!
//! ```bash
//! # Build and install the Python wheel first
//! cd crates/stabstream-py && maturin develop && cd ../..
//!
//! # Then run the Python example directly:
//! python python/examples/pymatching_bridge.py model.dem recording.qssf
//! ```
//!
//! The Rust side of the integration is the `SyndromeWindow` and
//! `UnionFindDecoder` — see `crates/stabstream-decoder/src/union_find.rs` for
//! the native O(n·α(n)) decoder that does not require Python at all.

use std::sync::Arc;

use stabstream_core::window::SyndromeWindow;
use stabstream_decoder::{Decoder, UnionFindDecoder};
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};
use stabstream_metrics::LogicalErrorAccumulator;

fn main() {
    let dem_text = r#"error(0.01) D0 D1 ^ L0
error(0.01) D1 D2
error(0.01) D2 ^ L0
detector D0
detector D1
detector D2
logical_observable L0
"#;

    // Parse DEM and build the spacetime graph
    let dem = DetectorErrorModel::parse(dem_text).expect("DEM parse failed");
    println!(
        "DEM: {} detectors, {} observables, {} error mechanisms",
        dem.detector_count,
        dem.observable_count,
        dem.errors.len()
    );

    let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
    println!(
        "SpacetimeGraph: {} nodes, {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    // Build the native Union-Find decoder (no Python required)
    let decoder = UnionFindDecoder::new(Arc::clone(&graph));

    // Accumulate logical error statistics
    let acc = LogicalErrorAccumulator::new(dem.observable_count);

    // Simulate 1000 shots with an empty syndrome window (no errors → expect p_L ≈ 0)
    let window = SyndromeWindow::new(dem.detector_count, 5);
    for _ in 0..1_000 {
        let result = decoder.decode_window(&window);
        acc.record(&result, 0 /* ground truth: no errors */);
    }

    let report = acc.report();
    println!(
        "\nLogical error rate (1000 shots, no errors): mean p_L = {:.4e}",
        report.mean_logical_error_rate
    );
    assert_eq!(report.total_shots, 1_000);

    println!("\nFor PyMatching MWPM integration, see:");
    println!("  python/examples/pymatching_bridge.py");
    println!("  stabstream-py exposes DetectorErrorModel.to_pymatching()");
    println!("  which builds a pymatching.Matching with -ln(p/(1-p)) edge weights.");
}
