# Tutorial 07 — Hardware Vendor SDK Integration

> **Honest scope** — This tutorial covers *offline* conversion of post-hoc
> experiment results from IBM Qiskit Runtime and Google Cirq into QSSF files
> for stabstream analysis.  Real-time syndrome streams from current hardware
> are not publicly available (see [What Vendors Expose Today](#what-vendors-expose-today)).

## Prerequisites

```bash
pip install stabstream numpy
# Optional: install vendor SDKs you actually have access to
pip install qiskit qiskit-ibm-runtime   # IBM
pip install cirq                         # Google
```

---

## Workflow Overview

```
IBM Qiskit Runtime result  ─┐
Google Cirq result          ├─► JSON file ─► stabstream-*-to-qssf ─► .qssf ─► stabstream-analyze
Plain numpy measurements   ─┘
```

All stabstream analysis tools (`stabstream-analyze`, `load_qssf`, the TUI
dashboard) consume `.qssf` files.  The two CLI tools shipped in this milestone
bridge vendor result objects to that format.

---

## IBM Qiskit Runtime

### Step 1 — Run your circuit on IBM hardware or the Qiskit simulator

```python
from qiskit import QuantumCircuit
from qiskit_ibm_runtime import QiskitRuntimeService, SamplerV2

service = QiskitRuntimeService()
backend = service.backend("ibm_sherbrooke")   # or ibm_kyiv, etc.
sampler = SamplerV2(backend)

# Build your syndrome-extraction circuit (distance-3 surface code example):
# qc = build_syndrome_circuit(distance=3)   ← your circuit here
job = sampler.run([qc], shots=10_000)
result = job.result()
```

### Step 2 — Serialise the result to JSON

Qiskit 1.1+ supports `result.to_json()`:

```python
import json

with open("ibm_result.json", "w") as f:
    json.dump(result.to_json(), f)
```

If your Qiskit version doesn't have `to_json`, use the plain-dict fallback:

```python
# Fallback: dump the raw measurement bitstrings
from stabstream.vendors.ibm import from_sampler_result
import json, numpy as np

frames = list(from_sampler_result(result, ancilla_register="meas"))
plain = {
    "meas": [f["detector_events"].tolist() for f in frames]
}
with open("ibm_result.json", "w") as f:
    json.dump(plain, f)
```

### Step 3 — Convert to QSSF

```bash
stabstream-qiskit-to-qssf ibm_result.json \
    --register meas \
    --out ibm_recording.qssf
# Wrote 10000 frames  (ancilla_count=24) → ibm_recording.qssf
```

If your circuit also measures logical observables in a separate register:

```bash
stabstream-qiskit-to-qssf ibm_result.json \
    --register meas \
    --observable-register logical_obs \
    --out ibm_recording.qssf
```

### Step 4 — Analyse

```bash
stabstream-analyze ibm_recording.qssf --decoder union-find --out report.json
```

Or load interactively:

```python
from stabstream.io import load_qssf, read_qssf
import numpy as np

frames = list(load_qssf("ibm_recording.qssf"))
matrix = np.stack([f.to_numpy_detector_events() for f in frames])
# matrix.shape == (10000, 24) for a 24-ancilla circuit

print(f"Mean syndrome weight: {matrix.mean(axis=1).mean():.4f}")
print(f"Ancilla fire rates:")
for i, rate in enumerate(matrix.mean(axis=0)):
    if rate > 0.1:
        print(f"  ancilla {i:3d}: {rate:.3f}  ← elevated")
```

---

## Google Cirq

### Step 1 — Simulate or sample from hardware

```python
import cirq

# Build your surface code syndrome circuit
# circuit = build_repetition_code_circuit(distance=3)

simulator = cirq.Simulator()
result = simulator.run(circuit, repetitions=10_000)
```

### Step 2 — Serialise to JSON

```python
import json

# Option A: Cirq's built-in JSON serialiser (recommended)
with open("cirq_result.json", "w") as f:
    json.dump(cirq.to_json(result), f)

# Option B: plain dict fallback
plain = {"ancilla": result.measurements["ancilla"].tolist()}
with open("cirq_result.json", "w") as f:
    json.dump(plain, f)
```

### Step 3 — Convert to QSSF

```bash
stabstream-cirq-to-qssf cirq_result.json \
    --key ancilla \
    --out cirq_recording.qssf
# Wrote 10000 frames  (ancilla_count=8) → cirq_recording.qssf
```

With logical observable measurements:

```bash
stabstream-cirq-to-qssf cirq_result.json \
    --key ancilla \
    --observable-key logical \
    --out cirq_recording.qssf
```

### Step 4 — Analyse

```bash
# View in the interactive dashboard
stabstream-dashboard --source cirq_recording.qssf

# Or run batch analysis
stabstream-analyze cirq_recording.qssf --out report.json
```

---

## Plain NumPy / Any Vendor

If you have raw measurement arrays (from any hardware provider, noisy
simulator, or custom acquisition system), use the plain-dict JSON format:

```python
import json
import numpy as np

# measurements.shape == (n_shots, ancilla_count), dtype=int or bool
measurements: np.ndarray  # your data here

plain = {"meas": measurements.astype(int).tolist()}
with open("my_result.json", "w") as f:
    json.dump(plain, f)
```

Then convert with either CLI:

```bash
stabstream-qiskit-to-qssf my_result.json --register meas --out recording.qssf
# or
stabstream-cirq-to-qssf my_result.json --key meas --out recording.qssf
```

---

## What Vendors Expose Today

| Provider | What's available | What's not |
|----------|-----------------|------------|
| **IBM Qiskit Runtime** | `SamplerV2` shot-level bitstrings (post-hoc), per-shot measurement outcomes | Per-round ancilla streams during execution, sub-µs feedback |
| **Google Cirq** | Simulation results, Sycamore bitstrings (research only) | Real-time syndrome rounds from production hardware |
| **Quantinuum / IonQ** | Shot-level measurement outputs | Per-round ancilla streams |

**The fundamental limitation:** current QPU control stacks expose *completed
shot* results, not *per-round ancilla syndromes* interleaved with gate
operations.  A real-time syndrome stream requires hardware-level support for
mid-circuit measurement and classical feedback with latency < 1 µs.

When vendors expose real-time APIs, stabstream's TCP and SHM transports
(`docs/tutorials/04_transport_modes.md`) are the right interface — no changes
to the analysis stack required.

---

## C/FPGA Producer Integration (SHM path)

The fastest integration path for FPGA or ASIC control electronics is the
POSIX SHM ring (50–200 ns IPC latency vs 2–5 µs for TCP).  `stabstream-ffi`
exposes a producer-side C API so firmware written in C can write syndrome
frames directly into the ring:

```c
#include "stabstream.h"

// Create the SHM ring — consumers open /dev/shm/my_qpu
StabstreamShmHandle* prod = stabstream_shm_open("my_qpu");
if (!prod) { /* handle error */ }

// Write a QSSF-encoded syndrome frame on each measurement cycle
int rc = stabstream_shm_write(prod, frame_bytes, frame_len);

// Tear down (does NOT delete /dev/shm/my_qpu — call shm_unlink separately)
stabstream_shm_close(prod);
```

On the decoder side, open the same SHM name as the source URI and read frames:

```bash
stabstream-analyze shm://my_qpu --decoder union-find --out report.json
```

### Retrieving decoder corrections from C

After each `stabstream_next_frame` call, retrieve the correction bitmask:

```c
// Read the next syndrome frame
int64_t n = stabstream_next_frame(handle, buf, sizeof(buf));

// Get observable_flips: bit i set → logical qubit i was flipped
int64_t flips = stabstream_decode_frame(handle);
// Apply feedback gates for each set bit...
```

**Current behaviour:** `stabstream_decode_frame` returns the `observable_flips`
value from QSSF TLV metadata (tag `0x10`) when present.  This is the simulator
ground-truth path — useful for hardware-in-the-loop testing with `stabstream-sim`
as the syndrome source.  For real hardware frames that carry no metadata, it
returns `0`.  Wiring a full spacetime-graph decoder (Union-Find or MWPM) into
the C FFI path is tracked as future work; the API surface is stable.

---

## Known Integration Gaps

### Gap D — Schema must be pre-registered; no dynamic push at connect time

`stabstream_open` currently uses `ValidationPolicy::Disabled` because the
hardware schema is not automatically loaded in the C/FFI path.  For fixed
topologies (e.g., a dedicated d=5 surface-code chip), pre-load the schema at
startup:

```rust
// Rust side: register schema before opening
let mut registry = SchemaRegistry::new();
registry.register_from_file("schemas/surface_code_d5.json")?;
```

For production systems where topology changes between calibration runs (qubit
dropout, reconfiguration), dynamic schema push is not yet supported.
**Workaround:** write the updated schema JSON to a well-known path and restart
the stabstream consumer process.  Future work: expose
`stabstream_register_schema_json(const char*, size_t)` in the C API.

### Gap E — SHM ring has no backpressure signal to the hardware producer

`ShmProducer::write_frame` silently overwrites the oldest slot when the 256-slot
ring is full.  The producer (FPGA firmware) has no way to detect that the decoder
is falling behind before frames are lost.

**SHM layout bytes 8–15** are currently zeroed/reserved.  The intended future use
is a `consumer_seq` counter written by the stabstream consumer after each frame
is processed.  Hardware can read this field to estimate lag:

```
lag = producer_seq - consumer_seq   // if lag > RING_SLOTS → overrun imminent
```

Until this is implemented, hardware should pace frame emission to no faster than
the decoder's sustained throughput (~1.5M frames/s on a modern x86 host).

---

## Round-Trip Verification

Verify the converted file parses correctly and matches your original data:

```python
from stabstream.io import load_qssf
import numpy as np

frames = list(load_qssf("ibm_recording.qssf"))

# Check frame count matches shot count
assert len(frames) == 10_000, f"Expected 10000 frames, got {len(frames)}"

# Check ancilla count
assert frames[0].ancilla_count == 24

# Reconstruct syndrome matrix and compare to raw
matrix = np.stack([f.to_numpy_detector_events() for f in frames])
print(f"Matrix shape: {matrix.shape}, dtype: {matrix.dtype}")
print(f"Non-zero rate: {matrix.mean():.4f}")
```

---

## CLI Reference

### `stabstream-qiskit-to-qssf`

```
usage: stabstream-qiskit-to-qssf [-h] --out FILE [--register NAME]
                                   [--observable-register NAME] [--quiet]
                                   input

positional arguments:
  input                     Path to Qiskit result JSON file

options:
  --register NAME           Classical register for ancilla measurements (default: meas)
  --observable-register NAME  Optional register for logical observable measurements
  --out FILE                Output QSSF file path (required)
  --quiet                   Suppress progress output
```

### `stabstream-cirq-to-qssf`

```
usage: stabstream-cirq-to-qssf [-h] --out FILE [--key KEY]
                                 [--observable-key KEY] [--round N] [--quiet]
                                 input

positional arguments:
  input                  Path to Cirq result JSON file

options:
  --key KEY              Measurement key for ancilla qubits (default: ancilla)
  --observable-key KEY   Optional key for logical observable results
  --round N              Round index embedded in all frames (default: 0)
  --out FILE             Output QSSF file path (required)
  --quiet                Suppress progress output
```
