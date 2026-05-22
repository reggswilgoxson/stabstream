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
        HW["Quantum Hardware\n──────────────\nReal device\nvia TCP stream"]
        SIM["Noise Simulator\n──────────────\nBernoulli sampling\nfrom .dem file"]
        FILE["Recorded File\n──────────────\n.qssf\n.qssf.zst"]
    end

    subgraph parse ["① Parse\nstabstream-deserialize"]
        PARSE["QSSF Frame Parser\n──────────────\nzero-copy ring buffer\nRLE detector events\n~600 ns / frame"]
    end

    subgraph validate ["② Validate\nstabstream-validate"]
        VAL["Integrity Checks\n──────────────\nCRC-32 checksum\nparity consistency\ntiming bounds"]
    end

    subgraph buffer ["③ Buffer\nstabstream-core"]
        WIN["Syndrome Window\n──────────────\nsliding N-round buffer\nrounds × ancillas matrix\nfed into decoder as a block"]
    end

    subgraph decode ["④ Decode\nstabstream-decoder"]
        DEM["Error Model .dem\n──────────────\nspacetime graph\nfrom Stim DEM file\nedge weights −ln p÷1−p"]
        UF["Union-Find\n──────────────\nO(n·α(n))  ~400 ns\nfast, near-optimal"]
        MW["MWPM\n──────────────\nO(n log n)  ~4 µs\noptimal matching"]
        DEM --> UF & MW
    end

    subgraph accum ["⑤ Accumulate\nstabstream-metrics"]
        ACC["Logical Error Counter\n──────────────\npredicted ⊕ ground truth\nlock-free atomic counters\np_L = errors ÷ shots"]
    end

    subgraph out ["Outputs"]
        direction TB
        JSON["JSON Report\np_L · latency percentiles\nancilla fire frequencies"]
        PLOT["SVG Threshold Plot\np_L vs code distance\nlocates threshold p*"]
        DASH["Live TUI Dashboard\nreal-time ancilla\nfire-rate monitor"]
        PY["Python / NumPy API\nzero-copy arrays\nPyMatching bridge"]
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
        CORE["stabstream-core\n─────────────────────\nSyndromeFrame  SyndromeWindow\nCodeType  HardwareSchema\nJSON hardware topology files"]
    end

    subgraph model ["Error Model"]
        DEM["stabstream-dem\n─────────────────────\nStim DEM parser\nSpacetimeGraph builder\nedge weights from p"]
    end

    subgraph ingestion ["Data Ingestion"]
        DESER["stabstream-deserialize\n─────────────────────\nQSSF binary parser\nzero-copy ring buffer\nRLE codec  async stream"]
        VAL["stabstream-validate\n─────────────────────\nCRC-32  parity checks\ntiming validation\nstrict schema mode"]
    end

    subgraph decoding ["Decoding"]
        DECODER["stabstream-decoder\n─────────────────────\nDecoder trait\nUnionFindDecoder\nFusionBlossomDecoder MWPM\nNullDecoder benchmark"]
    end

    subgraph measurement ["Measurement"]
        METRICS["stabstream-metrics\n─────────────────────\nLogicalErrorAccumulator\nLatency Histogram\nAnalysisReport JSON"]
    end

    subgraph applications ["Applications & Tools"]
        SIM["stabstream-sim\n─────────────────────\nQSSF noise simulator\nBernoulli shot sampler\nTCP  broadcast  SHM"]
        REPLAY["stabstream-replay\n─────────────────────\nzstd recording player\nStreamPlayer\nanalyze_file"]
        ANALYZE["stabstream-analyze\n─────────────────────\noffline decode CLI\nreplays .qssf files\nprints JSON report"]
        THRESH["stabstream-threshold\n─────────────────────\nthreshold sweep CLI\nparallel rayon workers\nSVG threshold plots"]
        CONVERT["stabstream-convert\n─────────────────────\nQSSF  Stim converters\nobservable ground-truth\nML dataset export"]
        DASH["dashboard\n─────────────────────\nratatui live TUI\nancilla fire rates\nconnects to TCP stream"]
    end

    subgraph bindings ["Language Bindings"]
        PY["stabstream-py\n─────────────────────\nPyO3  NumPy arrays\nDEM-to-PyMatching bridge\nvendor adapters"]
        FFI["stabstream-ffi\n─────────────────────\ncbindgen C headers\nfor C/C++ integration"]
    end

    CORE --> DESER
    CORE --> VAL
    CORE --> DEM
    CORE --> DECODER
    DEM  --> DECODER
    DECODER --> METRICS

    CORE & DESER & VAL & DEM & DECODER & METRICS --> REPLAY
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
        FH["File Header\n24 bytes  once per file\n────────────────\nmagic bytes  0x51535346\nformat version\nschema UUID\nancilla count"]
        F1["Frame\n(one QEC cycle)"]
        F2["Frame\n..."]
        FN["Frame\n(one QEC cycle)"]
        FH --> F1 --> F2 --> FN
    end

    subgraph frm ["Frame Contents"]
        direction LR
        HDR["Frame Header\n36 bytes\n────────────────\nhardware_id\nancilla_count\ncycle_time ns\ntimestamp\nflags"]
        PAY["Payload\nvariable length\n────────────────\nRLE detector events\nfired ancilla indices\n±1 measurement results"]
        TLV["Metadata TLV\noptional key-value tags\n────────────────\ntag 0x10\nlogical observable flips\nground truth for scoring"]
        TERM["Terminator\n2 bytes  0xDEAD"]
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
    SRC["Syndrome\nSource\nHW or sim"]

    subgraph direct ["Direct  TCP"]
        D["One-to-one\nTCP socket\nlowest setup"]
    end

    subgraph broadcast ["Broadcast  TCP"]
        B["One-to-many\nTCP fan-out\nring buffer\nskip-on-overrun\nfor slow clients"]
    end

    subgraph shm ["Shared Memory  SHM"]
        S["POSIX SHM ring\n/dev/shm/name\n50–200 ns latency\non-host IPC only\nlowest latency"]
    end

    ANALYZE["stabstream-analyze\nor custom consumer"]

    SRC -->|port 9000| D -->|QSSF frames| ANALYZE
    SRC -->|port 9000| B -->|QSSF frames| ANALYZE
    SRC -->|mmap| S -->|QSSF frames| ANALYZE
