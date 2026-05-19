"""
Internal QSSF binary writer used by the vendor CLI converters.

Frame layout (no metadata, flags=0):

  File header   26 bytes   MAGIC(4) VERSION(2) UUID(16) FLAGS(4)
  Per frame:
    Frame header 36 bytes  frame_id(8) round(4) ts(8) qubit_count(2)
                           ancilla_count(2) payload_len(4) code_type(1)
                           distance(1) flags(2) hdr_crc32(4)
    rle_len       2 bytes  u16 LE — length of RLE block
    RLE bytes     rle_len  detector event run-length encoding
    meas bytes    ancilla  0xFF = fired, 0x01 = not fired
    sentinel      2 bytes  0xFFFF
    frame crc32   4 bytes  CRC32 of the 36-byte header
"""

from __future__ import annotations

import struct
import time
import zlib
from typing import Iterator


# File header constants
_MAGIC = 0x51535346  # "QSSF" as big-endian u32
_VERSION = 1
_STIM_GENERIC_UUID = bytes.fromhex("000000005354494d0000000000000001")
_CODE_TYPE_GENERIC = 0x01


def _rle_encode(events: list[bool]) -> bytes:
    """Encode a bool sequence as stabstream RLE tokens [mode(1)|run(7)]."""
    if not events:
        return b""

    out: list[int] = []
    i = 0
    n = len(events)
    while i < n:
        bit = events[i]
        run = 1
        while i + run < n and events[i + run] == bit and run < 127:
            run += 1
        mode = 0x80 if bit else 0x00
        out.append(mode | run)
        i += run
    return bytes(out)


def _frame_header(
    frame_id: int,
    round_no: int,
    ancilla_count: int,
    payload_len: int,
    timestamp_ns: int,
) -> bytes:
    buf = bytearray(36)
    struct.pack_into("<Q", buf, 0, frame_id)
    struct.pack_into("<I", buf, 8, round_no)
    struct.pack_into("<Q", buf, 12, timestamp_ns)
    struct.pack_into("<H", buf, 20, 0)              # qubit_count
    struct.pack_into("<H", buf, 22, ancilla_count)
    struct.pack_into("<I", buf, 24, payload_len)
    buf[28] = _CODE_TYPE_GENERIC
    buf[29] = 0                                     # distance
    struct.pack_into("<H", buf, 30, 0)              # flags
    crc = zlib.crc32(bytes(buf[:32])) & 0xFFFFFFFF
    struct.pack_into("<I", buf, 32, crc)
    return bytes(buf)


def write_qssf(path: str, frames: Iterator[dict], schema_uuid: bytes | None = None) -> int:
    """
    Write an iterator of frame dicts to a QSSF file.

    Parameters
    ----------
    path:
        Output filesystem path (will be created or overwritten).
    frames:
        Iterator of dicts with keys ``frame_id``, ``round``, ``ancilla_count``,
        ``detector_events`` (bool sequence), ``observable_flips`` (int | None).
    schema_uuid:
        16 raw bytes of a UUID to embed in the file header.  Defaults to the
        generic Stim UUID ``00000000-5354-494d-0000-000000000001``.

    Returns
    -------
    int
        Number of frames written.
    """
    uuid_bytes = schema_uuid if schema_uuid is not None else _STIM_GENERIC_UUID

    # 26-byte file header
    file_hdr = bytearray(26)
    struct.pack_into("<I", file_hdr, 0, _MAGIC)
    struct.pack_into("<H", file_hdr, 4, _VERSION)
    file_hdr[6:22] = uuid_bytes
    struct.pack_into("<I", file_hdr, 22, 0)  # flags

    n_written = 0
    with open(path, "wb") as fh:
        fh.write(file_hdr)
        for frame in frames:
            events: list[bool] = list(frame["detector_events"])
            ancilla_count: int = frame["ancilla_count"]
            frame_id: int = int(frame["frame_id"])
            round_no: int = int(frame["round"])
            timestamp_ns: int = int(time.time_ns())

            rle = _rle_encode(events)
            meas = bytes(0xFF if e else 0x01 for e in events)
            payload_len = 2 + len(rle) + ancilla_count

            hdr = _frame_header(frame_id, round_no, ancilla_count, payload_len, timestamp_ns)
            frame_crc = zlib.crc32(hdr) & 0xFFFFFFFF

            fh.write(hdr)
            fh.write(struct.pack("<H", len(rle)))
            fh.write(rle)
            fh.write(meas)
            fh.write(struct.pack("<H", 0xFFFF))
            fh.write(struct.pack("<I", frame_crc))
            n_written += 1

    return n_written
