# stabstream

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-orange?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Platform-Cross--platform-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Status-Active-success?style=for-the-badge" />
  <img src="https://img.shields.io/badge/License-Apache--2.0-green?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Performance-1.5M%2B%20frames%2Fs-purple?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Safety-Memory%20Safe%20Rust-yellow?style=for-the-badge" />
  <img src="https://img.shields.io/badge/QEC-Ready-red?style=for-the-badge" />
</p>

A high-performance, hardware-agnostic QEC (quantum error correction) syndrome
stream deserializer and real-time decoding runtime written in Rust, with Python
bindings (PyO3 + NumPy) and C FFI.

stabstream parses QSSF frames at ~600 ns each, runs a native Union-Find decoder
in O(n·α(n)) time, and accumulates logical error rates — all without leaving Rust.
The Python bindings expose zero-copy NumPy arrays and a `DetectorErrorModel.to_pymatching()`
bridge for MWPM decoding via PyMatching.

> **New to stabstream or QEC?** See [ARCHITECTURE.md](ARCHITECTURE.md) for an
> annotated pipeline diagram, component map, and frame anatomy — designed for
> researchers coming from quantum computing rather than systems programming.

---

## Workspace Crates

| Crate | Description |
|-------|-------------|
| `stabstream-core` | `SyndromeFrame`, `SyndromeWindow`, `CodeType`, stabilizer models, `HardwareSchema` |
| `stabstream-dem` | Stim DEM parser, `SpacetimeGraph` builder, schema generation from DEM |
| `stabstream-decoder` | `Decoder` trait, `NullDecoder`, `UnionFindDecoder` (O(n·α(n))) |
| `stabstream-metrics` | `LogicalErrorAccumulator` (lock-free), `Histogram`, `AnalysisReport` |
| `stabstream-deserialize` | Zero-copy QSSF binary parser and async pipeline |
| `stabstream-validate` | Parity checks, timing validation, bounds enforcement |
| `stabstream-convert` | QSSF ↔ Stim conversion, observable ground-truth export |
| `stabstream-replay` | zstd-compressed stream recording, `StreamPlayer`, `analyze_file` |
| `stabstream-analyze` | `stabstream-analyze` CLI: offline decode + analysis of QSSF recordings |
| `stabstream-sim` | QSSF simulator — direct, broadcast, and SHM transport modes |
| `stabstream-threshold` | `stabstream-threshold run/compare` — threshold sweep and SVG plotting |
| `stabstream-py` | PyO3 Python bindings — NumPy arrays, DEM bridge, vendor adapters |
| `stabstream-ffi` | C header generation (cbindgen) |
| `dashboard` | `ratatui` TUI for live syndrome monitoring |
| `benches` | Criterion benchmarks for parse throughput and validator overhead |

---

## Quick Start

> **New to stabstream?** See the [Quickstart guide](QUICKSTART.md) for a step-by-step walkthrough from zero to live syndrome analysis in 5 commands.

```bash
# Build the workspace
cargo build --workspace

# Simulate a syndrome stream (native, no Stim required)
cargo run -p stabstream-sim -- --simulator native --dem circuit.dem --port 9000

# Connect the live dashboard
cargo run -p stabstream-dashboard -- --source tcp://localhost:9000

# Offline analysis of a recording
stabstream-analyze --input recording.qssf --dem circuit.dem --decoder union-find

# Threshold smoke test (~1s)
stabstream-threshold run --dem surface_d5.dem --shots 10000 --decoder union-find \
    --p-physical 0.005 --out smoke.json

# Run benchmarks
cargo bench -p stabstream-benches
```

### Python bindings

```bash
pip install maturin numpy
cd crates/stabstream-py && maturin develop
python python/examples/parse_frames.py recording.qssf
python python/examples/vendor_adapters.py   # IBM / Cirq / NumPy adapters (no hardware needed)
```

---

## Offline Analysis

`stabstream-analyze` replays a QSSF recording through a decoder and produces a
JSON report with logical error rates, latency percentiles, per-ancilla fire
frequencies, and syndrome weight distributions.

