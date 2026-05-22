"""
NeuralDecoder — framework-agnostic ML decoder adapter for stabstream.

Wraps any trained neural network that maps syndrome detector events to
observable flip predictions. Supports PyTorch (TorchScript and eager),
TensorFlow/Keras, ONNX Runtime, and plain NumPy callables.

This decoder targets **offline threshold analysis and research**, not
real-time operation. Neural decoders typically run in milliseconds per
window, far above the <1 µs budget required for live QEC hardware.

Prerequisites (install whichever framework your model uses)
-----------------------------------------------------------
    pip install torch          # PyTorch
    pip install tensorflow     # TensorFlow / Keras
    pip install onnxruntime    # ONNX Runtime

Usage
-----
    from stabstream.decoders import NeuralDecoder
    from stabstream import LogicalErrorAccumulator
    import numpy as np

    # Any callable that maps (rounds, ancillas) → float array of length observable_count
    def my_model(x: np.ndarray) -> np.ndarray:
        return np.zeros(1)  # placeholder

    decoder = NeuralDecoder(my_model, observable_count=1)
    result = decoder.decode(window.to_numpy_matrix())

    # Or load a saved model:
    decoder = NeuralDecoder.from_torch("decoder.pt", observable_count=1)
    decoder = NeuralDecoder.from_onnx("decoder.onnx", observable_count=1)

    # Batch inference (GPU-efficient):
    results = decoder.decode_batch(np.stack([m1, m2, m3]))

    # Measure p_L:
    acc = LogicalErrorAccumulator(observable_count=1)
    for frame in stream:
        window.push(frame)
        if window.is_full():
            result = decoder.decode(window.to_numpy_matrix())
            acc.record(result, ground_truth=frame.observable_flips or 0)
    print(f"p_L = {acc.mean_logical_error_rate():.4e}")
"""

from __future__ import annotations

from enum import Enum, auto
from typing import Any, Callable

import numpy as np


class _Backend(Enum):
    PLAIN = auto()       # plain callable: np.ndarray → np.ndarray
    TORCH = auto()       # torch.nn.Module or torch.jit.ScriptModule
    TENSORFLOW = auto()  # tf.keras.Model
    ONNX = auto()        # onnxruntime.InferenceSession


def _detect_backend(model: Any) -> _Backend:
    """Identify the framework of *model* by inspecting its type."""
    try:
        import torch
        if isinstance(model, torch.nn.Module):
            return _Backend.TORCH
    except ImportError:
        pass

    try:
        import tensorflow as tf
        if isinstance(model, tf.Module):
            return _Backend.TENSORFLOW
    except ImportError:
        pass

    try:
        import onnxruntime
        if isinstance(model, onnxruntime.InferenceSession):
            return _Backend.ONNX
    except ImportError:
        pass

    # Fall back to treating any callable as a plain NumPy function.
    if callable(model):
        return _Backend.PLAIN

    raise TypeError(
        f"Cannot detect ML framework for model of type {type(model).__name__}. "
        "Pass a torch.nn.Module, tf.keras.Model, onnxruntime.InferenceSession, "
        "or any callable that maps np.ndarray → np.ndarray."
    )


def _sigmoid(x: np.ndarray) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-x.astype(np.float64)))


def _logits_to_result(logits: np.ndarray, threshold: float):
    """Convert a 1-D float logits/probabilities array to (observable_flips, confidence)."""
    from stabstream import DecoderResult

    probs = _sigmoid(logits) if logits.min() < 0 or logits.max() > 1 else logits
    bits = (probs > threshold).astype(int)
    obs_bitmask = int(sum(int(b) << i for i, b in enumerate(bits)))
    confidence = float(np.prod(np.where(bits, probs, 1.0 - probs)))
    return DecoderResult(obs_bitmask, confidence)


