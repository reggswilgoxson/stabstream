# Tutorial 2: Offline Analysis with stabstream-analyze

`stabstream-analyze` reads a QSSF recording (plain or zstd-compressed), slides
a `SyndromeWindow` through every frame, decodes each full window, and emits an
`AnalysisReport` as JSON.

## Basic usage

```bash
# Analyze with the Union-Find decoder (requires a DEM)
stabstream-analyze \
    --input recording.qssf \
    --dem circuit.dem \
    --decoder union-find \
    --window-depth 5 \
    --observable-count 1 \
    --output report.json

# Print report to stdout (JSON) and human summary to stderr
stabstream-analyze --input recording.qssf --dem circuit.dem

# Null decoder (just computes latency / fire frequency, no p_L)
stabstream-analyze --input recording.qssf --decoder null
```

## Output format

```json
{
  "frames_processed": 10000,
  "total_shots": 9996,
  "observable_count": 1,
  "logical_error_rates": [0.00312],
  "mean_logical_error_rate": 0.00312,
  "ground_truth_available": true,
  "mean_decode_latency_ns": 384,
  "p50_decode_latency_ns": 310,
  "p99_decode_latency_ns": 1420,
  "max_decode_latency_ns": 5880,
  "ancilla_count": 24,
  "per_ancilla_fire_frequency": [0.051, 0.048, 0.053, ...],
  "syndrome_weight_histogram": [5210, 3420, 1180, 190, ...]
}
```

### Field reference

| Field | Description |
|-------|-------------|
| `frames_processed` | Total frames read from the file |
| `total_shots` | Decoder invocations (= `frames_processed - window_depth + 1`) |
| `logical_error_rates` | Per-observable p_L (requires tag 0x10 ground truth) |
| `ground_truth_available` | Whether QSSF tag 0x10 was present |
| `mean_decode_latency_ns` | Average wall-clock time per decode call |
| `p50/p99_decode_latency_ns` | Latency percentiles |
| `per_ancilla_fire_frequency` | Fraction of frames each ancilla fired — useful for spotting noisy qubits |
| `syndrome_weight_histogram` | Distribution of syndrome weights (0 = no errors detected) |

## From Rust: `StreamPlayer::analyze()`

```rust
use std::fs::File;
use stabstream_replay::{player::StreamPlayer, analyze::AnalysisConfig};
use stabstream_decoder::NullDecoder;

let file = File::open("recording.qssf.zst")?;
let mut player = StreamPlayer::new(file)?;

let config = AnalysisConfig {
    window_depth: 5,
    observable_count: 1,
};
let report = player.analyze(&NullDecoder, config)?;
println!("{}", report.summary());
```

For the Union-Find decoder:

```rust
use stabstream_decoder::union_find::UnionFindDecoder;
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};
use std::sync::Arc;

let dem = DetectorErrorModel::parse(&std::fs::read_to_string("circuit.dem")?)?;
let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
let decoder = UnionFindDecoder::new(Arc::clone(&graph));

let report = player.analyze(&decoder, AnalysisConfig::default())?;
println!("p_L = {:.4e}", report.mean_logical_error_rate);
```

## Hardware debugging: per-ancilla fire frequency

`per_ancilla_fire_frequency[i]` is the fraction of frames in which ancilla `i`
fired. Expected value for a healthy qubit: ~2p (twice the physical error rate,
for data errors + measurement errors). Persistent outliers indicate:

- **High fire rate** (>2×): noisy data qubit or ancilla qubit
- **Zero fire rate**: disconnected qubit or readout failure

Cross-reference with `HardwareSchema.stabilizers[i].qubits` to identify the
physical qubits involved.

## Generating ground-truth recordings

To get non-zero `logical_error_rates`, the recording must include tag 0x10
(observable flip bitmask per frame):

```bash
stabstream-convert stim-to-qssf \
    --circuit circuit.stim \
    --shots 100000 \
    --with-observables \
    --out training.qssf

stabstream-analyze \
    --input training.qssf \
    --dem circuit.dem \
    --decoder union-find \
    --output report.json
```

## Threshold estimation

To estimate the threshold, run `stabstream-analyze` at multiple distances and
error rates, then use `stabstream-threshold compare`:

```bash
# Generate recordings at d=3, d=5 for p=0.003 and p=0.008
for d in 3 5; do
  stabstream-threshold run \
    --dem surface_d${d}.dem \
    --p-physical 0.003 --p-physical 0.008 \
    --shots 100000 --decoder union-find \
    --out threshold_d${d}.json
done

stabstream-threshold compare \
    --input threshold_d3.json --label "d=3" \
    --input threshold_d5.json --label "d=5" \
    --plot threshold.svg
```

## Next steps

- [Tutorial 3: Python Integration](03_python_integration.md)
- [Tutorial 4: Transport Modes](04_transport_modes.md)
- [Theory: Decoder Guide](../theory/decoder_guide.md)
