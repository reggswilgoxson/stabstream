"""
Example: load a QSSF file and iterate over syndrome frames.

Usage:
    maturin develop  # from crates/stabstream-py/
    python python/examples/parse_frames.py <path-to-file.qssf>

Or point at a live TCP source:
    python python/examples/parse_frames.py tcp://localhost:9000
"""

import sys

import numpy as np

from stabstream import SyndromeWindow, load_qssf
from stabstream.io import load_qssf_batch, read_qssf

CODE_TYPE_NAMES = {
    0x01: "SurfaceCode",
    0x02: "HoneycombCode",
    0x03: "ColorCode",
    0x04: "RepetitionCode",
    0x05: "ToricCode",
    0x06: "BivariateBicycle",
    0x07: "HypergraphProduct",
    0x08: "FiberBundle",
    0xFF: "Custom",
}

WINDOW_DEPTH = 5


def demo_load_qssf(source: str) -> None:
    """Iterate with load_qssf() and push into a SyndromeWindow."""
    total_frames = 0
    total_events = 0
    window: SyndromeWindow | None = None

    print(f"Opening source: {source}")
    print("-" * 60)

    for frame in load_qssf(source):
        total_frames += 1
        total_events += frame.detector_event_count

        # Lazily create the window once we know ancilla_count
        if window is None:
            window = SyndromeWindow(frame.ancilla_count, WINDOW_DEPTH)

        # NumPy views of this frame's detector data
        det_events: np.ndarray = frame.to_numpy_detector_events()
        meas: np.ndarray = frame.to_numpy_meas_results()

        code_name = CODE_TYPE_NAMES.get(frame.code_type, "Unknown")
        fire_pct = (
            frame.detector_event_count / frame.ancilla_count * 100.0
            if frame.ancilla_count > 0
            else 0.0
        )

        if total_frames <= 5 or total_frames % 1000 == 0:
            print(
                f"frame_id={frame.frame_id:>8}  round={frame.round:>6}  "
                f"code={code_name:<18}  "
                f"ancilla={frame.ancilla_count:>4}  "
                f"events={frame.detector_event_count:>4} ({fire_pct:>5.1f}%)  "
                f"det.shape={det_events.shape}  meas.dtype={meas.dtype}"
            )

        if total_frames == 1:
            result = frame.null_decode()
            d = frame.to_dict()
            print(
                f"  to_dict() keys: {list(d.keys())}\n"
                f"  null_decode: {len(result.corrections)} corrections, "
                f"confidence={result.confidence:.2f}"
            )
            if frame.observable_flips is not None:
                print(f"  ground_truth observable_flips: {frame.observable_flips:#b}")

        # SyndromeWindow.push() from a SyndromeFrame
        window.push(frame)
        if window.is_full() and total_frames == WINDOW_DEPTH:
            mat: np.ndarray = window.to_numpy_matrix()
            print(
                f"\nWindow full — detector matrix: {mat.shape}  dtype={mat.dtype}"
            )
            active = window.active_detectors()
            print(f"  active detector indices: {active[:10]}{'...' if len(active) > 10 else ''}")

    print("-" * 60)
    print(f"Total frames: {total_frames}  Total events: {total_events}")
    if total_frames > 0:
        print(f"Mean fire pct: {total_events / total_frames:.2f}%")


def demo_pandas(source: str) -> None:
    """Load the full file into a pandas DataFrame via read_qssf()."""
    try:
        import pandas as pd
    except ImportError:
        print("Skipping pandas demo (pip install pandas)")
        return

    print("\n--- pandas demo ---")
    df = read_qssf(source, columns=["frame_id", "round", "ancilla_count", "detector_event_count"])
    print(df.head())
    print(f"DataFrame shape: {df.shape}")


def demo_batched(source: str) -> None:
    """Consume frames in batches via load_qssf_batch()."""
    print("\n--- batched NumPy demo ---")
    total_shots = 0
    for batch in load_qssf_batch(source, batch_size=64):
        # batch.shape == (n, ancilla_count), dtype=bool
        total_shots += batch.shape[0]
        if total_shots <= 64:
            print(f"batch.shape={batch.shape}  dtype={batch.dtype}")
    print(f"Total shots consumed in batches: {total_shots}")


def main() -> None:
    source = sys.argv[1] if len(sys.argv) > 1 else "recording.qssf"
    demo_load_qssf(source)
    demo_pandas(source)
    demo_batched(source)


if __name__ == "__main__":
    main()
