"""
Example: convert vendor SDK results to stabstream SyndromeWindow input.

Demonstrates the IBM Qiskit Runtime and Cirq adapters without requiring
real hardware — uses mock result objects that match the vendor APIs.

Usage:
    maturin develop  # from crates/stabstream-py/
    python python/examples/vendor_adapters.py
"""

from __future__ import annotations

import numpy as np

from stabstream import SyndromeWindow

# ---------------------------------------------------------------------------
# IBM Qiskit Runtime mock (no real qiskit required)
# ---------------------------------------------------------------------------


class _MockBitArray:
    """Minimal BitArray duck-type (matches qiskit.primitives.BitArray interface)."""

    def __init__(self, data: np.ndarray) -> None:
        # data shape: (shots, num_bits)
        self._data = data.astype(np.uint8)

    @property
    def num_bits(self) -> int:
        return self._data.shape[1]

    def get_bitstrings(self) -> list[str]:
        return ["".join(str(b) for b in row) for row in self._data]


class _MockDataBin:
    def __init__(self, ancilla: _MockBitArray) -> None:
        self.ancilla = ancilla


class _MockPubResult:
    def __init__(self, data: _MockDataBin) -> None:
        self.data = data


class _MockPrimitiveResult:
    def __init__(self, pub_results: list[_MockPubResult]) -> None:
        self._pubs = pub_results

    def __getitem__(self, i: int) -> _MockPubResult:
        return self._pubs[i]

    def __len__(self) -> int:
        return len(self._pubs)


def demo_ibm() -> None:
    from stabstream.vendors.ibm import from_sampler_result

    print("=== IBM Qiskit Runtime adapter ===")

    # Simulate 100 shots on a 5-qubit ancilla register
    rng = np.random.default_rng(42)
    shots_data = rng.integers(0, 2, size=(100, 5), dtype=np.uint8)
    mock_result = _MockPrimitiveResult(
        [_MockPubResult(_MockDataBin(_MockBitArray(shots_data)))]
    )

    window = SyndromeWindow(ancilla_count=5, window_depth=3)
    fired = 0

    for frame in from_sampler_result(mock_result, ancilla_register="ancilla"):
        window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])
        fired += int(frame["detector_events"].sum())

    matrix = window.to_numpy_matrix()
    print(f"  Processed 100 IBM shots, window matrix shape: {matrix.shape}")
    print(f"  Total ancilla firings: {fired}")
    print(f"  Sample frame dict keys: {list(frame.keys())}")


# ---------------------------------------------------------------------------
# Cirq mock
# ---------------------------------------------------------------------------


class _MockCirqResult:
    """Minimal cirq.Result duck-type."""

    def __init__(self, measurements: dict[str, np.ndarray]) -> None:
        self.measurements = measurements


def demo_cirq() -> None:
    from stabstream.vendors.cirq import from_cirq_result

    print("\n=== Google Cirq adapter ===")

    # Simulate 200 shots on a 7-qubit ancilla register
    rng = np.random.default_rng(99)
    measurements = rng.integers(0, 2, size=(200, 7))
    mock_result = _MockCirqResult({"ancilla": measurements})

    window = SyndromeWindow(ancilla_count=7, window_depth=4)
    frames_processed = 0

    for frame in from_cirq_result(mock_result, ancilla_key="ancilla"):
        window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])
        frames_processed += 1

    matrix = window.to_numpy_matrix()
    print(f"  Processed {frames_processed} Cirq shots, window matrix shape: {matrix.shape}")
    print(f"  Active detectors in final window: {window.active_detectors()[:8]}...")


# ---------------------------------------------------------------------------
# Generic NumPy adapter
# ---------------------------------------------------------------------------


def demo_numpy() -> None:
    from stabstream.vendors.cirq import from_numpy_measurements

    print("\n=== Generic NumPy adapter (vendors.cirq.from_numpy_measurements) ===")

    rng = np.random.default_rng(7)
    # 500 shots, 24 ancillas (d=5 surface code)
    measurements = rng.integers(0, 2, size=(500, 24))
    observables = rng.integers(0, 2, size=(500, 1))

    window = SyndromeWindow(ancilla_count=24, window_depth=5)

    for frame in from_numpy_measurements(measurements, observable_measurements=observables):
        window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])

    matrix = window.to_numpy_matrix()
    print(f"  Processed 500 shots, window matrix: {matrix.shape}")
    syndrome_weights = matrix.sum(axis=1)
    print(f"  Per-round syndrome weights: {syndrome_weights}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> None:
    demo_ibm()
    demo_cirq()
    demo_numpy()
    print("\nAll vendor adapter demos complete.")


if __name__ == "__main__":
    main()
