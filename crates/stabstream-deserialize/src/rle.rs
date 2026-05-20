/// Encode a detector event bitfield using QSSF run-length encoding.
///
/// Token byte layout: `[mode(1) | run_length(7)]`
/// Mode 0 = run of zeros, mode 1 = run of ones. `run_length` ∈ [1, 127].
pub fn encode_detector_events(events: &[bool]) -> Vec<u8> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(events.len() / 4 + 1);
    let mut bit_value = events[0];
    let mut run: u8 = 0;

    for &b in events {
        if b == bit_value && run < 127 {
            run += 1;
        } else {
            out.push(((bit_value as u8) << 7) | run);
            bit_value = b;
            run = 1;
        }
    }
    out.push(((bit_value as u8) << 7) | run);
    out
}

/// Decode a QSSF RLE-encoded detector event bitfield into a flat `Vec<bool>`.
pub fn decode_detector_events(encoded: &[u8]) -> Vec<bool> {
    let mut out = Vec::new();
    for &token in encoded {
        let mode = (token >> 7) != 0;
        let run = (token & 0x7F) as usize;
        out.extend(std::iter::repeat(mode).take(run));
    }
    out
}

/// Count fired detector events (set bits) in an RLE-encoded bitfield without
/// fully decoding it.
pub fn popcount_rle(encoded: &[u8]) -> u32 {
    encoded
        .iter()
        .filter(|&&t| t & 0x80 != 0) // mode bit = 1 → run of events
        .map(|&t| (t & 0x7F) as u32) // run_length
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(events: &[bool]) {
        let encoded = encode_detector_events(events);
        let decoded = decode_detector_events(&encoded);
        assert_eq!(decoded, events, "round-trip failed");
        assert_eq!(
            popcount_rle(&encoded),
            events.iter().filter(|&&b| b).count() as u32
        );
    }

    #[test]
    fn empty() {
        roundtrip(&[]);
    }

    #[test]
    fn all_zeros() {
        roundtrip(&[false; 24]);
    }

    #[test]
    fn all_ones() {
        roundtrip(&[true; 24]);
    }

    #[test]
    fn alternating() {
        let events: Vec<bool> = (0..24).map(|i| i % 2 == 0).collect();
        roundtrip(&events);
    }

    #[test]
    fn sparse_5_percent() {
        let mut events = vec![false; 100];
        events[3] = true;
        events[17] = true;
        events[42] = true;
        events[91] = true;
        roundtrip(&events);
    }

    #[test]
    fn run_longer_than_127() {
        // A run of 200 identical bits should produce two tokens.
        let events = vec![false; 200];
        let encoded = encode_detector_events(&events);
        assert_eq!(encoded.len(), 2); // 127 + 73
        let decoded = decode_detector_events(&encoded);
        assert_eq!(decoded, events);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn rle_roundtrip(events: Vec<bool>) {
            let encoded = encode_detector_events(&events);
            let decoded = decode_detector_events(&encoded);
            prop_assert_eq!(decoded, events);
        }

        #[test]
        fn popcount_matches_decode(events: Vec<bool>) {
            let encoded = encode_detector_events(&events);
            let by_popcount = popcount_rle(&encoded);
            let by_decode = decode_detector_events(&encoded)
                .iter()
                .filter(|&&b| b)
                .count() as u32;
            prop_assert_eq!(by_popcount, by_decode);
        }
    }
}
