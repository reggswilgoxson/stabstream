"""
stabstream.decoders — pluggable QEC decoder adapters.

Available decoders
------------------
PyMatchingDecoder   MWPM via PyMatching v2 (pip install pymatching)
ChromobiusDecoder   Color-code MWPM via Chromobius (pip install chromobius)
TesseractDecoder    LDPC MLE via Google Tesseract (requires tesseract binary)

All adapters expose the same interface::

    decoder.decode(matrix)         # single window → dict
    decoder.decode_batch(matrices) # batch → list[dict]

Each result dict has keys:
    "observable_flips": int   # bitmask of predicted logical flips
    "confidence": float       # 1.0 for hard-decision decoders
"""

from stabstream.decoders.pymatching_decoder import PyMatchingDecoder
from stabstream.decoders.chromobius import ChromobiusDecoder
from stabstream.decoders.tesseract import TesseractDecoder

__all__ = ["PyMatchingDecoder", "ChromobiusDecoder", "TesseractDecoder"]
