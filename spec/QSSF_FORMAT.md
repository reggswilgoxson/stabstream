# QEC Syndrome Stream Format (QSSF) — RFC v1.0.0

## 1. Overview and Design Principles

QSSF is a compact, schema-driven binary format for recording and streaming
quantum error correction (QEC) syndrome measurement data from quantum hardware.

**Design goals:**

| Goal | Mechanism |
|------|-----------|
| Zero-copy parsing | All payload slices are views into a ring buffer; no heap allocation per frame |
| Schema-driven | Hardware topology (stabilizers, qubit positions) lives in JSON schema files referenced by UUID |
| TLV extensible | Metadata blocks use a type-length-value encoding; unknown tags are skipped |
| Replay-friendly | Files are zstd-compressed; frame boundaries are self-describing |
| Streaming | Frames are delimited by a sentinel + checksum so TCP streams can be parsed incrementally |

---

## 2. File Header (24 bytes)

Appears once at the start of a `.qssf` or `.qssf.gz` file.

```
Offset  Size  Field        Description
------  ----  -----------  --------------------------------------------------
 0      4     magic        Must equal 0x51535346 (ASCII "QSSF"), little-endian
 4      2     version      Format version. Currently 1.
 6      16    schema_id    UUID v4 identifying the hardware schema (RFC 4122)
22      2     _reserved    Reserved, must be zero
```

**Flags word** (bytes 22–25, overlapping _reserved in earlier drafts — v1 uses
only bytes 22–23 for reserved; flags are in frame headers):

| Bit | Meaning |
|-----|---------|
| 0   | Payload is RLE-encoded (always 1 in v1) |
| 1   | Timing offsets present |
| 2   | Parity check field present |
| 3–31 | Reserved, must be zero |

---

## 3. Frame Header (36 bytes)

Immediately follows the file header and repeats for every measurement round.

```
Offset  Size  Field          Type    Description
------  ----  -------------  ------  ------------------------------------------
 0      8     frame_id       u64 LE  Monotonically increasing counter (wraps at 2^64)
 8      4     round          u32 LE  Measurement round index within an experiment
12      8     timestamp_ns   u64 LE  Hardware wall-clock nanoseconds (epoch-relative)
20      2     qubit_count    u16 LE  Number of data qubits this round
22      2     ancilla_count  u16 LE  Number of ancilla qubits measured
24      4     payload_len    u32 LE  Total byte length of the syndrome payload
28      1     code_type      u8      CodeType discriminant (see §8)
29      1     distance       u8      Code distance d
30      2     _pad           u16     Reserved, must be zero
32      4     crc32          u32 LE  CRC-32/ISO-HDLC of bytes [0..31] of this header
```

**Total: 36 bytes.**

---

## 4. Syndrome Payload Layout

Immediately follows the frame header. Total length is `payload_len` bytes.

### 4.1 `detector_events` — RLE-encoded bitfield

See §9 for the full RLE encoding specification.

Length: variable (first sub-field). Prefixed by a 2-byte `u16 LE` length in bytes.

Each decoded bit maps to one ancilla in schema order. Bit = 1 means the
ancilla measurement outcome flipped relative to the previous round
(a "detector event").

### 4.2 `meas_results` — raw ancilla outcomes

Immediately follows `detector_events`. Length: `ancilla_count` bytes.

Each byte encodes a signed measurement outcome: `0x01` = +1, `0xFF` = −1.

### 4.3 `timing_offsets` — per-ancilla timing

Immediately follows `meas_results`. Present only when flag bit 1 is set.
Length: `ancilla_count × 2` bytes.

Each `u16 LE` value is the ancilla's timing offset from the nominal cycle
start, in nanoseconds.

### 4.4 `parity_checks` — stabilizer XZ flags

Immediately follows `timing_offsets`. Present only when flag bit 2 is set.
Length: `ceil(ancilla_count / 8)` bytes.

Bit *i* = 1 means stabilizer *i* reported a parity violation.

---

## 5. Metadata Block (TLV-encoded)

An optional metadata block follows the syndrome payload. It begins with a
2-byte `u16 LE` tag count. Each TLV entry:

```
Offset  Size  Field    Description
------  ----  -------  --------------------------
 0      2     tag      u16 LE — field identifier
 2      2     len      u16 LE — value length in bytes
 4      len   value    raw bytes
```

**Known metadata tags:**

| Tag  | Type     | Description |
|------|----------|-------------|
| 0x01 | UTF-8    | Hardware ID string |
| 0x02 | f32 LE   | Fridge temperature in millikelvin |
| 0x03 | f32 LE   | Measurement cycle time in microseconds |
| 0x04 | u8       | Preferred decoder hint |

Unknown tags must be skipped (using the `len` field).

---

## 6. Logical Annotation Block

