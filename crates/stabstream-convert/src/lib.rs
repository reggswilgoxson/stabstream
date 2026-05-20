use std::io::{BufRead, Write};

use stabstream_core::{
    error::StabstreamError,
    frame::{FileHeader, FrameHeader, SyndromeFrame, QSSF_MAGIC},
};
use stabstream_deserialize::{
    parser::{write_annotations, write_frame_header, write_metadata_tlv},
    rle::encode_detector_events,
};
use uuid::Uuid;

/// UUID written into QSSF file headers for Stim-sourced streams.
pub const STIM_GENERIC_UUID: &str = "00000000-5354-494d-0000-000000000001";

// ---------------------------------------------------------------------------
// QSSF exporter
// ---------------------------------------------------------------------------

/// Writes a valid QSSF binary stream from a sequence of [`SyndromeFrame`] values.
pub struct QssfExporter<W: Write> {
    writer: W,
    frames_written: u64,
    header_written: bool,
    schema_id: Uuid,
}

impl<W: Write> QssfExporter<W> {
    pub fn new(writer: W, schema_id: Uuid) -> Self {
        Self {
            writer,
            frames_written: 0,
            header_written: false,
            schema_id,
        }
    }

    /// Write the 26-byte QSSF file header (called automatically on first frame if not called explicitly).
    pub fn write_file_header(&mut self) -> Result<(), StabstreamError> {
        let hdr = FileHeader {
            magic: QSSF_MAGIC,
            version: 1,
            schema_id: self.schema_id,
            flags: 0,
        };
        self.writer.write_all(&hdr.magic.to_le_bytes())?;
        self.writer.write_all(&hdr.version.to_le_bytes())?;
        self.writer.write_all(hdr.schema_id.as_bytes())?;
        self.writer.write_all(&hdr.flags.to_le_bytes())?;
        self.header_written = true;
        Ok(())
    }

    /// Write a single [`SyndromeFrame`] as QSSF binary.
    ///
    /// The file header is emitted automatically before the first frame.
    /// Metadata (`frame.metadata`) and annotations (`frame.annotations`) are
    /// serialised as TLV blocks when present; flag bits 2 and 3 are set
    /// accordingly and `payload_len` is computed from the actual written bytes.
    pub fn write_frame(&mut self, frame: &SyndromeFrame<'_>) -> Result<(), StabstreamError> {
        if !self.header_written {
            self.write_file_header()?;
        }

        // Pre-serialise optional blocks so we know their lengths before writing
        // the header (payload_len must cover all of them).
        let meta_bytes = frame.metadata.as_ref().map(write_metadata_tlv);
        let ann_bytes = frame.annotations.as_deref().map(write_annotations);

        let de = frame.payload.detector_events;
        let ancilla = frame.header.ancilla_count as usize;
        let timing_present = !frame.payload.timing_offsets.is_empty();
        let parity_present = !frame.payload.parity_checks.is_empty();

        let mut flags: u16 = 0;
        if timing_present {
            flags |= 0x01;
        }
        if parity_present {
            flags |= 0x02;
        }
        if meta_bytes.is_some() {
            flags |= 0x04;
        }
        if ann_bytes.is_some() {
            flags |= 0x08;
        }

        let timing_len = if timing_present { ancilla * 2 } else { 0 };
        let parity_len = if parity_present {
            ancilla.div_ceil(8)
        } else {
            0
        };
        let meta_len = meta_bytes.as_ref().map_or(0, |b| b.len());
        let ann_len = ann_bytes.as_ref().map_or(0, |b| b.len());
        let payload_len = 2 + de.len() + ancilla + timing_len + parity_len + meta_len + ann_len;

        // Build header with correct payload_len and flags.
        let mut hdr = frame.header.clone();
        hdr.flags = flags;
        hdr.payload_len = payload_len as u32;
        hdr.crc32 = 0; // recomputed by write_frame_header
        let hdr_bytes = write_frame_header(&hdr);
        self.writer.write_all(&hdr_bytes)?;

        // detector_events: 2-byte LE length + RLE bytes
        self.writer.write_all(&(de.len() as u16).to_le_bytes())?;
        self.writer.write_all(de)?;

        // meas_results: reinterpret i8 as u8
        let meas_u8: &[u8] = unsafe {
            std::slice::from_raw_parts(
                frame.payload.meas_results.as_ptr().cast::<u8>(),
                frame.payload.meas_results.len(),
            )
        };
        self.writer.write_all(meas_u8)?;

        // timing_offsets (if present)
        for &offset in frame.payload.timing_offsets {
            self.writer.write_all(&offset.to_le_bytes())?;
        }

        // parity_checks (if present)
        self.writer.write_all(frame.payload.parity_checks)?;

        // TLV metadata block (if present)
        if let Some(ref mb) = meta_bytes {
            self.writer.write_all(mb)?;
        }

        // Logical annotation block (if present)
        if let Some(ref ab) = ann_bytes {
            self.writer.write_all(ab)?;
        }

        // Frame terminator: 0xFFFF sentinel + CRC32 of header bytes
        self.writer.write_all(&0xFFFFu16.to_le_bytes())?;
        self.writer
            .write_all(&crc32fast::hash(&hdr_bytes).to_le_bytes())?;

        self.frames_written += 1;
        Ok(())
    }

