"""
stabstream — high-performance QEC syndrome stream library.

The compiled Rust extension lives in `stabstream._stabstream`; this
`__init__.py` re-exports everything so that the public import path
``from stabstream import SyndromeFrame, ...`` keeps working unchanged.

Additional pure-Python utilities:

  stabstream.load_qssf(path)       → generator of SyndromeFrame
  stabstream.read_qssf(path)       → pandas DataFrame (requires pandas)
  stabstream.vendors.ibm           → Qiskit Runtime adapter
  stabstream.vendors.cirq          → Cirq adapter
"""

try:
    from stabstream._stabstream import (  # noqa: F401  (re-export)
        CodeType,
        DecoderResult,
        DetectorErrorModel,
        LogicalCorrection,
        LogicalErrorAccumulator,
        SpacetimeGraph,
        StabstreamStream,
        SyndromeFrame,
        SyndromeWindow,
        __version__,
    )
except ModuleNotFoundError:
    # Extension not yet compiled (e.g. running from source without maturin).
    # Pure-Python submodules (vendors, io) remain importable.
    __version__ = "0.1.0-dev"

from stabstream.io import load_qssf, read_qssf, load_qssf_windows, load_dataset  # noqa: F401

__all__ = [
    # Rust classes
    "CodeType",
    "DecoderResult",
    "DetectorErrorModel",
    "LogicalCorrection",
    "LogicalErrorAccumulator",
    "SpacetimeGraph",
    "StabstreamStream",
    "SyndromeFrame",
    "SyndromeWindow",
    # Pure-Python utilities
    "load_qssf",
    "read_qssf",
    "load_qssf_windows",
    "load_dataset",
    "__version__",
]
