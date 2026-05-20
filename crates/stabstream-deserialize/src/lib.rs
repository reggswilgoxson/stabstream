pub mod parser;
pub mod ring_buffer;
pub mod rle;
pub mod stream;
pub mod testutil;

#[cfg(test)]
mod helpers {
    use stabstream_core::frame::{FrameHeader, QSSF_MAGIC};
    use uuid::Uuid;

    use crate::{parser::write_frame_header, rle::encode_detector_events};

    /// Build a minimal 2-frame QSSF stream where the second frame has
    /// `frame_id = second_id` (may be out-of-order relative to the first).
    pub fn two_frame_stream(first_id: u64, second_id: u64) -> Vec<u8> {
        let mut out = Vec::new();
        let schema_id: Uuid = crate::testutil::SURFACE_D5_UUID.parse().unwrap();
        // File header
        out.extend_from_slice(&QSSF_MAGIC.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(schema_id.as_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        for frame_id in [first_id, second_id] {
            let events = vec![false; 24];
            let de_rle = encode_detector_events(&events);
            let meas = vec![1u8; 24];
            let payload_len = (2 + de_rle.len() + 24) as u32;
            let hdr = FrameHeader {
                frame_id,
                round: frame_id as u32,
                timestamp_ns: frame_id * 1_100_000,
                qubit_count: 25,
                ancilla_count: 24,
                payload_len,
                code_type: 0x01,
                distance: 5,
                flags: 0,
                crc32: 0,
            };
            let hdr_bytes = write_frame_header(&hdr);
            out.extend_from_slice(&hdr_bytes);
            out.extend_from_slice(&(de_rle.len() as u16).to_le_bytes());
            out.extend_from_slice(&de_rle);
            out.extend_from_slice(&meas);
            out.extend_from_slice(&0xFFFFu16.to_le_bytes());
            out.extend_from_slice(&crc32fast::hash(&hdr_bytes).to_le_bytes());
        }
        out
    }
}

#[cfg(test)]
mod integration {
    use stabstream_core::error::StabstreamError;

    use crate::{
        helpers::two_frame_stream,
        stream::{QssfStream, StreamConfig},
        testutil::synthetic_surface_d5_stream,
    };
    use stabstream_validate::policy::ValidationPolicy;

    #[tokio::test]
    async fn parse_single_frame_roundtrip() {
        let bytes = synthetic_surface_d5_stream(1, 0.05);
        let cursor = std::io::Cursor::new(&bytes);
        let reader = tokio::io::BufReader::new(cursor);
        let config = StreamConfig {
            validation: ValidationPolicy::Disabled,
            ..Default::default()
        };
        let mut stream = QssfStream::new(reader, config);

        let frame = stream
            .next_frame()
            .await
            .unwrap()
            .expect("expected a frame");
        assert_eq!(frame.header.frame_id, 0);
        assert_eq!(frame.header.ancilla_count, 24);
        assert_eq!(frame.header.distance, 5);

        // Next call should return None (clean EOF)
        let eof = stream.next_frame().await.unwrap();
        assert!(eof.is_none(), "expected EOF after first frame");
    }

    #[tokio::test]
    async fn parse_multiple_frames() {
        const N: u64 = 10;
        let bytes = synthetic_surface_d5_stream(N, 0.1);
        let cursor = std::io::Cursor::new(&bytes);
        let reader = tokio::io::BufReader::new(cursor);
        let config = StreamConfig {
            validation: ValidationPolicy::Disabled,
            ..Default::default()
        };
        let mut stream = QssfStream::new(reader, config);

        for expected_id in 0..N {
            let frame = stream
                .next_frame()
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("expected frame {expected_id}"));
            assert_eq!(frame.header.frame_id, expected_id);
        }

        assert!(stream.next_frame().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn out_of_order_frame_id_rejected() {
        // Frame 0 then frame 0 again (not strictly increasing).
        let bytes = two_frame_stream(5, 3);
        let cursor = std::io::Cursor::new(&bytes);
        let reader = tokio::io::BufReader::new(cursor);
        let config = StreamConfig {
            validation: ValidationPolicy::Disabled,
            ..Default::default()
        };
        let mut stream = QssfStream::new(reader, config);

        // First frame (id=5) must succeed.
        stream
            .next_frame()
            .await
            .unwrap()
            .expect("expected first frame");

        // Second frame (id=3) must be rejected with FrameOutOfOrder.
        let err = match stream.next_frame().await {
            Err(e) => e,
            Ok(_) => panic!("expected FrameOutOfOrder error, got Ok"),
        };
        assert!(
            matches!(err, StabstreamError::FrameOutOfOrder { last_id: 5, got: 3 }),
            "unexpected error: {err:?}",
        );
    }
}