    pub fn flush(mut self) -> Result<W, StabstreamError> {
        self.writer.flush()?;
        Ok(self.writer)
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }
}

// ---------------------------------------------------------------------------
// Stim detector-event importer
// ---------------------------------------------------------------------------

/// An owned, heap-allocated snapshot of a syndrome frame (no buffer lifetime).
pub struct OwnedFrame {
    pub frame_id: u64,
    pub round: u32,
    pub timestamp_ns: u64,
    pub ancilla_count: u16,
    pub detector_events_rle: Vec<u8>,
    pub meas_results: Vec<i8>,
    /// Ground-truth observable flip bitmask written by `StimObsImporter`.
    /// Bit i = 1 means observable i was truly flipped by the physical error
    /// pattern. Stored in `FrameMetadata::observable_flips` when exporting.
    pub observable_flips: Option<u64>,
}

impl OwnedFrame {
    /// Build a minimal [`FrameHeader`] for this owned frame.
    pub fn to_frame_header(&self) -> FrameHeader {
        FrameHeader {
            frame_id: self.frame_id,
            round: self.round,
            timestamp_ns: self.timestamp_ns,
            qubit_count: 0,
            ancilla_count: self.ancilla_count,
            payload_len: (2 + self.detector_events_rle.len() + self.meas_results.len()) as u32,
            code_type: 0x01, // SurfaceCode placeholder
            distance: 0,
            flags: 0,
            crc32: 0, // recomputed by write_frame_header
        }
    }
}

/// Reads Stim detector-event output (01 text format) and produces [`OwnedFrame`]s.
///
/// The 01 format is the default `stim detect` output: one line per shot where
/// each character is `'0'` (no event) or `'1'` (event fired).
pub struct StimImporter<R: BufRead> {
    reader: R,
    frame_id: u64,
    ancilla_count: Option<u16>,
}

impl<R: BufRead> StimImporter<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            frame_id: 0,
            ancilla_count: None,
        }
    }

    /// Read the next frame. Returns `Ok(None)` on clean EOF.
    pub fn next_frame(&mut self) -> Result<Option<OwnedFrame>, StabstreamError> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(StabstreamError::Io)?;
            if n == 0 {
                return Ok(None); // EOF
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                break;
            }
        }

        let trimmed = line.trim();
        let events: Vec<bool> = trimmed.bytes().map(|b| b == b'1').collect();

        let ancilla_count = *self.ancilla_count.get_or_insert(events.len() as u16);
        let de_rle = encode_detector_events(&events);
        let meas: Vec<i8> = events.iter().map(|&e| if e { -1i8 } else { 1i8 }).collect();

        let frame = OwnedFrame {
            frame_id: self.frame_id,
            round: self.frame_id as u32,
            timestamp_ns: 0,
            ancilla_count,
            detector_events_rle: de_rle,
            meas_results: meas,
            observable_flips: None,
        };
        self.frame_id += 1;
        Ok(Some(frame))
    }
}

