"""
Tesseract decoder adapter (Google QAI LDPC MLE decoder).

Tesseract is a maximum-likelihood decoder for qLDPC codes developed by
Google's Quantum AI team. It is distributed as a Python package with a C++
core. Unlike MWPM it uses belief propagation + OSD as a post-processor,
giving near-ML performance on Bivariate Bicycle and other qLDPC families.

Prerequisites
-------------
    pip install tesseract-decoder   # when publicly available

    If the package is not yet on PyPI, Tesseract can also be invoked via its
    CLI binary using ``TesseractDecoder(binary="path/to/tesseract")``.

Usage
-----
    from stabstream.decoders import TesseractDecoder
    import numpy as np

    # Python-package mode (preferred when pip-installed)
    decoder = TesseractDecoder(dem_file="bivariate_bicycle.dem")
    result = decoder.decode(detector_events)

    # CLI subprocess mode (fallback)
    decoder = TesseractDecoder(dem_file="bivariate_bicycle.dem",
                               binary="/usr/local/bin/tesseract")
    result = decoder.decode(detector_events)
"""

from __future__ import annotations

import json
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path

import numpy as np

try:
    import tesseract_decoder as _tesseract  # type: ignore[import]

    _PACKAGE_AVAILABLE = True
except ImportError:
    _PACKAGE_AVAILABLE = False


class TesseractDecoder:
    """
    Decoder adapter for the Tesseract LDPC MLE decoder (Google QAI).

    Falls back to a subprocess invocation of the Tesseract CLI binary when
    the Python package is not installed. Designed for Bivariate Bicycle and
    other qLDPC codes where MWPM is suboptimal.

    Parameters
    ----------
    dem_file : str
        Path to the detector error model (``.dem``) file.
    binary : str, optional
        Path to the ``tesseract`` binary.  If ``None`` and the Python package
        is not installed, the binary is located on ``$PATH``.
    observable_count : int, optional
        Number of logical observables.  Inferred from DEM if not supplied.
    """

    def __init__(
        self,
        dem_file: str,
        binary: str | None = None,
        observable_count: int | None = None,
    ) -> None:
        self._dem_file = str(dem_file)
        self._observable_count = observable_count

        if _PACKAGE_AVAILABLE:
            self._mode = "package"
            self._decoder = _tesseract.Decoder(self._dem_file)
        else:
            self._mode = "subprocess"
            self._binary = binary or shutil.which("tesseract")
            if self._binary is None:
                raise RuntimeError(
                    "Tesseract is not installed and 'tesseract' binary was not found on PATH.\n"
                    "Install the Python package (pip install tesseract-decoder) or provide "
                    "the path to the tesseract binary via the 'binary' argument."
                )

    def decode(self, matrix: np.ndarray) -> dict:
        """
        Decode a single syndrome window.

        Parameters
        ----------
        matrix : np.ndarray
            Shape ``(rounds, ancillas)`` or ``(ancillas,)``, dtype bool.

        Returns
        -------
        dict
            ``{"observable_flips": int, "confidence": float}``
        """
        flat = matrix.ravel().astype(np.bool_)

        if self._mode == "package":
            return self._decode_package(flat)
        return self._decode_subprocess(flat.reshape(1, -1))[0]

    def decode_batch(self, matrices: np.ndarray) -> list[dict]:
        """
        Decode a batch of syndrome windows.

        Parameters
        ----------
        matrices : np.ndarray
            Shape ``(shots, ancillas)`` or ``(shots, rounds, ancillas)``,
            dtype bool.

        Returns
        -------
        list[dict]
        """
        shots = matrices.shape[0]
        flat = matrices.reshape(shots, -1).astype(np.bool_)

        if self._mode == "package":
            return [self._decode_package(flat[i]) for i in range(shots)]
        return self._decode_subprocess(flat)

    # ------------------------------------------------------------------
    # Internal dispatch
    # ------------------------------------------------------------------

    def _decode_package(self, flat: np.ndarray) -> dict:
        result = self._decoder.decode(flat)
        obs = getattr(result, "observable_flips", None)
        if obs is None:
            # Fallback: result might be an array
            obs = int(sum(int(b) << i for i, b in enumerate(result)))
        else:
            obs = int(obs)
        return {"observable_flips": obs, "confidence": 1.0}

    def _decode_subprocess(self, flat: np.ndarray) -> list[dict]:
        """
        Invoke the Tesseract binary with detection events written as a
        bit-packed binary file, parse the JSON output.
        """
        shots, n_det = flat.shape

        with tempfile.TemporaryDirectory() as tmp:
            det_path = Path(tmp) / "detectors.b8"
            obs_path = Path(tmp) / "observables.json"

            # Write bit-packed detection events (Stim b8 format)
            bytes_per_shot = (n_det + 7) // 8
            buf = bytearray(shots * bytes_per_shot)
            for s in range(shots):
                for i, val in enumerate(flat[s]):
                    if val:
                        buf[s * bytes_per_shot + i // 8] |= 1 << (i % 8)
            det_path.write_bytes(bytes(buf))

            try:
                subprocess.run(
                    [
                        self._binary,
                        "decode",
                        "--dem", self._dem_file,
                        "--dets", str(det_path),
                        "--out", str(obs_path),
                        "--out-format", "json",
                    ],
                    check=True,
                    capture_output=True,
                    timeout=60,
                )
            except subprocess.CalledProcessError as exc:
                raise RuntimeError(
                    f"Tesseract subprocess failed:\n{exc.stderr.decode()}"
                ) from exc
            except FileNotFoundError as exc:
                raise RuntimeError(
                    f"Tesseract binary not found: {self._binary}"
                ) from exc

            raw = json.loads(obs_path.read_text())

        # raw is expected to be a list of observable flip lists
        results = []
        for obs_list in raw:
            obs_bitmask = int(sum(int(b) << i for i, b in enumerate(obs_list)))
            results.append({"observable_flips": obs_bitmask, "confidence": 1.0})
        return results
