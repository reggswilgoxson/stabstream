use stabstream_core::{
    error::StabstreamError,
    frame::{FrameHeader, FrameMetadata, LogicalAnnotation, SyndromeFrame, SyndromePayload},
    schema::SchemaRegistry,
};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::{AsyncRead, AsyncReadExt};
use uuid::Uuid;

use crate::{parser, ring_buffer::RingBuffer};

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
    /// Scratch buffer used by `peek_wrapped` when payload data straddles the ring end.
    /// Pre-allocated to MAX_FRAME_SIZE; never grows during normal operation.
    scratch: Vec<u8>,
    /// True after the file header has been parsed.
    header_consumed: bool,
    /// Schema UUID read from the file header; used for StrictParity validation.
    schema_id: Uuid,
    /// Last seen frame_id; used to detect out-of-order frames.
    last_frame_id: Option<u64>,
}

impl<R: AsyncRead + Unpin> QssfStream<R> {
    pub fn new(reader: R, config: StreamConfig) -> Self {
        let buf_size = config.ring_buf_bytes;
        Self {
            reader,
            config,
            ring_buf: RingBuffer::new(buf_size),
            scratch: Vec::with_capacity(4096),
            header_consumed: false,
            schema_id: Uuid::nil(),
            last_frame_id: None,
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

    /// Fill the ring buffer from the underlying reader until at least `needed`
    /// bytes are available, or EOF is reached. Returns `false` on clean EOF
    /// when the buffer is empty.
    async fn fill_until(&mut self, needed: usize) -> Result<bool, StabstreamError> {
        while self.ring_buf.available_read() < needed {
            // Use a temporary stack buffer for the read; then copy into the ring.
            let mut tmp = [0u8; 4096];
            let n = self.reader.read(&mut tmp).await?;
            if n == 0 {
                return Ok(false);
            }
            let written = self.ring_buf.write(&tmp[..n]);
            if written < n {
                // Ring buffer full — stream is producing faster than we consume.
                // This is a logic error: increase ring_buf_bytes.
                return Err(StabstreamError::PayloadLengthMismatch {
                    declared: n as u32,
                    actual: written,
                });
            }
        }
        Ok(true)
    }

    /// Read and return the next parsed, validated [`SyndromeFrame`].
    ///
    /// Returns `Ok(None)` on clean end-of-stream.
    ///
    /// When the `otel` feature is enabled and `stabstream_core::otel::install()`
    /// has been called, this method emits the following spans exported via OTLP:
    /// - `qssf.frame_parse`  — covers header + payload deserialization
    /// - `qssf.frame_validate` — covers parity/timing validation
    pub async fn next_frame<'a>(
        &'a mut self,
    ) -> Result<Option<SyndromeFrame<'a>>, StabstreamError> {
        let parse_span = tracing::info_span!("qssf.frame_parse");
        // Consume the file header on the first call.
        if !self.header_consumed {
            if !self.fill_until(26).await? {
                return Ok(None);
            }
            let header_bytes =
                self.ring_buf
                    .peek(26)
                    .ok_or(StabstreamError::PayloadLengthMismatch {
                        declared: 26,
                        actual: self.ring_buf.available_read(),
                    })?;
            let (file_hdr, consumed) = parser::parse_file_header(header_bytes)?;
            self.schema_id = file_hdr.schema_id;
            self.ring_buf.consume(consumed);
            self.header_consumed = true;
        }

        // Parse the frame header (36 bytes).
        if !self.fill_until(36).await? {
            return Ok(None);
        }
        let (frame_hdr, saved_hdr): (FrameHeader, [u8; 36]) = {
            let hdr_bytes =
                self.ring_buf
                    .peek(36)
                    .ok_or(StabstreamError::PayloadLengthMismatch {
                        declared: 36,
                        actual: self.ring_buf.available_read(),
                    })?;
            let (h, _) = parser::parse_frame_header(hdr_bytes)?;
            let saved: [u8; 36] = hdr_bytes.try_into().expect("peek returned 36 bytes");
            self.ring_buf.consume(36);
            (h, saved)
        };

        // Enforce monotonically increasing frame IDs.
        if let Some(last_id) = self.last_frame_id {
            if frame_hdr.frame_id <= last_id {
                return Err(StabstreamError::FrameOutOfOrder {
                    last_id,
                    got: frame_hdr.frame_id,
                });
            }
        }
        self.last_frame_id = Some(frame_hdr.frame_id);

        // --- Parse syndrome payload fields ---
        let ancilla = frame_hdr.ancilla_count as usize;

        // 1. detector_events: 2-byte length prefix + RLE bytes
        if !self.fill_until(2).await? {
            return Err(StabstreamError::PayloadLengthMismatch {
                declared: 2,
                actual: self.ring_buf.available_read(),
            })
            .map(|_: Option<SyndromeFrame<'a>>| None);
        }
        let de_len = {
            let b = self
                .ring_buf
                .peek(2)
                .ok_or(StabstreamError::PayloadLengthMismatch {
                    declared: 2,
                    actual: self.ring_buf.available_read(),
                })?;
            u16::from_le_bytes([b[0], b[1]]) as usize
        };
        self.ring_buf.consume(2);

        // 2. Load all payload bytes into the ring at once.
        let timing_present = frame_hdr.flags & 0x01 != 0; // flag bit 0: timing offsets present
        let parity_present = frame_hdr.flags & 0x02 != 0; // flag bit 1: parity checks present
        let timing_len = if timing_present { ancilla * 2 } else { 0 };
        let parity_len = if parity_present {
            ancilla.div_ceil(8)
        } else {
            0
        };
        let total_payload = de_len + ancilla + timing_len + parity_len;

        if !self.fill_until(total_payload).await? {
            return Err(StabstreamError::PayloadLengthMismatch {
                declared: total_payload as u32,
                actual: self.ring_buf.available_read(),
            })
            .map(|_: Option<SyndromeFrame<'a>>| None);
        }

        // SAFETY: peek_wrapped gives a borrow whose data lives either in ring_buf.buf
        // (fast path, no wrap) or in self.scratch (wrap path). Both are heap-allocated
        // and stable. We extend the lifetime to 'a: next_frame takes &'a mut self so
        // no other mutable access to ring_buf or scratch is possible while frames are live.
        let de_slice: &'a [u8] = {
            let s = self
                .ring_buf
                .peek_wrapped(de_len, &mut self.scratch)
                .ok_or(StabstreamError::PayloadLengthMismatch {
                    declared: de_len as u32,
                    actual: self.ring_buf.available_read(),
                })?;
            unsafe { std::slice::from_raw_parts(s.as_ptr(), s.len()) }
        };
        self.ring_buf.consume(de_len);

        let meas_slice: &'a [i8] = {
            let s = self
                .ring_buf
                .peek_wrapped(ancilla, &mut self.scratch)
                .ok_or(StabstreamError::PayloadLengthMismatch {
                    declared: ancilla as u32,
                    actual: self.ring_buf.available_read(),
                })?;
            unsafe { std::slice::from_raw_parts(s.as_ptr().cast::<i8>(), s.len()) }
        };
        self.ring_buf.consume(ancilla);

        let timing_slice: &'a [u16] = if timing_len > 0 {
            let s = self
                .ring_buf
                .peek_wrapped(timing_len, &mut self.scratch)
                .ok_or(StabstreamError::PayloadLengthMismatch {
                    declared: timing_len as u32,
                    actual: self.ring_buf.available_read(),
                })?;
            let slice = unsafe { std::slice::from_raw_parts(s.as_ptr().cast::<u16>(), ancilla) };
            self.ring_buf.consume(timing_len);
            slice
        } else {
            &[]
        };