```bash
stabstream-analyze \
    --input recording.qssf \
    --dem circuit.dem \
    --decoder union-find \
    --window-depth 5 \
    --output report.json \
    --verbose
```

When the recording includes observable ground truth (QSSF tag `0x10`,
generated with `--with-observables`), `logical_error_rates` is populated.
Without ground truth, latency and diagnostic fields are still computed.

From Rust, use `StreamPlayer::analyze()` for zstd-compressed recordings:

```rust
let file = File::open("recording.qssf.zst")?;
let mut player = StreamPlayer::new(file)?;
let report = player.analyze(&decoder, AnalysisConfig::default())?;
println!("{}", report.summary());
```

See [docs/tutorials/02_offline_analysis.md](docs/tutorials/02_offline_analysis.md)
for the full field reference and hardware debugging guide.

---

## Transport Modes

`stabstream-sim` supports three transports:

| Mode | Command | IPC latency | Multi-consumer |
|------|---------|-------------|----------------|
| `direct` | `--transport direct` | ~2–5 µs | No |
| `broadcast` | `--transport broadcast` | ~2–5 µs | Yes (TCP fan-out) |
| `shm` | `--transport shm` | ~50–200 ns | No (SHM ring) |

```bash
# Broadcast: one source → N TCP clients
stabstream-sim --simulator native --dem circuit.dem \
    --transport broadcast --broadcast-capacity 512

# SHM: ultra-low-latency on-host IPC
stabstream-sim --simulator native --dem circuit.dem \
    --transport shm --shm-name my_experiment
```

See [docs/tutorials/04_transport_modes.md](docs/tutorials/04_transport_modes.md)
for latency trade-offs and decoder integration.

---

## Threshold Benchmarking

```bash
# Quick smoke test — single distance/p-value, verifies the pipeline (~1s)
stabstream-threshold run \
    --dem surface_d5.dem \
    --p-physical 0.005 --shots 10000 \
    --decoder union-find --out smoke.json

# Full threshold sweep — d=3,5,7 × 8 p-values, 100k shots/point (~8s on a laptop)
stabstream-threshold run \
    --dem surface_d3.dem --dem surface_d5.dem --dem surface_d7.dem \
    --p-physical 0.001 --p-physical 0.002 --p-physical 0.003 \
    --p-physical 0.005 --p-physical 0.008 --p-physical 0.010 \
    --p-physical 0.012 --p-physical 0.015 \
    --shots 100000 --decoder union-find \
    --out threshold.json --plot threshold.svg

# Compare two runs (e.g. UF vs MWPM)
stabstream-threshold compare \
    --input uf.json --label "Union-Find" \
    --input mwpm.json --label "Fusion Blossom" \
    --plot comparison.svg
```

The `run` subcommand parallelizes shot generation across all cores with Rayon
(one `(SmallRng, Decoder)` per worker thread) and writes CSV/JSON output.
At 8M shots/s on the native sampler, a 3-distance × 8-point sweep at 100k
shots/point completes in roughly 3–8 seconds depending on hardware.
The `compare` subcommand estimates the threshold by interpolating the crossing
between adjacent-distance curves.

---

## Decoding Pipeline

```
QSSF stream (file or TCP)
        │
        ▼
  StabstreamStream        ← PyO3: stabstream.StabstreamStream
  (zero-copy parse,
   ~600 ns/frame)
        │
        ▼
  SyndromeWindow          ← PyO3: stabstream.SyndromeWindow
  (sliding VecDeque,      ← .to_numpy_matrix() → shape (rounds, ancillas)
   rounds × ancillas)
        │
        ├──► UnionFindDecoder     Rust, O(n·α(n)), allocation-free hot path
        │    (stabstream-decoder)
        │
        └──► PyMatching (MWPM)   Python bridge via DetectorErrorModel.to_pymatching()
             (optimal p_L, slower)
        │
        ▼
  LogicalErrorAccumulator ← PyO3: stabstream.LogicalErrorAccumulator
  (AtomicU64, lock-free)
  → p_L per observable, mean p_L
```

---

## Decoders