// ---------------------------------------------------------------------------
// Stim observable importer (reads --obs-out-format=01 files)
// ---------------------------------------------------------------------------

/// Reads Stim's observable-flip output (01 text format, one line per shot)
/// and returns `u64` bitmasks where bit i = 1 means observable i was flipped.
///
/// Pair with `StimImporter` to populate `OwnedFrame::observable_flips`.
pub struct StimObsImporter<R: BufRead> {
    reader: R,
}

impl<R: BufRead> StimObsImporter<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Read the next observable flip bitmask. Returns `Ok(None)` at EOF.
    pub fn next_flips(&mut self) -> Result<Option<u64>, StabstreamError> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Ok(None);
            }
            if !line.trim().is_empty() {
                break;
            }
        }
        let mut mask: u64 = 0;
        for (i, b) in line.trim().bytes().enumerate().take(64) {
            if b == b'1' {
                mask |= 1u64 << i;
            }
        }
        Ok(Some(mask))
    }
}

/// Zip a `StimImporter` and a `StimObsImporter` together, injecting
/// observable ground truth into each `OwnedFrame`.
pub struct StimWithObsImporter<R1: BufRead, R2: BufRead> {
    det: StimImporter<R1>,
    obs: StimObsImporter<R2>,
}

impl<R1: BufRead, R2: BufRead> StimWithObsImporter<R1, R2> {
    pub fn new(det_reader: R1, obs_reader: R2) -> Self {
        Self {
            det: StimImporter::new(det_reader),
            obs: StimObsImporter::new(obs_reader),
        }
    }

