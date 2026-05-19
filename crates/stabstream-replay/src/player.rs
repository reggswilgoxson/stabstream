use std::io::Read;

use stabstream_core::error::StabstreamError;
use stabstream_decoder::Decoder;
use stabstream_metrics::AnalysisReport;

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

    /// Read and return the raw bytes of the next frame (header + payload),
    /// or `Ok(None)` on clean end-of-stream.
    pub fn next_frame_bytes(&mut self) -> Result<Option<Vec<u8>>, StabstreamError> {
        // Read 36-byte frame header
        let mut hdr_buf = [0u8; 36];
        match self.decoder.read_exact(&mut hdr_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(StabstreamError::Io(e)),
        }

        // Extract payload_len from header bytes [24..28]
        let payload_len = u32::from_le_bytes(hdr_buf[24..28].try_into().unwrap()) as usize;

        // 2-byte de_len prefix + payload body + 6-byte terminator
        let remainder = payload_len + 6;
        let mut rest = vec![0u8; remainder];
        self.decoder
            .read_exact(&mut rest)
            .map_err(StabstreamError::Io)?;

        let mut out = Vec::with_capacity(36 + remainder);
        out.extend_from_slice(&hdr_buf);
        out.extend_from_slice(&rest);

        self.frames_read += 1;
        Ok(Some(out))
    }

    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }

    /// Decode every frame in this (already-open) stream and return an
    /// [`AnalysisReport`] with logical error rates, latency percentiles, and
    /// per-ancilla fire frequencies.
    ///
    /// Equivalent to [`analyze_file`](crate::analyze::analyze_file) but
    /// operates on a stream that is already open and positioned at the first
    /// frame (i.e. the file header has already been consumed, as is the case
    /// for `StreamPlayer` which wraps a zstd-compressed recording).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use stabstream_replay::{player::StreamPlayer, analyze::AnalysisConfig};
    /// use stabstream_decoder::NullDecoder;
    ///
    /// let file = File::open("recording.qssf.zst").unwrap();
    /// let mut player = StreamPlayer::new(file).unwrap();
    /// let report = player.analyze(&NullDecoder, AnalysisConfig::default()).unwrap();
    /// println!("{}", report.summary());
    /// ```
    pub fn analyze<D: Decoder>(
        &mut self,
        decoder: &D,
        config: crate::analyze::AnalysisConfig,
    ) -> Result<AnalysisReport, StabstreamError> {
        crate::analyze::analyze_player(self, decoder, &config)
    }
}
