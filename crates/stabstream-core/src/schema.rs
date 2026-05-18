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
    pub fn load_dir(&mut self, _path: &std::path::Path) -> Result<usize, StabstreamError> {
        // TODO: walk directory, parse each JSON file, insert into self.schemas
        todo!("implement directory schema loading")
    }

    /// Load the built-in schemas bundled with the crate.
    pub fn with_builtins() -> Result<Self, StabstreamError> {
        // TODO: use include_str! to embed the schemas from ../../schemas/
        todo!("implement built-in schema loading")
    }

    /// Look up a schema by UUID.
    pub fn get(&self, id: &Uuid) -> Result<&HardwareSchema, StabstreamError> {
        self.schemas
            .get(id)
            .ok_or(StabstreamError::SchemaNotFound(*id))
    }

    /// Insert a schema into the registry, returning any previously stored schema for that UUID.
    pub fn insert(&mut self, schema: HardwareSchema) -> Option<HardwareSchema> {
        self.schemas.insert(schema.schema_id, schema)
    }
}
