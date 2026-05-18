# stabstream

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-orange?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Platform-Cross--platform-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Status-Active-success?style=for-the-badge" />
  <img src="https://img.shields.io/badge/License-MIT%2FApache--2.0-green?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Performance-1.5M%2B%20frames%2Fs-purple?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Safety-Memory%20Safe%20Rust-yellow?style=for-the-badge" />
  <img src="https://img.shields.io/badge/QEC-Ready-red?style=for-the-badge" />
</p>

A high-performance, hardware-agnostic QEC (quantum error correction) syndrome
stream deserializer and analysis runtime written in Rust, with C FFI and Python
bindings.

## Workspace Crates

| Crate | Description |
|-------|-------------|
| `stabstream-core` | `SyndromeFrame` types, `CodeType` enum, stabilizer models |
| `stabstream-deserialize` | Zero-copy QSSF binary parser and async pipeline |
| `stabstream-validate` | Parity checks, timing validation, bounds enforcement |
| `stabstream-replay` | Compressed stream logging (zstd) and playback |
| `stabstream-ffi` | C header generation (cbindgen) and Python bindings (PyO3) |
| `dashboard` | `ratatui` TUI for live syndrome monitoring |
| `benches` | Criterion benchmarks for parse throughput and validator overhead |

## Quick Start

```bash
# Build the workspace
cargo build --workspace

# Run the live dashboard (connects to a QSSF TCP source)
cargo run -p stabstream-dashboard -- --source tcp://localhost:9000

# Run benchmarks
cargo bench -p stabstream-benches
```

## Findings

Benchmark results on Linux x86-64, release build, Criterion 100-sample runs,
against a synthetic surface-code d=5 stream (`synthetic_surface_d5_stream`):

| Benchmark | Median latency | Throughput |
|---|---|---|
| Parse only (validation disabled) | 599.8 ns | ~1.67M frames/s |
| CRC validation | 669.7 ns | ~1.49M frames/s |
| Strict parity validation | 601.7 ns | ~1.66M frames/s |
| RLE popcount — 24 ancillas | 4.71 ns | ~212M ops/s |

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
iteration, which allocated (and freed) a 4 MiB `RingBuffer` each time.  On
Linux, glibc uses `mmap`/`munmap` for allocations above 128 KB, making each
4 MiB alloc ~170 µs.  The benchmarks now pass `ring_buf_bytes: 4096` (the
single-frame payload is ~135 bytes), eliminating the allocation noise and
surfacing the true parse cost.  The RLE popcount benchmark, which has no
allocation, was unaffected and produced consistent results across both runs.

## Format

See [`spec/QSSF_FORMAT.md`](spec/QSSF_FORMAT.md) for the full QEC Syndrome
Stream Format (QSSF) binary format specification.

## License

Apache-2.0 — see [LICENSE](LICENSE).
