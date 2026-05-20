use stabstream_core::{
    error::StabstreamError,
    frame::{FileHeader, FrameHeader, FrameMetadata, LogicalAnnotation, QSSF_MAGIC},
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// TLV tag IDs for FrameMetadata fields
// ---------------------------------------------------------------------------
const TAG_HARDWARE_ID: u16 = 0x0001;
const TAG_TEMPERATURE_MK: u16 = 0x0002;
const TAG_CYCLE_US: u16 = 0x0003;
const TAG_DECODER_HINT: u16 = 0x0004;
const TAG_OBSERVABLE_FLIPS: u16 = 0x0010;

// ---------------------------------------------------------------------------
// TLV metadata block — write
// ---------------------------------------------------------------------------

/// Serialise a [`FrameMetadata`] as a TLV block: `u16-LE tag_count` followed by
/// `(tag: u16-LE)(len: u16-LE)(value: len bytes)` per field.
pub fn write_metadata_tlv(meta: &FrameMetadata) -> Vec<u8> {
    let mut entries: Vec<(u16, Vec<u8>)> = Vec::new();

    if let Some(ref id) = meta.hardware_id {
        entries.push((TAG_HARDWARE_ID, id.as_bytes().to_vec()));
    }
    if let Some(v) = meta.temperature_mk {
        entries.push((TAG_TEMPERATURE_MK, v.to_le_bytes().to_vec()));
    }
    if let Some(v) = meta.cycle_us {
        entries.push((TAG_CYCLE_US, v.to_le_bytes().to_vec()));
    }
    if let Some(v) = meta.decoder_hint {
        entries.push((TAG_DECODER_HINT, vec![v]));
    }
    if let Some(v) = meta.observable_flips {
        entries.push((TAG_OBSERVABLE_FLIPS, v.to_le_bytes().to_vec()));
    }

    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, val) in &entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&(val.len() as u16).to_le_bytes());
        out.extend_from_slice(val);
    }
    out
}

// ---------------------------------------------------------------------------
// TLV metadata block — parse
// ---------------------------------------------------------------------------

/// Parse a TLV metadata block from a byte slice, returning a [`FrameMetadata`].
///
/// Unknown tags are silently skipped.
pub fn parse_metadata_tlv(bytes: &[u8]) -> FrameMetadata {
    let mut meta = FrameMetadata::default();
    if bytes.len() < 2 {
        return meta;
    }
    let tag_count = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    let mut cursor = 2;
    for _ in 0..tag_count {
        if cursor + 4 > bytes.len() {
            break;
        }
        let tag = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]);
        let val_len = u16::from_le_bytes([bytes[cursor + 2], bytes[cursor + 3]]) as usize;
        cursor += 4;
        if cursor + val_len > bytes.len() {
            break;
        }
        let val = &bytes[cursor..cursor + val_len];
        match (tag, val_len) {
            (TAG_HARDWARE_ID, _) => {
                meta.hardware_id = String::from_utf8(val.to_vec()).ok();
            }
            (TAG_TEMPERATURE_MK, 4) => {
                meta.temperature_mk = Some(f32::from_le_bytes(val.try_into().unwrap()));
            }
            (TAG_CYCLE_US, 4) => {
                meta.cycle_us = Some(f32::from_le_bytes(val.try_into().unwrap()));
            }
            (TAG_DECODER_HINT, 1) => {
                meta.decoder_hint = Some(val[0]);
            }
            (TAG_OBSERVABLE_FLIPS, 8) => {
                meta.observable_flips = Some(u64::from_le_bytes(val.try_into().unwrap()));
            }
            _ => {} // unknown or malformed tag — skip
        }
        cursor += val_len;
    }
    meta
}

// ---------------------------------------------------------------------------
// Logical annotation block — write / parse
// ---------------------------------------------------------------------------

