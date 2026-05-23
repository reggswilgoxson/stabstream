"""
Type stubs for stabstream.

The Rust extension module is ``stabstream._stabstream``; this package
re-exports everything from it plus pure-Python utilities.
"""

from __future__ import annotations

from pathlib import Path
from typing import AsyncIterator, Generator, Iterator, Optional, Union

import numpy as np
import numpy.typing as npt

__version__: str

# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------

class StabstreamError(Exception):
    """
    Raised for stabstream-specific errors.

    Common causes:
    - Accessing ``frame.observable_flips`` without configuring a decoder.
    - Checksum mismatch or malformed QSSF frame.
    - Out-of-order frame IDs.
    """
    ...

# ---------------------------------------------------------------------------
# CodeType
# ---------------------------------------------------------------------------

class CodeType:
    """QEC code family discriminant."""

    SURFACE_CODE: CodeType
    HONEYCOMB_CODE: CodeType
    COLOR_CODE: CodeType
    REPETITION_CODE: CodeType
    TORIC_CODE: CodeType
    BIVARIATE_BICYCLE: CodeType
    HYPERGRAPH_PRODUCT: CodeType
    FIBER_BUNDLE: CodeType
    CUSTOM: CodeType

    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# Decoder types
# ---------------------------------------------------------------------------

class LogicalCorrection:
    """A single logical-qubit Pauli correction."""

    logical_id: int
    pauli: str  # 'I' | 'X' | 'Y' | 'Z'
    def __repr__(self) -> str: ...

class DecoderResult:
    """Output of a decoder for one syndrome frame."""

    corrections: list[LogicalCorrection]
    confidence: float
    observable_flips: int
    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# SyndromeFrame
# ---------------------------------------------------------------------------

class SyndromeFrame:
    """
    A single parsed QEC syndrome frame from a QSSF stream.

    Read-only properties populated from the QSSF binary header and payload.
    """

    @property
    def frame_id(self) -> int: ...
    @property
    def round(self) -> int: ...
    @property
    def timestamp_ns(self) -> int: ...
    @property
    def qubit_count(self) -> int: ...
    @property
    def ancilla_count(self) -> int: ...
    @property
    def detector_event_count(self) -> int: ...
    @property
    def code_type(self) -> int: ...
    @property
    def distance(self) -> int: ...
    @property
    def observable_flips(self) -> int:
        """
        Decoded observable-flip bitmask.

        When a decoder is configured on the stream (via ``set_decoder`` or the
        ``decoder`` parameter to ``open``/``from_stim_circuit``), this returns
        the Union-Find decoder output.  For simulator frames without a decoder,
        it returns the ground-truth bitmask from QSSF tag 0x10.

        Raises ``StabstreamError`` if no decoder is configured and no
        ground-truth metadata is present (real hardware without a decoder).
        """
        ...

    def meas_results(self) -> bytes:
        """Raw ancilla measurement bytes (0x01 = +1, 0xFF = -1)."""
        ...

    def to_numpy_detector_events(self) -> npt.NDArray[np.bool_]:
        """Detector events as shape ``(ancilla_count,)`` bool array."""
        ...

    def to_numpy_meas_results(self) -> npt.NDArray[np.int8]:
        """Measurement results as shape ``(ancilla_count,)`` int8 array."""
        ...

    def to_dict(self) -> dict:
        """
        Serialise as a Python dict.

        Keys: ``frame_id``, ``round``, ``timestamp_ns``, ``qubit_count``,
        ``ancilla_count``, ``detector_event_count``, ``code_type``,
        ``distance``, ``detector_events`` (ndarray), ``observable_flips``
        (int or None when no decoder and no ground-truth metadata).

        Compatible with ``pd.DataFrame([f.to_dict() for f in stream])``.
        """
        ...

    def null_decode(self) -> DecoderResult:
        """Apply the NullDecoder (no corrections, confidence=1.0)."""
        ...

    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# SyndromeWindow
# ---------------------------------------------------------------------------

class SyndromeWindow:
    """
    Sliding multi-round syndrome window for spacetime-aware decoders.

    Holds up to ``window_depth`` rounds of syndrome data in a flat
    ``(rounds × ancilla_count)`` detector matrix.
    """

    def __init__(self, ancilla_count: int, window_depth: int) -> None: ...

    def push(self, frame: SyndromeFrame) -> None:
        """Push a ``SyndromeFrame``, evicting the oldest round if full."""
        ...

    def push_numpy(
        self,
        detector_events: npt.NDArray[np.bool_],
        frame_id: int = 0,
        round: int = 0,
    ) -> None:
        """
        Push detector events from a NumPy array without a SyndromeFrame.

        Use this with vendor adapters (IBM, Cirq) that yield raw arrays
        instead of SyndromeFrame objects.

        Parameters
        ----------
        detector_events:
            Shape ``(ancilla_count,)``, dtype bool.
        frame_id:
            Monotonic frame counter embedded in the window entry.
        round:
            Round index embedded in the window entry.
        """
        ...

    def to_numpy_matrix(self) -> npt.NDArray[np.bool_]:
        """
        Detector matrix as shape ``(rounds, ancilla_count)`` bool array.

        Row 0 is the oldest round; row ``len()-1`` is the newest.
        Returns shape ``(0, ancilla_count)`` when the window is empty.
        """
        ...

    def active_detectors(self) -> list[int]:
        """
        Flat indices of all fired detectors across all rounds.

        Node id for round ``r``, ancilla ``a`` = ``r * ancilla_count + a``.
        """
        ...

    def __len__(self) -> int: ...
    def is_full(self) -> bool: ...
    def is_empty(self) -> bool: ...
    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# StabstreamStream  (low-level Rust sync stream — prefer SyndromeStream)