        let parity_slice: &'a [u8] = if parity_len > 0 {
            let s = self
                .ring_buf
                .peek_wrapped(parity_len, &mut self.scratch)
                .ok_or(StabstreamError::PayloadLengthMismatch {
                    declared: parity_len as u32,
                    actual: self.ring_buf.available_read(),
                })?;
            let slice = unsafe { std::slice::from_raw_parts(s.as_ptr(), s.len()) };
            self.ring_buf.consume(parity_len);
            slice
        } else {
            &[]
        };

        // --- TLV metadata block (flag bit 2) ---
        let metadata: Option<FrameMetadata> =
            if frame_hdr.flags & 0x04 != 0 {
                // Read 2-byte tag count
                if !self.fill_until(2).await? {
                    return Err(StabstreamError::PayloadLengthMismatch {
                        declared: 2,
                        actual: self.ring_buf.available_read(),
                    })
                    .map(|_: Option<SyndromeFrame<'a>>| None);
                }
                let tag_count = {
                    let b =
                        self.ring_buf
                            .peek(2)
                            .ok_or(StabstreamError::PayloadLengthMismatch {
                                declared: 2,
                                actual: self.ring_buf.available_read(),
                            })?;
                    u16::from_le_bytes([b[0], b[1]]) as usize
                };
                self.ring_buf.consume(2);

                let mut meta = FrameMetadata::default();
                for _ in 0..tag_count {
                    // Read tag (2) + len (2)
                    if !self.fill_until(4).await? {
                        break;
                    }
                    let (tag, val_len) = {
                        let b = self.ring_buf.peek(4).ok_or(
                            StabstreamError::PayloadLengthMismatch {
                                declared: 4,
                                actual: self.ring_buf.available_read(),
                            },
                        )?;
                        (
                            u16::from_le_bytes([b[0], b[1]]),
                            u16::from_le_bytes([b[2], b[3]]) as usize,
                        )
                    };
                    self.ring_buf.consume(4);

                    if !self.fill_until(val_len).await? {
                        break;
                    }
                    let val_slice = self.ring_buf.peek(val_len).ok_or(
                        StabstreamError::PayloadLengthMismatch {
                            declared: val_len as u32,
                            actual: self.ring_buf.available_read(),
                        },
                    )?;
                    // Decode known tags
                    match (tag, val_len) {
                        (0x0001, _) => {
                            meta.hardware_id = String::from_utf8(val_slice.to_vec()).ok();
                        }
                        (0x0002, 4) => {
                            meta.temperature_mk =
                                Some(f32::from_le_bytes(val_slice.try_into().unwrap()));
                        }
                        (0x0003, 4) => {
                            meta.cycle_us = Some(f32::from_le_bytes(val_slice.try_into().unwrap()));
                        }
                        (0x0004, 1) => {
                            meta.decoder_hint = Some(val_slice[0]);
                        }
                        (0x0010, 8) => {
                            meta.observable_flips =
                                Some(u64::from_le_bytes(val_slice.try_into().unwrap()));
                        }
                        _ => {} // unknown or malformed tag — skip
                    }
                    self.ring_buf.consume(val_len);
                }
                Some(meta)
            } else {
                None
            };

        // --- Logical annotation block (flag bit 3) ---
        let annotations: Option<Vec<LogicalAnnotation>> = if frame_hdr.flags & 0x08 != 0 {
            // 1-byte count
            if !self.fill_until(1).await? {
                return Err(StabstreamError::PayloadLengthMismatch {
                    declared: 1,
                    actual: self.ring_buf.available_read(),
                })
                .map(|_: Option<SyndromeFrame<'a>>| None);
            }
            let count = self
                .ring_buf
                .peek(1)
                .ok_or(StabstreamError::PayloadLengthMismatch {
                    declared: 1,
                    actual: self.ring_buf.available_read(),
                })?[0] as usize;
            self.ring_buf.consume(1);

            let ann_bytes_len = count * 10;
            if ann_bytes_len > 0 {
                if !self.fill_until(ann_bytes_len).await? {
                    return Err(StabstreamError::PayloadLengthMismatch {
                        declared: ann_bytes_len as u32,
                        actual: self.ring_buf.available_read(),
                    })
                    .map(|_: Option<SyndromeFrame<'a>>| None);
                }
                let raw = self.ring_buf.peek(ann_bytes_len).ok_or(
                    StabstreamError::PayloadLengthMismatch {
                        declared: ann_bytes_len as u32,
                        actual: self.ring_buf.available_read(),
                    },
                )?;
                // Prepend the count byte for parse_annotations
                let mut ann_buf = Vec::with_capacity(1 + ann_bytes_len);
                ann_buf.push(count as u8);
                ann_buf.extend_from_slice(raw);
                self.ring_buf.consume(ann_bytes_len);
                Some(parser::parse_annotations(&ann_buf))
            } else {
                Some(Vec::new())
            }
        } else {
            None
        };

        // Validate frame terminator: 0xFFFF sentinel (2 bytes) + CRC32 of header (4 bytes).
        if self.fill_until(6).await? {
            if let Some(term) = self.ring_buf.peek(6) {
                let sentinel = u16::from_le_bytes([term[0], term[1]]);
                let stored_crc = u32::from_le_bytes(term[2..6].try_into().unwrap());
                let expected_crc = crc32fast::hash(&saved_hdr);
                if sentinel != 0xFFFF || stored_crc != expected_crc {
                    return Err(StabstreamError::ChecksumMismatch {
                        expected: expected_crc,
                        actual: stored_crc,
                    });
                }
            }
            self.ring_buf.consume(6);
        }

        let payload = SyndromePayload {
            detector_events: de_slice,
            meas_results: meas_slice,
            timing_offsets: timing_slice,
            parity_checks: parity_slice,
        };

        drop(parse_span);

        // Validation
        let frame = SyndromeFrame {
            header: frame_hdr,
            payload,
            metadata,
            annotations,
        };

        let _validate_span =
            tracing::info_span!("qssf.frame_validate", frame_id = frame.header.frame_id,).entered();

        match self.config.validation {
            ValidationPolicy::StrictParity => {
                stabstream_validate::timing::check_timing(&frame)?;
                // Schema-dependent checks require the schema; skip if not registered.
                if let Ok(schema) = self
                    .config
                    .schema_registry
                    .get(&self.schema_id)
                    .map_err(|_| ())
                {
                    stabstream_validate::schema_consistency::check_schema_consistency(
                        &frame, schema,
                    )?;
                    stabstream_validate::parity::check_parity(&frame, schema)?;
                }
            }
            ValidationPolicy::CrcOnly | ValidationPolicy::Disabled => {}
        }

        Ok(Some(frame))
    }
}
