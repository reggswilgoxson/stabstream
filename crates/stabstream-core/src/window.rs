//! Multi-round syndrome window for stateful (spacetime-aware) decoders.
//!
//! MWPM and Union-Find decoders need the full syndrome history across multiple
//! measurement rounds — not just the most recent frame. `SyndromeWindow` is a
//! fixed-depth ring of owned syndrome data that keeps a flat detector matrix
//! (rounds × ancillas) for efficient decoder consumption.

use std::collections::VecDeque;

use crate::frame::SyndromeFrame;

/// Owned syndrome data for a single round, extracted from a borrowed frame.
#[derive(Debug, Clone)]
pub struct OwnedSyndromeData {
    pub frame_id: u64,
    pub round: u32,
    pub timestamp_ns: u64,
    /// Decoded detector events: one bool per ancilla. `true` = syndrome flip.
    pub detector_events: Vec<bool>,
    /// Raw ancilla measurement outcomes (±1).
    pub meas_results: Vec<i8>,
}

impl OwnedSyndromeData {
    /// Decode RLE-encoded detector events into a flat bool vector.
    ///
    /// QSSF RLE token: bit 7 = run mode (0 = zeros, 1 = ones), bits 0–6 = run length.
    fn decode_rle(rle: &[u8], ancilla_count: usize) -> Vec<bool> {
        let mut out = Vec::with_capacity(ancilla_count);
        for &token in rle {
            let mode = (token & 0x80) != 0;
            let run = (token & 0x7F) as usize;
            for _ in 0..run {
                out.push(mode);
            }
        }
        out.resize(ancilla_count, false);
        out
    }

    pub fn from_frame(frame: &SyndromeFrame<'_>) -> Self {
        let ancilla_count = frame.header.ancilla_count as usize;
        Self {
            frame_id: frame.header.frame_id,
            round: frame.header.round,
            timestamp_ns: frame.header.timestamp_ns,
            detector_events: Self::decode_rle(frame.payload.detector_events, ancilla_count),
            meas_results: frame.payload.meas_results.to_vec(),
        }
    }
}

/// A sliding window of `OwnedSyndromeData` rounds for spacetime decoding.
///
/// # Layout
///
/// `detector_matrix` is a flat `Vec<bool>` of shape
/// `(current_depth × ancilla_count)` in row-major order.  Row 0 is the oldest
/// round; row `len()-1` is the most recent.  This layout lets decoders take a
/// single contiguous slice without copying.
pub struct SyndromeWindow {
    frames: VecDeque<OwnedSyndromeData>,
    /// Maximum number of rounds to hold.
    pub window_depth: usize,
    /// Flat detector matrix (rows = rounds, cols = ancillas).
    detector_matrix: Vec<bool>,
    pub ancilla_count: usize,
}

impl SyndromeWindow {
    /// Create an empty window for the given ancilla count and maximum depth.
    pub fn new(ancilla_count: usize, window_depth: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(window_depth),
            window_depth,
            detector_matrix: Vec::with_capacity(window_depth * ancilla_count),
            ancilla_count,
        }
    }

    /// Push a new syndrome frame into the window, evicting the oldest when full.
    pub fn push(&mut self, frame: &SyndromeFrame<'_>) {
        let data = OwnedSyndromeData::from_frame(frame);
        if self.frames.len() == self.window_depth {
            self.frames.pop_front();
        }
        self.frames.push_back(data);
        self.rebuild_matrix();
    }

    /// Number of rounds currently in the window.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Returns `true` when the window holds exactly `window_depth` rounds.
    pub fn is_full(&self) -> bool {
        self.frames.len() == self.window_depth
    }

    /// Flat detector matrix: shape `(len × ancilla_count)`, row-major.
    ///
    /// Row 0 = oldest round, row `len()-1` = newest round.
    pub fn detector_matrix(&self) -> &[bool] {
        &self.detector_matrix
    }

    /// Detector events for a specific round (0 = oldest in window).
    pub fn round_events(&self, round_idx: usize) -> Option<&[bool]> {
        if round_idx >= self.frames.len() {
            return None;
        }
        let start = round_idx * self.ancilla_count;
        let end = start + self.ancilla_count;
        self.detector_matrix.get(start..end)
    }

    /// The most recently pushed frame's metadata.
    pub fn latest_frame(&self) -> Option<&OwnedSyndromeData> {
        self.frames.back()
    }

    /// Iterate over all rounds oldest-first.
    pub fn iter(&self) -> impl Iterator<Item = &OwnedSyndromeData> {
        self.frames.iter()
    }

    /// Collect all active (fired) detector node indices across all rounds.
    ///
    /// The node id is `round_idx * ancilla_count + ancilla_idx`.
    pub fn active_detectors(&self) -> Vec<u32> {
        self.detector_matrix
            .iter()
            .enumerate()
            .filter_map(|(i, &fired)| if fired { Some(i as u32) } else { None })
            .collect()
    }

    fn rebuild_matrix(&mut self) {
        self.detector_matrix.clear();
        for frame_data in &self.frames {
            let events = &frame_data.detector_events;
            // Ensure exactly `ancilla_count` entries per row
            let n = events.len().min(self.ancilla_count);
            self.detector_matrix.extend_from_slice(&events[..n]);
            for _ in n..self.ancilla_count {
                self.detector_matrix.push(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{FrameHeader, SyndromeFrame, SyndromePayload};

    fn make_frame(frame_id: u64, rle: &'static [u8], ancilla_count: u16) -> SyndromeFrame<'static> {
        SyndromeFrame {
            header: FrameHeader {
                frame_id,
                round: frame_id as u32,
                timestamp_ns: 0,
                qubit_count: 25,
                ancilla_count,
                payload_len: 0,
                code_type: 0x01,
                distance: 5,
                flags: 0,
                crc32: 0,
            },
            payload: SyndromePayload {
                detector_events: rle,
                meas_results: &[],
                timing_offsets: &[],
                parity_checks: &[],
            },
            metadata: None,
            annotations: None,
        }
    }

    #[test]
    fn window_slides_correctly() {
        let mut w = SyndromeWindow::new(4, 3);
        // RLE: 0x82 = mode 1, run 2 → [true, true]; 0x02 = mode 0, run 2 → [false, false]
        let f0 = make_frame(0, &[0x82, 0x02], 4);
        let f1 = make_frame(1, &[0x04], 4);
        let f2 = make_frame(2, &[0x81, 0x03], 4);
        let f3 = make_frame(3, &[0x04], 4);

        w.push(&f0);
        w.push(&f1);
        w.push(&f2);
        assert!(w.is_full());

        w.push(&f3); // evicts f0
        assert_eq!(w.len(), 3);
        assert_eq!(w.latest_frame().unwrap().frame_id, 3);
    }

    #[test]
    fn active_detectors_correct() {
        let mut w = SyndromeWindow::new(4, 2);
        // Frame: [true, false, true, false] → detectors 0 and 2 fired
        let f = make_frame(0, &[0x81, 0x01, 0x81, 0x01], 4);
        w.push(&f);
        let active = w.active_detectors();
        assert!(active.contains(&0));
        assert!(active.contains(&2));
        assert!(!active.contains(&1));
    }
}
