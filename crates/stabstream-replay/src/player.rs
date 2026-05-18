use std::io::Read;

use stabstream_core::error::StabstreamError;

/// Replays a recorded `.qssf.gz` stream, decompressing on the fly.
pub struct StreamPlayer<R: Read> {
    decoder: zstd::Decoder<'static, std::io::BufReader<R>>,
    frames_read: u64,
}

impl<R: Read> StreamPlayer<R> {
    /// Create a player wrapping `reader` (a zstd-compressed QSSF stream).
    pub fn new(reader: R) -> Result<Self, StabstreamError> {
        let decoder = zstd::Decoder::new(reader)?;
        Ok(Self {
            decoder,
            frames_read: 0,
        })
    }

    /// Read and return the raw bytes of the next frame, or `None` on end-of-stream.
    pub fn next_frame_bytes(&mut self) -> Result<Option<Vec<u8>>, StabstreamError> {
        // TODO: read FrameHeader length prefix from self.decoder,
        //       then read payload_len bytes and return the concatenated buffer.
        let _ = &self.decoder;
        todo!("implement frame decompression and read")
    }

    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }
}