    pub fn next_frame(&mut self) -> Result<Option<OwnedFrame>, StabstreamError> {
        match self.det.next_frame()? {
            None => Ok(None),
            Some(mut frame) => {
                frame.observable_flips = self.obs.next_flips()?;
                Ok(Some(frame))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience: convert an OwnedFrame to a SyndromeFrame
// ---------------------------------------------------------------------------

/// Write an [`OwnedFrame`] to a [`QssfExporter`] by constructing a temporary
/// [`SyndromeFrame`] view.
pub fn export_owned_frame<W: Write>(
    exporter: &mut QssfExporter<W>,
    frame: &OwnedFrame,
) -> Result<(), StabstreamError> {
    use stabstream_core::frame::SyndromePayload;

    let header = frame.to_frame_header();
    let meas_u8: &[u8] = unsafe {
        std::slice::from_raw_parts(
            frame.meas_results.as_ptr().cast::<u8>(),
            frame.meas_results.len(),
        )
    };
    let meas_i8: &[i8] =
        unsafe { std::slice::from_raw_parts(meas_u8.as_ptr().cast::<i8>(), meas_u8.len()) };
    use stabstream_core::frame::FrameMetadata;
    let metadata = if frame.observable_flips.is_some() {
        Some(FrameMetadata {
            observable_flips: frame.observable_flips,
            ..Default::default()
        })
    } else {
        None
    };
    let sf = SyndromeFrame {
        header,
        payload: SyndromePayload {
            detector_events: &frame.detector_events_rle,
            meas_results: meas_i8,
            timing_offsets: &[],
            parity_checks: &[],
        },
        metadata,
        annotations: None,
    };
    exporter.write_frame(&sf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stim_importer_parses_01_lines() {
        let input = b"0110\n1001\n0000\n";
        let mut imp = StimImporter::new(Cursor::new(&input[..]));

        let f0 = imp.next_frame().unwrap().unwrap();
        assert_eq!(f0.frame_id, 0);
        assert_eq!(f0.ancilla_count, 4);
        assert_eq!(f0.meas_results, vec![1, -1, -1, 1]);

        let f1 = imp.next_frame().unwrap().unwrap();
        assert_eq!(f1.frame_id, 1);
        assert_eq!(f1.meas_results, vec![-1, 1, 1, -1]);

        let f2 = imp.next_frame().unwrap().unwrap();
        assert_eq!(f2.frame_id, 2);
        assert_eq!(f2.meas_results, vec![1, 1, 1, 1]);

        assert!(imp.next_frame().unwrap().is_none());
    }

    #[test]
    fn qssf_exporter_roundtrip() {
        // Import from Stim format and export as QSSF; check magic bytes.
        let input = b"01010101\n10101010\n";
        let mut imp = StimImporter::new(Cursor::new(&input[..]));
        let schema_id: Uuid = STIM_GENERIC_UUID.parse().unwrap();
        let mut out = Vec::new();
        {
            let mut exp = QssfExporter::new(&mut out, schema_id);
            while let Some(frame) = imp.next_frame().unwrap() {
                export_owned_frame(&mut exp, &frame).unwrap();
            }
            assert_eq!(exp.frames_written(), 2);
            exp.flush().unwrap();
        }

        // Check QSSF magic at offset 0 (exporter dropped, borrow released)
        let magic = u32::from_le_bytes(out[0..4].try_into().unwrap());
        assert_eq!(magic, QSSF_MAGIC);
    }

    #[test]
    fn qssf_exporter_observable_flips_roundtrip() {
        use stabstream_core::frame::FrameMetadata;
        use stabstream_deserialize::parser::parse_metadata_tlv;

        // Build a frame with observable_flips = 0b11 (observables 0 and 1 flipped).
        let input = b"0101\n1010\n";
        let mut imp = StimImporter::new(Cursor::new(&input[..]));
        let schema_id: Uuid = STIM_GENERIC_UUID.parse().unwrap();
        let mut out = Vec::new();
        {
            let mut exp = QssfExporter::new(&mut out, schema_id);
            while let Some(mut frame) = imp.next_frame().unwrap() {
                frame.observable_flips = Some(0b11);
                export_owned_frame(&mut exp, &frame).unwrap();
            }
            exp.flush().unwrap();
        }

        // Locate the TLV block in the first frame.
        // File header: 26 bytes. Then frame header: 36 bytes.
        // payload_len at [24..28] of the frame header.
        let fhdr_start = 26;
        let payload_len =
            u32::from_le_bytes(out[fhdr_start + 24..fhdr_start + 28].try_into().unwrap()) as usize;

        // Flags byte at [30..32]: bit 2 should be set.
        let flags = u16::from_le_bytes(out[fhdr_start + 30..fhdr_start + 32].try_into().unwrap());
        assert_eq!(flags & 0x04, 0x04, "metadata flag bit should be set");

        // The block starting at file_hdr(26) + frame_hdr(36) + de_len_field(2) + de + ancilla
        // contains the TLV. We read all payload bytes and find the TLV at the end.
        let payload_start = fhdr_start + 36;
        let payload_bytes = &out[payload_start..payload_start + payload_len];

        // de_len at [0..2], then de_len bytes of RLE, then 4 bytes meas
        let de_len = u16::from_le_bytes(payload_bytes[0..2].try_into().unwrap()) as usize;
        let ancilla = 4usize;
        let tlv_offset = 2 + de_len + ancilla; // no timing, no parity
        let tlv_bytes = &payload_bytes[tlv_offset..];
        let meta = parse_metadata_tlv(tlv_bytes);
        assert_eq!(meta.observable_flips, Some(0b11));
    }
}
