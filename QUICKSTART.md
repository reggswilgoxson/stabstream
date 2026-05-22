# Quickstart: stabstream in 5 Commands

This guide gets you from zero to a live syndrome stream analysis with no prior
files required. Total time: ~5 minutes.

## Prerequisites

- [Rust](https://rustup.rs) (1.75+)
- Python 3.9+ with pip

## 1. Get Stim

Stim generates the quantum error correction circuits and noise models that
stabstream consumes.

```bash
pip install stim
```

## 2. Generate a surface code circuit and noise model

```bash
stim gen --code surface_code --task memory_x --distance 5 --rounds 5 > surface_d5.stim
stim analyze_errors --decompose_errors < surface_d5.stim > surface_d5.dem
```

This creates a d=5 surface code (24 ancilla qubits, 5 syndrome rounds) with a
circuit-level depolarizing noise model. The `.dem` file describes which physical
errors trigger which detectors — stabstream uses it to weight the decoder graph.

## 3. Clone and build stabstream

```bash
git clone https://github.com/reggswilgoxson/stabstream
cd stabstream
cargo build --release -p stabstream-sim -p stabstream-analyze
```

## 4. Start the simulator (terminal 1)

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

## 5. Analyze the live stream (terminal 2)

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

---

## What to try next

| Goal | Command / file |
|------|---------------|
| Larger simulation | Increase `--shots` or switch to `--distance 7` |
| Record to disk for later | Add `--record recording.qssf` to the sim command |
| Replay a recording | `stabstream-analyze --input recording.qssf --dem surface_d5.dem` |
| Run performance benchmarks | `cargo bench` (requires [gnuplot](http://gnuplot.info)) |
| Threshold sweep | `cargo run -p stabstream-threshold --release -- run --dem surface_d5.dem --shots 50000` |
| Python API | See [docs/tutorials/03_python_integration.md](docs/tutorials/03_python_integration.md) |
| Live dashboard | `cargo run -p stabstream-dashboard --release -- --source tcp://localhost:9000` |

## Going deeper

- [Tutorial 1: Parse your first QSSF stream (Rust)](docs/tutorials/01_hello_syndrome.md)
- [Tutorial 2: Offline analysis](docs/tutorials/02_offline_analysis.md)
- [Tutorial 3: Python integration](docs/tutorials/03_python_integration.md)
- [Theory: QEC primer](docs/theory/qec_primer.md)
- [QSSF format spec](spec/QSSF_FORMAT.md)
