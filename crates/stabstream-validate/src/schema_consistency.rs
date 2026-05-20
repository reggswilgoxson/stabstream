use stabstream_core::{
    code::CodeType, error::StabstreamError, frame::SyndromeFrame, schema::HardwareSchema,
};

/// Map a schema `code_type` string to the `u8` discriminant used in frame headers.
fn schema_code_type_to_u8(code_type: &str) -> Option<u8> {
    match code_type {
        "surface" | "surface_code" => Some(CodeType::SurfaceCode as u8),
        "honeycomb" | "honeycomb_code" => Some(CodeType::HoneycombCode as u8),
        "color" | "color_code" => Some(CodeType::ColorCode as u8),
        "repetition" | "repetition_code" => Some(CodeType::RepetitionCode as u8),
        "toric" | "toric_code" => Some(CodeType::ToricCode as u8),
        "bivariate_bicycle" => Some(CodeType::BivariateBicycle as u8),
        "hypergraph_product" => Some(CodeType::HypergraphProduct as u8),
        "fiber_bundle" => Some(CodeType::FiberBundle as u8),
        "custom" => Some(CodeType::Custom as u8),
        _ => None,
    }
}

/// Verify that a frame's header fields are consistent with the registered schema.
///
/// Checks:
/// - `ancilla_count` matches
/// - `qubit_count` matches
/// - `code_type` discriminant matches (if the schema code_type string is known)
///
/// Returns `Ok(())` on success or `StabstreamError::SchemaFrameMismatch` on the
/// first detected inconsistency.
pub fn check_schema_consistency(
    frame: &SyndromeFrame<'_>,
    schema: &HardwareSchema,
) -> Result<(), StabstreamError> {
    if frame.header.ancilla_count != schema.ancilla_count {
        return Err(StabstreamError::SchemaFrameMismatch {
            field: "ancilla_count",
            expected: schema.ancilla_count as u32,
            actual: frame.header.ancilla_count as u32,
        });
    }

    if frame.header.qubit_count != schema.qubit_count {
        return Err(StabstreamError::SchemaFrameMismatch {
            field: "qubit_count",
            expected: schema.qubit_count as u32,
            actual: frame.header.qubit_count as u32,
        });
    }

    if let Some(expected_code) = schema_code_type_to_u8(&schema.code_type) {
        if frame.header.code_type != expected_code {
            return Err(StabstreamError::SchemaFrameMismatch {
                field: "code_type",
                expected: expected_code as u32,
                actual: frame.header.code_type as u32,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stabstream_core::{
        frame::{FrameHeader, SyndromeFrame, SyndromePayload},
        schema::SchemaRegistry,
    };

    fn make_frame(ancilla_count: u16, qubit_count: u16, code_type: u8) -> SyndromeFrame<'static> {
        SyndromeFrame {
            header: FrameHeader {
                frame_id: 0,
                round: 0,
                timestamp_ns: 0,
                qubit_count,
                ancilla_count,
                payload_len: 0,
                code_type,
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
    fn consistent_frame_passes() {
        let reg = SchemaRegistry::with_builtins().unwrap();
        let schema = reg
            .get(&"a3f7c210-4e12-4b0a-9b3c-1f2e8d7a5c01".parse().unwrap())
            .unwrap();
        // surface_code_d5: 25 qubits, 24 ancillas, code_type=SurfaceCode(0x01)
        let frame = make_frame(24, 25, 0x01);
        assert!(check_schema_consistency(&frame, schema).is_ok());
    }

    #[test]
    fn ancilla_mismatch_rejected() {
        let reg = SchemaRegistry::with_builtins().unwrap();
        let schema = reg
            .get(&"a3f7c210-4e12-4b0a-9b3c-1f2e8d7a5c01".parse().unwrap())
            .unwrap();
        let frame = make_frame(99, 25, 0x01); // wrong ancilla_count
        let err = check_schema_consistency(&frame, schema).unwrap_err();
        assert!(matches!(
            err,
            StabstreamError::SchemaFrameMismatch {
                field: "ancilla_count",
                ..
            }
        ));
    }

    #[test]
    fn code_type_mismatch_rejected() {
        let reg = SchemaRegistry::with_builtins().unwrap();
        let schema = reg
            .get(&"a3f7c210-4e12-4b0a-9b3c-1f2e8d7a5c01".parse().unwrap())
            .unwrap();
        let frame = make_frame(24, 25, 0x02); // HoneycombCode instead of SurfaceCode
        let err = check_schema_consistency(&frame, schema).unwrap_err();
        assert!(matches!(
            err,
            StabstreamError::SchemaFrameMismatch {
                field: "code_type",
                ..
            }
        ));
    }
}
