use stabstream_core::{error::StabstreamError, frame::SyndromeFrame, schema::HardwareSchema};

/// Verify that the syndrome payload satisfies the stabilizer parity constraints
/// defined in the hardware schema.
///
/// For each stabilizer, XOR the measurement outcomes of its qubits.
/// A parity violation is raised when a stabilizer's combined outcome disagrees
/// with the detector-event bit for that stabilizer index.
pub fn check_parity(
    frame: &SyndromeFrame<'_>,
    schema: &HardwareSchema,
) -> Result<(), StabstreamError> {
    for (stab_idx, stabilizer) in schema.stabilizers.iter().enumerate() {
        // Compute parity: XOR the low bit of each ancilla meas_result.
        // meas_results[i]: 0x01 = +1 outcome, 0xFF = -1 outcome.
        // We treat 0x01 as parity-0 and 0xFF (or any odd byte) as parity-1.
        let mut parity: u8 = 0;
        for &qubit_idx in &stabilizer.qubits {
            let idx = qubit_idx as usize;
            if let Some(&meas) = frame.payload.meas_results.get(idx) {
                // -1 outcomes (0xFF) have low bit 1; +1 outcomes (0x01) have low bit 1 too.
                // Distinguish: +1 = 1, -1 = 0xFF. Use sign: treat as i8.
                let bit = if meas > 0 { 0u8 } else { 1u8 };
                parity ^= bit;
            }
        }

        // Decode detector-event bit for this stabilizer from the RLE stream.
        // We count total events up to stab_idx to find the right bit position.
        let detector_bit = decode_detector_bit(frame.payload.detector_events, stab_idx);

        if parity != detector_bit {
            return Err(StabstreamError::ParityViolation {
                frame_id: frame.header.frame_id,
                stabilizer: stab_idx,
            });
        }
    }
    Ok(())
}

/// Decode a single bit at position `bit_index` from a QSSF RLE-encoded
/// bitfield without allocating.
fn decode_detector_bit(encoded: &[u8], bit_index: usize) -> u8 {
    let mut pos = 0usize;
    for &token in encoded {
        let mode = (token >> 7) & 1;
        let run = (token & 0x7F) as usize;
        if bit_index < pos + run {
            return mode;
        }
        pos += run;
    }
    0 // index past end of stream → treat as no event
}
