"""
stabstream — high-performance QEC syndrome stream library.

The compiled Rust extension lives in `stabstream._stabstream`; this
`__init__.py` re-exports everything so that the public import path
``from stabstream import SyndromeFrame, ...`` keeps working unchanged.

Additional pure-Python utilities:

  stabstream.open(source, *, decoder, window_depth, queue_depth)
                                      → SyndromeStream (sync + async)
  stabstream.from_stim_circuit(source, circuit, ...)
                                      → SyndromeStream (decoder auto-configured)
  stabstream.from_stim_dem(source, dem, ...)
                                      → SyndromeStream
  stabstream.load_qssf(path)          → generator of SyndromeFrame
  stabstream.read_qssf(path)          → pandas DataFrame (requires pandas)
  stabstream.vendors.ibm              → Qiskit Runtime adapter
  stabstream.vendors.cirq             → Cirq adapter
"""

import asyncio

try:
    from stabstream._stabstream import (  # noqa: F401  (re-export)
        CodeType,
        DecoderResult,
        DetectorErrorModel,
        LogicalCorrection,
        LogicalErrorAccumulator,
        SpacetimeGraph,
        StabstreamError,
        StabstreamStream,
        SyndromeFrame,
        SyndromeWindow,
        __version__,
    )
except ModuleNotFoundError:
    # Extension not yet compiled (e.g. running from source without maturin).
    # Pure-Python submodules (vendors, io) remain importable.
    __version__ = "0.1.0-dev"
    StabstreamError = Exception  # fallback so type annotations don't crash

from stabstream.io import load_qssf, read_qssf, load_qssf_windows, load_dataset  # noqa: F401


# ---------------------------------------------------------------------------
# SyndromeStream — dual-mode (sync + async) wrapper
# ---------------------------------------------------------------------------

class SyndromeStream:
    """
    Dual-mode (sync + async) QSSF syndrome stream with integrated UF decoder.

    Supports both ``with`` / ``for`` (sync) and ``async with`` / ``async for``
    (async) protocols — the same object, the same API.

    Parameters
    ----------
    source:
        File path, ``tcp://host:port``, or ``shm://name``.
    decoder:
        Optional DEM to configure the Union-Find decoder immediately.
        Accepts a file path, inline DEM text, or ``stim.DetectorErrorModel``.
    window_depth:
        Syndrome window depth. 0 = auto-infer from DEM (default).
    queue_depth:
        Reserved for future buffered-producer mode. Currently unused; the
        stream reads one frame at a time (effective depth = 1).

    Examples
    --------
    Sync::

        with stabstream.open("experiment.qssf") as stream:
            stream.set_decoder("surface_d5.dem")
            for frame in stream:
                apply_correction(frame.observable_flips)

    Async::

        async with stabstream.open("tcp://fpga:9000", decoder=circuit.detector_error_model()) as stream:
            async for frame in stream:
                await apply_correction(frame.observable_flips)
    """

    def __init__(
        self,
        source: str,
        *,
        decoder=None,
        window_depth: int = 0,
        queue_depth: int = 64,
    ) -> None:
        self._inner = StabstreamStream(source)
        self._queue_depth = queue_depth  # reserved for future buffered mode
        if decoder is not None:
            self.set_decoder(decoder, window_depth)

    def set_decoder(self, dem, window_depth: int = 0) -> None:
        """
        Configure the Union-Find decoder.

        Parameters
        ----------
        dem:
            File path (str or ``pathlib.Path``), inline DEM text string,
            or a ``stim.DetectorErrorModel`` object.
        window_depth:
            Override the auto-inferred syndrome window depth.
        """
        self._inner.set_decoder(dem, window_depth)

    # ── sync protocol ────────────────────────────────────────────────────────

    def __enter__(self) -> "SyndromeStream":
        return self

    def __exit__(self, exc_type, exc_val, tb) -> bool:
        self._inner.close()
        return False

    def __iter__(self) -> "SyndromeStream":
        return self

    def __next__(self) -> "SyndromeFrame":
        return next(self._inner)

    def close(self) -> None:
        """Close the stream and release underlying resources."""
        self._inner.close()

    # ── async protocol ───────────────────────────────────────────────────────

    async def __aenter__(self) -> "SyndromeStream":
        return self

    async def __aexit__(self, exc_type, exc_val, tb) -> bool:
        await self.aclose()
        return False

    def __aiter__(self) -> "SyndromeStream":
        return self

    async def __anext__(self) -> "SyndromeFrame":
        """
        Read the next frame without blocking the asyncio event loop.

        The blocking I/O call is dispatched to the default thread-pool
        executor so other coroutines can run while waiting for hardware data.

        Note: concurrent calls on the *same* stream are not safe. Use one
        ``async for`` loop per stream. For multiple independent hardware
        sources, open one ``SyndromeStream`` per source and run them
        concurrently with ``asyncio.gather``.
        """
        loop = asyncio.get_running_loop()
        frame = await loop.run_in_executor(None, lambda: next(self._inner, None))
        if frame is None:
            raise StopAsyncIteration
        return frame

    async def aclose(self) -> None:
        """Async-safe stream close."""
        loop = asyncio.get_running_loop()
        await loop.run_in_executor(None, self._inner.close)

    def __repr__(self) -> str:
        return f"SyndromeStream(source=<open>)"


