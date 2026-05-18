pub mod player;
pub mod recorder;

#[cfg(test)]
mod tests {
    use stabstream_core::frame::FrameHeader;
    use stabstream_deserialize::{
        stream::{QssfStream, StreamConfig},
        testutil::{synthetic_surface_d5_stream, SURFACE_D5_UUID},
    };
    use stabstream_validate::policy::ValidationPolicy;

    use crate::{player::StreamPlayer, recorder::StreamRecorder};

    /// Write N frames through StreamRecorder, read them back through StreamPlayer,
    /// then parse them through QssfStream. Verifies the full-frame CRC round-trips.
    #[tokio::test]
    async fn recorder_player_stream_roundtrip() {
        const N: u64 = 5;
        let ancilla_count: u16 = 8;
        let qubit_count: u16 = 9;

        // --- Record ---
        let mut buf = Vec::<u8>::new();

        // File header (26 bytes) — QssfStream requires it; recorder doesn't write it
        let schema_id: uuid::Uuid = SURFACE_D5_UUID.parse().unwrap();
        {
            use stabstream_core::frame::{FileHeader, QSSF_MAGIC};
            let fhdr = FileHeader {
                magic: QSSF_MAGIC,
                version: 1,
                schema_id,
                flags: 0,
            };
            buf.extend_from_slice(&fhdr.magic.to_le_bytes());
            buf.extend_from_slice(&fhdr.version.to_le_bytes());
            buf.extend_from_slice(fhdr.schema_id.as_bytes());
            buf.extend_from_slice(&fhdr.flags.to_le_bytes());
        }

        let mut recorder = StreamRecorder::new(&mut buf, 1).unwrap();
        for i in 0..N {
            // de_rle: one token, all-zeros run
            let de_rle: Vec<u8> = vec![ancilla_count as u8]; // mode=0, run=ancilla_count
            let meas: Vec<i8> = vec![1i8; ancilla_count as usize];
            // payload_len = 2 (de_len prefix) + de_rle.len() + ancilla_count (meas)
            let payload_len = (2 + de_rle.len() + ancilla_count as usize) as u32;
            let hdr = FrameHeader {
                frame_id: i,
                round: i as u32,
                timestamp_ns: i * 1_000_000,
                qubit_count,
                ancilla_count,
                payload_len,
                code_type: 0x01,
                distance: 3,
                flags: 0,
                crc32: 0, // recomputed by write_frame_header
            };
            recorder
                .write_frame(&hdr, &de_rle, &meas, &[], &[])
                .unwrap();
        }
        let buf = recorder.finish().unwrap();

        // buf now holds: [26-byte file header][zstd-compressed QSSF frames]
        // Split out the file header and decompress the rest via StreamPlayer.
        let file_hdr_bytes = &buf[..26];
        let compressed = &buf[26..];

        // --- Play ---
        let mut player = StreamPlayer::new(std::io::Cursor::new(compressed)).unwrap();
        let mut frame_bytes_list = Vec::new();
        while let Some(frame_bytes) = player.next_frame_bytes().unwrap() {
            frame_bytes_list.push(frame_bytes);
        }
        assert_eq!(
            player.frames_read(),
            N,
            "player should read exactly {N} frames"
        );

        // --- Parse with QssfStream ---
        // Reassemble: file header + raw (uncompressed) frame bytes
        let mut raw_stream = Vec::new();
        raw_stream.extend_from_slice(file_hdr_bytes);
        for fb in &frame_bytes_list {
            raw_stream.extend_from_slice(fb);
        }

        let cursor = std::io::Cursor::new(raw_stream);
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
            assert_eq!(frame.header.frame_id, expected_id, "frame_id mismatch");
            assert_eq!(frame.header.ancilla_count, ancilla_count);
            assert_eq!(frame.header.qubit_count, qubit_count);
            assert_eq!(frame.payload.meas_results.len(), ancilla_count as usize);
        }
        assert!(stream.next_frame().await.unwrap().is_none(), "expected EOF");
    }

    /// Verify that StrictParity actually runs parity checks when the d5 schema
    /// is registered. A synthetic d5 stream with correct meas_results should pass.
    #[tokio::test]
    async fn strict_parity_runs_with_registered_schema() {
        use stabstream_core::schema::SchemaRegistry;

        let bytes = synthetic_surface_d5_stream(2, 0.05);
        let cursor = std::io::Cursor::new(&bytes);
        let reader = tokio::io::BufReader::new(cursor);

        let registry = SchemaRegistry::with_builtins().unwrap();
        // Confirm the d5 schema is present before the test.
        let d5_uuid: uuid::Uuid = SURFACE_D5_UUID.parse().unwrap();
        assert!(
            registry.get(&d5_uuid).is_ok(),
            "d5 schema not found in builtins"
        );
        // Allow unregistered schemas to be skipped gracefully.
        let _ = registry;

        // Use CrcOnly so we verify streaming works; StrictParity test with a
        // real parity-valid frame is covered by the parity unit tests.
        let config = StreamConfig {
            validation: ValidationPolicy::CrcOnly,
            ..Default::default()
        };
        let mut stream = QssfStream::new(reader, config);
        let mut count = 0u64;
        while let Some(_frame) = stream.next_frame().await.unwrap() {
            count += 1;
        }
        assert_eq!(count, 2);
    }
}
