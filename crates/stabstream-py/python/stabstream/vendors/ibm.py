"""
IBM Qiskit Runtime adapter for stabstream.

Converts ``SamplerV2`` / ``SamplerPubResult`` results from Qiskit Runtime
into stabstream-compatible frame dicts.  Each dict carries:

  frame_id         : int   — shot index
  round            : int   — always 0 (Qiskit doesn't expose round numbers)
  ancilla_count    : int   — number of classical bits in the ancilla register
  detector_events  : ndarray[bool, (ancilla_count,)]
  observable_flips : None  — set externally if logical observables are measured

The dicts are drop-in compatible with ``SyndromeWindow.push_numpy()``::

    from stabstream import SyndromeWindow
    from stabstream.vendors.ibm import from_sampler_result

    for frame in from_sampler_result(result):
        window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])
        matrix = window.to_numpy_matrix()

Requirements
------------
``qiskit`` and ``qiskit-ibm-runtime`` are NOT required at import time —
they are only imported inside the conversion functions, so this module is
always safe to import.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Iterator

import numpy as np

if TYPE_CHECKING:
    pass  # avoid hard dep at import time


def from_sampler_result(
    result: object,
    ancilla_register: str = "meas",
    observable_register: str | None = None,
) -> Iterator[dict]:
    """
    Yield frame dicts from a Qiskit Runtime ``SamplerV2`` result.

    Supports both single-pub results (``SamplerPubResult``) and multi-pub
    results (``PrimitiveResult[SamplerPubResult]``).

    Parameters
    ----------
    result:
        ``qiskit_ibm_runtime.result.SamplerPubResult`` **or**
        ``qiskit.primitives.PrimitiveResult`` from ``SamplerV2.run(...).result()``.
    ancilla_register:
        Name of the classical register containing ancilla measurements.
        Defaults to ``"meas"`` (the Qiskit default for transpiled circuits).
    observable_register:
        Optional name of a separate classical register holding logical
        observable measurements. When supplied, the bits are packed into
        ``observable_flips`` as a little-endian bitmask.

    Yields
    ------
    dict
        Keys: ``frame_id``, ``round``, ``ancilla_count``,
        ``detector_events`` (ndarray bool), ``observable_flips`` (int | None).

    Examples
    --------
    ::

        from qiskit_ibm_runtime import QiskitRuntimeService, SamplerV2

        service = QiskitRuntimeService()
        backend = service.backend("ibm_sherbrooke")
        sampler = SamplerV2(backend)

        job = sampler.run([circuit], shots=1000)
        result = job.result()

        from stabstream.vendors.ibm import from_sampler_result
        frames = list(from_sampler_result(result, ancilla_register="ancilla"))
    """
    pub_results = _unpack_pub_results(result)

    for pub_result in pub_results:
        data = pub_result.data

        if not hasattr(data, ancilla_register):
            raise ValueError(
                f"Register '{ancilla_register}' not found in result.data. "
                f"Available: {list(vars(data).keys())}"
            )

        ancilla_bits = getattr(data, ancilla_register)
        # BitArray.array has shape (shots, num_bits // 8) — use .get_int_counts
        # or .get_bitstrings() depending on qiskit version.
        shots_array = _bitarray_to_bool(ancilla_bits)  # (shots, num_bits)

        obs_array: np.ndarray | None = None
        if observable_register is not None and hasattr(data, observable_register):
            obs_bits = getattr(data, observable_register)
            obs_array = _bitarray_to_bool(obs_bits)

        n_shots, ancilla_count = shots_array.shape

        for i in range(n_shots):
            det_events = shots_array[i]  # shape (ancilla_count,)

            obs_flips: int | None = None
            if obs_array is not None:
                obs_row = obs_array[i]
                obs_flips = int(sum(int(b) << j for j, b in enumerate(obs_row)))

            yield {
                "frame_id": i,
                "round": 0,
                "ancilla_count": ancilla_count,
                "detector_events": det_events,
                "observable_flips": obs_flips,
            }


def from_bit_array(
    bit_array: object,
    observable_bit_array: object | None = None,
) -> Iterator[dict]:
    """
    Yield frame dicts from a raw Qiskit ``BitArray``.

    Lower-level alternative to ``from_sampler_result`` when you already have
    a ``BitArray`` object (e.g. from ``BitArray.from_samples(bitstrings)``).

    Parameters
    ----------
    bit_array:
        ``qiskit.primitives.BitArray`` of shape ``(shots, num_bits)``.
    observable_bit_array:
        Optional ``BitArray`` for logical observables.
    """
    shots_array = _bitarray_to_bool(bit_array)
    obs_array = _bitarray_to_bool(observable_bit_array) if observable_bit_array is not None else None
    n_shots, ancilla_count = shots_array.shape

    for i in range(n_shots):
        obs_flips: int | None = None
        if obs_array is not None:
            obs_row = obs_array[i]
            obs_flips = int(sum(int(b) << j for j, b in enumerate(obs_row)))
        yield {
            "frame_id": i,
            "round": 0,
            "ancilla_count": ancilla_count,
            "detector_events": shots_array[i],
            "observable_flips": obs_flips,
        }


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _unpack_pub_results(result: object) -> list:
    """
    Return a list of SamplerPubResult objects regardless of whether `result`
    is a PrimitiveResult (multi-pub) or a bare SamplerPubResult.
    """
    # PrimitiveResult is indexable: result[i] is a SamplerPubResult
    if hasattr(result, "__getitem__") and hasattr(result, "__len__"):
        return [result[i] for i in range(len(result))]
    # Already a single SamplerPubResult
    if hasattr(result, "data"):
        return [result]
    raise TypeError(
        f"Unsupported result type: {type(result).__name__}. "
        "Expected PrimitiveResult or SamplerPubResult."
    )


def _bitarray_to_bool(bit_array: object) -> np.ndarray:
    """
    Convert a Qiskit ``BitArray`` to a 2-D NumPy bool array of shape
    ``(shots, num_bits)``.

    Supports both Qiskit 1.x ``BitArray`` (preferred) and plain ndarray
    inputs for testing.
    """
    if isinstance(bit_array, np.ndarray):
        return bit_array.astype(bool)

    # Qiskit 1.x BitArray: use get_bitstrings() as a universal interface
    if hasattr(bit_array, "get_bitstrings"):
        bitstrings = bit_array.get_bitstrings()
        return np.array([[c == "1" for c in bs] for bs in bitstrings], dtype=bool)

    # Qiskit 1.x BitArray: .array has shape (shots, num_bytes) uint8
    if hasattr(bit_array, "array") and hasattr(bit_array, "num_bits"):
        arr = np.array(bit_array.array)
        num_bits: int = int(bit_array.num_bits)
        shots = arr.shape[0]
        # Unpack uint8 rows into bits, trim to num_bits
        bits = np.unpackbits(arr, axis=1, bitorder="big")[:, :num_bits]
        return bits.astype(bool)

    raise TypeError(
        f"Cannot convert {type(bit_array).__name__} to bool array. "
        "Expected qiskit.primitives.BitArray or numpy.ndarray."
    )
