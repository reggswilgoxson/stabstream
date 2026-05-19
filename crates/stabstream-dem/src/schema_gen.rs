//! Generate `HardwareSchema` JSON from a parsed `DetectorErrorModel`.
//!
//! Detectors with coordinates [x, y, t] are mapped to stabilizer entries.
//! This provides a path from DEM → schema JSON without manually authoring
//! schema files for each hardware topology.

use stabstream_core::schema::{HardwareSchema, StabilizerEntry, StabilizerKind};
use uuid::Uuid;

use crate::parser::DetectorErrorModel;

/// Generate a `HardwareSchema` from a `DetectorErrorModel`.
///
/// Each detector becomes a stabilizer entry. Without coordinate information
/// the kind defaults to `Z`; with coordinates the parity of `round(x + y)`
/// selects X vs Z (matches the surface code checker pattern).
pub fn schema_from_dem(dem: &DetectorErrorModel, name: &str) -> HardwareSchema {
    let mut stabilizers: Vec<StabilizerEntry> = Vec::new();

    for det in &dem.detectors {
        let kind = if let Some(coords) = det.coords {
            let parity = (coords[0].round() as i64 + coords[1].round() as i64).abs() % 2;
            if parity == 0 {
                StabilizerKind::Z
            } else {
                StabilizerKind::X
            }
        } else {
            StabilizerKind::Z
        };

        stabilizers.push(StabilizerEntry {
            id: det.id as u16,
            kind,
            // Qubit indices are not encoded in the DEM — leave empty and let
            // the user populate them from circuit analysis if needed.
            qubits: Vec::new(),
        });
    }

    HardwareSchema {
        schema_id: Uuid::new_v4(),
        version: "1.0.0".to_string(),
        name: name.to_string(),
        description: format!(
            "Auto-generated from DEM: {} detectors, {} observables",
            dem.detector_count, dem.observable_count
        ),
        code_type: "Custom".to_string(),
        distance: 0,
        qubit_count: 0,
        ancilla_count: dem.detector_count as u16,
        stabilizers,
        measurement_cycle_us: 1.0,
        ancilla_layout: "auto".to_string(),
        ldpc_hz_matrix: None,
        ldpc_hx_matrix: None,
        logical_z_matrix: None,
        logical_x_matrix: None,
        encoding_rate: None,
        dem_path: None,
    }
}