class NeuralDecoder:
    """
    Framework-agnostic neural network QEC decoder adapter.

    Accepts any model callable — PyTorch, TensorFlow/Keras, ONNX Runtime,
    or a plain NumPy function — and wraps it to match the stabstream
    ``stabstream.DecoderResult`` interface used by
    ``LogicalErrorAccumulator``.

    The model must map an input array of shape
    ``(rounds, ancillas)`` or ``(ancillas,)`` to a 1-D float array of
    length ``observable_count`` containing logits or probabilities. Logits
    (values outside [0, 1]) are passed through sigmoid automatically.

    Parameters
    ----------
    model:
        Trained model or callable.  Framework is auto-detected.
    observable_count:
        Number of logical observables the model predicts.
    threshold:
        Decision threshold applied to sigmoid(logit) for bit prediction.
        Default 0.5. Higher values → more conservative (fewer predicted
        flips).
    flatten_input:
        If True, flatten the ``(rounds, ancillas)`` matrix to a 1-D
        vector before passing to the model.  Default True.  Set False
        for models that expect 2-D or 3-D input (e.g. CNNs, transformers).
    """

    def __init__(
        self,
        model: Any,
        *,
        observable_count: int = 1,
        threshold: float = 0.5,
        flatten_input: bool = True,
    ) -> None:
        self._model = model
        self._observable_count = observable_count
        self._threshold = threshold
        self._flatten = flatten_input
        self._backend = _detect_backend(model)

    # ------------------------------------------------------------------
    # Convenience constructors
    # ------------------------------------------------------------------

    @classmethod
    def from_torch(cls, path: str, **kwargs) -> "NeuralDecoder":
        """
        Load a TorchScript model from *path* (``torch.jit.load``).

        Parameters
        ----------
        path:
            Path to a ``.pt`` / ``.torchscript`` file saved with
            ``torch.jit.save`` or ``model.save()``.
        **kwargs:
            Forwarded to ``NeuralDecoder.__init__`` (e.g.
            ``observable_count``, ``threshold``).
        """
        try:
            import torch
        except ImportError as exc:
            raise ImportError(
                "PyTorch is not installed. Install with:  pip install torch"
            ) from exc
        model = torch.jit.load(path, map_location="cpu")
        model.eval()
        return cls(model, **kwargs)

    @classmethod
    def from_onnx(cls, path: str, **kwargs) -> "NeuralDecoder":
        """
        Load an ONNX model from *path* (``onnxruntime.InferenceSession``).

        Parameters
        ----------
        path:
            Path to a ``.onnx`` file.
        **kwargs:
            Forwarded to ``NeuralDecoder.__init__``.
        """
        try:
            import onnxruntime
        except ImportError as exc:
            raise ImportError(
                "ONNX Runtime is not installed. "
                "Install with:  pip install onnxruntime"
            ) from exc
        session = onnxruntime.InferenceSession(path)
        return cls(session, **kwargs)

    # ------------------------------------------------------------------
    # Single-window decode
    # ------------------------------------------------------------------

    def decode(self, matrix: np.ndarray):
        """
        Decode a single syndrome window.

        Parameters
        ----------
        matrix:
            Shape ``(rounds, ancillas)`` or ``(ancillas,)``, dtype bool or
            float.

        Returns
        -------
        stabstream.DecoderResult
            Compatible with ``LogicalErrorAccumulator.record()``.
        """
        x = matrix.astype(np.float32)
        if self._flatten:
            x = x.ravel()

        logits = self._forward_single(x)
        return _logits_to_result(np.asarray(logits, dtype=np.float64), self._threshold)

    # ------------------------------------------------------------------
    # Batch decode
    # ------------------------------------------------------------------

    def decode_batch(self, matrices: np.ndarray) -> list:
        """
        Decode a batch of syndrome windows in one forward pass.

        Parameters
        ----------
        matrices:
            Shape ``(shots, rounds, ancillas)`` or ``(shots, ancillas)``,
            dtype bool or float.

        Returns
        -------
        list[stabstream.DecoderResult]
        """
        shots = matrices.shape[0]
        x = matrices.astype(np.float32)
        if self._flatten:
            x = x.reshape(shots, -1)

        logits_batch = self._forward_batch(x)
        return [
            _logits_to_result(np.asarray(logits_batch[i], dtype=np.float64), self._threshold)
            for i in range(shots)
        ]

    # ------------------------------------------------------------------
    # Internal dispatch per backend
    # ------------------------------------------------------------------

    def _forward_single(self, x: np.ndarray) -> np.ndarray:
        if self._backend == _Backend.TORCH:
            return self._forward_torch_single(x)
        if self._backend == _Backend.TENSORFLOW:
            return self._forward_tf_single(x)
        if self._backend == _Backend.ONNX:
            return self._forward_onnx_batch(x[np.newaxis])[0]
        # Plain callable
        result = self._model(x)
        return np.atleast_1d(np.asarray(result, dtype=np.float32))

    def _forward_batch(self, x: np.ndarray) -> np.ndarray:
        if self._backend == _Backend.TORCH:
            return self._forward_torch_batch(x)
        if self._backend == _Backend.TENSORFLOW:
            return self._forward_tf_batch(x)
        if self._backend == _Backend.ONNX:
            return self._forward_onnx_batch(x)
        # Plain callable: iterate (no vectorized path guaranteed)
        return np.stack([
            np.atleast_1d(np.asarray(self._model(x[i]), dtype=np.float32))
            for i in range(x.shape[0])
        ])

    def _forward_torch_single(self, x: np.ndarray) -> np.ndarray:
        import torch
        with torch.no_grad():
            t = torch.from_numpy(x).unsqueeze(0)  # (1, features)
            out = self._model(t)
            return out.squeeze(0).cpu().numpy().astype(np.float32)

    def _forward_torch_batch(self, x: np.ndarray) -> np.ndarray:
        import torch
        with torch.no_grad():
            t = torch.from_numpy(x)
            out = self._model(t)
            return out.cpu().numpy().astype(np.float32)

    def _forward_tf_single(self, x: np.ndarray) -> np.ndarray:
        import tensorflow as tf
        out = self._model(tf.constant(x[np.newaxis], dtype=tf.float32), training=False)
        return out.numpy().squeeze(0).astype(np.float32)

    def _forward_tf_batch(self, x: np.ndarray) -> np.ndarray:
        import tensorflow as tf
        out = self._model(tf.constant(x, dtype=tf.float32), training=False)
        return out.numpy().astype(np.float32)

    def _forward_onnx_batch(self, x: np.ndarray) -> np.ndarray:
        input_name = self._model.get_inputs()[0].name
        result = self._model.run(None, {input_name: x})
        return np.asarray(result[0], dtype=np.float32)

    # ------------------------------------------------------------------

    def __repr__(self) -> str:
        return (
            f"NeuralDecoder(backend={self._backend.name}, "
            f"observable_count={self._observable_count}, "
            f"threshold={self._threshold})"
        )