```

Use **direct** for single-consumer experiments, **broadcast** to feed multiple tools
at once (e.g., dashboard + analyzer simultaneously), and **SHM** when the simulator
and decoder run on the same host and latency matters.

---

## End-to-End Example Walk-Through

```
circuit.stim  ──stim analyze_errors──►  surface_d5.dem
                                               │
                                         ┌─────▼──────┐
                                         │ stabstream- │
                                         │    sim      │  ◄── --simulator native
                                         │             │       --dem surface_d5.dem
                                         └─────┬───────┘       --port 9000
                                               │ QSSF frames (TCP)
                                         ┌─────▼───────┐
                                         │   ring buf  │  (stabstream-deserialize)
                                         └─────┬───────┘
                                               │ SyndromeFrame
                                         ┌─────▼───────┐
                                         │  validator  │  CRC + parity
                                         └─────┬───────┘
                                               │
                                         ┌─────▼───────┐
                                         │  syndrome   │  5 rounds × 24 ancillas
                                         │   window    │  (stabstream-core)
                                         └─────┬───────┘
                                               │ detector matrix
                                      ┌────────▼────────┐
                     surface_d5.dem ──►  Union-Find      │  ~400 ns
                     (SpacetimeGraph)  │  Decoder        │  (stabstream-decoder)
                                      └────────┬────────┘
                                               │ DecoderResult
                                         ┌─────▼───────┐
                                         │  accumulate │  predicted ⊕ truth
                                         └─────┬───────┘
                                               │
                                         report.json
                                         { "logical_error_rate": 3.1e-3,
                                           "p50_decode_ns": 298,
                                           "p99_decode_ns": 841 }
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
