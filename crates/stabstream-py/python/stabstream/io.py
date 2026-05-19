"""
QSSF file I/O utilities for the Python ecosystem.

Functions
---------
load_qssf(path)
    Generator of ``SyndromeFrame`` objects — compatible with any for-loop,
    ``list()``, or (via ``to_dict()``) pandas construction.

read_qssf(path, *, columns)
    Convenience wrapper that loads the whole file into a ``pandas.DataFrame``.
    Each row corresponds to one frame; ``detector_events`` is stored as a
    NumPy bool array in an object column.

load_qssf_batch(path, batch_size)
    Yields 2-D NumPy arrays of shape ``(batch_size, ancilla_count)`` — suited
    for batched ML inference pipelines that expect matrix inputs.

Examples
--------
Iterate over frames::

    from stabstream import load_qssf
    for frame in load_qssf("data.qssf"):
        print(frame.frame_id, frame.detector_event_count)

Build a DataFrame::

    import pandas as pd
    from stabstream.io import read_qssf
    df = read_qssf("data.qssf")
    print(df[["frame_id", "round", "detector_event_count"]].head())

Batched NumPy::

    from stabstream.io import load_qssf_batch
    import numpy as np
    for batch in load_qssf_batch("data.qssf", batch_size=256):
        # batch.shape == (256, ancilla_count), dtype=bool
        predictions = model.predict(batch)
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Generator, Iterator, Optional

import numpy as np

if TYPE_CHECKING:
    import pandas as pd

if TYPE_CHECKING:
    from stabstream._stabstream import SyndromeFrame


def load_qssf(path: str) -> "Iterator[SyndromeFrame]":
    """
    Yield ``SyndromeFrame`` objects from a QSSF file (or ``tcp://host:port``).

    The generator holds an open file handle; it is automatically released when
    iteration is exhausted or the generator is garbage-collected.

    Parameters
    ----------
    path:
        Filesystem path to a ``.qssf`` or ``.qssf.zst`` file, or a TCP URI
        ``tcp://host:port``.

    Yields
    ------
    SyndromeFrame

    Examples
    --------
    ::

        for frame in load_qssf("recording.qssf"):
            arr = frame.to_numpy_detector_events()  # shape (ancilla_count,)
    """
    from stabstream._stabstream import StabstreamStream

    with StabstreamStream(path) as stream:
        yield from stream


def read_qssf(
    path: str,
    *,
    columns: Optional[list[str]] = None,
) -> "pd.DataFrame":
    """
    Load a QSSF file into a ``pandas.DataFrame``.

    Requires ``pandas`` (``pip install pandas``). Each row is one frame;
    ``detector_events`` holds a 1-D NumPy bool array.

    Parameters
    ----------
    path:
        Filesystem path or TCP URI accepted by ``load_qssf``.
    columns:
        Subset of column names to retain. ``None`` keeps all columns.

    Returns
    -------
    pandas.DataFrame
        Columns: ``frame_id``, ``round``, ``timestamp_ns``, ``qubit_count``,
        ``ancilla_count``, ``detector_event_count``, ``code_type``,
        ``distance``, ``detector_events``, ``observable_flips``.
    """
    try:
        import pandas as pd
    except ImportError as exc:
        raise ImportError(
            "pandas is required for read_qssf — install with: pip install pandas"
        ) from exc

    rows = [frame.to_dict() for frame in load_qssf(path)]
    df = pd.DataFrame(rows)
    if columns is not None:
        df = df[columns]
    return df


def load_qssf_batch(
    path: str,
    batch_size: int = 256,
) -> "Generator[np.ndarray, None, None]":
    """
    Yield batches of detector events as 2-D NumPy bool arrays.

    Each yielded array has shape ``(batch_size, ancilla_count)`` except
    possibly the last batch, which may be smaller.  Frames with mismatched
    ``ancilla_count`` are silently dropped.

    Parameters
    ----------
    path:
        Filesystem path or TCP URI accepted by ``load_qssf``.
    batch_size:
        Number of frames per yielded array.

    Yields
    ------
    np.ndarray
        Shape ``(n, ancilla_count)``, ``dtype=bool``, where ``n ≤ batch_size``.

    Examples
    --------
    ::

        for batch in load_qssf_batch("data.qssf", batch_size=512):
            # batch.shape == (512, 24) for a d=5 surface code
            logits = model(torch.from_numpy(batch.astype(np.float32)))
    """
    anchor_ancillas: Optional[int] = None
    buf: list[np.ndarray] = []

    for frame in load_qssf(path):
        ac = frame.ancilla_count
        if anchor_ancillas is None:
            anchor_ancillas = ac
        if ac != anchor_ancillas:
            continue

        buf.append(frame.to_numpy_detector_events())
        if len(buf) == batch_size:
            yield np.stack(buf)
            buf = []

    if buf:
        yield np.stack(buf)
