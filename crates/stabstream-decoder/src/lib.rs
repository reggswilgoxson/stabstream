use stabstream_core::frame::SyndromeFrame;

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

/// Output of a decoder for one syndrome frame.
#[derive(Debug, Clone)]
pub struct DecoderResult {
    /// Logical corrections recommended by the decoder (may be empty).
    pub corrections: Vec<LogicalCorrection>,
    /// Decoder confidence in [0.0, 1.0]. Higher means more certain.
    pub confidence: f64,
}

impl DecoderResult {
    pub fn empty() -> Self {
        Self {
            corrections: Vec::new(),
            confidence: 1.0,
        }
    }
}

/// Trait implemented by any QEC syndrome decoder.
///
/// Implementations must be `Send + Sync` so they can be shared across threads
/// or placed behind an `Arc`.
pub trait Decoder: Send + Sync {
    fn decode(&self, frame: &SyndromeFrame<'_>) -> DecoderResult;
}

/// A no-op decoder that returns no corrections with full confidence.
///
/// Useful for benchmarking the parse/deserialise pipeline without coupling
/// to a real decoder backend.
pub struct NullDecoder;

impl Decoder for NullDecoder {
    fn decode(&self, _frame: &SyndromeFrame<'_>) -> DecoderResult {
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
        let result = dec.decode(&frame);
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
}
