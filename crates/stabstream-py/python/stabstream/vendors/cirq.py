"""
Google Cirq adapter for stabstream.

Converts Cirq ``Result`` / ``SimulatesSamples`` measurement records into
stabstream-compatible frame dicts.  Works with both:

* ``cirq.Simulator().simulate(circuit)`` (single shot, exact simulation)
* ``cirq.Simulator().run(circuit, repetitions=N)`` (multi-shot sampling)
* ``cirq.DensityMatrixSimulator().run(...)``
* Any Cirq-compatible sampler that returns a ``cirq.Result``

Frame dict schema
-----------------
Each dict has:

  frame_id         : int   — shot index (0-based)
  round            : int   — measurement round (from ``round_key`` if given)
  ancilla_count    : int   — number of bits in the ancilla measurement key
  detector_events  : ndarray[bool, (ancilla_count,)]
  observable_flips : int | None

Usage::

    import cirq
    from stabstream import SyndromeWindow
    from stabstream.vendors.cirq import from_cirq_result, from_cirq_simulator

    result = cirq.Simulator().run(circuit, repetitions=1000)
    for frame in from_cirq_result(result, ancilla_key="ancilla"):
        window.push_numpy(frame["detector_events"], frame["frame_id"], frame["round"])
"""

from __future__ import annotations

from typing import Iterator

import numpy as np


def from_cirq_result(
    result: object,
    ancilla_key: str = "ancilla",
    observable_key: str | None = None,
    round_index: int = 0,
) -> Iterator[dict]:
    """
    Yield frame dicts from a Cirq ``Result`` object.

    Parameters
    ----------
    result:
        ``cirq.Result`` from ``cirq.Simulator().run(circuit, repetitions=N)``
        or equivalent.
    ancilla_key:
        Key used in the circuit's ``cirq.measure()`` call for ancilla qubits.
        Defaults to ``"ancilla"``.
    observable_key:
        Optional key for logical observable measurements.  When supplied,
        bits are packed into ``observable_flips`` as a little-endian bitmask.
    round_index:
        Round number to embed in all yielded frames (useful when combining
        results from multiple rounds into a sliding window).

    Yields
    ------
    dict
        Keys: ``frame_id``, ``round``, ``ancilla_count``,
        ``detector_events`` (ndarray bool), ``observable_flips`` (int | None).

    Examples
    --------
    ::

        import cirq
        from stabstream.vendors.cirq import from_cirq_result

        q = cirq.LineQubit.range(5)
        circuit = cirq.Circuit([cirq.H(q[0]), cirq.measure(*q, key="ancilla")])
        result = cirq.Simulator().run(circuit, repetitions=1000)

        frames = list(from_cirq_result(result, ancilla_key="ancilla"))
        # frames[0]["detector_events"].shape == (5,)
    """
    measurements = _get_measurements(result, ancilla_key)
    # measurements shape: (repetitions, num_bits) — int8 / int
    ancilla_count = measurements.shape[1]

    obs_measurements = None
    if observable_key is not None:
        obs_measurements = _get_measurements(result, observable_key)

    for i, row in enumerate(measurements):
        det_events = row.astype(bool)

        obs_flips: int | None = None
        if obs_measurements is not None:
            obs_row = obs_measurements[i]
            obs_flips = int(sum(int(b) << j for j, b in enumerate(obs_row)))

        yield {
            "frame_id": i,
            "round": round_index,
            "ancilla_count": ancilla_count,
            "detector_events": det_events,
            "observable_flips": obs_flips,
        }


def from_cirq_simulator(
    result: object,
    ancilla_key: str = "ancilla",
    observable_key: str | None = None,
) -> Iterator[dict]:
    """
    Yield frame dicts from a Cirq ``SimulationResult`` (single-shot exact sim).

    ``cirq.Simulator().simulate(circuit)`` returns a ``StateVectorTrialResult``
    which also exposes ``.measurements``.  This adapter handles that case.

    Parameters
    ----------
    result:
        ``cirq.SimulationResult`` or any object with a ``.measurements`` dict
        mapping key → 2-D int array of shape ``(1, num_bits)``.

    Yields
    ------
    dict
        Same schema as ``from_cirq_result``, with ``frame_id=0``.
    """
    yield from from_cirq_result(
        result,
        ancilla_key=ancilla_key,
        observable_key=observable_key,
        round_index=0,
    )


def from_numpy_measurements(
    measurements: np.ndarray,
    observable_measurements: np.ndarray | None = None,
    round_index: int = 0,
) -> Iterator[dict]:
    """
    Yield frame dicts from a raw NumPy measurement array.

    Parameters
    ----------
    measurements:
        Shape ``(shots, ancilla_count)``.  Any integer or bool dtype.
    observable_measurements:
        Optional shape ``(shots, observable_count)``.
    round_index:
        Round label embedded in every yielded frame.

    Yields
    ------
    dict
        Same schema as ``from_cirq_result``.
    """
    measurements = np.asarray(measurements)
    n_shots, ancilla_count = measurements.shape

    for i in range(n_shots):
        obs_flips: int | None = None
        if observable_measurements is not None:
            obs_row = np.asarray(observable_measurements[i])
            obs_flips = int(sum(int(b) << j for j, b in enumerate(obs_row)))

        yield {
            "frame_id": i,
            "round": round_index,
            "ancilla_count": ancilla_count,
            "detector_events": measurements[i].astype(bool),
            "observable_flips": obs_flips,
        }


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _get_measurements(result: object, key: str) -> np.ndarray:
    """
    Extract measurement array for ``key`` from a Cirq result.

    Returns a 2-D array of shape ``(shots, num_bits)``.
    """
    if not hasattr(result, "measurements"):
        raise TypeError(
            f"{type(result).__name__} has no .measurements attribute. "
            "Expected cirq.Result or cirq.SimulationResult."
        )

    measurements: dict = result.measurements
    if key not in measurements:
        raise KeyError(
            f"Measurement key '{key}' not found in result.measurements. "
            f"Available keys: {list(measurements.keys())}"
        )

    arr = np.asarray(measurements[key])
    if arr.ndim == 1:
        # Single-shot exact simulation returns shape (num_bits,)
        arr = arr[np.newaxis, :]
    return arr
