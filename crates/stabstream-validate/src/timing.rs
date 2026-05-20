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
fn make_frame<'a>(timing: &'a [u16]) -> stabstream_core::frame::SyndromeFrame<'a> {
    use stabstream_core::frame::{FrameHeader, SyndromeFrame, SyndromePayload};
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

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use super::{check_timing, make_frame, MAX_TIMING_OFFSET_NS};

    proptest! {
        /// Any offsets all at or below the threshold must pass.
        #[test]
        fn all_under_threshold_passes(
            offsets in prop::collection::vec(0u16..=MAX_TIMING_OFFSET_NS, 0..=16),
        ) {
            prop_assert!(check_timing(&make_frame(&offsets)).is_ok());
        }

        /// When at least one offset exceeds the threshold the check must fail.
        #[test]
        fn any_over_threshold_fails(
            mut offsets in prop::collection::vec(0u16..=MAX_TIMING_OFFSET_NS, 1..=15),
            bad_offset in (MAX_TIMING_OFFSET_NS + 1)..=u16::MAX,
            insert_at in any::<prop::sample::Index>(),
        ) {
            let idx = insert_at.index(offsets.len() + 1);
            offsets.insert(idx, bad_offset);
            let result = check_timing(&make_frame(&offsets));
            prop_assert!(result.is_err(), "expected TimingOutOfBounds but got Ok");
        }
    }
}
