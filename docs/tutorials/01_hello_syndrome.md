# Tutorial 1: Hello Syndrome — Parse Your First QSSF Stream

## Prerequisites

```bash
# Rust toolchain (1.75+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build the workspace
git clone https://github.com/your-org/stabstream
cd stabstream
cargo build --workspace

# Optional: Stim (for generating test data)
pip install stim
```

## Step 1: Generate a test QSSF recording

```bash
# Generate 1000 shots from a d=5 surface code circuit
stabstream-convert stim-to-qssf \
    --circuit circuit.stim \
    --shots 1000 \
    --with-observables \
    --out test.qssf
```

Or use `stabstream-sim` to serve a live stream:

```bash
# Terminal 1: start the simulator
cargo run -p stabstream-sim -- --simulator native --dem circuit.dem --port 9000

# Terminal 2: connect with the dashboard
cargo run -p stabstream-dashboard -- --source tcp://localhost:9000
```

## Step 2: Parse frames in Rust

```rust
use stabstream_core::frame::SyndromeFrame;
use stabstream_deserialize::stream::StabstreamStream;

let stream = StabstreamStream::from_file("test.qssf")?;
let mut frame_count = 0;

for frame in stream.frames() {
    let frame: SyndromeFrame = frame?;
    frame_count += 1;

    if frame_count <= 3 {
        println!(
            "frame_id={} ancilla_count={} detector_events={}",
            frame.frame_id,
            frame.ancilla_count,
            frame.detector_event_count,
        );
    }
}
println!("Total frames: {frame_count}");
```

## Step 3: Build a sliding SyndromeWindow

```rust
use stabstream_core::window::SyndromeWindow;
use stabstream_decoder::{Decoder, UnionFindDecoder};
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};
use std::sync::Arc;

let dem_text = std::fs::read_to_string("circuit.dem")?;
let dem = DetectorErrorModel::parse(&dem_text)?;
let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
let decoder = UnionFindDecoder::new(Arc::clone(&graph));

let window_depth = 5;
let mut window = SyndromeWindow::new(dem.detector_count, window_depth);

for frame in stream.frames() {
    let frame = frame?;
    window.push(&frame);

    if window.is_full() {
        let result = decoder.decode_window(&window);
        println!(
            "observable_flips={:#b} confidence={:.3}",
            result.observable_flips, result.confidence
        );
    }
}
```

## Step 4: Accumulate logical error rates

```rust
use stabstream_metrics::LogicalErrorAccumulator;

let acc = LogicalErrorAccumulator::new(1); // 1 logical qubit

for frame in stream.frames() {
    let frame = frame?;
    window.push(&frame);

    if window.is_full() {
        let result = decoder.decode_window(&window);
        // ground_truth comes from the QSSF tag 0x10 (if --with-observables was used)
        let ground_truth = window.latest_frame()
            .and_then(|f| f.observable_flips)
            .unwrap_or(0);
        acc.record(&result, ground_truth);
    }
}

let report = acc.report();
println!("p_L = {:.4e}", report.mean_logical_error_rate);
```

## Next steps

- [Tutorial 2: Offline Analysis with stabstream-analyze](02_offline_analysis.md)
- [Tutorial 3: Python Integration](03_python_integration.md)
- [Theory: QEC Primer](../theory/qec_primer.md)
