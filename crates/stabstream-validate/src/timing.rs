use stabstream_core::{error::StabstreamError, frame::SyndromeFrame};

/// Maximum allowed timing offset in nanoseconds before a frame is rejected.
pub const MAX_TIMING_OFFSET_NS: u16 = 10_000;

/// Validate per-ancilla timing offsets against [`MAX_TIMING_OFFSET_NS`].
///
/// Returns [`StabstreamError::TimingOutOfBounds`] for the first ancilla whose
/// offset exceeds the threshold.
pub fn check_timing(_frame: &SyndromeFrame<'_>) -> Result<(), StabstreamError> {
    // TODO:
    // For each (ancilla_idx, &offset) in frame.payload.timing_offsets.iter().enumerate():
    //   if offset > MAX_TIMING_OFFSET_NS {
    //     return Err(StabstreamError::TimingOutOfBounds { ancilla: ancilla_idx, offset_ns: offset });
    //   }
    todo!("implement per-ancilla timing validation")
}