# ---------------------------------------------------------------------------

class StabstreamStream:
    """
    Low-level sync QSSF stream.  Prefer ``SyndromeStream`` for new code.

    ``SyndromeStream`` wraps this class and adds async support, the
    ``set_decoder`` method, and the ``from_stim_circuit`` / ``open``
    convenience constructors.
    """

    def __init__(self, source: str) -> None:
        """
        Open a QSSF source.

        Parameters
        ----------
        source:
            Filesystem path (``.qssf``) or TCP URI ``tcp://host:port``.
        """
        ...

    def set_decoder(
        self,
        dem: Union[str, Path, object],
        window_depth: int = 0,
    ) -> None:
        """
        Configure the Union-Find decoder.

        Parameters
        ----------
        dem:
            File path, inline DEM text, or ``stim.DetectorErrorModel``.
        window_depth:
            Syndrome window depth.  0 = auto-infer (default).
        """
        ...

    def __iter__(self) -> StabstreamStream: ...
    def __next__(self) -> SyndromeFrame: ...
    def close(self) -> None: ...
    def __enter__(self) -> StabstreamStream: ...
    def __exit__(self, exc_type: object, exc_val: object, tb: object) -> bool: ...

# ---------------------------------------------------------------------------
# SyndromeStream  (async-capable high-level wrapper)
# ---------------------------------------------------------------------------

class SyndromeStream:
    """
    Dual-mode (sync + async) QSSF syndrome stream with integrated UF decoder.

    Supports both ``with``/``for`` (sync) and ``async with``/``async for``
    (async) protocols.

    Parameters
    ----------
    source:
        File path, ``tcp://host:port``, or ``shm://name``.
    decoder:
        Optional DEM for the Union-Find decoder.  Accepts a file path, inline
        DEM text string, or a ``stim.DetectorErrorModel`` object.
    window_depth:
        Syndrome window depth.  0 = auto-infer (default).
    queue_depth:
        Reserved for future buffered-producer mode.
    """

    def __init__(
        self,
        source: str,
        *,
        decoder: Union[str, Path, object, None] = None,
        window_depth: int = 0,
        queue_depth: int = 64,
    ) -> None: ...

    def set_decoder(
        self,
        dem: Union[str, Path, object],
        window_depth: int = 0,
    ) -> None:
        """
        Configure the Union-Find decoder.

        Parameters
        ----------
        dem:
            File path, inline DEM text, or ``stim.DetectorErrorModel``.
        window_depth:
            Override the auto-inferred syndrome window depth.
        """
        ...

    # ── sync ──────────────────────────────────────────────────────────────────
    def __enter__(self) -> SyndromeStream: ...
    def __exit__(self, exc_type: object, exc_val: object, tb: object) -> bool: ...
    def __iter__(self) -> Iterator[SyndromeFrame]: ...
    def __next__(self) -> SyndromeFrame: ...
    def close(self) -> None: ...

    # ── async ─────────────────────────────────────────────────────────────────
    async def __aenter__(self) -> SyndromeStream: ...
    async def __aexit__(self, exc_type: object, exc_val: object, tb: object) -> bool: ...
    def __aiter__(self) -> AsyncIterator[SyndromeFrame]: ...
    async def __anext__(self) -> SyndromeFrame: ...
    async def aclose(self) -> None: ...

    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# DetectorErrorModel
# ---------------------------------------------------------------------------

class DetectorErrorModel:
    """Parsed Stim Detector Error Model (.dem)."""

    @staticmethod
    def parse(text: str) -> DetectorErrorModel:
        """Parse DEM from a text string."""
        ...

    @staticmethod
    def from_file(path: str) -> DetectorErrorModel:
        """Load DEM from a file path."""
        ...

    @property
    def detector_count(self) -> int: ...
    @property
    def observable_count(self) -> int: ...
    @property
    def error_count(self) -> int: ...

    def to_pymatching(self) -> object:
        """
        Construct a ``pymatching.Matching`` graph from this DEM.

        Requires ``pip install pymatching``.
        Edge weights: ``-ln(p / (1-p))``.
        """
        ...

    def to_schema_json(self, name: str) -> str:
        """Serialise as a HardwareSchema-compatible JSON string."""
        ...

    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# SpacetimeGraph
# ---------------------------------------------------------------------------

