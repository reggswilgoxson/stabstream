# stabstream

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-orange?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Platform-Cross--platform-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Status-Active-success?style=for-the-badge" />
  <img src="https://img.shields.io/badge/License-Apache--2.0-green?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Performance-1.5M%2B%20frames%2Fs-purple?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Safety-Memory%20Safe%20Rust-yellow?style=for-the-badge" />
  <img src="https://img.shields.io/badge/QEC-Ready-red?style=for-the-badge" />
  <img src="https://img.shields.io/pypi/v/stabstream?style=for-the-badge&logo=pypi&logoColor=white&color=3775A9" />
</p>

A high-performance, hardware-agnostic QEC (quantum error correction) syndrome
stream deserializer and real-time decoding runtime written in Rust, with Python
bindings (PyO3 + NumPy) and C FFI.

stabstream parses QSSF frames at ~600 ns each, runs a native Union-Find decoder
in O(n·α(n)) time, and accumulates logical error rates — all without leaving Rust.
The Python bindings expose a zero-config `from_stim_circuit` entry point,
zero-copy NumPy arrays, and a `DetectorErrorModel.to_pymatching()` bridge for
MWPM decoding via PyMatching.

> **New to stabstream or QEC?** See [ARCHITECTURE.md](ARCHITECTURE.md) for an
> annotated pipeline diagram, component map, and frame anatomy — designed for
> researchers coming from quantum computing rather than systems programming.

## Mission

Quantum computers make mistakes — a lot of them. To run useful computations, they need a companion system that watches for errors and issues corrections fast enough that the errors don't pile up. That window is measured in **microseconds**: too slow, and the quantum state is already gone.

stabstream is the software that lives in that window. It receives a stream of error signals from quantum hardware, figures out what went wrong using a built-in decoder, and hands back a correction — all in under a millionth of a second. It is designed to work with any quantum processor, speak directly to the chips and FPGAs that sit closest to the hardware, and scale from a laptop experiment to a production control system without changing a line of research code.

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
pip install stabstream        # pre-built wheels for Linux / macOS / Windows
```

Or build from source (requires a Rust toolchain):

```bash
pip install maturin
cd crates/stabstream-py && maturin develop
```

---

## Python Bindings

### Zero-config entry point

If you have a [Stim](https://github.com/quantumlib/Stim) circuit, stabstream
configures the Union-Find decoder for you automatically:

```python
import stim
import stabstream

circuit = stim.Circuit.from_file("surface_d5.stim")

with stabstream.from_stim_circuit("recording.qssf", circuit) as stream:
    for frame in stream:
        correction = frame.observable_flips   # int — decoded correction bitmask
        apply_correction(correction)
```

No DEM files, no window-depth tuning, no decoder setup.

### With an explicit DEM

If you have a `.dem` file instead of a circuit:

```python
with stabstream.open("recording.qssf", decoder="surface_d5.dem") as stream:
    for frame in stream:
        print(frame.observable_flips)
```

`set_decoder` also accepts an inline DEM text string or a
`stim.DetectorErrorModel` object — whichever you already have.

### Raw frame access (no decoder)

```python
with stabstream.open("recording.qssf") as stream:
    for frame in stream:
        events = frame.to_numpy_detector_events()  # np.ndarray[bool], shape (ancilla_count,)
        meas   = frame.to_numpy_meas_results()     # np.ndarray[int8], shape (ancilla_count,)
        # frame.observable_flips raises StabstreamError here — no decoder configured
```

---

## Async

### When to use async

**Most research workflows don't need async.** The sync API (`with` / `for`) is
simpler and runs at full speed for file replay, threshold sweeps, and single
live-hardware sources.

Use `async for` when you genuinely need to interleave syndrome decoding with
other async work — for example, feeding corrections into an async control loop
or reading from two hardware sources concurrently:

```python
import asyncio
import stabstream

async def main():
    circuit = ...  # stim.Circuit

    # Two independent hardware chips, processed concurrently
    async def drain(source):
        async with stabstream.from_stim_circuit(source, circuit) as stream:
            async for frame in stream:
                await apply_correction(frame.observable_flips)

    await asyncio.gather(
        drain("tcp://chip-a:9000"),
        drain("tcp://chip-b:9000"),
    )

