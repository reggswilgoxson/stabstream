# Tutorial 3: Python Integration

stabstream's Python bindings expose zero-copy NumPy arrays, a pandas-compatible
generator, and vendor adapters for IBM Qiskit Runtime and Google Cirq.

## Installation

```bash
pip install maturin numpy

# Build and install the extension in development mode
cd crates/stabstream-py
maturin develop

# Or install from a wheel (when published)
pip install stabstream
```

## Parsing QSSF files

```python
import numpy as np
from stabstream import SyndromeWindow, load_qssf

window = None
WINDOW_DEPTH = 5

for frame in load_qssf("recording.qssf"):
    if window is None:
        window = SyndromeWindow(frame.ancilla_count, WINDOW_DEPTH)

    # Zero-copy NumPy views
    det_events: np.ndarray = frame.to_numpy_detector_events()  # shape (ancilla_count,), bool
    meas: np.ndarray = frame.to_numpy_meas_results()           # shape (ancilla_count,), int8

    window.push(frame)
    if window.is_full():
        matrix = window.to_numpy_matrix()  # shape (window_depth, ancilla_count), bool
        active = window.active_detectors()  # list[int] of ancilla indices that fired
```

## Loading into pandas

```python
from stabstream.io import read_qssf

df = read_qssf(
    "recording.qssf",
    columns=["frame_id", "round", "ancilla_count", "detector_event_count"],
)
print(df.head())
print(f"Mean fire rate: {df['detector_event_count'].mean():.1f} / {df['ancilla_count'].iloc[0]}")
```

## Batched NumPy loading

```python
from stabstream.io import load_qssf_batch

total_shots = 0
for batch in load_qssf_batch("recording.qssf", batch_size=256):
    # batch.shape == (n, ancilla_count), dtype=bool
    total_shots += batch.shape[0]
    syndrome_weights = batch.sum(axis=1)  # weight per shot
```

## IBM Qiskit Runtime adapter

```python
from stabstream import SyndromeWindow
from stabstream.vendors.ibm import from_sampler_result

# result = sampler.run([circuit], shots=1000).result()  # real hardware
# For testing, use mock objects as in python/examples/vendor_adapters.py

window = SyndromeWindow(ancilla_count=5, window_depth=3)
for frame in from_sampler_result(result, ancilla_register="ancilla"):
    window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])

matrix = window.to_numpy_matrix()
```

The adapter handles both `SamplerV2` / `PrimitiveResult` (multi-pub) and bare
`SamplerPubResult`. Pass `observable_register="meas_obs"` if you measure logical
observables in a separate register.

## Google Cirq adapter

```python
import cirq
from stabstream.vendors.cirq import from_cirq_result

circuit = cirq.Circuit(...)
result = cirq.Simulator().run(circuit, repetitions=1000)

window = SyndromeWindow(ancilla_count=7, window_depth=4)
for frame in from_cirq_result(result, ancilla_key="ancilla"):
    window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])
```

For single-shot exact simulation use `from_cirq_simulator`, and for raw NumPy
arrays use `from_numpy_measurements`.

## PyMatching bridge

```python
from stabstream import DetectorErrorModel

dem = DetectorErrorModel.from_file("circuit.dem")
matching = dem.to_pymatching()  # returns pymatching.Matching

# Now use matching.decode() with stabstream-generated syndrome matrices
```

## Frame dict schema

All vendor adapters yield dicts with the same schema, compatible with
`SyndromeWindow.push_numpy()`:

| Key | Type | Description |
|-----|------|-------------|
| `frame_id` | int | Shot index |
| `round` | int | Measurement round (0 for single-round hardware) |
| `ancilla_count` | int | Number of ancilla bits |
| `detector_events` | ndarray[bool] | Shape `(ancilla_count,)` |
| `observable_flips` | int \| None | Logical observable bitmask, if available |

## Running the examples

```bash
# Parse a QSSF file
python python/examples/parse_frames.py recording.qssf

# Vendor adapter demos (no real hardware needed)
python python/examples/vendor_adapters.py
```

## Next steps

- [Tutorial 4: Transport Modes](04_transport_modes.md)
- [Theory: Decoder Guide](../theory/decoder_guide.md)