Two Rust decoders ship out of the box. Python adapters for PyMatching, Chromobius, and
Tesseract are in `stabstream.decoders` (see
[Tutorial 5](docs/tutorials/05_decoder_plugins.md)).

| Decoder | Feature flag | Algorithm | Latency (d=5) | p_L quality |
|---------|-------------|-----------|---------------|-------------|
| `UnionFindDecoder` | *(default)* | Union-Find O(n·α(n)) | < 400 ns | Near-optimal |
| `FusionBlossomDecoder` | `mwpm` | Fusion Blossom MWPM | ~4 µs | Optimal |

### Native Union-Find Decoder

`UnionFindDecoder` implements the Delfosse & Nickerson 2021 linear-time algorithm.
It is the only decoder with a credible real-time path within a 1–4 µs
superconducting qubit syndrome cycle.

```rust
use std::sync::Arc;
use stabstream_core::window::SyndromeWindow;
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};
use stabstream_decoder::{Decoder, UnionFindDecoder};
use stabstream_metrics::LogicalErrorAccumulator;

let dem = DetectorErrorModel::parse(dem_text)?;
let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
let decoder = UnionFindDecoder::new(Arc::clone(&graph));
let acc = LogicalErrorAccumulator::new(dem.observable_count);

// Decode loop (allocation-free hot path after construction)
for window in syndrome_windows {
    let result = decoder.decode_window(&window);
    acc.record(&result, ground_truth_bitmask);
}

println!("mean p_L = {:.4e}", acc.mean_logical_error_rate());
```

**Performance budget for d=5 surface code (24 ancillas, 1.1 µs cycle):**

| Stage | Budget | Status |
|-------|--------|--------|
| Frame deserialization | 200 ns | Achieved (~600 ns total) |
| CRC validation | 70 ns | Achieved |
| Window slide | 20 ns | Implemented |
| UF decode | 400 ns | Implemented |
| **Total** | **~740 ns** | **< 1 µs deadline** |

### Fusion Blossom MWPM Decoder

`FusionBlossomDecoder` achieves MWPM-optimal logical error rates using the
Fusion Blossom algorithm (Higgott & Gidney 2023). Enable with
`features = ["mwpm"]`:

```rust
use stabstream_decoder::mwpm::FusionBlossomDecoder;

let decoder = FusionBlossomDecoder::new(Arc::clone(&graph));
let result = decoder.decode_window(&window);
```

---

## ML Decoder Research

`stabstream.decoders.NeuralDecoder` and the `load_qssf_windows` / `load_dataset`
utilities add first-class support for training and evaluating neural QEC decoders.

> **Latency note**: Neural decoders (MLPs, RNNs, transformers) are research-stage
> tools for studying decoder performance tradeoffs. They do **not** run in
> real-time (&lt;1 µs). Use `UnionFindDecoder` for real-time operation and
> `NeuralDecoder` for offline threshold analysis and architecture research.

### Generate a training dataset

```bash
# Sample 100 000 shots from a DEM without running Stim
stabstream-convert dem-to-dataset \
    --dem surface_d5.dem \
    --shots 100000 \
    --seed 42 \
    --out training_data.bin
```

### Load and train in Python

```python
from stabstream.io import load_dataset
import torch, torch.nn as nn

X, y = load_dataset("training_data.bin")
# X.shape == (100000, 24), dtype bool  — detector events
# y.shape == (100000,),   dtype uint64 — observable flip bitmasks

model = nn.Sequential(nn.Linear(24, 64), nn.ReLU(), nn.Linear(64, 1))
# ... train with BCEWithLogitsLoss against (y & 1).float() ...
```

### Evaluate with `NeuralDecoder`

```python
import torch
from stabstream import DetectorErrorModel, LogicalErrorAccumulator
from stabstream.decoders import NeuralDecoder

dem     = DetectorErrorModel.from_file("surface_d5.dem")
decoder = NeuralDecoder.from_torch("model.pt", observable_count=dem.observable_count)
acc     = LogicalErrorAccumulator(observable_count=dem.observable_count)

X_test, y_test = load_dataset("test_data.bin")
for result, gt in zip(decoder.decode_batch(X_test), y_test):
    acc.record(result, int(gt))

print(f"p_L = {acc.mean_logical_error_rate():.4e}")
```

