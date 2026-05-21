//! Offline replay analysis: stream a QSSF recording through a decoder and
//! produce an `AnalysisReport` with latency percentiles, per-ancilla fire
//! frequency, syndrome weight histogram, and (when QSSF tag 0x10 is present)
//! logical error rates.

use std::io::Read;
use std::path::Path;
use std::time::Instant;

use stabstream_core::{error::StabstreamError, window::OwnedSyndromeData};
use stabstream_decoder::Decoder;
use stabstream_metrics::{AnalysisReport, LogicalErrorAccumulator};

use crate::player::StreamPlayer;

// QSSF file header magic bytes, derived from core so they can't drift.
const QSSF_MAGIC: [u8; 4] = stabstream_core::frame::QSSF_MAGIC.to_le_bytes();
// Zstd frame magic bytes.
const ZSTD_MAGIC: [u8; 4] = [0xFD, 0x2F, 0xB5, 0x28];
// QSSF file header is 26 bytes: magic(4) + version(2) + schema_id(16) + flags(4).
const QSSF_FILE_HEADER_LEN: usize = 26;

/// Configuration for `analyze_file`.
pub struct AnalysisConfig {
    /// Number of rounds to hold in the sliding syndrome window.
    pub window_depth: usize,
    /// Number of logical observables to track for p_L.
    pub observable_count: usize,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            window_depth: 5,
            observable_count: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Raw frame parsing helpers
// ---------------------------------------------------------------------------

struct ParsedFrame {
    frame_id: u64,
    round: u32,
    timestamp_ns: u64,
    ancilla_count: usize,
    detector_events: Vec<bool>,
    meas_results: Vec<i8>,
    /// QSSF metadata tag 0x10, if present.
    observable_flips: Option<u64>,
}

/// Parse a raw frame byte slice (header + payload + terminator) into fields.
///
/// Layout:
/// - [0..36]  frame header (flags at [30..32]: bit 0=timing, bit 1=parity, bit 2=TLV meta, bit 3=annotations)
/// - [36..38] de_len: u16 LE
/// - [38..38+de_len] detector-events RLE
/// - [38+de_len..38+de_len+ancilla] meas_results
/// - [optional timing: ancilla*2 bytes if flags & 0x01]
/// - [optional parity: (ancilla+7)/8 bytes if flags & 0x02]
/// - [optional TLV block if flags & 0x04]
/// - [optional annotation block if flags & 0x08]
/// - 2-byte sentinel + 4-byte CRC
fn parse_raw_frame(bytes: &[u8]) -> Option<ParsedFrame> {
    if bytes.len() < 42 {
        return None;
    }

    let frame_id = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let round = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let timestamp_ns = u64::from_le_bytes(bytes[12..20].try_into().ok()?);
    let ancilla_count = u16::from_le_bytes(bytes[22..24].try_into().ok()?) as usize;
    let flags = u16::from_le_bytes(bytes[30..32].try_into().ok()?);

    let de_len = u16::from_le_bytes(bytes[36..38].try_into().ok()?) as usize;
    let de_end = 38 + de_len;
    if de_end > bytes.len() {
        return None;
    }
    let detector_events = OwnedSyndromeData::decode_rle(&bytes[38..de_end], ancilla_count);

    let meas_end = de_end + ancilla_count;
    let meas_results: Vec<i8> = if meas_end <= bytes.len() {
        bytes[de_end..meas_end].iter().map(|&b| b as i8).collect()
    } else {
        vec![0i8; ancilla_count]
    };

    // Skip optional timing and parity blocks
    let timing_len = if flags & 0x01 != 0 {
        ancilla_count * 2
    } else {
        0
    };
    let parity_len = if flags & 0x02 != 0 {
        ancilla_count.div_ceil(8)
    } else {
        0
    };
    let mut cursor = meas_end + timing_len + parity_len;

    // Parse TLV metadata block (flag bit 2) to extract observable_flips
    let observable_flips = if flags & 0x04 != 0 && cursor + 2 <= bytes.len() {
        let tag_count = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().ok()?) as usize;
        cursor += 2;
        let mut obs = None;
        for _ in 0..tag_count {
            if cursor + 4 > bytes.len() {
                break;
            }
            let tag = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().ok()?);
            let val_len =
                u16::from_le_bytes(bytes[cursor + 2..cursor + 4].try_into().ok()?) as usize;
            cursor += 4;
            if cursor + val_len > bytes.len() {
                break;
            }
            if tag == 0x0010 && val_len == 8 {
                obs = Some(u64::from_le_bytes(
                    bytes[cursor..cursor + 8].try_into().ok()?,
                ));
            }
            cursor += val_len;
        }
        obs
    } else {
        None
    };

    Some(ParsedFrame {
        frame_id,
        round,
        timestamp_ns,
        ancilla_count,
        detector_events,
        meas_results,
        observable_flips,
    })
}


