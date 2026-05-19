"""
stabstream-cirq-to-qssf — convert Cirq/Stim simulation results to QSSF.

Usage
-----
::

    stabstream-cirq-to-qssf result.json --key ancilla --out recording.qssf
    stabstream-cirq-to-qssf result.json --key ancilla \\
        --observable-key logical --out recording.qssf

The input JSON must be one of:

1. **Cirq Result JSON** — produced by ``cirq.Result.to_json()``:
   ``{"measurements": {"ancilla": [[0,1,...], ...]}, ...}``

2. **Plain dict** — any JSON mapping measurement key → 2-D list of 0/1 ints:
   ``{"ancilla": [[0,1,0,...], [1,0,1,...]], "logical": [[0],[1]]}``

Examples
--------
::

    # Dump a Cirq result to JSON from Python
    import json, cirq
    result = cirq.Simulator().run(circuit, repetitions=1000)
    with open("cirq_result.json", "w") as f:
        json.dump({"ancilla": result.measurements["ancilla"].tolist()}, f)

    # Convert to QSSF
    stabstream-cirq-to-qssf cirq_result.json --key ancilla --out recording.qssf
"""

from __future__ import annotations

import argparse
import json
import sys

import numpy as np

from stabstream._qssf_write import write_qssf


def _load_cirq_json(
    data: object, key: str, obs_key: str | None, round_index: int
) -> list[dict]:
    """
    Load frames from a Cirq result JSON blob or a plain dict.

    Tries ``cirq.read_json()`` first if cirq is installed.  Falls back to
    treating the input as a plain ``{"key": [[0,1,...], ...]}`` dict.
    """
    try:
        import cirq  # type: ignore[import]
        from stabstream.vendors.cirq import from_cirq_result

        # cirq.read_json accepts a JSON string
        cirq_result = cirq.read_json(json_text=json.dumps(data))
        return list(from_cirq_result(cirq_result, ancilla_key=key, observable_key=obs_key, round_index=round_index))
    except Exception:
        pass

    # Fall back: plain dict with measurement arrays
    if not isinstance(data, dict):
        raise ValueError(
            "Input JSON is neither a Cirq Result JSON nor a plain "
            "{'key': [[0,1,...], ...]} dict."
        )

    # Support Cirq Result JSON structure: {"measurements": {"key": [...]}}
    measurements: dict
    if "measurements" in data and isinstance(data["measurements"], dict):
        measurements = data["measurements"]
    else:
        measurements = data

    if key not in measurements:
        raise KeyError(
            f"Key '{key}' not found. "
            f"Available: {list(measurements.keys())}"
        )

    shots = np.asarray(measurements[key], dtype=bool)
    if shots.ndim == 1:
        shots = shots[np.newaxis, :]

    obs_arr: np.ndarray | None = None
    if obs_key is not None and obs_key in measurements:
        obs_arr = np.asarray(measurements[obs_key], dtype=bool)
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
                "round": round_index,
                "ancilla_count": ancilla_count,
                "detector_events": shots[i].tolist(),
                "observable_flips": obs_flips,
            }
        )
    return frames


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="stabstream-cirq-to-qssf",
        description="Convert a Cirq simulation result JSON to QSSF.",
    )
    parser.add_argument("input", help="Path to Cirq result JSON file")
    parser.add_argument(
        "--key",
        default="ancilla",
        metavar="KEY",
        help="Measurement key for ancilla qubits (default: ancilla)",
    )
    parser.add_argument(
        "--observable-key",
        default=None,
        metavar="KEY",
        help="Optional measurement key for logical observable results",
    )
    parser.add_argument(
        "--round",
        type=int,
        default=0,
        dest="round_index",
        metavar="N",
        help="Round index to embed in all frames (default: 0)",
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

    frames = _load_cirq_json(raw, args.key, args.observable_key, args.round_index)

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
