# stabstream

[![PyPI](https://img.shields.io/pypi/v/stabstream)](https://pypi.org/project/stabstream/)
[![License](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

High-performance QEC syndrome stream deserializer and real-time Union-Find decoder
for Python — built in Rust via PyO3.

## Installation

```bash
pip install stabstream
```

## Quick start

### Zero-config (Stim users)

```python
import stim
import stabstream

circuit = stim.Circuit.from_file("surface_d5.stim")

with stabstream.from_stim_circuit("recording.qssf", circuit) as stream:
    for frame in stream:
        apply_correction(frame.observable_flips)
```

### Explicit DEM

```python
with stabstream.open("recording.qssf", decoder="surface_d5.dem") as stream:
    for frame in stream:
        print(frame.observable_flips)
```

### Async (live hardware, multiple sources)

```python
import asyncio, stabstream

async def main():
    circuit = stim.Circuit.from_file("surface_d5.stim")
    async with stabstream.from_stim_circuit("tcp://fpga:9000", circuit) as stream:
        async for frame in stream:
            await apply_correction(frame.observable_flips)

asyncio.run(main())
```

### Raw NumPy arrays

```python
with stabstream.open("recording.qssf") as stream:
    for frame in stream:
        events = frame.to_numpy_detector_events()  # shape (ancilla_count,), dtype bool
        meas   = frame.to_numpy_meas_results()     # shape (ancilla_count,), dtype int8
```

## Key classes

| Class | Description |
|---|---|
| `SyndromeStream` | Dual sync+async stream with integrated UF decoder |
| `SyndromeFrame` | Single parsed frame — NumPy arrays, `observable_flips` |
| `SyndromeWindow` | Sliding multi-round detector matrix |
| `DetectorErrorModel` | Stim DEM parser + PyMatching bridge |
| `LogicalErrorAccumulator` | Lock-free p_L accumulator |
| `StabstreamError` | Exception raised on decode/stream errors |

## Source

[github.com/reggswilgoxson/stabstream](https://github.com/reggswilgoxson/stabstream)
