"""
Chromobius decoder adapter (2D color codes only).

Chromobius is a pure-Python package that wraps PyMatching with color-code
specific decomposition. It only supports triangular 2D color codes (and
equivalent topological codes).

Prerequisites
-------------
    pip install chromobius stim

Usage
-----
    from stabstream.decoders import ChromobiusDecoder
    import numpy as np

    # Build from a Stim circuit file
    decoder = ChromobiusDecoder(circuit_file="color_code.stim")

    result = decoder.decode(detector_events)
    print(result["observable_flips"])
"""

from __future__ import annotations

import numpy as np

try:
    import chromobius as _chromobius

    _AVAILABLE = True
except ImportError:
    _AVAILABLE = False


def _require_chromobius() -> None:
    if not _AVAILABLE:
        raise ImportError(
            "Chromobius is not installed. "
            "Install it with:  pip install chromobius stim"
        )


class ChromobiusDecoder:
    """
    Decoder adapter wrapping Chromobius for 2D color codes.

    Only supports color codes derived from a Stim circuit (triangular lattice).
    For surface or repetition codes, use ``PyMatchingDecoder`` instead.

    Parameters
    ----------
    circuit_file : str, optional
        Path to a Stim ``.stim`` circuit file.
    circuit_text : str, optional
        Raw Stim circuit text.

    Exactly one of ``circuit_file`` or ``circuit_text`` must be provided.
    """

    def __init__(
        self,
        circuit_file: str | None = None,
        circuit_text: str | None = None,
    ) -> None:
        _require_chromobius()
        try:
            import stim
        except ImportError as exc:
            raise ImportError(
                "stim is required by Chromobius. "
                "Install it with:  pip install stim"
            ) from exc

        if circuit_file is not None:
            circuit = stim.Circuit.from_file(circuit_file)
        elif circuit_text is not None:
            circuit = stim.Circuit(circuit_text)
        else:
            raise ValueError("Provide either circuit_file or circuit_text.")

        self._decoder = _chromobius.compile_decoder_for_circuit(circuit)

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
        flat = matrix.ravel().astype(np.bool_)
        prediction = self._decoder.decode(flat)
        obs_bitmask = int(sum(int(b) << i for i, b in enumerate(prediction)))
        return {"observable_flips": obs_bitmask, "confidence": 1.0}

    def decode_batch(self, matrices: np.ndarray) -> list[dict]:
        """
        Decode a batch of syndrome windows.

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
        shots = matrices.shape[0]
        flat = matrices.reshape(shots, -1).astype(np.bool_)
        return [self.decode(flat[i]) for i in range(shots)]