class SpacetimeGraph:
    """Read-only spacetime syndrome graph built from a DEM."""

    @property
    def node_count(self) -> int: ...
    @property
    def edge_count(self) -> int: ...
    @property
    def boundary_node(self) -> int: ...
    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# LogicalErrorAccumulator
# ---------------------------------------------------------------------------

class LogicalErrorAccumulator:
    """
    Lock-free p_L accumulator for multi-threaded threshold simulation.

    Uses ``AtomicU64`` counters — safe to record from multiple threads.
    """

    def __init__(self, observable_count: int) -> None: ...

    def record(self, result: DecoderResult, ground_truth: int) -> None:
        """Record one decoder result against the ground-truth observable bitmask."""
        ...

    def logical_error_rate(self, observable: int) -> float:
        """Logical error rate for a specific observable index."""
        ...

    def mean_logical_error_rate(self) -> float:
        """Mean logical error rate across all observables."""
        ...

    def total_shots(self) -> int: ...
    def reset(self) -> None: ...
    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# Convenience constructors
# ---------------------------------------------------------------------------

def open(
    source: str,
    *,
    decoder: Union[str, Path, object, None] = None,
    window_depth: int = 0,
    queue_depth: int = 64,
) -> SyndromeStream:
    """
    Open a QSSF stream (sync + async).

    Parameters
    ----------
    source:
        File path, ``tcp://host:port``, or ``shm://name``.
    decoder:
        Optional DEM for the Union-Find decoder.  Accepts a file path, inline
        DEM text, or a ``stim.DetectorErrorModel`` object.
    window_depth:
        Syndrome window depth.  0 = auto-infer (default).
    queue_depth:
        Reserved for future buffered-producer mode.
    """
    ...

def from_stim_circuit(
    source: str,
    circuit: object,
    *,
    window_depth: int = 0,
    queue_depth: int = 64,
) -> SyndromeStream:
    """
    Open a QSSF stream with the decoder auto-configured from a Stim circuit.

    Calls ``circuit.detector_error_model(decompose_errors=True)`` internally.

    Parameters
    ----------
    source:
        File path or ``tcp://host:port``.
    circuit:
        A ``stim.Circuit`` object.
    window_depth:
        Override the auto-inferred syndrome window depth.

    Examples
    --------
    ::

        import stim, stabstream

        circuit = stim.Circuit.from_file("surface_d5.stim")

        async with stabstream.from_stim_circuit("tcp://fpga:9000", circuit) as stream:
            async for frame in stream:
                await apply_correction(frame.observable_flips)
    """
    ...

def from_stim_dem(
    source: str,
    dem: object,
    *,
    window_depth: int = 0,
    queue_depth: int = 64,
) -> SyndromeStream:
    """
    Open a QSSF stream with the decoder configured from a Stim DEM object.

    Parameters
    ----------
    source:
        File path or ``tcp://host:port``.
    dem:
        A ``stim.DetectorErrorModel`` object or DEM text string.
    window_depth:
        Override the auto-inferred syndrome window depth.
    """
    ...

# ---------------------------------------------------------------------------
# Pure-Python utilities (from stabstream.io)
# ---------------------------------------------------------------------------

def load_qssf(path: str) -> Iterator[SyndromeFrame]:
    """
    Generator of SyndromeFrame objects from a QSSF file or TCP URI.

    ::

        for frame in load_qssf("data.qssf"):
            arr = frame.to_numpy_detector_events()
    """
    ...

def read_qssf(
    path: str,
    *,
    columns: Optional[list[str]] = None,
) -> object:  # pandas.DataFrame
    """
    Load a QSSF file into a pandas DataFrame (requires ``pip install pandas``).

    Each row is one frame. ``detector_events`` column holds a NumPy bool array.
    """
    ...

def load_qssf_windows(
    path: str,
    window_depth: int,
    batch_size: int = 256,
    *,
    with_labels: bool = False,
) -> Generator[
    Union[
        npt.NDArray[np.bool_],
        tuple[npt.NDArray[np.bool_], npt.NDArray[np.uint64]],
    ],
    None,
    None,
]:
    """
    Yield batches of multi-round syndrome windows for ML training/inference.

    Parameters
    ----------
    path:
        Filesystem path or TCP URI.
    window_depth:
        Number of rounds per window.
    batch_size:
        Windows per batch.
    with_labels:
        If True, yield ``(X, y)`` where ``y`` is a ``(batch_size,)``
        uint64 observable-flip bitmask from QSSF tag 0x10.

    Yields
    ------
    X : ndarray, shape ``(n, window_depth, ancilla_count)``, dtype bool
    y : ndarray, shape ``(n,)``, dtype uint64 — only when with_labels=True
    """
    ...

def load_dataset(path: str) -> tuple[npt.NDArray[np.bool_], npt.NDArray[np.uint64]]:
    """
    Load an ML training dataset written by ``stabstream-convert dem-to-dataset``.

    Returns
    -------
    X : ndarray, shape ``(shots, detector_count)``, dtype bool
    y : ndarray, shape ``(shots,)``, dtype uint64 (observable flip bitmasks)
    """
    ...