/// Serialise a slice of [`LogicalAnnotation`]s: `u8 count` + `10 bytes each`.
///
/// Each annotation is: `logical_id(1) observable_mask(8 LE) frame_basis(1)`.
pub fn write_annotations(anns: &[LogicalAnnotation]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + anns.len() * 10);
    out.push(anns.len().min(255) as u8);
    for a in anns.iter().take(255) {
        out.push(a.logical_id);
        out.extend_from_slice(&a.observable_mask.to_le_bytes());
        out.push(a.frame_basis);
    }
    out
}

/// Parse a logical annotation block from a byte slice.
pub fn parse_annotations(bytes: &[u8]) -> Vec<LogicalAnnotation> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let count = bytes[0] as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 1 + i * 10;
        if offset + 10 > bytes.len() {
            break;
        }
        let logical_id = bytes[offset];
        let observable_mask = u64::from_le_bytes(bytes[offset + 1..offset + 9].try_into().unwrap());
        let frame_basis = bytes[offset + 9];
        out.push(LogicalAnnotation {
            logical_id,
            observable_mask,
            frame_basis,
        });
    }
    out
}

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
    fn metadata_tlv_round_trip() {
        let meta = FrameMetadata {
            hardware_id: Some("ibm_sherbrooke".to_string()),
            temperature_mk: Some(15.3),
            cycle_us: Some(1.1),
            decoder_hint: Some(2),
            observable_flips: Some(0b101),
        };
        let bytes = write_metadata_tlv(&meta);
        let parsed = parse_metadata_tlv(&bytes);
        assert_eq!(parsed.hardware_id.as_deref(), Some("ibm_sherbrooke"));
        assert!((parsed.temperature_mk.unwrap() - 15.3).abs() < 1e-5);
        assert!((parsed.cycle_us.unwrap() - 1.1).abs() < 1e-5);
        assert_eq!(parsed.decoder_hint, Some(2));
        assert_eq!(parsed.observable_flips, Some(0b101));
    }

    #[test]
    fn metadata_tlv_observable_flips_only() {
        let meta = FrameMetadata {
            observable_flips: Some(0xDEADBEEF_CAFEBABE),
            ..Default::default()
        };
        let bytes = write_metadata_tlv(&meta);
        // 2 (count) + 4 (tag+len) + 8 (u64) = 14 bytes
        assert_eq!(bytes.len(), 14);
        let parsed = parse_metadata_tlv(&bytes);
        assert_eq!(parsed.observable_flips, Some(0xDEADBEEF_CAFEBABE));
        assert!(parsed.hardware_id.is_none());
    }

    #[test]
    fn metadata_tlv_empty() {
        let meta = FrameMetadata::default();
        let bytes = write_metadata_tlv(&meta);
        assert_eq!(bytes, vec![0, 0]); // tag count = 0
        let parsed = parse_metadata_tlv(&bytes);
        assert!(parsed.observable_flips.is_none());
    }

    #[test]
    fn annotations_round_trip() {
        let anns = vec![
            LogicalAnnotation {
                logical_id: 0,
                observable_mask: 0b01,
                frame_basis: 0,
            },
            LogicalAnnotation {
                logical_id: 1,
                observable_mask: 0b10,
                frame_basis: 1,
            },
        ];
        let bytes = write_annotations(&anns);
        // 1 (count) + 2 * 10 = 21 bytes
        assert_eq!(bytes.len(), 21);
        let parsed = parse_annotations(&bytes);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].logical_id, 0);
        assert_eq!(parsed[0].observable_mask, 0b01);
        assert_eq!(parsed[0].frame_basis, 0);
        assert_eq!(parsed[1].logical_id, 1);
        assert_eq!(parsed[1].observable_mask, 0b10);
        assert_eq!(parsed[1].frame_basis, 1);
    }

    #[test]
    fn annotations_empty() {
        let bytes = write_annotations(&[]);
        assert_eq!(bytes, vec![0]); // count = 0
        let parsed = parse_annotations(&bytes);
        assert!(parsed.is_empty());
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
