# stabstream Architecture

> A visual guide for quantum researchers new to the codebase.

stabstream is a real-time **quantum error correction (QEC) pipeline**. It takes raw
ancilla measurement results from a quantum device (or simulator), runs a decoder to
infer the most likely error pattern, and tracks how often the decoder fails — giving
you the **logical error rate** p_L.

---

## The Big Picture

Every QEC experiment follows the same loop:

1. Run stabilizer measurements → get **syndromes** (which ancillas fired)
2. Feed syndromes into a **decoder** → get a predicted correction
3. Compare the correction to ground truth → did we recover the logical state?
4. Repeat and accumulate → compute the **logical error rate p_L**

stabstream automates this loop at hardware speed.

---

## Pipeline Overview

```mermaid
flowchart LR
    subgraph src ["Syndrome Sources"]
        direction TB
        HW["**Quantum Hardware**<br/>Real device via TCP"]
        SIM["**Noise Simulator**<br/>Bernoulli sampling<br/>from .dem file"]
        FILE["**Recorded File**<br/>.qssf / .qssf.zst"]
    end

    subgraph parse ["1. Parse — stabstream-deserialize"]
        PARSE["**QSSF Frame Parser**<br/>zero-copy ring buffer<br/>~600 ns / frame"]
    end

    subgraph validate ["2. Validate — stabstream-validate"]
        VAL["**Integrity Checks**<br/>CRC-32 · parity · timing"]
    end

    subgraph buffer ["3. Buffer — stabstream-core"]
        WIN["**Syndrome Window**<br/>sliding N-round buffer<br/>rounds x ancillas matrix"]
    end

    subgraph decode ["4. Decode — stabstream-decoder"]
        DEM["**Error Model .dem**<br/>spacetime graph<br/>edge weights -ln(p / 1-p)"]
        UF["**Union-Find**<br/>O(n·alpha(n))  ~400 ns"]
        MW["**MWPM**<br/>O(n log n)  ~4 us"]
        DEM --> UF & MW
    end

    subgraph accum ["5. Accumulate — stabstream-metrics"]
        ACC["**Logical Error Counter**<br/>predicted XOR ground truth<br/>p_L = errors / shots"]
    end

    subgraph out ["Outputs"]
        direction TB
        JSON["**JSON Report**<br/>p_L · latency percentiles"]
        PLOT["**SVG Threshold Plot**<br/>p_L vs code distance"]
        DASH["**Live TUI Dashboard**<br/>real-time ancilla fire rates"]
        PY["**Python / NumPy API**<br/>zero-copy · PyMatching bridge"]
    end

    HW  -->|QSSF bytes| PARSE
    SIM -->|QSSF bytes| PARSE
    FILE -->|QSSF bytes| PARSE

    PARSE --> VAL --> WIN
    WIN -->|detector matrix| UF & MW
    UF & MW -->|DecoderResult| ACC

    ACC --> JSON --> PLOT
    ACC --> DASH
    ACC --> PY
```

### Stage-by-stage explanation

| Stage | What happens | Key QEC concept |
|-------|-------------|-----------------|
| **Syndrome Sources** | Ancilla measurement results arrive as a binary stream in QSSF format — one frame per QEC cycle | Stabilizer measurements yield a syndrome: a bit string indicating which stabilizers anticommuted with the error |
| **Parse** | The QSSF binary stream is decoded frame-by-frame using a zero-copy ring buffer. Detector events are RLE-compressed (only the ancillas that *changed* are stored) | A *detector event* fires when an ancilla gives a different result than its previous measurement — the spacetime signal used by decoders |
| **Validate** | Each frame is checked for CRC integrity, parity consistency, and timing plausibility. Bad frames are flagged before reaching the decoder | Corrupt syndrome data would cause spurious corrections; validation protects error-rate statistics |
| **Buffer** | Frames are accumulated into a *syndrome window*: a matrix of `rounds × ancillas` booleans. Most decoders need multiple rounds to resolve ambiguous errors in time | Space-time decoding uses correlations across rounds to improve accuracy |
| **Decode** | The decoder is given the syndrome window and a *Detector Error Model* (DEM). It constructs a spacetime graph and finds the minimum-weight set of errors consistent with the observed syndrome | The DEM encodes which physical errors produce which detector events and with what probability |
| **Accumulate** | The decoder's predicted observable flips are XOR'd against the ground-truth logical outcome. Mismatches are logical errors. Counts are stored in lock-free atomic integers | p_L = (logical errors) / (total shots) |
| **Outputs** | Reports, plots, a TUI dashboard, or Python-accessible arrays | A threshold plot shows how p_L scales with code distance — when it decreases, you are below threshold |

