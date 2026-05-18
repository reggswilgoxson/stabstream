use stabstream_core::{
    error::StabstreamError,
    frame::{FileHeader, FrameHeader, QSSF_MAGIC},
};

/// Parse a [`FileHeader`] from the front of a byte slice.
///
/// Returns the parsed header and the number of bytes consumed.
pub fn parse_file_header(input: &[u8]) -> Result<(FileHeader, usize), StabstreamError> {
    if input.len() < 24 {
        return Err(StabstreamError::PayloadLengthMismatch {
            declared: 24,
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

    // TODO: parse schema_id (bytes 6..22) and flags (bytes 22..26) properly
    todo!("implement full file header parsing")
}

/// Parse a [`FrameHeader`] from a 36-byte slice.
///
/// Returns the parsed header and the number of bytes consumed.
pub fn parse_frame_header(input: &[u8]) -> Result<(FrameHeader, usize), StabstreamError> {
    if input.len() < 36 {
        return Err(StabstreamError::PayloadLengthMismatch {
            declared: 36,
            actual: input.len(),
        });
    }

    // TODO: parse all fields and verify CRC32
    todo!("implement frame header parsing")
}
