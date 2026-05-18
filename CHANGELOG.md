# Changelog

All notable changes to stabstream are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [v0.1.0] — 2026-05-18

### Core crates (pre-existing)

- **stabstream-core** — `SyndromeFrame`, `FrameHeader`, `SyndromePayload`, `CodeType`, `FileHeader`, `LogicalAnnotation`, `FrameMetadata`, `SchemaRegistry`, and the full `StabstreamError` enum. Six built-in hardware schemas included (surface d=3/5/7, honeycomb d=4, color d=5, repetition d=11).
- **stabstream-deserialize** — Zero-copy async QSSF parser (`QssfStream`), RLE encoder/decoder for detector events, ring-buffer implementation, and a synthetic stream generator for tests and benchmarks.
- **stabstream-validate** — `ValidationPolicy` enum (`StrictParity`, `CrcOnly`, `Disabled`), per-ancilla timing bounds checking, and stabilizer parity verification against hardware schemas.
- **stabstream-replay** — zstd-compressed QSSF stream recorder (`StreamRecorder`) and player (`StreamPlayer`).
- **stabstream-ffi** — C ABI (`stabstream_open`, `stabstream_next_frame`, `stabstream_close`, `stabstream_version`) and opt-in PyO3 Python bindings (feature `python`).
- **stabstream-dashboard** — ratatui TUI for live syndrome monitoring; supports TCP, file, and `stim:` sources.
- **stabstream-benches** — Criterion benchmarks for single-frame parse throughput and validator overhead.

### New: stabstream-decoder

- `Decoder` trait with `decode(&self, frame: &SyndromeFrame<'_>) -> DecoderResult`.
- `DecoderResult` containing `Vec<LogicalCorrection>` and a `confidence: f64` field.
- `LogicalCorrection` with `logical_id: u8` and `pauli: PauliOp`.
- `NullDecoder` reference implementation that returns empty corrections with confidence 1.0 — useful for benchmarking the parse pipeline without a real decoder backend.

### New: stabstream-convert

- `StimImporter` — reads Stim's `stim detect` 01-text output (one shot per line of `'0'`/`'1'` characters) and yields `OwnedFrame` values.
- `QssfExporter` — writes valid QSSF binary from a sequence of `SyndromeFrame` or `OwnedFrame` values, including file header, per-frame RLE payload, and frame terminator CRC.
- `export_owned_frame` helper to bridge `OwnedFrame` → `QssfExporter`.
- CLI binary `stabstream-convert --from stim --input events.dets --output out.qssf`.

### New: stabstream-py

- PyO3 Python extension module `stabstream` exposing `SyndromeFrame`, `StabstreamStream`, `CodeType`, `DecoderResult`, and `LogicalCorrection`.
- `pyproject.toml` with maturin ≥1.4 as build backend; `maturin develop` installs the module into the active virtualenv.
- Typed stubs `stabstream.pyi` covering all exported types and their docstrings.
- Example script `python/examples/parse_frames.py` demonstrating file and TCP iteration.

### New: stabstream-sim

- `serve_circuit_to_socket` — spawns a `stim detect` subprocess for a given `.stim` circuit, encodes detector-event output as QSSF frames, and streams them over a `TcpStream`.
- CLI binary `stabstream-sim --circuit circuit.stim --port 9000 --shots 10000` serves one client per connection.
- Reuses `circuit.stim` in the repo root as the default circuit.

### New: OpenTelemetry tracing (feature `otel`)

- Added optional `otel` feature to `stabstream-core` and `stabstream-deserialize` (off by default, no build-time cost when unused).
- `stabstream_core::otel::install(endpoint)` initialises an OTLP gRPC tracer and installs it as the global tracing subscriber; reads `OTEL_EXPORTER_OTLP_ENDPOINT` env var with `endpoint` as fallback.
- `stabstream_core::otel::shutdown()` flushes buffered spans on exit.
- `stabstream-deserialize` emits two tracing spans per frame: `qssf.frame_parse` (header + payload deserialization) and `qssf.frame_validate` (parity/timing checks). Spans are exported via OTLP when the `otel` feature is enabled and `install()` has been called.

### Infrastructure

- Workspace `Cargo.toml` bumped all member `description`, `license`, and `version` fields to be consistent.
- Added `clap 4` (derive feature) and the OpenTelemetry suite (`opentelemetry 0.22`, `opentelemetry_sdk 0.22`, `opentelemetry-otlp 0.15`, `tracing-opentelemetry 0.23`) to workspace dependencies.
- Added `.github/workflows/publish.yml` — runs `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` on every push to `main`.
