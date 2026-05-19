"""
PyMatching v2 MWPM decoder adapter.

Prerequisites
-------------
    pip install pymatching

Usage
-----
    from stabstream import DetectorErrorModel
    from stabstream.decoders import PyMatchingDecoder
    import numpy as np

    dem = DetectorErrorModel.from_file("circuit.dem")
    decoder = PyMatchingDecoder(dem)

    # Single window (shape (rounds, ancillas) or flat)
    result = decoder.decode(window.to_numpy_matrix())
    print(result["observable_flips"])

    # Batch (shape (shots, ancillas) or (shots, rounds, ancillas))
    results = decoder.decode_batch(np.stack([m1, m2, m3]))
"""

from __future__ import annotations

import numpy as np

try:
    import pymatching

    _AVAILABLE = True
except ImportError:
    _AVAILABLE = False


def _require_pymatching() -> None:
    if not _AVAILABLE:
        raise ImportError(
            "PyMatching is not installed. "
            "Install it with:  pip install pymatching"
        )


def _prediction_to_bitmask(prediction) -> int:
    """Convert a pymatching prediction array to an integer bitmask."""
    return int(sum(int(b) << i for i, b in enumerate(prediction)))


class PyMatchingDecoder:
    """
    MWPM decoder backed by PyMatching v2.

    Achieves optimal logical error rates for surface and repetition codes.
    Slower than Union-Find for large codes but gives a tight lower bound on
    achievable p_L. Supports batch decoding via ``decode_batch`` which uses
    PyMatching's vectorised C++ core.

    Parameters
    ----------
    dem : stabstream.DetectorErrorModel
        Detector error model.  ``dem.to_pymatching()`` is called once during
        construction to build the weighted graph; subsequent ``decode`` calls
        are allocation-free in the matching core.
    """

    def __init__(self, dem) -> None:
        _require_pymatching()
        self._matching: pymatching.Matching = dem.to_pymatching()
        self._observable_count: int = dem.observable_count

    def decode(self, matrix: np.ndarray) -> dict:
        """
        Decode a single syndrome window.

        Parameters
        ----------
        matrix : np.ndarray
            Shape ``(rounds, ancillas)`` or ``(ancillas,)``, dtype bool or uint8.

        Returns
        -------
        dict
            ``{"observable_flips": int, "confidence": float}``
        """
        flat = matrix.ravel().astype(np.uint8)
        prediction = self._matching.decode(flat)
        return {"observable_flips": _prediction_to_bitmask(prediction), "confidence": 1.0}

    def decode_batch(self, matrices: np.ndarray) -> list[dict]:
        """
        Decode a batch of syndrome windows using PyMatching's vectorised path.

        Parameters
        ----------
        matrices : np.ndarray
            Shape ``(shots, ancillas)`` or ``(shots, rounds, ancillas)``,
            dtype bool or uint8.

        Returns
        -------
        list[dict]
            One dict per shot: ``{"observable_flips": int, "confidence": float}``.
        """
        flat = matrices.reshape(matrices.shape[0], -1).astype(np.uint8)
        predictions = self._matching.decode_batch(flat)
        return [
            {"observable_flips": _prediction_to_bitmask(pred), "confidence": 1.0}
            for pred in predictions
        ]
