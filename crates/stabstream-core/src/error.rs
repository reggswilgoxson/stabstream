use thiserror::Error;

#[derive(Debug, Error)]
pub enum StabstreamError {
    #[error("invalid QSSF magic bytes: expected 0x51535346, got {0:#010x}")]
    InvalidMagic(u32),

    #[error("unsupported QSSF format version: {0}")]
    UnsupportedVersion(u16),

    #[error("unknown code type discriminant: {0:#04x}")]
    UnknownCodeType(u8),

    #[error("schema not found for id {0}")]
    SchemaNotFound(uuid::Uuid),

    #[error("CRC32 mismatch: expected {expected:#010x}, got {actual:#010x}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("parity violation in frame {frame_id} at stabilizer index {stabilizer}")]
    ParityViolation { frame_id: u64, stabilizer: usize },

    #[error("timing offset out of bounds: ancilla {ancilla}, offset {offset_ns} ns")]
    TimingOutOfBounds { ancilla: usize, offset_ns: u16 },

    #[error("payload length mismatch: header declares {declared} bytes, found {actual}")]
    PayloadLengthMismatch { declared: u32, actual: usize },

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json schema error: {0}")]
    SchemaJson(#[from] serde_json::Error),
}
