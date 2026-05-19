"""
Example: load a .qssf file and iterate over syndrome frames.

Usage:
    maturin develop  # from crates/stabstream-py/
    python python/examples/parse_frames.py <path-to-file.qssf>

Or point at a live TCP source:
    python python/examples/parse_frames.py tcp://localhost:9000
"""

import sys

import numpy as np

from stabstream import CodeType, StabstreamStream, SyndromeWindow

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


def main() -> None:
    source = sys.argv[1] if len(sys.argv) > 1 else "recording.qssf"

    total_frames = 0
    total_events = 0
    window = None

    print(f"Opening source: {source}")
    print("-" * 60)

    with StabstreamStream(source) as stream:
        for frame in stream:
            total_frames += 1
            total_events += frame.detector_event_count

            # Lazily create the window once we know ancilla_count
            if window is None:
                window = SyndromeWindow(frame.ancilla_count, WINDOW_DEPTH)

            # NumPy detector events for this frame (shape: ancilla_count,)
            det_events: np.ndarray = frame.to_numpy_detector_events()
            # NumPy measurement results (shape: ancilla_count,)
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
                    f"det_events.shape={det_events.shape}  "
                    f"meas.dtype={meas.dtype}"
                )

            # Demonstrate null_decode() + observable_flips
            if total_frames == 1:
                result = frame.null_decode()
                print(
                    f"  → null_decode: {len(result.corrections)} corrections, "
                    f"confidence={result.confidence:.2f}, "
                    f"observable_flips={result.observable_flips:#b}"
                )
                if frame.observable_flips is not None:
                    print(f"  → ground_truth observable_flips: {frame.observable_flips:#b}")

            # Push into the sliding window; demo matrix shape
            window.push(frame)
            if window.is_full() and total_frames == WINDOW_DEPTH:
                mat: np.ndarray = window.to_numpy_matrix()
                print(
                    f"\nSyndromeWindow filled — detector matrix shape: {mat.shape} "
                    f"(rounds × ancillas), dtype={mat.dtype}"
                )
                print(f"  active_detectors (flat indices): {window.active_detectors()[:10]}...")

    print("-" * 60)
    print(f"Total frames : {total_frames}")
    print(f"Total events : {total_events}")
    if total_frames > 0:
        print(f"Mean fire pct: {total_events / total_frames:.2f}% per frame")


if __name__ == "__main__":
    main()