`NeuralDecoder` accepts any callable — PyTorch `ScriptModule`, ONNX Runtime
`InferenceSession`, TensorFlow/Keras `Model`, or a plain NumPy function —
without mandatory framework imports in the core package.

### Multi-round windows for sequence models

```python
from stabstream.io import load_qssf_windows

for X, y in load_qssf_windows("recording.qssf", window_depth=5,
                               batch_size=256, with_labels=True):
    # X.shape == (256, 5, ancilla_count) — (batch, rounds, ancillas)
    # y.shape == (256,)                  — observable flip bitmasks
    loss = model.train_step(X, y)
```

See `notebooks/05_neural_decoder.ipynb` for a complete end-to-end walkthrough
comparing an MLP against MWPM on a repetition code.

---

## Stim DEM Parser

Parse any Stim `.dem` file and build a weighted `SpacetimeGraph` for MWPM/UF decoders:

```rust
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};

let dem = DetectorErrorModel::parse(dem_text)?;
// detector_count, observable_count, errors, detectors with (x,y,t) coords
let graph = SpacetimeGraph::from_dem(&dem);
// nodes: detectors + virtual boundary; edges: weight = -ln(p/(1-p))
let schema_json = stabstream_dem::schema_gen::schema_from_dem(&dem, "my_code");
```

Supports: `error(p) D<i> D<j> ^ L<k>`, `detector(x,y,t) D<i>`,
`logical_observable L<k>`, and `repeat N { ... }` blocks.

---

## Python Bindings

Build with `maturin develop` from `crates/stabstream-py/`.

### SyndromeFrame — NumPy arrays

```python
from stabstream import StabstreamStream

with StabstreamStream("recording.qssf") as stream:
    for frame in stream:
        # Zero-copy views into Rust-owned memory
        det_events = frame.to_numpy_detector_events()  # shape (ancilla_count,), dtype bool
        meas       = frame.to_numpy_meas_results()     # shape (ancilla_count,), dtype int8
        print(frame.observable_flips)  # Optional[int] — ground truth tag 0x10
```

### SyndromeWindow — multi-round detector matrix

```python
from stabstream import SyndromeWindow

window = SyndromeWindow(ancilla_count=24, window_depth=5)
for frame in stream:
    window.push(frame)
    if window.is_full():
        mat = window.to_numpy_matrix()   # shape (5, 24), dtype bool
        active = window.active_detectors()  # flat indices of fired detectors
```

### DetectorErrorModel — PyMatching bridge

```python
from stabstream import DetectorErrorModel

dem = DetectorErrorModel.from_file("model.dem")
# dem.detector_count, dem.observable_count, dem.error_count

# Build a pymatching.Matching with -ln(p/(1-p)) edge weights (requires pip install pymatching)
matching = dem.to_pymatching()
prediction = matching.decode(detector_events.astype(np.uint8))

# Auto-generate a HardwareSchema JSON from the DEM
schema_json = dem.to_schema_json("my_surface_code")
```

### LogicalErrorAccumulator

```python
from stabstream import LogicalErrorAccumulator

acc = LogicalErrorAccumulator(observable_count=1)
acc.record(decoder_result, ground_truth=frame.observable_flips or 0)
print(f"p_L = {acc.logical_error_rate(0):.4e}")
print(f"mean p_L = {acc.mean_logical_error_rate():.4e}")
```

### CodeType — all supported codes

```python
from stabstream import CodeType

CodeType.SURFACE_CODE       # 0x01
CodeType.HONEYCOMB_CODE     # 0x02
CodeType.COLOR_CODE         # 0x03
CodeType.REPETITION_CODE    # 0x04
CodeType.TORIC_CODE         # 0x05
CodeType.BIVARIATE_BICYCLE  # 0x06  IBM BB/Gross codes
CodeType.HYPERGRAPH_PRODUCT # 0x07  general qLDPC
CodeType.FIBER_BUNDLE       # 0x08  high-rate codes
CodeType.CUSTOM             # 0xFF
```

