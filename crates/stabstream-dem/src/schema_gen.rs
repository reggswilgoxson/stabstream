//! Generate `HardwareSchema` JSON from a `DetectorErrorModel` or BB parameters.

use stabstream_core::schema::{HardwareSchema, StabilizerEntry, StabilizerKind};
use uuid::Uuid;

use crate::ldpc::{encode_csr_base64, BbParams};
use crate::parser::DetectorErrorModel;

/// Generate a `HardwareSchema` from BB polynomial parameters.
///
/// Builds Hz and Hx check matrices, encodes them as base64 CSR, and populates
/// all stabilizer entries from the matrix rows.
pub fn schema_from_bb(params: &BbParams, name: &str) -> HardwareSchema {
    let (hz_rows, hx_rows) = params.build_check_rows();
    let n = params.n();
    let n_anc = params.l * params.m;

    let mut stabilizers: Vec<StabilizerEntry> = Vec::with_capacity(2 * n_anc);
    for (id, row) in hz_rows.iter().enumerate() {
        stabilizers.push(StabilizerEntry {
            id: id as u16,
            kind: StabilizerKind::Z,
            qubits: row.clone(),
        });
    }
    for (id, row) in hx_rows.iter().enumerate() {
        stabilizers.push(StabilizerEntry {
            id: (n_anc + id) as u16,
            kind: StabilizerKind::X,
            qubits: row.clone(),
        });
    }

    HardwareSchema {
        schema_id: Uuid::new_v4(),
        version: "1.0.0".to_string(),
        name: name.to_string(),
        description: format!(
            "Bivariate Bicycle [[{},{},{}]] code (l={}, m={})",
            n, params.logical_qubits, params.distance, params.l, params.m
        ),
        code_type: "bivariate_bicycle".to_string(),
        distance: params.distance,
        qubit_count: n as u16,
        ancilla_count: params.ancilla_count() as u16,
        stabilizers,
        measurement_cycle_us: 1.1,
        ancilla_layout: "bivariate_bicycle".to_string(),
        ldpc_hz_matrix: Some(encode_csr_base64(&hz_rows, n)),
        ldpc_hx_matrix: Some(encode_csr_base64(&hx_rows, n)),
        logical_z_matrix: None,
        logical_x_matrix: None,
        encoding_rate: Some(params.encoding_rate()),
        dem_path: None,
    }
}

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
