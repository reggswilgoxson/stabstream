use std::collections::HashMap;
use uuid::Uuid;

use serde::{Deserialize, Serialize};

use crate::error::StabstreamError;

/// A stabilizer entry within a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilizerEntry {
    pub id: u16,
    #[serde(rename = "type")]
    pub kind: StabilizerKind,
    pub qubits: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StabilizerKind {
    X,
    Z,
}

/// A hardware schema loaded from a JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSchema {
    pub schema_id: Uuid,
    pub version: String,
    pub name: String,
    pub description: String,
    pub code_type: String,
    pub distance: u8,
    pub qubit_count: u16,
    pub ancilla_count: u16,
    pub stabilizers: Vec<StabilizerEntry>,
    pub measurement_cycle_us: f32,
    pub ancilla_layout: String,

    // ---- qLDPC extensions (optional, absent for stabilizer codes) ----
    /// Base64-encoded CSR sparse Hz check matrix (Z stabilizers × data qubits).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ldpc_hz_matrix: Option<String>,
    /// Base64-encoded CSR sparse Hx check matrix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ldpc_hx_matrix: Option<String>,
    /// Base64-encoded logical-Z operator matrix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_z_matrix: Option<String>,
    /// Base64-encoded logical-X operator matrix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_x_matrix: Option<String>,
    /// k/n encoding rate, e.g. 12/144 ≈ 0.0833 for the Gross code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_rate: Option<f64>,
    /// Path to the DEM file associated with this schema (informational).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dem_path: Option<String>,
}

/// Registry that maps schema UUIDs to their loaded definitions.
pub struct SchemaRegistry {
    schemas: HashMap<Uuid, HardwareSchema>,
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Load all `.json` schema files from a directory.
    pub fn load_dir(&mut self, path: &std::path::Path) -> Result<usize, StabstreamError> {
        let mut loaded = 0;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json = std::fs::read_to_string(&path)?;
            let schema: HardwareSchema = serde_json::from_str(&json)?;
            self.schemas.insert(schema.schema_id, schema);
            loaded += 1;
        }
        Ok(loaded)
    }

    /// Load the built-in schemas bundled with the crate at compile time.
    pub fn with_builtins() -> Result<Self, StabstreamError> {
        const BUILTIN_SCHEMAS: &[&str] = &[
            include_str!("../../../schemas/surface_code_d3.json"),
            include_str!("../../../schemas/surface_code_d5.json"),
            include_str!("../../../schemas/surface_code_d7.json"),
            include_str!("../../../schemas/honeycomb_d4.json"),
            include_str!("../../../schemas/color_code_d5.json"),
            include_str!("../../../schemas/repetition_d11.json"),
        ];
        let mut registry = Self::new();
        for &json in BUILTIN_SCHEMAS {
            let schema: HardwareSchema = serde_json::from_str(json)?;
            registry.schemas.insert(schema.schema_id, schema);
        }
        Ok(registry)
    }

    /// Look up a schema by UUID.
    pub fn get(&self, id: &Uuid) -> Result<&HardwareSchema, StabstreamError> {
        self.schemas
            .get(id)
            .ok_or(StabstreamError::SchemaNotFound(*id))
    }

    /// Insert a schema, returning any previously stored schema for that UUID.
    pub fn insert(&mut self, schema: HardwareSchema) -> Option<HardwareSchema> {
        self.schemas.insert(schema.schema_id, schema)
    }

    /// Number of schemas currently registered.
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load() {
        let reg = SchemaRegistry::with_builtins().unwrap();
        assert_eq!(reg.len(), 6, "expected 6 built-in schemas");
    }

    #[test]
    fn builtin_surface_d5_uuid() {
        let reg = SchemaRegistry::with_builtins().unwrap();
        let id: Uuid = "a3f7c210-4e12-4b0a-9b3c-1f2e8d7a5c01".parse().unwrap();
        let schema = reg.get(&id).unwrap();
        assert_eq!(schema.distance, 5);
        assert_eq!(schema.qubit_count, 25);
    }
}
