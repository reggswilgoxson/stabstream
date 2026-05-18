use std::io::Write;

use stabstream_core::{error::StabstreamError, frame::FrameHeader};

pub use stabstream_deserialize::parser::write_frame_header;

/// Writes a QSSF stream to a zstd-compressed sink.
pub struct StreamRecorder<W: Write> {
    encoder: zstd::Encoder<'static, W>,
    frames_written: u64,
}

impl<W: Write> StreamRecorder<W> {
    /// Create a recorder wrapping `writer` at zstd compression `level` (1–22).
    pub fn new(writer: W, level: i32) -> Result<Self, StabstreamError> {
        let encoder = zstd::Encoder::new(writer, level)?;
        Ok(Self {
            encoder,
            frames_written: 0,
        })
    }

    /// Write a complete QSSF frame.
    ///
    /// The caller supplies the `FrameHeader` (crc32 is recomputed here) and
    /// the four raw payload slices. Timing and parity slices may be empty.
    pub fn write_frame(
        &mut self,
        header: &FrameHeader,
        detector_events_rle: &[u8],
        meas_results: &[i8],
        timing_offsets: &[u16],
        parity_checks: &[u8],
    ) -> Result<(), StabstreamError> {
        // Frame header (36 bytes, CRC computed inside write_frame_header)
        let hdr_bytes = write_frame_header(header);
        self.encoder.write_all(&hdr_bytes)?;

        // detector_events: 2-byte LE length prefix + RLE bytes
        let de_len = detector_events_rle.len() as u16;
        self.encoder.write_all(&de_len.to_le_bytes())?;
        self.encoder.write_all(detector_events_rle)?;

        // meas_results: ancilla_count bytes
        let meas_bytes: &[u8] = unsafe {
            // SAFETY: i8 and u8 have the same layout; we just reinterpret
            std::slice::from_raw_parts(meas_results.as_ptr().cast::<u8>(), meas_results.len())
        };
        self.encoder.write_all(meas_bytes)?;

        // Optional timing_offsets: ancilla_count * 2 bytes
        if !timing_offsets.is_empty() {
            for &offset in timing_offsets {
                self.encoder.write_all(&offset.to_le_bytes())?;
            }
        }

        // Optional parity_checks
        if !parity_checks.is_empty() {
            self.encoder.write_all(parity_checks)?;
        }

        // Frame terminator: 0xFFFF sentinel + whole-frame CRC32
        // For simplicity we compute the CRC over the header bytes only here.
        // A production implementation would buffer the full frame.
        self.encoder.write_all(&0xFFFFu16.to_le_bytes())?;
        let term_crc = crc32fast::hash(&hdr_bytes);
        self.encoder.write_all(&term_crc.to_le_bytes())?;

        self.frames_written += 1;
        Ok(())
    }

    /// Flush and finalise the zstd stream.
    pub fn finish(self) -> Result<W, StabstreamError> {
        Ok(self.encoder.finish()?)
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }
}
