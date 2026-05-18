use stabstream_core::{error::StabstreamError, frame::SyndromeFrame};

/// Maximum allowed timing offset in nanoseconds before a frame is rejected.
pub const MAX_TIMING_OFFSET_NS: u16 = 10_000;

/// Validate per-ancilla timing offsets against [`MAX_TIMING_OFFSET_NS`].
///
/// Returns [`StabstreamError::TimingOutOfBounds`] for the first ancilla whose
/// offset exceeds the threshold.
pub fn check_timing(frame: &SyndromeFrame<'_>) -> Result<(), StabstreamError> {
    for (ancilla, &offset_ns) in frame.payload.timing_offsets.iter().enumerate() {
        if offset_ns > MAX_TIMING_OFFSET_NS {
            return Err(StabstreamError::TimingOutOfBounds { ancilla, offset_ns });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use stabstream_core::frame::{FrameHeader, SyndromeFrame, SyndromePayload};

    use super::*;

    fn make_frame<'a>(timing: &'a [u16]) -> SyndromeFrame<'a> {
        SyndromeFrame {
            header: FrameHeader {
                frame_id: 1,
                round: 0,
                timestamp_ns: 0,
                qubit_count: 0,
                ancilla_count: timing.len() as u16,
                payload_len: 0,
                code_type: 0x01,
                distance: 3,
                flags: 0,
                crc32: 0,
            },
            payload: SyndromePayload {
                detector_events: &[],
                meas_results: &[],
                timing_offsets: timing,
                parity_checks: &[],
            },
            metadata: None,
            annotations: None,
        }
    }

    #[test]
    fn all_under_threshold() {
        let timing = [100u16, 500, 9_999, 0];
        assert!(check_timing(&make_frame(&timing)).is_ok());
    }

    #[test]
    fn exactly_at_threshold() {
        let timing = [10_000u16];
        assert!(check_timing(&make_frame(&timing)).is_ok());
    }

    #[test]
    fn over_threshold_detected() {
        let timing = [100u16, 10_001];
        let err = check_timing(&make_frame(&timing)).unwrap_err();
        assert!(matches!(
            err,
            StabstreamError::TimingOutOfBounds {
                ancilla: 1,
                offset_ns: 10_001
            }
        ));
    }
}
