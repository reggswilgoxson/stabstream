"""
stabstream-qiskit-to-qssf — convert Qiskit Runtime results to QSSF.

Usage
-----
::

    stabstream-qiskit-to-qssf result.json --register ancilla --out recording.qssf
    stabstream-qiskit-to-qssf result.json --register ancilla \\
        --observable-register obs --out recording.qssf

The input JSON must be a serialised Qiskit ``PrimitiveResult`` or a list of
``SamplerPubResult`` dicts, produced by::

    import json
    result = sampler.run([circuit], shots=N).result()
    # Qiskit ≥ 1.1 supports result.to_json()
    with open("result.json", "w") as f:
        json.dump(result.to_json() if hasattr(result, "to_json") else ..., f)

Alternatively, pass a plain NumPy-style JSON: a mapping ``{"meas": [[0,1,...], ...]}``
where each inner list is one shot and values are 0/1 integers.

Examples
--------
::

    # Convert an IBM Qiskit result
    stabstream-qiskit-to-qssf ibm_result.json --out recording.qssf

    # Inspect the output
    python -c "
    from stabstream.io import load_qssf
    frames = list(load_qssf('recording.qssf'))
    print(f'{len(frames)} frames, ancilla_count={frames[0].ancilla_count}')
    "
"""

from __future__ import annotations

import argparse
import json
import sys

import numpy as np

from stabstream._qssf_write import write_qssf


def _load_numpy_json(data: dict, register: str, obs_register: str | None) -> list[dict]:
    """
    Fall-back: plain JSON dict mapping register name → 2-D list of 0/1 ints.
    This is the simplest format to produce from any vendor result exporter.
    """
    if register not in data:
        raise KeyError(
            f"Register '{register}' not found in JSON. "
            f"Available keys: {list(data.keys())}"
        )

    shots = np.asarray(data[register], dtype=bool)
    if shots.ndim == 1:
        shots = shots[np.newaxis, :]

    obs_arr: np.ndarray | None = None
    if obs_register is not None:
        if obs_register not in data:
            raise KeyError(
                f"Observable register '{obs_register}' not found. "
                f"Available keys: {list(data.keys())}"
            )
        obs_arr = np.asarray(data[obs_register], dtype=bool)
        if obs_arr.ndim == 1:
            obs_arr = obs_arr[np.newaxis, :]

    n_shots, ancilla_count = shots.shape
    frames = []
    for i in range(n_shots):
        obs_flips: int | None = None
        if obs_arr is not None:
            obs_flips = int(sum(int(b) << j for j, b in enumerate(obs_arr[i])))
        frames.append(
            {
                "frame_id": i,
                "round": 0,
                "ancilla_count": ancilla_count,
                "detector_events": shots[i].tolist(),
                "observable_flips": obs_flips,
            }
        )
    return frames


def _load_qiskit_json(
    data: object, register: str, obs_register: str | None
) -> list[dict]:
    """
    Attempt to deserialize a Qiskit PrimitiveResult JSON blob.

    Qiskit 1.x PrimitiveResult.to_json() produces a dict with keys
    ``"__type__"`` and ``"__value__"``; inner pub results have per-register
    BitArray data.  We accept any register-keyed 0/1 dict as a fallback.
    """
    try:
        from qiskit.primitives import PrimitiveResult  # type: ignore[import]
        from stabstream.vendors.ibm import from_sampler_result

        # Re-hydrate via qiskit serialisation
        from qiskit.primitives.containers import DataBin  # type: ignore[import]

        result_obj = PrimitiveResult.from_json(json.dumps(data)) if isinstance(data, dict) else data
        return list(from_sampler_result(result_obj, ancilla_register=register, observable_register=obs_register))
    except Exception:
        # Qiskit not installed or JSON schema mismatch — fall back to plain dict
        if isinstance(data, dict):
            return _load_numpy_json(data, register, obs_register)
        raise ValueError(
            "Input JSON is not a plain register-keyed dict and qiskit is not "
            "installed.  Install qiskit to deserialize PrimitiveResult JSON, "
            "or provide a plain JSON dict {'register': [[0,1,...], ...]}."
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="stabstream-qiskit-to-qssf",
        description="Convert a Qiskit Runtime SamplerV2 result JSON to QSSF.",
    )
    parser.add_argument("input", help="Path to Qiskit result JSON file")
    parser.add_argument(
        "--register",
        default="meas",
        metavar="NAME",
        help="Classical register name for ancilla measurements (default: meas)",
    )
    parser.add_argument(
        "--observable-register",
        default=None,
        metavar="NAME",
        help="Optional register name for logical observable measurements",
    )
    parser.add_argument(
        "--out",
        required=True,
        metavar="FILE",
        help="Output QSSF file path",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Suppress progress output",
    )
    args = parser.parse_args(argv)

    with open(args.input) as fh:
        raw = json.load(fh)

    frames = _load_qiskit_json(raw, args.register, args.observable_register)

    if not frames:
        print("No frames found in input JSON.", file=sys.stderr)
        return 1

    n = write_qssf(args.out, iter(frames))
    if not args.quiet:
        ancilla = frames[0]["ancilla_count"]
        print(f"Wrote {n} frames  (ancilla_count={ancilla}) → {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
