"""
Type stubs for the stabstream Python extension module.

Build with:  maturin develop
             (from crates/stabstream-py/)
"""
from __future__ import annotations

__version__: str

class CodeType:
    """QEC code family discriminant."""

    SURFACE_CODE: CodeType
    HONEYCOMB_CODE: CodeType
    COLOR_CODE: CodeType
    REPETITION_CODE: CodeType
    TORIC_CODE: CodeType
    CUSTOM: CodeType

    def __repr__(self) -> str: ...

class LogicalCorrection:
    """A single logical-qubit Pauli correction."""

    logical_id: int
    """Index of the logical qubit."""
    pauli: str
    """Pauli operator: 'I', 'X', 'Y', or 'Z'."""

    def __repr__(self) -> str: ...

class DecoderResult:
    """Output of a decoder for one syndrome frame."""

    corrections: list[LogicalCorrection]
    """Recommended logical corrections (may be empty)."""
    confidence: float
    """Decoder confidence in [0.0, 1.0]."""

    def __repr__(self) -> str: ...

class SyndromeFrame:
    """
    A single parsed QEC syndrome frame.

    All fields are read-only properties populated from the QSSF binary stream.
    """

    @property
    def frame_id(self) -> int:
        """Monotonically increasing frame counter."""
        ...

    @property
    def round(self) -> int:
        """Measurement round index within an experiment."""
        ...

    @property
    def timestamp_ns(self) -> int:
        """Hardware wall-clock nanoseconds (epoch-relative)."""
        ...

    @property
    def qubit_count(self) -> int:
        """Number of data qubits this round."""
        ...

    @property
    def ancilla_count(self) -> int:
        """Number of ancilla qubits measured."""
        ...

    @property
    def detector_event_count(self) -> int:
        """Number of detector events that fired this round."""
        ...

    @property
    def code_type(self) -> int:
        """Raw CodeType discriminant byte."""
        ...

    @property
    def distance(self) -> int:
        """Code distance d."""
        ...

    def meas_results(self) -> bytes:
        """
        Raw ancilla measurement outcomes.

        Each byte is 0x01 (+1 outcome) or 0xFF (-1 outcome).
        """
        ...

    def null_decode(self) -> DecoderResult:
        """Apply the NullDecoder (returns empty corrections, confidence=1.0)."""
        ...

    def __repr__(self) -> str: ...

class StabstreamStream:
    """
    Async QSSF stream reader exposed as a Python iterator and context manager.

    Example::

        with StabstreamStream("path/to/recording.qssf") as stream:
            for frame in stream:
                print(frame.frame_id, frame.detector_event_count)

    TCP sources::

        with StabstreamStream("tcp://localhost:9000") as stream:
            for frame in stream:
                ...
    """

    def __init__(self, source: str) -> None:
        """
        Open a QSSF source.

        Parameters
        ----------
        source:
            Either a filesystem path (``/path/to/file.qssf``) or a TCP URI
            (``tcp://host:port``).
        """
        ...

    def __iter__(self) -> StabstreamStream: ...
    def __next__(self) -> SyndromeFrame: ...
    def close(self) -> None: ...
    def __enter__(self) -> StabstreamStream: ...
    def __exit__(self, exc_type: object, exc_val: object, tb: object) -> bool: ...