/// Read the next raw frame from a plain byte stream (no file header already consumed).
///
/// Same logic as `StreamPlayer::next_frame_bytes` but for a plain `Read`.
fn read_next_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, StabstreamError> {
    let mut hdr = [0u8; 36];
    match reader.read_exact(&mut hdr) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(StabstreamError::Io(e)),
    }
    let payload_len = u32::from_le_bytes(hdr[24..28].try_into().unwrap()) as usize;
    let remainder = payload_len + 6;
    let mut out = Vec::with_capacity(36 + remainder);
    out.extend_from_slice(&hdr);
    out.resize(36 + remainder, 0);
    reader.read_exact(&mut out[36..]).map_err(StabstreamError::Io)?;
    Ok(Some(out))
}

// ---------------------------------------------------------------------------
// Latency percentile helper
// ---------------------------------------------------------------------------

fn percentile(sorted: &[u64], p: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (sorted.len() as u64 * p as u64).div_ceil(100) as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

// ---------------------------------------------------------------------------
// Core analysis loop
// ---------------------------------------------------------------------------

fn run_analysis<D: Decoder>(
    next_frame: &mut impl FnMut() -> Result<Option<Vec<u8>>, StabstreamError>,
    decoder: &D,
    config: &AnalysisConfig,
) -> Result<AnalysisReport, StabstreamError> {
    use stabstream_core::window::SyndromeWindow;

    let acc = LogicalErrorAccumulator::new(config.observable_count);
    let mut latencies: Vec<u64> = Vec::new();
    let mut ancilla_fire_counts: Vec<u64> = Vec::new();
    let mut syndrome_weight_hist: Vec<u64> = Vec::new();
    let mut window: Option<SyndromeWindow> = None;
    let mut frames_processed = 0u64;
    let mut total_shots = 0u64;
    let mut any_ground_truth = false;

    while let Some(raw) = next_frame()? {
        let Some(frame) = parse_raw_frame(&raw) else {
            continue;
        };
        frames_processed += 1;

        let ancilla_count = frame.ancilla_count;

        // Lazy init on first frame (we learn ancilla_count here)
        if window.is_none() {
            ancilla_fire_counts = vec![0u64; ancilla_count];
            syndrome_weight_hist = vec![0u64; ancilla_count + 1];
            window = Some(SyndromeWindow::new(ancilla_count, config.window_depth));
        }

        // Per-ancilla fire count + syndrome weight
        let mut weight = 0usize;
        for (i, &fired) in frame.detector_events.iter().enumerate() {
            if fired {
                if i < ancilla_fire_counts.len() {
                    ancilla_fire_counts[i] += 1;
                }
                weight += 1;
            }
        }
        if weight < syndrome_weight_hist.len() {
            syndrome_weight_hist[weight] += 1;
        }

        // Ground truth
        if frame.observable_flips.is_some() {
            any_ground_truth = true;
        }
        let ground_truth = frame.observable_flips.unwrap_or(0);

        // Push into the sliding window and decode when full
        if let Some(w) = window.as_mut() {
            w.push_owned(OwnedSyndromeData {
                frame_id: frame.frame_id,
                round: frame.round,
                timestamp_ns: frame.timestamp_ns,
                detector_events: frame.detector_events,
                meas_results: frame.meas_results,
            });

            if w.is_full() {
                let t0 = Instant::now();
                let result = decoder.decode_window(w);
                let elapsed_ns = t0.elapsed().as_nanos() as u64;

                latencies.push(elapsed_ns);
                total_shots += 1;
                acc.record(&result, ground_truth);
            }
        }
    }

    // Finalize latency stats
    latencies.sort_unstable();
    let mean_latency = if latencies.is_empty() {
        0
    } else {
        latencies.iter().sum::<u64>() / latencies.len() as u64
    };
    let p50 = percentile(&latencies, 50);
    let p99 = percentile(&latencies, 99);
    let max_latency = latencies.last().copied().unwrap_or(0);

    let ancilla_count = ancilla_fire_counts.len();
    let per_ancilla_fire_frequency = ancilla_fire_counts
        .iter()
        .map(|&c| {
            if frames_processed > 0 {
                c as f64 / frames_processed as f64
            } else {
                0.0
            }
        })
        .collect();

    let metrics = acc.report();

    Ok(AnalysisReport {
        frames_processed,
        total_shots,
        observable_count: config.observable_count,
        logical_error_rates: metrics.logical_error_rates,
        mean_logical_error_rate: metrics.mean_logical_error_rate,
        ground_truth_available: any_ground_truth,
        mean_decode_latency_ns: mean_latency,
        p50_decode_latency_ns: p50,
        p99_decode_latency_ns: p99,
        max_decode_latency_ns: max_latency,
        ancilla_count,
        per_ancilla_fire_frequency,
        syndrome_weight_histogram: syndrome_weight_hist,
    })
}

// ---------------------------------------------------------------------------
// StreamPlayer bridge
// ---------------------------------------------------------------------------

/// Run analysis on a `StreamPlayer` that is already positioned at the first frame.
///
/// This is the implementation backing `StreamPlayer::analyze()`.
pub(crate) fn analyze_player<R: Read, D: Decoder>(
    player: &mut crate::player::StreamPlayer<R>,
    decoder: &D,
    config: &AnalysisConfig,
) -> Result<AnalysisReport, StabstreamError> {
    run_analysis(&mut || player.next_frame_bytes(), decoder, config)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Analyze a QSSF recording file (plain or zstd-compressed) using `decoder`.
///
/// The file format is auto-detected from the first 4 bytes:
/// - QSSF magic (`QSSF`) → plain QSSF; the 24-byte file header is skipped.
/// - Zstd magic (`0xFD2FB528`) → zstd-compressed QSSF recording (as written
///   by `StreamRecorder`); frames are read directly after decompression.
///
/// Returns an `AnalysisReport` with latency percentiles, per-ancilla fire
/// frequency, syndrome weight histogram, and logical error rates (only when
/// QSSF metadata tag 0x10 is present in the recording).
pub fn analyze_file<D: Decoder>(
    path: &Path,
    decoder: &D,
    config: AnalysisConfig,
) -> Result<AnalysisReport, StabstreamError> {
    let mut file = std::fs::File::open(path).map_err(StabstreamError::Io)?;

    // Read first 4 bytes to detect format
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(StabstreamError::Io)?;

    if magic == QSSF_MAGIC {
        // Plain QSSF: skip the remaining 20 bytes of the 24-byte file header
        let mut rest_of_header = [0u8; QSSF_FILE_HEADER_LEN - 4];
        file.read_exact(&mut rest_of_header)
            .map_err(StabstreamError::Io)?;
        let mut reader = std::io::BufReader::new(file);
        run_analysis(&mut || read_next_frame(&mut reader), decoder, &config)
    } else if magic == ZSTD_MAGIC {
        // Zstd-compressed QSSF recording (as written by StreamRecorder).
        // The 4 magic bytes are part of the zstd frame, so we must prepend them.
        let remainder = std::io::Read::chain(std::io::Cursor::new(magic), file);
        let mut player = StreamPlayer::new(remainder)?;
        run_analysis(&mut || player.next_frame_bytes(), decoder, &config)
    } else {
        // Unknown format: attempt zstd decompression anyway (magic bytes consumed)
        let remainder = std::io::Read::chain(std::io::Cursor::new(magic), file);
        let mut player = StreamPlayer::new(remainder)?;
        run_analysis(&mut || player.next_frame_bytes(), decoder, &config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stabstream_decoder::NullDecoder;

    // ── unit tests ──────────────────────────────────────────────────────────

    #[test]
    fn decode_rle_basic() {
        // 0x82 = mode 1 (ones), run 2 → [true, true]
        // 0x02 = mode 0 (zeros), run 2 → [false, false]
        let out = OwnedSyndromeData::decode_rle(&[0x82, 0x02], 4);
        assert_eq!(out, vec![true, true, false, false]);
    }

    #[test]
    fn decode_rle_truncates_to_ancilla_count() {
        let out = OwnedSyndromeData::decode_rle(&[0x85], 3); // 5 ones, capped to 3
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|&v| v));
    }

    #[test]
    fn percentile_single_element() {
        assert_eq!(percentile(&[42], 99), 42);
    }

    #[test]
    fn percentile_p50() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&data, 50), 5);
    }

    #[test]
    fn percentile_empty_returns_zero() {
        assert_eq!(percentile(&[], 99), 0);
    }

    #[test]
    fn analysis_config_default() {
        let c = AnalysisConfig::default();
        assert_eq!(c.window_depth, 5);
        assert_eq!(c.observable_count, 1);
    }

    // ── integration tests ────────────────────────────────────────────────────

    /// Build a minimal valid QSSF frame byte blob (no file header).
    fn build_frame(frame_id: u64, round: u32, detector_events: &[bool]) -> Vec<u8> {
        use stabstream_deserialize::rle::encode_detector_events;

        let ancilla_count = detector_events.len() as u16;
        let rle = encode_detector_events(detector_events);
        // payload_len = 2-byte de_len prefix + rle bytes + ancilla_count meas bytes
        let payload_len = (2 + rle.len() + ancilla_count as usize) as u32;

        let mut hdr = [0u8; 36];
        hdr[0..8].copy_from_slice(&frame_id.to_le_bytes());
        hdr[8..12].copy_from_slice(&round.to_le_bytes());
        // timestamp_ns at [12..20] = 0, qubit_count at [20..22] = 0
        hdr[22..24].copy_from_slice(&ancilla_count.to_le_bytes());
        hdr[24..28].copy_from_slice(&payload_len.to_le_bytes());
        hdr[28] = 0x01; // SurfaceCode
        let header_crc = crc32fast::hash(&hdr[0..32]);
        hdr[32..36].copy_from_slice(&header_crc.to_le_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&(rle.len() as u16).to_le_bytes());
        out.extend_from_slice(&rle);
        for &e in detector_events {
            out.push(if e { 0xFF } else { 0x01 });
        }
        out.extend_from_slice(&0xFFFFu16.to_le_bytes()); // sentinel
        out.extend_from_slice(&0u32.to_le_bytes()); // frame CRC (not checked by analyze)
        out
    }

    #[test]
    fn analyze_counts_frames_and_shots() {
        use std::io::Cursor;

        let window_depth = 3;
        let ancilla_count = 4;
        let n_frames = 10u64;

        let mut stream = Vec::new();
        for i in 0..n_frames {
            stream.extend_from_slice(&build_frame(i, i as u32, &[false; 4]));
        }

        let dec = NullDecoder;
        let config = AnalysisConfig {
            window_depth,
            observable_count: 1,
        };
        let mut cursor = Cursor::new(stream);
        let report = run_analysis(&mut || read_next_frame(&mut cursor), &dec, &config).unwrap();

        assert_eq!(report.frames_processed, n_frames);
        // Sliding window: first window fills after window_depth frames, then
        // fires on every subsequent push → shots = n_frames - window_depth + 1
        assert_eq!(report.total_shots, n_frames - window_depth as u64 + 1);
        assert_eq!(report.ancilla_count, ancilla_count);
    }

    #[test]
    fn analyze_per_ancilla_fire_frequency() {
        use std::io::Cursor;

        let n_frames = 20u64;
        let ancilla_count = 4;
        // Only ancilla 0 fires in every frame.
        let events = |i: u64| {
            let mut e = vec![false; ancilla_count];
            if i % 2 == 0 {
                e[0] = true; // ancilla 0 fires every other frame
            }
            e
        };

        let mut stream = Vec::new();
        for i in 0..n_frames {
            stream.extend_from_slice(&build_frame(i, i as u32, &events(i)));
        }

        let dec = NullDecoder;
        let config = AnalysisConfig {
            window_depth: 1,
            observable_count: 1,
        };
        let mut cursor = Cursor::new(stream);
        let report = run_analysis(&mut || read_next_frame(&mut cursor), &dec, &config).unwrap();

        // Ancilla 0 fires in half the frames → frequency ≈ 0.5
        let freq0 = report.per_ancilla_fire_frequency[0];
        assert!(
            (freq0 - 0.5).abs() < 0.05,
            "ancilla 0 freq={freq0}, expected ~0.5"
        );
        // Ancilla 1–3 never fire → frequency = 0
        for &f in &report.per_ancilla_fire_frequency[1..] {
            assert_eq!(f, 0.0);
        }
    }

    #[test]
    fn analyze_syndrome_weight_histogram() {
        use std::io::Cursor;

        let n_frames = 6u64;
        // 3 frames with weight 0, 3 frames with weight 2
        let frames_data: Vec<Vec<bool>> = (0..n_frames)
            .map(|i| {
                if i < 3 {
                    vec![false, false, false, false]
                } else {
                    vec![true, true, false, false]
                }
            })
            .collect();

        let mut stream = Vec::new();
        for (i, events) in frames_data.iter().enumerate() {
            stream.extend_from_slice(&build_frame(i as u64, i as u32, events));
        }

        let dec = NullDecoder;
        let config = AnalysisConfig {
            window_depth: 1,
            observable_count: 1,
        };
        let mut cursor = Cursor::new(stream);
        let report = run_analysis(&mut || read_next_frame(&mut cursor), &dec, &config).unwrap();

        assert_eq!(report.syndrome_weight_histogram[0], 3); // 3 frames with weight 0
        assert_eq!(report.syndrome_weight_histogram[2], 3); // 3 frames with weight 2
    }

    /// Build a frame with TLV metadata containing observable_flips.
    fn build_frame_with_obs(
        frame_id: u64,
        round: u32,
        detector_events: &[bool],
        obs: u64,
    ) -> Vec<u8> {
        use stabstream_core::frame::FrameMetadata;
        use stabstream_deserialize::{parser::write_metadata_tlv, rle::encode_detector_events};

        let ancilla_count = detector_events.len() as u16;
        let rle = encode_detector_events(detector_events);
        let meta = FrameMetadata {
            observable_flips: Some(obs),
            ..Default::default()
        };
        let tlv = write_metadata_tlv(&meta);

        let payload_len = (2 + rle.len() + ancilla_count as usize + tlv.len()) as u32;
        let flags: u16 = 0x04; // metadata present

        let mut hdr = [0u8; 36];
        hdr[0..8].copy_from_slice(&frame_id.to_le_bytes());
        hdr[8..12].copy_from_slice(&round.to_le_bytes());
        hdr[22..24].copy_from_slice(&ancilla_count.to_le_bytes());
        hdr[24..28].copy_from_slice(&payload_len.to_le_bytes());
        hdr[28] = 0x01;
        hdr[30..32].copy_from_slice(&flags.to_le_bytes());
        let header_crc = crc32fast::hash(&hdr[0..32]);
        hdr[32..36].copy_from_slice(&header_crc.to_le_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&(rle.len() as u16).to_le_bytes());
        out.extend_from_slice(&rle);
        for &e in detector_events {
            out.push(if e { 0xFF } else { 0x01 });
        }
        out.extend_from_slice(&tlv);
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // frame CRC
        out
    }

    #[test]
    fn observable_flips_extracted_from_tlv() {
        use std::io::Cursor;

        let window_depth = 3;
        let n_frames = 10u64;
        // All odd frames have observable flipped (obs=1), even frames have obs=0.
        let mut stream = Vec::new();
        for i in 0..n_frames {
            let obs = i % 2; // alternating 0 / 1
            stream.extend_from_slice(&build_frame_with_obs(i, i as u32, &[false; 4], obs));
        }

        let dec = NullDecoder;
        let config = AnalysisConfig {
            window_depth,
            observable_count: 1,
        };
        let mut cursor = Cursor::new(stream);
        let report = run_analysis(&mut || read_next_frame(&mut cursor), &dec, &config).unwrap();

        assert_eq!(report.frames_processed, n_frames);
        assert!(
            report.ground_truth_available,
            "ground truth should be detected"
        );
        // NullDecoder always returns 0 flips; ~50% of shots have obs=1 → p_L ≈ 0.5
        let p_l = report.logical_error_rates[0];
        assert!((p_l - 0.5).abs() < 0.15, "expected p_L ≈ 0.5, got {p_l}");
    }

    #[test]
    fn stream_player_analyze_method() {
        let n_frames = 8u64;
        let window_depth = 2;

        // Build a raw (uncompressed) frame stream and wrap it in a zstd stream,
        // since StreamPlayer expects zstd-compressed input.
        let mut raw = Vec::new();
        for i in 0..n_frames {
            raw.extend_from_slice(&build_frame(i, 0, &[false, false, false]));
        }

        let mut compressed = Vec::new();
        {
            let mut enc = zstd::Encoder::new(&mut compressed, 1).unwrap();
            std::io::Write::write_all(&mut enc, &raw).unwrap();
            enc.finish().unwrap();
        }

        let mut player =
            crate::player::StreamPlayer::new(std::io::Cursor::new(compressed)).unwrap();
        let config = AnalysisConfig {
            window_depth,
            observable_count: 1,
        };
        let report = player.analyze(&NullDecoder, config).unwrap();

        assert_eq!(report.frames_processed, n_frames);
        assert_eq!(report.total_shots, n_frames - window_depth as u64 + 1);
        assert_eq!(report.ancilla_count, 3);
    }
}
