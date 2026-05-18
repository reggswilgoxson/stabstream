use stabstream_core::{error::StabstreamError, frame::SyndromeFrame, schema::SchemaRegistry};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::AsyncRead;

use crate::ring_buffer::RingBuffer;

/// Configuration for a QSSF stream reader.
pub struct StreamConfig {
    pub schema_registry: SchemaRegistry,
    pub validation: ValidationPolicy,
    /// Ring buffer capacity in bytes. Default: 4 MiB.
    pub ring_buf_bytes: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            schema_registry: SchemaRegistry::new(),
            validation: ValidationPolicy::StrictParity,
            ring_buf_bytes: 4 * 1024 * 1024,
        }
    }
}

/// An async QSSF stream. Wraps any [`AsyncRead`] source (TCP socket, file, SHM).
pub struct QssfStream<R: AsyncRead + Unpin> {
    reader: R,
    config: StreamConfig,
    ring_buf: RingBuffer,
}

impl<R: AsyncRead + Unpin> QssfStream<R> {
    pub fn new(reader: R, config: StreamConfig) -> Self {
        let buf_size = config.ring_buf_bytes;
        Self {
            reader,
            config,
            ring_buf: RingBuffer::new(buf_size),
        }
    }

    /// Connect to a QSSF stream over TCP.
    pub async fn connect(
        addr: &str,
        config: StreamConfig,
    ) -> Result<QssfStream<tokio::net::TcpStream>, StabstreamError> {
        let stream = tokio::net::TcpStream::connect(addr).await?;
        Ok(QssfStream::new(stream, config))
    }

    /// Read and return the next parsed, validated [`SyndromeFrame`].
    ///
    /// Returns `Ok(None)` on clean end-of-stream.
    pub async fn next_frame<'a>(
        &'a mut self,
    ) -> Result<Option<SyndromeFrame<'a>>, StabstreamError> {
        // TODO:
        // 1. Fill ring buffer from self.reader
        // 2. Parse frame header via crate::parser::parse_frame_header
        // 3. Parse syndrome payload (zero-copy slice into ring_buf)
        // 4. Run validation via stabstream_validate
        // 5. Return the frame; advance ring buffer read cursor
        let _ = &self.reader;
        let _ = &self.config;
        let _ = &self.ring_buf;
        todo!("implement async frame reading pipeline")
    }
}