# ---------------------------------------------------------------------------
# Convenience constructors
# ---------------------------------------------------------------------------

def open(
    source: str,
    *,
    decoder=None,
    window_depth: int = 0,
    queue_depth: int = 64,
) -> SyndromeStream:
    """
    Open a QSSF stream.

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

    Returns
    -------
    SyndromeStream
        Supports both ``with``/``for`` and ``async with``/``async for``.

    Examples
    --------
    ::

        # Sync — offline file analysis
        with stabstream.open("run42.qssf", decoder="surface_d5.dem") as s:
            for frame in s:
                print(frame.observable_flips)

        # Async — live hardware
        async with stabstream.open("tcp://fpga:9000", decoder=dem) as s:
            async for frame in s:
                await apply_correction(frame.observable_flips)
    """
    return SyndromeStream(source, decoder=decoder, window_depth=window_depth, queue_depth=queue_depth)


def from_stim_circuit(
    source: str,
    circuit,
    *,
    window_depth: int = 0,
    queue_depth: int = 64,
) -> SyndromeStream:
    """
    Open a QSSF stream with the decoder auto-configured from a Stim circuit.

    Calls ``circuit.detector_error_model(decompose_errors=True)`` internally
    so callers never need to interact with DEM objects directly.

    Parameters
    ----------
    source:
        File path or ``tcp://host:port``.
    circuit:
        A ``stim.Circuit`` object.
    window_depth:
        Override the auto-inferred syndrome window depth.
    queue_depth:
        Reserved for future buffered-producer mode.

    Examples
    --------
    ::

        import stim, stabstream

        circuit = stim.Circuit.from_file("surface_d5.stim")

        async with stabstream.from_stim_circuit("tcp://fpga:9000", circuit) as stream:
            async for frame in stream:
                await apply_correction(frame.observable_flips)
    """
    dem = circuit.detector_error_model(decompose_errors=True)
    return SyndromeStream(
        source, decoder=dem, window_depth=window_depth, queue_depth=queue_depth
    )


def from_stim_dem(
    source: str,
    dem,
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
    return SyndromeStream(
        source, decoder=dem, window_depth=window_depth, queue_depth=queue_depth
    )


__all__ = [
    # Rust classes
    "CodeType",
    "DecoderResult",
    "DetectorErrorModel",
    "LogicalCorrection",
    "LogicalErrorAccumulator",
    "SpacetimeGraph",
    "StabstreamError",
    "StabstreamStream",
    "SyndromeFrame",
    "SyndromeWindow",
    # Python wrappers / constructors
    "SyndromeStream",
    "open",
    "from_stim_circuit",
    "from_stim_dem",
    # Pure-Python I/O utilities
    "load_qssf",
    "read_qssf",
    "load_qssf_windows",
    "load_dataset",
    "__version__",
]