---

## Component Map

Each crate has a single responsibility. The arrows show compile-time dependencies.

```mermaid
flowchart TD
    subgraph foundation ["Core Data Structures"]
        CORE["**stabstream-core**<br/>SyndromeFrame · SyndromeWindow<br/>CodeType · HardwareSchema"]
    end

    subgraph model ["Error Model"]
        DEM["**stabstream-dem**<br/>Stim DEM parser<br/>SpacetimeGraph builder"]
    end

    subgraph ingestion ["Data Ingestion"]
        DESER["**stabstream-deserialize**<br/>QSSF binary parser<br/>zero-copy ring buffer"]
        VAL["**stabstream-validate**<br/>CRC-32 · parity checks<br/>timing validation"]
    end

    subgraph decoding ["Decoding"]
        DECODER["**stabstream-decoder**<br/>Decoder trait<br/>UnionFind · MWPM · Null"]
    end

    subgraph measurement ["Measurement"]
        METRICS["**stabstream-metrics**<br/>LogicalErrorAccumulator<br/>LatencyHistogram · AnalysisReport"]
    end

    subgraph applications ["Applications & Tools"]
        SIM["**stabstream-sim**<br/>QSSF noise simulator<br/>TCP · broadcast · SHM"]
        REPLAY["**stabstream-replay**<br/>zstd recording player<br/>StreamPlayer · analyze_file"]
        ANALYZE["**stabstream-analyze**<br/>offline decode CLI<br/>prints JSON report"]
        THRESH["**stabstream-threshold**<br/>threshold sweep CLI<br/>SVG threshold plots"]
        CONVERT["**stabstream-convert**<br/>QSSF / Stim converters<br/>ML dataset export"]
        DASH["**dashboard**<br/>ratatui live TUI<br/>ancilla fire rates"]
    end

    subgraph bindings ["Language Bindings"]
        PY["**stabstream-py**<br/>PyO3 · NumPy arrays<br/>DEM-to-PyMatching bridge"]
        FFI["**stabstream-ffi**<br/>cbindgen C headers<br/>for C/C++ integration"]
    end

    CORE --> DESER & VAL & DEM & DECODER
    DEM  --> DECODER
    DECODER --> METRICS

    CORE --> REPLAY
    DESER --> REPLAY
    VAL --> REPLAY
    DEM --> REPLAY
    DECODER --> REPLAY
    METRICS --> REPLAY
    REPLAY --> ANALYZE

    CORE & DEM & DECODER & METRICS --> THRESH
    CORE & DESER & DEM --> SIM
    CORE & DESER & DEM --> CONVERT
    DESER --> DASH

    CORE & DESER & DEM & DECODER & METRICS --> PY
    CORE & DESER & DEM & DECODER & METRICS --> FFI
```

---

## QSSF Frame Anatomy

Every QEC cycle produces one **QSSF frame** on the wire. Here is what is inside it:

```mermaid
flowchart LR
    subgraph stream ["QSSF Binary Stream"]
        direction LR
        FH["**File Header**<br/>24 bytes · once per file<br/>magic 0x51535346 · schema UUID"]
        F1["**Frame**<br/>one QEC cycle"]
        F2["**Frame**<br/>..."]
        FN["**Frame**<br/>one QEC cycle"]
        FH --> F1 --> F2 --> FN
    end

    subgraph frm ["Frame Contents"]
        direction LR
        HDR["**Frame Header**<br/>36 bytes<br/>hardware_id · ancilla_count · timestamp"]
        PAY["**Payload**<br/>variable length<br/>RLE detector events · ancilla indices"]
        TLV["**Metadata TLV**<br/>optional key-value tags<br/>tag 0x10: logical observable flips"]
        TERM["**Terminator**<br/>2 bytes 0xDEAD"]
        HDR --> PAY --> TLV --> TERM
    end

    F1 -.->|"contains"| HDR
```

**Why RLE?** In a well-functioning device most ancillas agree with the previous round
(no error). Run-length encoding stores only the ancillas that *changed*, shrinking a
frame from O(ancilla count) to O(error weight), which is typically very small.

