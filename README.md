# stabstream

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

## Format

See [`spec/QSSF_FORMAT.md`](spec/QSSF_FORMAT.md) for the full QEC Syndrome
Stream Format (QSSF) binary format specification.

## License

Apache-2.0 — see [LICENSE](LICENSE).
