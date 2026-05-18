use std::io::Write;

use stabstream_core::error::StabstreamError;

/// Writes a QSSF stream to a `.qssf.gz` file using zstd compression.
pub struct StreamRecorder<W: Write> {
    encoder: zstd::Encoder<'static, W>,
    frames_written: u64,
}

impl<W: Write> StreamRecorder<W> {
    /// Create a recorder wrapping `writer` with the given zstd compression `level` (1–22).
    pub fn new(writer: W, level: i32) -> Result<Self, StabstreamError> {
        let encoder = zstd::Encoder::new(writer, level)?;
        Ok(Self {
            encoder,
            frames_written: 0,
        })
    }

    /// Write a single raw serialized frame to the compressed stream.
    pub fn write_frame(&mut self, _frame_bytes: &[u8]) -> Result<(), StabstreamError> {
        // TODO: serialize FrameHeader + SyndromePayload → QSSF wire format,
        //       then write to self.encoder
        self.frames_written += 1;
        todo!("implement frame serialization and write")
    }

    /// Flush and finalize the zstd stream. Must be called before dropping.
    pub fn finish(self) -> Result<W, StabstreamError> {
        Ok(self.encoder.finish()?)
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }
}
