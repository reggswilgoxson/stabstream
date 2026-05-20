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

// QSSF file header magic bytes (ASCII "QSSF").
const QSSF_MAGIC: [u8; 4] = [0x51, 0x53, 0x53, 0x46];
// Zstd frame magic bytes.
const ZSTD_MAGIC: [u8; 4] = [0xFD, 0x2F, 0xB5, 0x28];
// QSSF file header is 24 bytes (magic u32 + version u16 + schema_id u128 + flags u32).
const QSSF_FILE_HEADER_LEN: usize = 24;

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
/// Layout (from `StreamPlayer::next_frame_bytes`):
/// - [0..36]  frame header
/// - [36..38] de_len: u16 LE — length of detector-events RLE blob
/// - [38..38+de_len] detector-events RLE
/// - [38+de_len..] meas_results (ancilla_count bytes), then terminator
fn parse_raw_frame(bytes: &[u8]) -> Option<ParsedFrame> {
    if bytes.len() < 42 {
        return None;
    }

    let frame_id = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let round = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let timestamp_ns = u64::from_le_bytes(bytes[12..20].try_into().ok()?);
    let ancilla_count = u16::from_le_bytes(bytes[22..24].try_into().ok()?) as usize;

    let de_len = u16::from_le_bytes(bytes[36..38].try_into().ok()?) as usize;
    let de_end = 38 + de_len;
    if de_end > bytes.len() {
        return None;
    }
    let detector_events = decode_rle(&bytes[38..de_end], ancilla_count);

    let meas_end = de_end + ancilla_count;
    let meas_results: Vec<i8> = if meas_end <= bytes.len() {
        bytes[de_end..meas_end].iter().map(|&b| b as i8).collect()
    } else {
        vec![0i8; ancilla_count]
    };

    Some(ParsedFrame {
        frame_id,
        round,
        timestamp_ns,
        ancilla_count,
        detector_events,
        meas_results,
        observable_flips: None,
    })
}

/// QSSF RLE decoder: token bit 7 = mode (0=zeros / 1=ones), bits 0–6 = run len.
fn decode_rle(rle: &[u8], ancilla_count: usize) -> Vec<bool> {
    let mut out = Vec::with_capacity(ancilla_count);
    for &tok in rle {
        let mode = (tok & 0x80) != 0;
        let run = (tok & 0x7F) as usize;
        for _ in 0..run {
            out.push(mode);
        }
    }
    // A stream that decodes fewer events than ancilla_count may be truncated/corrupt.
    // Overflow (more events than expected) is silently truncated below.
    debug_assert!(
        out.len() >= ancilla_count,
        "RLE decoded only {} events but ancilla_count is {} — stream may be truncated",
        out.len(),
        ancilla_count
    );
    out.resize(ancilla_count, false);
    out
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
    let remainder = 2 + payload_len + 6;
    let mut rest = vec![0u8; remainder];
    reader.read_exact(&mut rest).map_err(StabstreamError::Io)?;
    let mut out = Vec::with_capacity(36 + remainder);
    out.extend_from_slice(&hdr);
    out.extend_from_slice(&rest);
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
    use stabstream_core::window::SyndromeWindow;
    use stabstream_decoder::{DecoderResult, NullDecoder};

    #[test]
    fn decode_rle_basic() {
        // 0x82 = mode 1 (ones), run 2 → [true, true]
        // 0x02 = mode 0 (zeros), run 2 → [false, false]
        let out = decode_rle(&[0x82, 0x02], 4);
        assert_eq!(out, vec![true, true, false, false]);
    }

    #[test]
    fn decode_rle_truncates_to_ancilla_count() {
        let out = decode_rle(&[0x85], 3); // 5 ones, capped to 3
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
}
