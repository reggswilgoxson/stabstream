#![no_main]

use libfuzzer_sys::fuzz_target;
use stabstream_deserialize::rle::{decode_detector_events, popcount_rle};

// Feed arbitrary bytes as if they were an RLE-encoded detector-event bitfield.
// Invariant: popcount_rle must agree with the decoded fired-bit count.
fuzz_target!(|data: &[u8]| {
    let decoded = decode_detector_events(data);
    let by_popcount = popcount_rle(data);
    let by_decode = decoded.iter().filter(|&&b| b).count() as u32;
    assert_eq!(by_popcount, by_decode);
});
