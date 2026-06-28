"""
stabstream.replay — deterministic QSSF replay with integrated decoding.

Distinct from realtime_stream (which throttles by wall-clock time).
ReplayStream iterates as fast as the decoder allows and is designed for
reproducibility checks and offline analysis.
"""

from __future__ import annotations

from stabstream.io import load_qssf
from stabstream import SyndromeWindow, LogicalErrorAccumulator


class ReplayStream:
    """
    Replay a QSSF recording through a decoder, yielding (frame, result) pairs.

    Uses non-overlapping windows of depth ``window_depth``. Each full window
    is decoded independently. This matches stabstream's current SyndromeWindow
    semantics — production real-time decoders use causal sliding windows with
    one-round overlap to avoid boundary artifacts (see GitHub issue #2).

    Parameters
    ----------
    qssf_path : str
        Path to a QSSF file written by simulate_circuit_to_qssf or the CLI.
    decoder :
        Any object with a ``decode(matrix: np.ndarray) -> DecoderResult`` method.
        Both UnionFindDecoder and NeuralDecoder satisfy this interface.
    window_depth : int
        Number of QSSF frames per decode window. Use 1 when each frame already
        encodes all rounds of a complete shot (default from simulate_circuit_to_qssf).
    observable_count : int
        Number of logical observables tracked by LogicalErrorAccumulator.
    """

    def __init__(
        self,
        qssf_path: str,
        decoder,
        *,
        window_depth: int = 1,
        observable_count: int = 1,
    ) -> None:
        self.path = qssf_path
        self.decoder = decoder
        self.window_depth = window_depth
        self._acc = LogicalErrorAccumulator(observable_count=observable_count)

    def __iter__(self):
        window: SyndromeWindow | None = None
        for frame in load_qssf(self.path):
            if window is None:
                window = SyndromeWindow(
                    ancilla_count=frame.ancilla_count,
                    window_depth=self.window_depth,
                )
            window.push(frame)
            if window.is_full():
                matrix = window.to_numpy_matrix()
                result = self.decoder.decode(matrix)
                gt = frame.observable_flips if frame.observable_flips is not None else 0
                self._acc.record(result, gt)
                yield frame, result
                window = SyndromeWindow(
                    ancilla_count=frame.ancilla_count,
                    window_depth=self.window_depth,
                )

    def summary(self) -> dict:
        """Return aggregate statistics after iterating the stream."""
        return {
            "total_shots": self._acc.total_shots(),
            "mean_logical_error_rate": self._acc.mean_logical_error_rate(),
        }
