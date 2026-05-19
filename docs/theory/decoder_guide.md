# Decoder Guide: Union-Find, MWPM, and Real-Time Trade-offs

## The Latency Budget

Superconducting qubits have measurement cycles of ~1 µs. A fault-tolerant
processor must complete decoding within one cycle to prevent error backlog:

| Stage | Budget | stabstream status |
|-------|--------|-------------------|
| Frame deserialize | 200 ns | ✅ ~600 ns (all-in) |
| CRC validation | 70 ns | ✅ included above |
| Window slide | 20 ns | ✅ `SyndromeWindow::push_owned` |
| Decode | 400 ns | ✅ UF target; not yet benchmarked end-to-end |
| Correction output | 50 ns | pending |
| **Total** | **~740 ns** | **< 1 µs hard deadline** |

## Minimum-Weight Perfect Matching (MWPM)

MWPM finds the minimum-weight set of edges in the spacetime graph whose
boundary matches the observed syndrome. It gives **optimal** logical error rates
for any code whose error model is captured by a DEM.

**Reference**: Dennis et al. (2002); Kolmogorov (2009) for Blossom V.

**Implementation**: [PyMatching](https://github.com/pauli-space/PyMatching) — the
industry standard. `DetectorErrorModel.to_pymatching()` in stabstream constructs
a `pymatching.Matching` object directly.

### When to use MWPM

- Offline threshold simulations (`stabstream-threshold run`)
- Neural decoder training data generation
- Benchmarking the UF decoder's sub-optimality
- Any research workflow where 20–50 µs latency is acceptable

## Union-Find Decoder

Union-Find (Delfosse & Nickerson 2021) runs in $O(n \cdot \alpha(n)) \approx O(n)$
time, where $n$ is the number of detectors in the window. Google's QAI team used
a UF variant in their 2023 below-threshold surface code experiment.

### Algorithm sketch

1. Each syndrome defect (detector firing) is an odd cluster.
2. Grow clusters uniformly outward, merging via union-find.
3. Stop when no odd clusters remain.
4. Peel the resulting spanning forest to extract corrections.

**Performance on d=5 surface code (24 ancillas × 5 rounds = 120 detectors)**:
target ≤ 400 ns per window. The implementation pre-allocates all buffers at
construction time — the decode inner loop is allocation-free.

**Sub-optimality**: For surface codes, UF achieves $\approx 95\%$ of MWPM's
threshold. For qLDPC codes with higher-weight checks, the gap may be larger.

### When to use Union-Find

- Real-time decoding on superconducting hardware
- Any workflow where latency < 1 µs is required
- As a fast baseline in threshold simulations

## stabstream's `Decoder` Trait

```rust
pub trait Decoder: Send + Sync {
    /// Single-frame stateless decode (NullDecoder, threshold simulation).
    fn decode_frame(&self, frame: &SyndromeFrame<'_>) -> DecoderResult {
        DecoderResult::empty()
    }
    /// Multi-round window decode (UnionFindDecoder, PyMatching bridge).
    fn decode_window(&self, window: &SyndromeWindow) -> DecoderResult {
        DecoderResult::empty()
    }
}
```

The default implementations return an empty result, so `NullDecoder` needs no
code. `UnionFindDecoder` overrides only `decode_window`.

## Choosing `window_depth`

`window_depth` controls how many syndrome rounds the decoder sees simultaneously.
For a distance-$d$ surface code, use `window_depth = d` (the standard choice).

- Too small: measurement errors span the window boundary and cause systematic
  miscorrections.
- Too large: no accuracy gain, just higher latency.

For Bivariate Bicycle codes (e.g. `[[144, 12, 12]]`), the optimal window depth
is an open research question; 12 is a reasonable starting point.

## The PyMatching Bridge

`DetectorErrorModel.to_pymatching()` converts a DEM directly into a
`pymatching.Matching` object without re-parsing:

```python
from stabstream import DetectorErrorModel, SyndromeWindow

dem = DetectorErrorModel.from_file("circuit.dem")
matching = dem.to_pymatching()  # pymatching.Matching

window = SyndromeWindow(ancilla_count=dem.detector_count, window_depth=5)
for frame in stabstream.load_qssf("recording.qssf"):
    window.push(frame)
    if window.is_full():
        matrix = window.to_numpy_matrix()  # shape (5, ancilla_count)
        correction = matching.decode(matrix.flatten())
```

This path is intended for research and benchmarking. For production real-time
decoding, use `UnionFindDecoder` from Rust.
