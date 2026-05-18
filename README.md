# stabstream

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-orange?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Platform-Cross--platform-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Status-Active-success?style=for-the-badge" />
  <img src="https://img.shields.io/badge/License-MIT%2FApache--2.0-green?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Performance-70K%2B%20frames%2Fs-purple?style=for-the-badge" />
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

Benchmark results on Windows 10 (x86-64, release build, Criterion 100-sample runs)
against a synthetic surface-code d=5 stream (`synthetic_surface_d5_stream`):

| Benchmark | Median latency | Throughput |
|---|---|---|
| Parse only (validation disabled) | 14.3 µs | ~70K frames/s |
| CRC validation | 14.1 µs | ~71K frames/s |
| Strict parity validation | 13.7 µs | ~73K frames/s |
| RLE popcount — 24 ancillas | 6.1 ns | 164M ops/s |

**Validation overhead is negligible.** Strict parity and disabled validation
land within noise of each other (~13.7 µs). CRC adds ~0.6 µs per frame.

**The ~14 µs floor is async I/O overhead, not parsing cost.** The 6 ns RLE
popcount shows the core decode logic is extremely fast; the `tokio::io::BufReader`
and `block_on` wrapping dominate at this frame size.

**~70K frames/s is well above current hardware syndrome rates.** Real
superconducting processors batch syndrome rounds at rates orders of magnitude
below this ceiling, so stabstream is not a bottleneck in the QEC pipeline.

## Format

See [`spec/QSSF_FORMAT.md`](spec/QSSF_FORMAT.md) for the full QEC Syndrome
Stream Format (QSSF) binary format specification.

## License

Apache-2.0 — see [LICENSE](LICENSE).