**Tag 0x10 (ground truth):** When generating data with `--with-observables`, Stim
embeds the true logical outcome in this tag. stabstream uses it to score the decoder:
`logical_error = predicted_flips XOR ground_truth_flips`.

---

## Transport Modes

stabstream supports three ways to deliver QSSF frames to a consumer:

```mermaid
flowchart LR
    SRC["**Syndrome Source**<br/>HW or sim"]

    subgraph direct ["Direct TCP"]
        D["**One-to-one**<br/>TCP socket<br/>lowest setup"]
    end

    subgraph broadcast ["Broadcast TCP"]
        B["**One-to-many**<br/>TCP fan-out · ring buffer<br/>skip-on-overrun for slow clients"]
    end

    subgraph shm ["Shared Memory SHM"]
        S["**POSIX SHM ring**<br/>/dev/shm/name<br/>50-200 ns latency · on-host only"]
    end

    ANALYZE["**stabstream-analyze**<br/>or custom consumer"]

    SRC -->|port 9000| D -->|QSSF frames| ANALYZE
    SRC -->|port 9000| B -->|QSSF frames| ANALYZE
    SRC -->|mmap| S -->|QSSF frames| ANALYZE
```

Use **direct** for single-consumer experiments, **broadcast** to feed multiple tools
at once (e.g., dashboard + analyzer simultaneously), and **SHM** when the simulator
and decoder run on the same host and latency matters.

---

## End-to-End Example Walk-Through

```mermaid
flowchart TD
    STIM["**circuit.stim**<br/>Stim circuit file"]
    DEM_FILE["**surface_d5.dem**<br/>stim analyze_errors output"]
    SIM_NODE["**stabstream-sim**<br/>--simulator native --dem surface_d5.dem<br/>--port 9000"]
    RINGBUF["**Ring Buffer**<br/>stabstream-deserialize<br/>QSSF frames via TCP"]
    VALID["**Validator**<br/>CRC-32 + parity checks"]
    WINDOW["**Syndrome Window**<br/>stabstream-core<br/>5 rounds x 24 ancillas"]
    DECODE_NODE["**Union-Find Decoder**<br/>stabstream-decoder<br/>SpacetimeGraph from surface_d5.dem · ~400 ns"]
    ACCUM["**Accumulate**<br/>stabstream-metrics<br/>predicted XOR ground truth"]
    REPORT["**report.json**<br/>logical_error_rate: 3.1e-3<br/>p50_decode_ns: 298 · p99_decode_ns: 841"]

    STIM -->|stim analyze_errors| DEM_FILE
    DEM_FILE --> SIM_NODE
    SIM_NODE -->|QSSF frames TCP| RINGBUF
    RINGBUF -->|SyndromeFrame| VALID
    VALID -->|validated frame| WINDOW
    WINDOW -->|detector matrix| DECODE_NODE
    DEM_FILE -->|SpacetimeGraph| DECODE_NODE
    DECODE_NODE -->|DecoderResult| ACCUM
    ACCUM --> REPORT
```

---

## Decoder Comparison

| Decoder | Algorithm | Latency (d=5) | Quality | Use case |
|---------|-----------|--------------|---------|----------|
| `union-find` | Union-Find (Delfosse & Nickerson 2021) | ~400 ns | Near-optimal | Real-time, default choice |
| `mwpm` | Minimum-Weight Perfect Matching (Fusion Blossom) | ~4 µs | Optimal | Offline analysis, threshold benchmarks |
| `null` | No-op | < 5 ns | None | Parser/validator benchmarking |

The **threshold** is the physical error rate p* below which p_L *decreases* as
code distance d increases. Use `stabstream-threshold run` to locate it for your
hardware's noise model.

---

## Further Reading

| Resource | Location |
|----------|----------|
| QSSF binary format specification | [`spec/QSSF_FORMAT.md`](spec/QSSF_FORMAT.md) |
| QEC primer (stabilizers, syndromes, thresholds) | [`docs/theory/qec_primer.md`](docs/theory/qec_primer.md) |
| Decoder algorithm guide | [`docs/theory/decoder_guide.md`](docs/theory/decoder_guide.md) |
| Five-command quick-start | [`QUICKSTART.md`](QUICKSTART.md) |
| Tutorial notebooks | [`notebooks/`](notebooks/) |
| Hardware integration guide | [`docs/tutorials/hardware_integration.md`](docs/tutorials/hardware_integration.md) |