asyncio.run(main())
```

### Async foot guns

**1. Don't share a stream between concurrent tasks.**
One `async for` loop per stream. The stream is not thread-safe; concurrent
`__anext__` calls on the same object will corrupt its internal state.

```python
# WRONG — two tasks racing on the same stream
stream = stabstream.open("tcp://fpga:9000")
await asyncio.gather(task_a(stream), task_b(stream))  # data corruption

# RIGHT — one stream per source
await asyncio.gather(task_a("tcp://fpga:9000"), task_b("tcp://fpga:9001"))
```

**2. Jupyter notebooks already have a running event loop — `asyncio.run()` will fail.**

```python
# WRONG in Jupyter
asyncio.run(main())   # RuntimeError: This event loop is already running

# RIGHT in Jupyter — await directly in a cell
async with stabstream.from_stim_circuit("data.qssf", circuit) as stream:
    async for frame in stream:
        print(frame.observable_flips)
```

**3. Always use `async with`, not a bare `async for`.**
Without `async with`, the stream is never explicitly closed. On TCP sources
this leaves the socket open until garbage collection.

```python
# WRONG — resource leak on TCP
async for frame in stabstream.open("tcp://fpga:9000"):
    ...

# RIGHT
async with stabstream.open("tcp://fpga:9000") as stream:
    async for frame in stream:
        ...
```

**4. Async doesn't mean parallel — Python's GIL still applies.**
`async for` keeps the event loop responsive (other coroutines can run while
waiting for the next frame), but two `async for` loops on two streams will
still take turns, not run simultaneously. For true CPU parallelism across
multiple streams use `multiprocessing`.

**5. `sync` and `async` interfaces are the same object — don't mix them.**

```python
# WRONG — mixing sync next() with async loop on the same object
stream = stabstream.open("tcp://fpga:9000")
frame0 = next(stream)           # advances the sync cursor
async for frame in stream:      # picks up from frame1 — surprising but not fatal
    ...                         # close() is never called — resource leak

# RIGHT — pick one interface and use a context manager
with stabstream.open("tcp://fpga:9000") as stream:       # sync
    for frame in stream: ...

async with stabstream.open("tcp://fpga:9000") as stream: # async
    async for frame in stream: ...
```

---

## SyndromeWindow — multi-round detector matrix

```python
from stabstream import SyndromeWindow

window = SyndromeWindow(ancilla_count=24, window_depth=5)
for frame in stream:
    window.push(frame)
    if window.is_full():
        mat    = window.to_numpy_matrix()    # shape (5, 24), dtype bool
        active = window.active_detectors()   # flat indices of fired detectors
```

---

## DetectorErrorModel — PyMatching bridge

```python
from stabstream import DetectorErrorModel

dem = DetectorErrorModel.from_file("model.dem")
# dem.detector_count, dem.observable_count, dem.error_count

# Build a pymatching.Matching with -ln(p/(1-p)) edge weights
matching = dem.to_pymatching()   # requires pip install pymatching
prediction = matching.decode(detector_events.astype(np.uint8))

# Auto-generate a HardwareSchema JSON from the DEM
schema_json = dem.to_schema_json("my_surface_code")
```

---

## LogicalErrorAccumulator

```python
from stabstream import LogicalErrorAccumulator

acc = LogicalErrorAccumulator(observable_count=1)
acc.record(decoder_result, ground_truth=frame.observable_flips)
print(f"p_L = {acc.logical_error_rate(0):.4e}")
print(f"mean p_L = {acc.mean_logical_error_rate():.4e}")
```

---

## CodeType — all supported codes

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

> **Note:** The SHM transport is intended for C/FPGA producers writing frames
> directly into shared memory via `stabstream_shm_open` / `stabstream_shm_write`
> (C FFI). The Python bindings read from files and TCP only; `shm://` URIs
> raise a clear error pointing to the C API.

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

---

## Decoding Pipeline

