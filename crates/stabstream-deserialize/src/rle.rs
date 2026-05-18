/// Decode a run-length encoded detector event bitfield into a flat `Vec<bool>`.
///
/// The QSSF RLE encoding stores alternating (run_length, bit_value) pairs.
/// See `spec/QSSF_FORMAT.md` §4 for the full encoding specification.
pub fn decode_detector_events(_encoded: &[u8]) -> Vec<bool> {
    // TODO: implement QSSF RLE decoding
    todo!("implement RLE decoder")
}

/// Encode a detector event bitfield using QSSF run-length encoding.
pub fn encode_detector_events(_events: &[bool]) -> Vec<u8> {
    // TODO: implement QSSF RLE encoding
    todo!("implement RLE encoder")
}

/// Count the number of set bits (fired detector events) in an RLE-encoded
/// bitfield without fully decoding it.
pub fn popcount_rle(_encoded: &[u8]) -> u32 {
    // TODO: implement zero-copy RLE popcount
    todo!("implement RLE popcount")
}
