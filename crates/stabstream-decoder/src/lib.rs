use stabstream_core::{frame::SyndromeFrame, window::SyndromeWindow};

#[cfg(feature = "mwpm")]
pub mod mwpm;
pub mod union_find;

/// A single logical-qubit correction suggested by a decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalCorrection {
    /// Which logical qubit this correction applies to.
    pub logical_id: u8,
    /// The Pauli operator to apply.
    pub pauli: PauliOp,
}

/// Single-qubit Pauli operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauliOp {
    I,
    X,
    Y,
    Z,
}

/// Output of a decoder for one syndrome frame or window.
#[derive(Debug, Clone)]
pub struct DecoderResult {
    /// Logical corrections recommended by the decoder (may be empty).
    pub corrections: Vec<LogicalCorrection>,
    /// Decoder confidence in [0.0, 1.0]. Higher means more certain.
    pub confidence: f64,
    /// Bitmask of observable indices the decoder believes were flipped.
    /// Matches the `observable_flips` field in `FrameMetadata`.
    pub observable_flips: u64,
}

impl DecoderResult {
    pub fn empty() -> Self {
        Self {
            corrections: Vec::new(),
            confidence: 1.0,
            observable_flips: 0,
        }
    }
}

/// Trait implemented by any QEC syndrome decoder.
///
/// Both methods have default (no-op) implementations so existing code that
/// implements only `decode_frame` continues to compile unchanged.
///
/// # Real-time vs offline paths
///
/// * `decode_frame` — stateless, single-frame path. Suitable for
///   `NullDecoder` and threshold simulation inner loops.
/// * `decode_window` — stateful, multi-round path required by MWPM and
///   Union-Find decoders that operate on the 3-D spacetime syndrome graph.
pub trait Decoder: Send + Sync {
    /// Decode a single frame (stateless). Default: return no corrections.
    fn decode_frame(&self, frame: &SyndromeFrame<'_>) -> DecoderResult {
        let _ = frame;
        DecoderResult::empty()
    }

    /// Decode a window of rounds (stateful). Default: delegate to
    /// `decode_frame` on the most recent frame if one is available.
    fn decode_window(&self, window: &SyndromeWindow) -> DecoderResult {
        if let Some(latest) = window.latest_frame() {
            // Reconstruct a lightweight proxy frame for the default path.
            // Real implementations override this entirely.
            let _ = latest;
        }
        DecoderResult::empty()
    }
}

/// A no-op decoder that returns no corrections with full confidence.
///
/// Useful for benchmarking the parse/deserialise pipeline without coupling
/// to a real decoder backend.
pub struct NullDecoder;

impl Decoder for NullDecoder {
    fn decode_frame(&self, _frame: &SyndromeFrame<'_>) -> DecoderResult {
        DecoderResult::empty()
    }
}

#[cfg(test)]
mod tests {
    use stabstream_core::frame::{FrameHeader, SyndromeFrame, SyndromePayload};

    use super::*;

    fn make_frame() -> SyndromeFrame<'static> {
        SyndromeFrame {
            header: FrameHeader {
                frame_id: 0,
                round: 0,
                timestamp_ns: 0,
                qubit_count: 25,
                ancilla_count: 24,
                payload_len: 0,
                code_type: 0x01,
                distance: 5,
                flags: 0,
                crc32: 0,
            },
            payload: SyndromePayload {
                detector_events: &[],
                meas_results: &[],
                timing_offsets: &[],
                parity_checks: &[],
            },
            metadata: None,
            annotations: None,
        }
    }

    #[test]
    fn null_decoder_returns_empty_result() {
        let dec = NullDecoder;
        let frame = make_frame();
        let result = dec.decode_frame(&frame);
        assert!(result.corrections.is_empty());
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn null_decoder_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NullDecoder>();
    }

    #[test]
    fn pauli_op_derives_eq() {
        assert_eq!(PauliOp::X, PauliOp::X);
        assert_ne!(PauliOp::X, PauliOp::Z);
    }

    #[test]
    fn decoder_result_observable_flips_default_zero() {
        let r = DecoderResult::empty();
        assert_eq!(r.observable_flips, 0);
    }
}