```
QSSF stream (file or TCP)
        │
        ▼
  stabstream.open() / from_stim_circuit()    ← Python entry points
  (zero-copy parse, ~600 ns/frame)
        │
        ▼
  SyndromeWindow                             ← sliding rounds × ancillas matrix
        │
        ├──► UnionFindDecoder     Rust, O(n·α(n)), allocation-free hot path
        │    (built-in, default)
        │
        └──► PyMatching (MWPM)   Python bridge via DetectorErrorModel.to_pymatching()
             (optimal p_L, slower)
        │
        ▼
  LogicalErrorAccumulator                    ← AtomicU64, lock-free
  → p_L per observable, mean p_L
```

---

## Decoders

Two Rust decoders ship out of the box. Python adapters for PyMatching,
Chromobius, and Tesseract are in `stabstream.decoders` (see
[Tutorial 5](docs/tutorials/05_decoder_plugins.md)).

| Decoder | Feature flag | Algorithm | Latency (d=5) | p_L quality |
|---------|-------------|-----------|---------------|-------------|
| `UnionFindDecoder` | *(default)* | Union-Find O(n·α(n)) | < 400 ns | Near-optimal |
| `FusionBlossomDecoder` | `mwpm` | Fusion Blossom MWPM | ~4 µs | Optimal |

### Native Union-Find Decoder

`UnionFindDecoder` implements the Delfosse & Nickerson 2021 linear-time
algorithm. It is the only decoder with a credible real-time path within a
1–4 µs superconducting qubit syndrome cycle.

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

### Multi-round windows for sequence models

```python
from stabstream.io import load_qssf_windows

for X, y in load_qssf_windows("recording.qssf", window_depth=5,
                               batch_size=256, with_labels=True):
    # X.shape == (256, 5, ancilla_count) — (batch, rounds, ancillas)
    # y.shape == (256,)                  — observable flip bitmasks
    loss = model.train_step(X, y)
```

---

## Stim DEM Parser

Parse any Stim `.dem` file and build a weighted `SpacetimeGraph` for MWPM/UF decoders:

```rust
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};

let dem = DetectorErrorModel::parse(dem_text)?;
let graph = SpacetimeGraph::from_dem(&dem);
let schema_json = stabstream_dem::schema_gen::schema_from_dem(&dem, "my_code");
```

Supports: `error(p) D<i> D<j> ^ L<k>`, `detector(x,y,t) D<i>`,
`logical_observable L<k>`, and `repeat N { ... }` blocks.

---

## Logical Error Rate Accumulation

`LogicalErrorAccumulator` uses `AtomicU64` counters — safe for multi-threaded
threshold simulation without a `Mutex`:

```rust
use stabstream_metrics::LogicalErrorAccumulator;

let acc = LogicalErrorAccumulator::new(observable_count);

acc.record(&decoder_result, ground_truth_bitmask);

let report = acc.report();
println!("{}", report.summary());
```

---

## Observable Ground Truth (tag 0x10)

QSSF frames can carry the simulator's true observable flip bitmask in metadata
tag `0x10`. This enables offline threshold analysis from replay files:

```bash
stabstream-convert stim-to-qssf \
    --circuit circuit.stim --shots 100000 \
    --with-observables --out training_data.qssf
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

---

## Benchmarks

Benchmark results on Linux x86-64, release build, Criterion 100-sample runs:

| Benchmark | Median latency | Throughput |
|---|---|---|
| Parse only (validation disabled) | 599.8 ns | ~1.67M frames/s |
| CRC validation | 669.7 ns | ~1.49M frames/s |
| Strict parity validation | 601.7 ns | ~1.66M frames/s |
| RLE popcount — 24 ancillas | 4.71 ns | ~212M ops/s |
| `analyze_file` + NullDecoder (10K frames) | 4.82 ms | ~2.07M frames/s |

---

## Format

See [`spec/QSSF_FORMAT.md`](spec/QSSF_FORMAT.md) for the full QEC Syndrome
Stream Format (QSSF) binary format specification.

## License

Apache-2.0 — see [LICENSE](LICENSE).
