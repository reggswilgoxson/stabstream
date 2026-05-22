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


def load_qssf_windows(
    path: str,
    window_depth: int,
    batch_size: int = 256,
    *,
    with_labels: bool = False,
) -> "Generator[np.ndarray | tuple[np.ndarray, np.ndarray], None, None]":
    """
    Yield batches of multi-round syndrome windows for ML training/inference.

    Each batch is a NumPy array of shape
    ``(batch_size, window_depth, ancilla_count)`` with dtype ``bool``.
    Frames with a mismatched ``ancilla_count`` are silently dropped.

    Parameters
    ----------
    path:
        Filesystem path or TCP URI accepted by ``load_qssf``.
    window_depth:
        Number of rounds per window (temporal depth).  Equivalent to the
        ``window_depth`` argument of ``stabstream.SyndromeWindow``.
    batch_size:
        Number of windows per yielded array.  The final batch may be
        smaller.
    with_labels:
        If True, yield ``(X, y)`` tuples where ``y`` is a
        ``(batch_size,)`` uint64 array of observable flip bitmasks read
        from QSSF metadata tag 0x10.  Frames without ground truth
        contribute ``y=0``.

    Yields
    ------
    np.ndarray or tuple[np.ndarray, np.ndarray]
        ``X`` has shape ``(n, window_depth, ancilla_count)``, dtype bool.
        ``y`` (only when ``with_labels=True``) has shape ``(n,)``,
        dtype uint64.

    Examples
    --------
    Training loop::

        from stabstream.io import load_qssf_windows
        for X, y in load_qssf_windows("data.qssf", window_depth=5,
                                       batch_size=512, with_labels=True):
            # X.shape == (512, 5, 24)  — (batch, rounds, ancillas)
            # y.shape == (512,)        — observable flip bitmasks
            loss = model.train_step(X, y)

    Inference::

        for X in load_qssf_windows("live.qssf", window_depth=5):
            predictions = model(X)
    """
    import collections

    anchor_ancillas: Optional[int] = None
    ring: "collections.deque[np.ndarray]" = collections.deque()
    label_ring: "collections.deque[int]" = collections.deque()

    x_buf: list[np.ndarray] = []
    y_buf: list[int] = []

    for frame in load_qssf(path):
        ac = frame.ancilla_count
        if anchor_ancillas is None:
            anchor_ancillas = ac
        if ac != anchor_ancillas:
            continue

        row = frame.to_numpy_detector_events()
        label = int(frame.observable_flips or 0)

        ring.append(row)
        label_ring.append(label)

        if len(ring) > window_depth:
            ring.popleft()
            label_ring.popleft()

        if len(ring) == window_depth:
            x_buf.append(np.stack(list(ring)))  # (window_depth, ancillas)
            y_buf.append(label_ring[-1])

            if len(x_buf) == batch_size:
                X = np.stack(x_buf)
                if with_labels:
                    yield X, np.array(y_buf, dtype=np.uint64)
                else:
                    yield X
                x_buf = []
                y_buf = []

    if x_buf:
        X = np.stack(x_buf)
        if with_labels:
            yield X, np.array(y_buf, dtype=np.uint64)
        else:
            yield X


# ---------------------------------------------------------------------------
# ML training dataset I/O
# ---------------------------------------------------------------------------

_DATASET_MAGIC = b"SSDS"  # StabStream DataSet
_DATASET_VERSION = 1


def load_dataset(
    path: str,
) -> "tuple[np.ndarray, np.ndarray]":
    """
    Load an ML training dataset written by ``stabstream-convert dem-to-dataset``.

    The file uses a compact binary layout::

        magic (4 bytes): b"SSDS"
        version (u8): 1
        shots (u64 LE)
        detector_count (u32 LE)
        observable_count (u32 LE)
        X: uint8 array, shape (shots, detector_count), row-major
        y: uint64 array, shape (shots,), LE

    Parameters
    ----------
    path:
        Path to a ``.bin`` dataset file produced by
        ``stabstream-convert dem-to-dataset``.

    Returns
    -------
    X : np.ndarray
        Shape ``(shots, detector_count)``, dtype ``bool``.  1 = detector
        fired, 0 = no event.
    y : np.ndarray
        Shape ``(shots,)``, dtype ``uint64``.  Each value is a bitmask of
        observable flip ground truth (bit i = 1 if observable i flipped).

    Examples
    --------
    ::

        from stabstream.io import load_dataset
        X, y = load_dataset("training_data.bin")
        # X.shape == (100000, 24) for d=5 surface code
        # y.shape == (100000,)
    """
    import struct

    with open(path, "rb") as f:
        raw = f.read()

    if len(raw) < 14:
        raise ValueError(f"dataset file too short: {len(raw)} bytes")

    if raw[:4] != _DATASET_MAGIC:
        raise ValueError(
            f"invalid dataset magic: {raw[:4]!r} (expected {_DATASET_MAGIC!r})"
        )
    if raw[4] != _DATASET_VERSION:
        raise ValueError(
            f"unsupported dataset version {raw[4]} (expected {_DATASET_VERSION})"
        )

    offset = 5
    shots, det, obs = struct.unpack_from("<QII", raw, offset)
    offset += 16  # 8 + 4 + 4

    x_size = shots * det
    y_size = shots * 8

    if len(raw) < offset + x_size + y_size:
        raise ValueError(
            f"dataset file truncated: expected {offset + x_size + y_size} bytes, "
            f"got {len(raw)}"
        )

    X = np.frombuffer(raw, dtype=np.uint8, count=x_size, offset=offset).reshape(
        shots, det
    ).astype(bool)
    offset += x_size

    y = np.frombuffer(raw, dtype="<u8", count=shots, offset=offset)

    return X, y