An optional block following the metadata. Contains zero or more logical qubit
annotations. Begins with a 1-byte annotation count.

Each annotation is 10 bytes:

```
Offset  Size  Field             Description
------  ----  ----------------  -----------------------
 0      1     logical_id        u8 — logical qubit index
 1      8     observable_mask   u64 LE — observable bitmask
 9      1     frame_basis       u8 — 0=Z, 1=X, 2=Y
```

---

## 7. Frame Terminator

Every frame ends with a 6-byte sentinel:

```
Offset  Size  Field      Description
------  ----  ---------  --------------------------------------------------
 0      2     sentinel   0xFFFF — marks end of frame
 2      4     crc32      CRC-32/ISO-HDLC of the entire frame (header + payload + metadata + annotations)
```

Parsers must verify the CRC before yielding the frame.

---

## 8. CodeType Enum Table

| Value | Name           | Description |
|-------|----------------|-------------|
| 0x01  | SurfaceCode    | Rotated or unrotated surface code |
| 0x02  | HoneycombCode  | Floquet/honeycomb code |
| 0x03  | ColorCode      | 2D color code (triangular lattice) |
| 0x04  | RepetitionCode | 1D repetition code (bit-flip or phase-flip) |
| 0x05  | ToricCode      | Toric code (periodic boundary conditions) |
| 0xFF  | Custom         | Hardware-defined; schema provides full description |

---

## 9. Detector Event RLE Encoding Specification

The `detector_events` bitfield is compressed with a simple run-length encoding
(RLE) optimised for sparse syndrome data.

### Bit layout of encoded stream

The encoded stream is a sequence of 1-byte tokens:

```
Token byte:  [ 1-bit mode | 7-bit run_length ]
```

| Mode bit | Meaning |
|----------|---------|
| 0        | Run of **0** bits (no detector events) |
| 1        | Run of **1** bits (detector events fired) |

`run_length` is in the range [1, 127]. A value of 0 is invalid.

### Run encoding algorithm

```
1. Initialize: bit_value ← 0, run ← 0, output ← []
2. For each bit b in the input bitfield (MSB-first within each byte):
   a. If b == bit_value and run < 127:  run ← run + 1
   b. Else:
        emit token: (bit_value << 7) | run
        bit_value ← b
        run ← 1
3. Emit final token: (bit_value << 7) | run
```

### Decoding algorithm

```
1. For each token byte t in the encoded stream:
   a. mode       ← (t >> 7) & 1
   b. run_length ← t & 0x7F
   c. Emit run_length copies of mode into the output bitfield
2. Stop when output length == ancilla_count
```

---

## 10. Schema JSON Format Specification

Schema files describe the hardware topology for a specific quantum processor
configuration. They are identified by a UUID v4 and must be registered in a
`SchemaRegistry` before parsing frames.

### Required fields

| Field                  | Type     | Description |
|------------------------|----------|-------------|
| `schema_id`            | string   | UUID v4 (hyphenated) |
| `version`              | string   | Semver (e.g. "1.0.0") |
| `name`                 | string   | Short identifier |
| `description`          | string   | Human-readable description |
| `code_type`            | string   | One of: surface, honeycomb, color, repetition, toric, custom |
| `distance`             | integer  | Code distance d |
| `qubit_count`          | integer  | Total data qubits |
| `ancilla_count`        | integer  | Total ancilla qubits |
| `stabilizers`          | array    | Array of `StabilizerEntry` objects (see below) |
| `measurement_cycle_us` | float    | Nominal measurement cycle in microseconds |
| `ancilla_layout`       | string   | Geometry hint: row_major, honeycomb, triangular, linear |

### `StabilizerEntry` object

```json
{ "id": 0, "type": "X", "qubits": [0, 1, 5, 6] }
```

| Field    | Type    | Description |
|----------|---------|-------------|
| `id`     | integer | Ancilla index (0-based, matches `meas_results` order) |
| `type`   | string  | "X" or "Z" |
| `qubits` | array   | Data-qubit indices acted on by this stabilizer |

### Optional fields

| Field          | Type   | Description |
|----------------|--------|-------------|
| `hardware_id`  | string | Vendor-specific processor identifier |
| `qubit_layout` | object | Coordinate map `{ "0": [x, y], ... }` for rendering |

---

## 11. Changelog

### v1.0.0 — 2024-01-01 (initial release)

- Initial specification of the QSSF binary format.
- File header (24 bytes), frame header (36 bytes), syndrome payload.
- RLE encoding for detector event bitfields.
- TLV metadata blocks and logical annotation blocks.
- Frame terminator with CRC-32/ISO-HDLC integrity check.
- Six built-in hardware schemas: surface (d=3,5,7), honeycomb (d=4), color (d=5), repetition (d=11).
