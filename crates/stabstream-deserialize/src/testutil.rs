//! Test utilities: synthetic QSSF frame generators.
//!
//! Not gated behind `#[cfg(test)]` so benchmark harnesses can import them.

use stabstream_core::frame::{FileHeader, FrameHeader, QSSF_MAGIC};
use uuid::Uuid;

use crate::{parser::write_frame_header, rle::encode_detector_events};

/// UUID of the built-in `surface_code_d5` schema.
pub const SURFACE_D5_UUID: &str = "a3f7c210-4e12-4b0a-9b3c-1f2e8d7a5c01";

/// Build a complete, valid QSSF byte stream containing `frame_count` frames
/// for the `surface_code_d5` configuration (25 qubits, 24 ancillas).
///
/// `fire_rate` controls the fraction of detector events that fire (0.0–1.0).
pub fn synthetic_surface_d5_stream(frame_count: u64, fire_rate: f64) -> Vec<u8> {
    let mut out = Vec::new();

    // File header
    let schema_id: Uuid = SURFACE_D5_UUID.parse().unwrap();
    let file_hdr = FileHeader {
        magic: QSSF_MAGIC,
        version: 1,
        schema_id,
        flags: 0,
    };
    write_file_header(&mut out, &file_hdr);

    let ancilla_count: u16 = 24;

    for i in 0..frame_count {
        // Detector events: fire_rate fraction are set.
        let events: Vec<bool> = (0..ancilla_count as usize)
            .map(|j| (i + j as u64) % ((1.0 / fire_rate.max(0.01)) as u64 + 1) == 0)
            .collect();
        let de_rle = encode_detector_events(&events);

        // meas_results: +1 (0x01) for non-events, -1 (0xFF) for events.
        let meas: Vec<i8> = events.iter().map(|&e| if e { -1i8 } else { 1i8 }).collect();

        let payload_len = (2 + de_rle.len() + ancilla_count as usize) as u32;

        let hdr = FrameHeader {
            frame_id: i,
            round: i as u32,
            timestamp_ns: i * 1_100_000, // 1.1 µs per round
            qubit_count: 25,
            ancilla_count,
            payload_len,
            code_type: 0x01, // SurfaceCode
            distance: 5,
            flags: 0,
            crc32: 0, // recomputed by write_frame_header
        };
        let hdr_bytes = write_frame_header(&hdr);
        out.extend_from_slice(&hdr_bytes);

        // Payload
        out.extend_from_slice(&(de_rle.len() as u16).to_le_bytes());
        out.extend_from_slice(&de_rle);
        let meas_u8: &[u8] = unsafe {
            // SAFETY: i8 and u8 have identical layout
            std::slice::from_raw_parts(meas.as_ptr().cast(), meas.len())
        };
        out.extend_from_slice(meas_u8);

        // Terminator: 0xFFFF + CRC of full frame (header + de_len + de_rle + meas)
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&hdr_bytes);
        hasher.update(&(de_rle.len() as u16).to_le_bytes());
        hasher.update(&de_rle);
        hasher.update(meas_u8);
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        out.extend_from_slice(&hasher.finalize().to_le_bytes());
    }

    out
}

fn write_file_header(out: &mut Vec<u8>, hdr: &FileHeader) {
    out.extend_from_slice(&hdr.magic.to_le_bytes());
    out.extend_from_slice(&hdr.version.to_le_bytes());
    out.extend_from_slice(hdr.schema_id.as_bytes()); // 16 bytes RFC 4122
    out.extend_from_slice(&hdr.flags.to_le_bytes());
}
