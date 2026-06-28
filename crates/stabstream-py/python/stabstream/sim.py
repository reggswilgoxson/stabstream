"""
stabstream.sim — Stim-backed QSSF frame generation and simulated real-time dispatch.
"""

from __future__ import annotations

import time
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    pass


def simulate_circuit_to_qssf(
    circuit,
    shots: int,
    output_path: str,
    *,
    seed: int = 42,
) -> int:
    """
    Sample a Stim circuit and write results as a QSSF file.

    Each QSSF frame corresponds to one shot (all detector rounds flattened).
    Use window_depth=1 when reading back with ReplayStream or SyndromeWindow.

    Uses stim.Circuit.compile_detector_sampler() — pure Python, no subprocess.
    For >100K shots prefer the CLI: stabstream-convert dem-to-dataset (~10x faster).

    Returns the number of frames written.
    """
    import stim

    from stabstream._qssf_write import write_qssf

    if isinstance(circuit, str):
        circuit = stim.Circuit(circuit)

    sampler = circuit.compile_detector_sampler(seed=seed)
    det_array, obs_array = sampler.sample(shots, separate_observables=True)
    ancilla_count = int(det_array.shape[1])

    def _frame_iter():
        for i in range(shots):
            obs_flips = int(
                sum(int(b) << j for j, b in enumerate(obs_array[i].tolist()))
            )
            yield {
                "frame_id": i,
                "round": 0,
                "ancilla_count": ancilla_count,
                "detector_events": det_array[i].tolist(),
                "observable_flips": obs_flips,
            }

    return write_qssf(output_path, _frame_iter())


def simulate_dem_to_qssf(
    dem_text: str,
    shots: int,
    output_path: str,
    *,
    seed: int = 42,
) -> int:
    """
    Sample a DEM directly and write results as a QSSF file.

    Equivalent to simulate_circuit_to_qssf but accepts DEM text instead of a circuit.
    """
    import stim

    from stabstream._qssf_write import write_qssf

    dem = stim.DetectorErrorModel(dem_text)
    sampler = dem.compile_sampler(seed=seed)
    det_array, obs_array = sampler.sample(shots, separate_observables=True)
    ancilla_count = int(det_array.shape[1])

    def _frame_iter():
        for i in range(shots):
            obs_flips = int(
                sum(int(b) << j for j, b in enumerate(obs_array[i].tolist()))
            )
            yield {
                "frame_id": i,
                "round": 0,
                "ancilla_count": ancilla_count,
                "detector_events": det_array[i].tolist(),
                "observable_flips": obs_flips,
            }

    return write_qssf(output_path, _frame_iter())


def realtime_stream(
    qssf_path: str,
    decoder,
    *,
    frame_rate_hz: float = 1000.0,
    batch_size: int = 64,
):
    """
    Replay a QSSF file through a decoder at a simulated hardware rate.

    Yields (SyndromeFrame, DecoderResult) tuples.

    Uses batch_size to amortize decoder inference cost — especially important
    for NeuralDecoder where single-frame inference has higher Python/C overhead
    than batch inference. frame_rate_hz controls inter-batch sleep, not per-frame
    sleep. This simulates throughput rate only, not per-frame decode latency.

    Both UnionFindDecoder and NeuralDecoder expose decode_batch(X) where X has
    shape (shots, detectors).
    """
    from stabstream.io import load_qssf

    batch_interval = batch_size / frame_rate_hz
    frame_buffer: list = []

    for frame in load_qssf(qssf_path):
        frame_buffer.append(frame)
        if len(frame_buffer) >= batch_size:
            X = np.stack(
                [f.to_numpy_detector_events() for f in frame_buffer]
            )
            results = decoder.decode_batch(X)
            for f, r in zip(frame_buffer, results):
                yield f, r
            frame_buffer.clear()
            time.sleep(batch_interval)

    if frame_buffer:
        X = np.stack(
            [f.to_numpy_detector_events() for f in frame_buffer]
        )
        results = decoder.decode_batch(X)
        for f, r in zip(frame_buffer, results):
            yield f, r
