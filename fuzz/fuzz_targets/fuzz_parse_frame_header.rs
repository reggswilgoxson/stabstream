#![no_main]

use libfuzzer_sys::fuzz_target;
use stabstream_deserialize::parser::{parse_frame_header, write_frame_header};

fuzz_target!(|data: &[u8]| {
    if let Ok((hdr, consumed)) = parse_frame_header(data) {
        assert_eq!(consumed, 36);
        // Any successfully parsed header must round-trip through write_frame_header.
        let rewritten = write_frame_header(&hdr);
        let (hdr2, _) = parse_frame_header(&rewritten)
            .expect("write_frame_header must produce a parseable header");
        assert_eq!(hdr.frame_id, hdr2.frame_id);
        assert_eq!(hdr.ancilla_count, hdr2.ancilla_count);
        assert_eq!(hdr.flags, hdr2.flags);
        assert_eq!(hdr.crc32, hdr2.crc32);
    }
});