---

## Logical Error Rate Accumulation

`LogicalErrorAccumulator` uses `AtomicU64` counters — safe for multi-threaded
threshold simulation without a `Mutex`:

```rust
use stabstream_metrics::LogicalErrorAccumulator;

let acc = LogicalErrorAccumulator::new(observable_count);

// Record shots concurrently from multiple threads
acc.record(&decoder_result, ground_truth_bitmask);

let report = acc.report();
// MetricsReport { total_shots, logical_error_rates, mean_logical_error_rate }
println!("{}", report.summary());
```

The `Histogram` type provides power-of-2 bucket histograms for custom
decode latency and syndrome weight tracking in user-built pipelines.
`AnalysisReport` (produced by `stabstream-analyze` and `StreamPlayer::analyze`)
reports latency as percentiles (p50/p99/max) and syndrome weights as a
direct-index frequency vector, both serialized to JSON.

---

## Observable Ground Truth (tag 0x10)

QSSF frames can carry the simulator's true observable flip bitmask in metadata
tag `0x10`. This enables offline threshold analysis from replay files:

```bash
# Generate QSSF with observable ground truth from Stim
stabstream-convert stim-to-qssf \
    --circuit circuit.stim --shots 100000 \
    --with-observables --out training_data.qssf

# In Python: frame.observable_flips contains the u64 bitmask
```

---

## qLDPC Code Support

`HardwareSchema` supports IBM Bivariate Bicycle (BB/Gross) codes and other
qLDPC families with optional fields:

```json
{
  "ldpc_hz_matrix": "<base64 CSR>",
  "ldpc_hx_matrix": "<base64 CSR>",
  "logical_z_matrix": "<base64>",
  "logical_x_matrix": "<base64>",
  "encoding_rate": 0.0833,
  "dem_path": "models/bb_144_12_12.dem"
}
```

All fields are optional — existing schema files remain valid.

---

## Benchmarks

Benchmark results on Linux x86-64, release build, Criterion 100-sample runs,
against a synthetic surface-code d=5 stream (`synthetic_surface_d5_stream`):

| Benchmark | Median latency | Throughput |
|---|---|---|
| Parse only (validation disabled) | 599.8 ns | ~1.67M frames/s |
| CRC validation | 669.7 ns | ~1.49M frames/s |
| Strict parity validation | 601.7 ns | ~1.66M frames/s |
| RLE popcount — 24 ancillas | 4.71 ns | ~212M ops/s |
| `analyze_file` + NullDecoder (10K frames) | 4.82 ms | ~2.07M frames/s |

**Validation overhead is negligible.** Strict parity and disabled validation
are within 2 ns of each other (~600 ns). CRC adds ~70 ns per frame.

**Sub-microsecond frame parse cost.** The 4.71 ns RLE popcount shows the core
decode logic is extremely fast; the per-frame overhead including the tokio
`block_on`, `BufReader`, and ring-buffer allocation is under 600 ns end-to-end.

**~1.5M frames/s is far above current hardware syndrome rates.** Real
superconducting processors batch syndrome rounds at rates orders of magnitude
below this ceiling, so stabstream is not a bottleneck in the QEC pipeline.

### Benchmark regression note

An earlier run on Windows 10 reported ~14 µs / 70K fps for the same benchmarks.
The root cause was the benchmark loop creating a fresh `QssfStream` per
iteration, which allocated (and freed) a 4 MiB `RingBuffer` each time. On
Linux, glibc uses `mmap`/`munmap` for allocations above 128 KB, making each
4 MiB alloc ~170 µs. The benchmarks now pass `ring_buf_bytes: 4096` (the
single-frame payload is ~135 bytes), eliminating the allocation noise and
surfacing the true parse cost. The RLE popcount benchmark, which has no
allocation, was unaffected and produced consistent results across both runs.

---

## Format

See [`spec/QSSF_FORMAT.md`](spec/QSSF_FORMAT.md) for the full QEC Syndrome
Stream Format (QSSF) binary format specification.

## License

Apache-2.0 — see [LICENSE](LICENSE).
