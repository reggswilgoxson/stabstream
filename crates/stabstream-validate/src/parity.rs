use stabstream_core::{error::StabstreamError, frame::SyndromeFrame, schema::HardwareSchema};

/// Verify that the syndrome payload satisfies the stabilizer parity constraints
/// defined in the hardware schema.
pub fn check_parity(
    _frame: &SyndromeFrame<'_>,
    _schema: &HardwareSchema,
) -> Result<(), StabstreamError> {
    // TODO:
    // For each stabilizer in schema.stabilizers:
    //   XOR the meas_results for its qubit indices.
    //   If the result is inconsistent with detector_events for this stabilizer,
    //   return StabstreamError::ParityViolation { frame_id, stabilizer }.
    todo!("implement stabilizer parity check")
}
