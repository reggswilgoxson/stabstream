"""
Example: parse a Stim DEM and run PyMatching decoding on syndrome frames.

Prerequisites:
    pip install pymatching
    maturin develop  # from crates/stabstream-py/

Usage:
    python python/examples/pymatching_bridge.py <model.dem> <frames.qssf>

The script:
  1. Parses the DEM into stabstream's DetectorErrorModel.
  2. Builds a pymatching.Matching graph (edge weights = -ln(p/(1-p))).
  3. Streams syndrome frames and feeds detector events to the MWPM decoder.
  4. Accumulates logical error counts via LogicalErrorAccumulator.
  5. Prints per-observable and mean logical error rate.
"""

import sys

import numpy as np

from stabstream import (
    DetectorErrorModel,
    LogicalErrorAccumulator,
    StabstreamStream,
    SyndromeWindow,
)

WINDOW_DEPTH = 5


def main() -> None:
    if len(sys.argv) < 3:
        print(
            "Usage: pymatching_bridge.py <model.dem> <frames.qssf>",
            file=sys.stderr,
        )
        sys.exit(1)

    dem_path, qssf_path = sys.argv[1], sys.argv[2]

    # ------------------------------------------------------------------
    # 1. Parse DEM and build pymatching graph
    # ------------------------------------------------------------------
    print(f"Loading DEM: {dem_path}")
    dem = DetectorErrorModel.from_file(dem_path)
    print(f"  {dem}")

    try:
        matching = dem.to_pymatching()
    except ImportError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)

    print(f"  pymatching.Matching built ({dem.error_count} edges)")

    # ------------------------------------------------------------------
    # 2. Set up accumulator and sliding window
    # ------------------------------------------------------------------
    acc = LogicalErrorAccumulator(dem.observable_count)
    window: SyndromeWindow | None = None
    total_frames = 0

    # ------------------------------------------------------------------
    # 3. Stream frames and decode
    # ------------------------------------------------------------------
    with StabstreamStream(qssf_path) as stream:
        for frame in stream:
            total_frames += 1

            if window is None:
                window = SyndromeWindow(frame.ancilla_count, WINDOW_DEPTH)

            window.push(frame)

            if not window.is_full():
                continue

            # Detector matrix shape: (rounds, ancilla_count)
            mat: np.ndarray = window.to_numpy_matrix()
            # PyMatching expects a flat 1-D array for multi-round decoding
            # or 2-D (shots × detectors) for batch mode.
            active = np.flatnonzero(mat.ravel()).tolist()

            # Decode: returns a list of observable flip bits
            prediction = matching.decode(mat.ravel().astype(np.uint8))
            # Convert to bitmask
            obs_bitmask = int(sum(int(b) << i for i, b in enumerate(prediction)))

            # Ground truth from QSSF metadata (tag 0x10), if present
            ground_truth = frame.observable_flips or 0

            # PyDecoderResult-compatible object for the accumulator
            from stabstream import DecoderResult  # noqa: F401 — for type hint only

            # Build a minimal result object
            class _Result:
                observable_flips = obs_bitmask
                confidence = 1.0
                corrections = []

            acc.record(_Result(), ground_truth)

    # ------------------------------------------------------------------
    # 4. Report
    # ------------------------------------------------------------------
    print(f"\nShots processed : {acc.total_shots()}")
    for i in range(dem.observable_count):
        print(
            f"  Observable {i}: p_L = {acc.logical_error_rate(i):.4e}"
        )
    print(f"  Mean p_L      : {acc.mean_logical_error_rate():.4e}")


if __name__ == "__main__":
    main()
