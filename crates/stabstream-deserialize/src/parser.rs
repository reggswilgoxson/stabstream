use stabstream_core::{
    error::StabstreamError,
    frame::{FileHeader, FrameHeader, QSSF_MAGIC},
};
use uuid::Uuid;

/// Parse a [`FileHeader`] from the front of a byte slice.
///
/// Returns `(header, bytes_consumed)` where `bytes_consumed` is 26.
pub fn parse_file_header(input: &[u8]) -> Result<(FileHeader, usize), StabstreamError> {
    if input.len() < 26 {
        return Err(StabstreamError::PayloadLengthMismatch {
            declared: 26,
            actual: input.len(),
        });
    }

    let magic = u32::from_le_bytes(input[0..4].try_into().unwrap());
    if magic != QSSF_MAGIC {
        return Err(StabstreamError::InvalidMagic(magic));
    }

    let version = u16::from_le_bytes(input[4..6].try_into().unwrap());
    if version != 1 {
        return Err(StabstreamError::UnsupportedVersion(version));
    }

    let uuid_bytes: [u8; 16] = input[6..22].try_into().unwrap();
    let schema_id = Uuid::from_bytes(uuid_bytes); // RFC 4122 big-endian

    let flags = u32::from_le_bytes(input[22..26].try_into().unwrap());

    Ok((
        FileHeader {
            magic,
            version,
            schema_id,
            flags,
        },
        26,
    ))
}

/// Parse a [`FrameHeader`] from a 36-byte slice.
///
/// Verifies CRC-32/ISO-HDLC of the first 32 bytes before returning.
/// Returns `(header, 36)` on success.
pub fn parse_frame_header(input: &[u8]) -> Result<(FrameHeader, usize), StabstreamError> {
    if input.len() < 36 {
        return Err(StabstreamError::PayloadLengthMismatch {
            declared: 36,
            actual: input.len(),
        });
    }

    let actual_crc = u32::from_le_bytes(input[32..36].try_into().unwrap());
    let expected_crc = crc32fast::hash(&input[0..32]);
    if expected_crc != actual_crc {
        return Err(StabstreamError::ChecksumMismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }

    let header = FrameHeader {
        frame_id: u64::from_le_bytes(input[0..8].try_into().unwrap()),
        round: u32::from_le_bytes(input[8..12].try_into().unwrap()),
        timestamp_ns: u64::from_le_bytes(input[12..20].try_into().unwrap()),
        qubit_count: u16::from_le_bytes(input[20..22].try_into().unwrap()),
        ancilla_count: u16::from_le_bytes(input[22..24].try_into().unwrap()),
        payload_len: u32::from_le_bytes(input[24..28].try_into().unwrap()),
        code_type: input[28],
        distance: input[29],
        flags: u16::from_le_bytes(input[30..32].try_into().unwrap()),
        crc32: actual_crc,
    };

    Ok((header, 36))
}

/// Serialise a [`FrameHeader`] to a 36-byte array, computing the CRC32 field.
pub fn write_frame_header(h: &FrameHeader) -> [u8; 36] {
    let mut buf = [0u8; 36];
    buf[0..8].copy_from_slice(&h.frame_id.to_le_bytes());
    buf[8..12].copy_from_slice(&h.round.to_le_bytes());
    buf[12..20].copy_from_slice(&h.timestamp_ns.to_le_bytes());
    buf[20..22].copy_from_slice(&h.qubit_count.to_le_bytes());
    buf[22..24].copy_from_slice(&h.ancilla_count.to_le_bytes());
    buf[24..28].copy_from_slice(&h.payload_len.to_le_bytes());
    buf[28] = h.code_type;
    buf[29] = h.distance;
    buf[30..32].copy_from_slice(&h.flags.to_le_bytes());
    let crc = crc32fast::hash(&buf[0..32]);
    buf[32..36].copy_from_slice(&crc.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame_header_bytes(frame_id: u64, ancilla_count: u16) -> [u8; 36] {
        let mut h = FrameHeader {
            frame_id,
            round: 1,
            timestamp_ns: 1_000_000,
            qubit_count: 25,
            ancilla_count,
            payload_len: 0,
            code_type: 0x01,
            distance: 5,
            flags: 0,
            crc32: 0, // filled by write_frame_header
        };
        let bytes = write_frame_header(&h);
        // patch crc32 back into h for round-trip check
        h.crc32 = u32::from_le_bytes(bytes[32..36].try_into().unwrap());
        let _ = h;
        bytes
    }

    #[test]
    fn round_trip_frame_header() {
        let bytes = make_frame_header_bytes(42, 24);
        let (hdr, consumed) = parse_frame_header(&bytes).unwrap();
        assert_eq!(consumed, 36);
        assert_eq!(hdr.frame_id, 42);
        assert_eq!(hdr.ancilla_count, 24);
    }

    #[test]
    fn bad_crc_rejected() {
        let mut bytes = make_frame_header_bytes(1, 24);
        bytes[32] ^= 0xFF; // corrupt the CRC
        assert!(matches!(
            parse_frame_header(&bytes),
            Err(StabstreamError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn bad_magic_rejected() {
        let input = [0u8; 26];
        assert!(matches!(
            parse_file_header(&input),
            Err(StabstreamError::InvalidMagic(_))
        ));
    }

    #[test]
    fn synthetic_stream_frame_header_parses() {
        use crate::testutil::synthetic_surface_d5_stream;
        let bytes = synthetic_surface_d5_stream(1, 0.05);
        // File header is 26 bytes, frame header immediately follows.
        assert!(bytes.len() >= 62, "stream too short: {} bytes", bytes.len());
        parse_frame_header(&bytes[26..62])
            .expect("frame header CRC should be valid in synthetic stream");
    }
}
