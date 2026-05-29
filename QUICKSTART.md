# Quickstart: stabstream

stabstream receives a stream of error signals from a quantum processor,
decodes what went wrong, and returns a correction — all in under a
microsecond. It ships as a Python package (pre-built wheels, **no Rust
required**) and as a high-performance Rust CLI for production deployments.

**Pick your path:**

| I want to… | Start here |
|---|---|
| Explore syndrome data in a notebook | [Python quickstart](#python-quickstart-colab--jupyter) ← *start here* |
| Run pre-built notebooks | [Jupyter notebooks](#jupyter-notebooks) |
| Real-time decoding at sub-µs latency | [Rust CLI](#rust-cli-advanced--production) |

---

## Python quickstart (Colab / Jupyter)

No Rust. No compilation. Each block below is one notebook cell.

### 1. Install

```python
# In Colab or Jupyter:
!pip install stabstream stim numpy matplotlib pandas

# In a terminal:
# pip install stabstream stim numpy matplotlib pandas
```

### 2. Generate a surface code circuit

A quantum error correcting code is run by repeatedly measuring *stabilizers*
— special multi-qubit parity checks. Stim lets you describe one in a single
call. `distance=5` gives a d=5 surface code (24 ancilla qubits). `rounds=5`
means five rounds of stabilizer measurements. `after_clifford_depolarization`
sets the physical error rate to 0.1%.

```python
import stim

circuit = stim.Circuit.generated(
    "surface_code:memory_x",
    distance=5,
    rounds=5,
    after_clifford_depolarization=0.001,
)
print(f"{circuit.num_qubits} qubits, {circuit.num_detectors} detectors/shot")
```

### 3. Sample syndrome data

A *detector* fires when a stabilizer measurement disagrees with the outcome
predicted by a noiseless run. The pattern of fired detectors in a given shot
is the *syndrome* — the raw signal that the decoder must interpret. Stim
samples this at ~1M shots/s on a laptop.

```python
import numpy as np

sampler = circuit.compile_detector_sampler()
shots = sampler.sample(shots=2000)   # shape (2000, n_detectors), dtype bool

print(f"Shape: {shots.shape}")
print(f"Mean fire rate: {shots.mean():.4f} events per detector per shot")
```

### 4. Write to QSSF and load with stabstream

QSSF is stabstream's binary wire format — the same format used by real
quantum hardware adapters and the Rust simulator. The Python writer converts
NumPy arrays to QSSF so every stabstream loader can read them.

```python
from stabstream._qssf_write import write_qssf
from stabstream.io import load_qssf_batch

def _frames(samples):
    n = samples.shape[1]
    for i, row in enumerate(samples):
        yield {
            "frame_id": i,
            "round": i % 5,
            "ancilla_count": n,
            "detector_events": row.tolist(),
            "observable_flips": None,
        }

write_qssf("synthetic.qssf", _frames(shots))
print("Wrote synthetic.qssf")

# Load as (batch_size, ancilla_count) bool arrays — ready for ML or analysis
for batch in load_qssf_batch("synthetic.qssf", batch_size=256):
    print(f"Batch shape: {batch.shape}  dtype: {batch.dtype}")
    break  # just peek at the first batch
```

### 5. Decode with the built-in Union-Find decoder

*Decoding* means taking the syndrome pattern and inferring which physical
errors most likely occurred so a correction can be applied. stabstream's
built-in Union-Find decoder does this in ~400 ns per frame. Pass it a
*Detector Error Model* (DEM) that describes how errors propagate in your
circuit — Stim generates one automatically.

```python
import stabstream

dem = circuit.detector_error_model(decompose_errors=True)

with stabstream.open("synthetic.qssf", decoder=dem) as stream:
    for i, frame in enumerate(stream):
        if i < 3:
            print(
                f"frame {frame.frame_id}: "
                f"events={frame.detector_event_count}  "
                f"observable_flips={frame.observable_flips}"
            )
```

`observable_flips` is a bitmask: bit `i` is set when the decoder concludes
that logical observable `i` has been flipped by errors. A non-zero value
means a logical error occurred (or the decoder got it wrong).

### 6. Visualise detector events (optional)

```python
import matplotlib.pyplot as plt
from stabstream.io import load_qssf_batch

matrix = np.vstack(list(load_qssf_batch("synthetic.qssf", batch_size=10_000)))
# matrix.shape == (shots, ancillas)

fig, ax = plt.subplots(figsize=(12, 4))
ax.imshow(matrix[:100].T, aspect="auto", cmap="Blues", interpolation="nearest")
ax.set_xlabel("Shot")
ax.set_ylabel("Ancilla (detector index)")
ax.set_title("Detector events — first 100 shots, d=5 surface code, p=0.1%")
plt.tight_layout()
plt.show()

print(f"Global fire rate : {matrix.mean():.4f}")
print(f"Mean syndrome weight: {matrix.sum(axis=1).mean():.2f} / {matrix.shape[1]} ancillas")
```

---

## Jupyter notebooks

Pre-built notebooks live in [`notebooks/`](notebooks/). Each one installs
stabstream in its first cell, so they work in Colab or locally with no extra
setup.

| Notebook | What you learn |
|---|---|
| [`01_syndrome_exploration.ipynb`](notebooks/01_syndrome_exploration.ipynb) | Heatmaps, fire-rate distributions, temporal autocorrelation |
| [`02_threshold_sweep.ipynb`](notebooks/02_threshold_sweep.ipynb) | Logical error rate vs physical error rate — where the code threshold is |
| [`03_decoder_comparison.ipynb`](notebooks/03_decoder_comparison.ipynb) | Union-Find vs PyMatching MWPM vs neural decoder |
| [`04_hardware_debugging.ipynb`](notebooks/04_hardware_debugging.ipynb) | Spotting drifting qubits, leakage, and systematic errors in real data |
| [`05_neural_decoder.ipynb`](notebooks/05_neural_decoder.ipynb) | Training a neural network decoder from QSSF recordings |

To run in Colab: open the notebook from GitHub (File → Open notebook → GitHub),
then run all cells. The first cell does `!pip install stabstream`.

---

## Key Python API

| Class / function | What it does |
|---|---|
| `stabstream.open(path, decoder=dem)` | Open a QSSF stream — sync (`for`) or async (`async for`) |
| `stabstream.from_stim_circuit(path, circuit)` | Open a stream with decoder auto-configured from a Stim circuit |
| `stabstream.from_stim_dem(path, dem)` | Open a stream with decoder configured from a Stim DEM object |
| `load_qssf(path)` | Generator of `SyndromeFrame` objects |
| `load_qssf_batch(path, batch_size)` | Yield `(n, ancillas)` bool arrays — ML-ready |
| `load_qssf_windows(path, window_depth)` | Yield `(n, depth, ancillas)` windows for recurrent models |
| `write_qssf(path, frames_iter)` | Write frame dicts to QSSF from Python |
| `SyndromeWindow` | Sliding multi-round detector matrix |
| `LogicalErrorAccumulator` | Lock-free p_L accumulator for threshold sweeps |

Full reference: [`docs/tutorials/03_python_integration.md`](docs/tutorials/03_python_integration.md)

---

## Next steps

| Goal | Where to go |
|---|---|
| Interactive syndrome exploration | [`notebooks/01_syndrome_exploration.ipynb`](notebooks/01_syndrome_exploration.ipynb) |
| Threshold curves (p_L vs p_phys) | [`notebooks/02_threshold_sweep.ipynb`](notebooks/02_threshold_sweep.ipynb) |
| Use PyMatching (MWPM decoder) | [`docs/tutorials/05_decoder_plugins.md`](docs/tutorials/05_decoder_plugins.md) |
| Train a neural decoder | [`notebooks/05_neural_decoder.ipynb`](notebooks/05_neural_decoder.ipynb) |
| IBM Qiskit / Google Cirq hardware | [`docs/tutorials/07_hardware_integration.md`](docs/tutorials/07_hardware_integration.md) |
| Full Python API walkthrough | [`docs/tutorials/03_python_integration.md`](docs/tutorials/03_python_integration.md) |
| QEC background (no prior knowledge needed) | [`docs/theory/qec_primer.md`](docs/theory/qec_primer.md) |

---

## Rust CLI (advanced / production)

The Rust CLI is for when you need sub-microsecond real-time decoding of a
live hardware stream, or want to run large threshold sweeps as fast as
possible. It requires [Rust 1.75+](https://rustup.rs) and compiles from
source. Python users can skip this entirely.

### Prerequisites

- [Rust](https://rustup.rs) (1.75+)
- Python 3.9+ with pip

### 1. Get Stim

```bash
pip install stim
```

### 2. Generate a surface code circuit and noise model

```bash
stim gen --code surface_code --task memory_x --distance 5 --rounds 5 > surface_d5.stim
stim analyze_errors --decompose_errors < surface_d5.stim > surface_d5.dem
```

This creates a d=5 surface code (24 ancilla qubits, 5 syndrome rounds) with a
circuit-level depolarizing noise model. The `.dem` file describes which physical
errors trigger which detectors — stabstream uses it to weight the decoder graph.

### 3. Clone and build stabstream

```bash
git clone https://github.com/reggswilgoxson/stabstream
cd stabstream
cargo build --release -p stabstream-sim -p stabstream-analyze
```

### 4. Start the simulator (terminal 1)

```bash
cargo run -p stabstream-sim --release -- \
  --simulator native \
  --dem surface_d5.dem \
  --shots 10000 \
  --port 9000
```

This samples 10,000 syndrome shots from the noise model and serves them as a
QSSF stream over TCP. No Stim subprocess is involved — sampling runs natively
in Rust at ~2M frames/s.

### 5. Analyze the live stream (terminal 2)

```bash
cargo run -p stabstream-analyze --release -- \
  --addr 127.0.0.1:9000 \
  --dem surface_d5.dem \
  --decoder union-find
```

When complete you will see a report like:

```
frames_processed : 10000
total_shots      : 9996
mean_decode_ns   : 312
p50_decode_ns    : 298
p99_decode_ns    : 841
p_L (obs 0)      : 3.12e-3
mean_p_L         : 3.12e-3
```

`p_L` is the logical error rate — the fraction of syndrome windows where the
decoder returned the wrong observable flip. At d=5 and physical error rate ~0.1%
you should see `p_L` in the low 10⁻³ range.

### More Rust CLI options

| Goal | Command |
|---|---|
| Larger simulation | Increase `--shots` or use `--distance 7` |
| Record to disk for later | Add `--record recording.qssf` to the sim command |
| Replay a recording | `stabstream-analyze --input recording.qssf --dem surface_d5.dem` |
| Performance benchmarks | `cargo bench` (requires [gnuplot](http://gnuplot.info)) |
| Threshold sweep | `cargo run -p stabstream-threshold --release -- run --dem surface_d5.dem --shots 50000` |
| Live TUI dashboard | `cargo run -p stabstream-dashboard --release -- --source tcp://localhost:9000` |
| Architecture overview | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
