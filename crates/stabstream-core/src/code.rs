use serde::{Deserialize, Serialize};

/// Identifies the quantum error correction code family.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeType {
    SurfaceCode = 0x01,
    HoneycombCode = 0x02,
    ColorCode = 0x03,
    RepetitionCode = 0x04,
    ToricCode = 0x05,
    Custom = 0xFF,
}

impl TryFrom<u8> for CodeType {
    type Error = crate::error::StabstreamError;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0x01 => Ok(Self::SurfaceCode),
            0x02 => Ok(Self::HoneycombCode),
            0x03 => Ok(Self::ColorCode),
            0x04 => Ok(Self::RepetitionCode),
            0x05 => Ok(Self::ToricCode),
            0xFF => Ok(Self::Custom),
            other => Err(crate::error::StabstreamError::UnknownCodeType(other)),
        }
    }
}

/// Static metadata for a code at a given distance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeMetadata {
    pub code_type: CodeType,
    pub distance: u8,
    pub qubit_count: u16,
    pub ancilla_count: u16,
    /// Human-readable name, e.g. "Rotated surface code d=5"
    pub display_name: String,
}
