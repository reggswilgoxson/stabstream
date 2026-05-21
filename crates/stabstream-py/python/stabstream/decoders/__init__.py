"""
stabstream.decoders — pluggable QEC decoder adapters.

Available decoders
------------------
PyMatchingDecoder   MWPM via PyMatching v2 (pip install pymatching)
ChromobiusDecoder   Color-code MWPM via Chromobius (pip install chromobius)
TesseractDecoder    LDPC MLE via Google Tesseract (requires tesseract binary)

All adapters expose the same interface::

    decoder.decode(matrix)         # single window → DecoderResult
    decoder.decode_batch(matrices) # batch → list[DecoderResult]

Each result is a ``stabstream.DecoderResult`` compatible with
``LogicalErrorAccumulator.record()``.  The underlying dict form
(``{"observable_flips": int, "confidence": float}``) is also retained
for backward compatibility.

Integration example::

    from stabstream import DetectorErrorModel, LogicalErrorAccumulator
    from stabstream.decoders import PyMatchingDecoder

    dem = DetectorErrorModel.from_file("circuit.dem")
    decoder = PyMatchingDecoder(dem)
    acc = LogicalErrorAccumulator(observable_count=dem.observable_count)

    for frame in stream:
        window.push(frame)
        if window.is_full():
            result = decoder.decode(window.to_numpy_matrix())
            acc.record(result, ground_truth=frame.observable_flips or 0)

    print(f"p_L = {acc.mean_logical_error_rate():.4e}")
"""

from stabstream.decoders.pymatching_decoder import PyMatchingDecoder
from stabstream.decoders.chromobius import ChromobiusDecoder
from stabstream.decoders.tesseract import TesseractDecoder

__all__ = ["PyMatchingDecoder", "ChromobiusDecoder", "TesseractDecoder"]
