use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use stabstream_core::frame::{FrameHeader, QSSF_MAGIC};
use stabstream_deserialize::{parser::write_frame_header, rle::encode_detector_events};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use uuid::Uuid;

/// UUID written into QSSF file headers for Stim-sourced streams.
pub const STIM_GENERIC_UUID: &str = "00000000-5354-494d-0000-000000000001";

/// Encode a single line of Stim 01 detector-event output as QSSF bytes.
fn encode_frame(line: &str, frame_id: u64, ancilla_count: u16) -> Vec<u8> {
    let events: Vec<bool> = line.bytes().map(|b| b == b'1').collect();
    let de_rle = encode_detector_events(&events);
    let meas: Vec<u8> = events
        .iter()
        .map(|&e| if e { 0xFF } else { 0x01 })
        .collect();

    let timestamp_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let hdr = FrameHeader {
        frame_id,
        round: frame_id as u32,
        timestamp_ns,
        qubit_count: 0,
        ancilla_count,
        payload_len: (2 + de_rle.len() + ancilla_count as usize) as u32,
        code_type: 0x01,
        distance: 0,
        flags: 0,
        crc32: 0,
    };
    let hdr_bytes = write_frame_header(&hdr);

    let mut out = Vec::with_capacity(36 + 2 + de_rle.len() + meas.len() + 6);
    out.extend_from_slice(&hdr_bytes);
    out.extend_from_slice(&(de_rle.len() as u16).to_le_bytes());
    out.extend_from_slice(&de_rle);
    out.extend_from_slice(&meas);
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&hdr_bytes).to_le_bytes());
    out
}

/// Build the 26-byte QSSF file header.
fn file_header_bytes(schema_id: Uuid) -> [u8; 26] {
    let mut buf = [0u8; 26];
    buf[0..4].copy_from_slice(&QSSF_MAGIC.to_le_bytes());
    buf[4..6].copy_from_slice(&1u16.to_le_bytes());
    buf[6..22].copy_from_slice(schema_id.as_bytes());
    buf[22..26].copy_from_slice(&0u32.to_le_bytes()); // flags
    buf
}

/// Spawn a `stim detect` subprocess for `circuit_path` and stream QSSF frames
/// to `socket`. The subprocess is given `shots` shots.
///
/// Returns the number of frames written on success.
pub async fn serve_circuit_to_socket(
    circuit_path: &str,
    shots: u64,
    mut socket: TcpStream,
) -> anyhow::Result<u64> {
    let circuit_file = std::fs::File::open(circuit_path)?;

    let mut child = tokio::process::Command::new("stim")
        .args(["detect", "--shots", &shots.to_string()])
        .stdin(Stdio::from(circuit_file))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let mut lines = BufReader::new(stdout).lines();

    let schema_id: Uuid = STIM_GENERIC_UUID.parse().unwrap();

    // Write QSSF file header first.
    socket.write_all(&file_header_bytes(schema_id)).await?;

    let mut frame_id: u64 = 0;
    let mut ancilla_count: Option<u16> = None;

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let ac = *ancilla_count.get_or_insert(trimmed.len() as u16);
        let frame_bytes = encode_frame(trimmed, frame_id, ac);
        socket.write_all(&frame_bytes).await?;
        frame_id += 1;
    }

    child.wait().await?;
    Ok(frame_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_frame_magic_present_in_file_header() {
        let schema_id: Uuid = STIM_GENERIC_UUID.parse().unwrap();
        let hdr = file_header_bytes(schema_id);
        let magic = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        assert_eq!(magic, QSSF_MAGIC);
    }

    #[test]
    fn encode_frame_correct_structure() {
        let bytes = encode_frame("0110", 0, 4);
        // Minimum: 36 (hdr) + 2 (de_len) + rle + 4 (meas) + 6 (terminator)
        assert!(bytes.len() >= 48);
        // Terminator sentinel at correct offset
        let sentinel_off = bytes.len() - 6;
        let sentinel =
            u16::from_le_bytes(bytes[sentinel_off..sentinel_off + 2].try_into().unwrap());
        assert_eq!(sentinel, 0xFFFF);
    }
}
