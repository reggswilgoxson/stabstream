use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Magic bytes identifying a QSSF file: ASCII "QSSF"
pub const QSSF_MAGIC: u32 = 0x5153_5346;

/// Top-level file header. Fixed 24 bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHeader {
    /// Must equal [`QSSF_MAGIC`].
    pub magic: u32,
    /// Format version, currently 1.
    pub version: u16,
    /// Identifies the hardware schema.
    pub schema_id: Uuid,
    /// Compression hints and ordering flags.
    pub flags: u32,
}

/// Per-round frame header. Fixed 36 bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    /// Monotonic counter.
    pub frame_id: u64,
    /// Measurement round index.
    pub round: u32,
    /// Hardware wall-clock nanoseconds.
    pub timestamp_ns: u64,
    /// Data qubits this round.
    pub qubit_count: u16,
    /// Ancilla qubits measured.
    pub ancilla_count: u16,
    /// Bytes in the syndrome payload.
    pub payload_len: u32,
    /// [`CodeType`] discriminant.
    pub code_type: u8,
    /// Code distance *d*.
    pub distance: u8,
    /// Per-frame flags (bytes 30–31). Bit 0: timing offsets present. Bit 1: parity checks present.
    pub flags: u16,
    /// Header integrity checksum.
    pub crc32: u32,
}

/// Variable-length syndrome payload for one measurement round.
///
/// All slices are zero-copy views into the ring buffer. The lifetime `'buf`
/// is tied to the underlying [`crate::schema::SchemaRegistry`] ring buffer.
#[derive(Debug)]
pub struct SyndromePayload<'buf> {
    /// RLE-encoded detector event bitfield. 1 = syndrome flip vs previous round.
    pub detector_events: &'buf [u8],
    /// Raw ancilla measurement outcomes: +1 or -1 per ancilla.
    pub meas_results: &'buf [i8],
    /// Per-ancilla timing offset in nanoseconds.
    pub timing_offsets: &'buf [u16],
    /// Stabilizer XZ parity flags.
    pub parity_checks: &'buf [u8],
}

/// Optional metadata block (TLV-encoded, hardware-specific).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrameMetadata {
    pub hardware_id: Option<String>,
    /// Fridge temperature in millikelvin.
    pub temperature_mk: Option<f32>,
    /// Measurement cycle time in microseconds.
    pub cycle_us: Option<f32>,
    /// Preferred decoder enum value.
    pub decoder_hint: Option<u8>,
}

/// Optional logical qubit annotations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalAnnotation {
    pub logical_id: u8,
    pub observable_mask: u64,
    /// Frame basis: 0 = Z, 1 = X, 2 = Y.
    pub frame_basis: u8,
}

/// A complete, parsed syndrome frame.
pub struct SyndromeFrame<'buf> {
    pub header: FrameHeader,
    pub payload: SyndromePayload<'buf>,
    pub metadata: Option<FrameMetadata>,
    pub annotations: Option<Vec<LogicalAnnotation>>,
}

impl<'buf> SyndromeFrame<'buf> {
    /// Count the number of detector events that fired this round.
    ///
    /// Uses the QSSF RLE token layout directly: tokens with mode-bit 1
    /// represent runs of fired events; the 7-bit run length is their count.
    pub fn detector_event_count(&self) -> u32 {
        self.payload
            .detector_events
            .iter()
            .filter(|&&t| t & 0x80 != 0)
            .map(|&t| (t & 0x7F) as u32)
            .sum()
    }
}
