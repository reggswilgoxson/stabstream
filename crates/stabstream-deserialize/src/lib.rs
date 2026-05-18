pub mod parser;
pub mod ring_buffer;
pub mod rle;
pub mod stream;
pub mod testutil;

#[cfg(test)]
mod integration {
    use crate::{
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
}
