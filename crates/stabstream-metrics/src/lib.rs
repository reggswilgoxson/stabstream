//! Logical error rate accumulation and QEC benchmarking metrics.
//!
//! The fundamental question in QEC is: does the logical error rate p_L fall
//! below the physical error rate p as code distance d increases?
//! `LogicalErrorAccumulator` answers this question by recording decoder
//! outputs against ground-truth observable flips.
//!
//! # Thread safety
//!
//! Counters use `AtomicU64` so multiple decode threads can record results
//! concurrently without a `Mutex`.

use std::sync::atomic::{AtomicU64, Ordering};

use stabstream_decoder::DecoderResult;

pub mod report;
pub mod histogram;

pub use report::MetricsReport;
pub use histogram::Histogram;

/// Accumulates logical error statistics across many decoder shots.
///
/// A "shot" is one invocation of a decoder against a syndrome window.
/// A logical error occurs when the decoder's `observable_flips` XOR
/// `ground_truth` is non-zero for any observable.
pub struct LogicalErrorAccumulator {
    total_shots: AtomicU64,
    /// Per-observable error counts (up to 64 observables via bitmask).
    logical_errors: Vec<AtomicU64>,
    observable_count: usize,
}

impl LogicalErrorAccumulator {
    pub fn new(observable_count: usize) -> Self {
        let observable_count = observable_count.min(64);
        let logical_errors = (0..observable_count).map(|_| AtomicU64::new(0)).collect();
        Self {
            total_shots: AtomicU64::new(0),
            logical_errors,
            observable_count,
        }
    }

    /// Record one decoder result against the ground-truth observable flip bitmask.
    ///
    /// `ground_truth` is a bitmask where bit i = 1 means observable i was
    /// truly flipped by the physical error pattern (as written by the
    /// simulator into `FrameMetadata::observable_flips`).
    pub fn record(&self, result: &DecoderResult, ground_truth: u64) {
        self.total_shots.fetch_add(1, Ordering::Relaxed);
        let errors = result.observable_flips ^ ground_truth;
        for i in 0..self.observable_count {
            if errors & (1u64 << i) != 0 {
                self.logical_errors[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Logical error rate for a specific observable index.
    pub fn logical_error_rate(&self, observable: usize) -> f64 {
        let shots = self.total_shots.load(Ordering::Relaxed);
        if shots == 0 || observable >= self.observable_count {
            return 0.0;
        }
        self.logical_errors[observable].load(Ordering::Relaxed) as f64 / shots as f64
    }

    /// Mean logical error rate averaged across all observables.
    pub fn mean_logical_error_rate(&self) -> f64 {
        if self.observable_count == 0 {
            return 0.0;
        }
        let sum: f64 = (0..self.observable_count)
            .map(|i| self.logical_error_rate(i))
            .sum();
        sum / self.observable_count as f64
    }

    pub fn total_shots(&self) -> u64 {
        self.total_shots.load(Ordering::Relaxed)
    }

    pub fn total_logical_errors(&self, observable: usize) -> u64 {
        if observable >= self.observable_count {
            return 0;
        }
        self.logical_errors[observable].load(Ordering::Relaxed)
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.total_shots.store(0, Ordering::Relaxed);
        for c in &self.logical_errors {
            c.store(0, Ordering::Relaxed);
        }
    }

    /// Build a snapshot report from current counter values.
    pub fn report(&self) -> MetricsReport {
        let shots = self.total_shots();
        let rates: Vec<f64> = (0..self.observable_count)
            .map(|i| self.logical_error_rate(i))
            .collect();
        let errors: Vec<u64> = (0..self.observable_count)
            .map(|i| self.total_logical_errors(i))
            .collect();
        MetricsReport {
            total_shots: shots,
            logical_error_rates: rates,
            logical_errors: errors,
            mean_logical_error_rate: self.mean_logical_error_rate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use stabstream_decoder::DecoderResult;

    use super::*;

    fn make_result(obs_flips: u64) -> DecoderResult {
        DecoderResult {
            corrections: Vec::new(),
            confidence: 1.0,
            observable_flips: obs_flips,
        }
    }

    #[test]
    fn no_errors_gives_zero_rate() {
        let acc = LogicalErrorAccumulator::new(2);
        acc.record(&make_result(0b00), 0b00);
        acc.record(&make_result(0b00), 0b00);
        assert!((acc.logical_error_rate(0)).abs() < f64::EPSILON);
        assert!((acc.logical_error_rate(1)).abs() < f64::EPSILON);
    }

    #[test]
    fn all_errors_gives_one_rate() {
        let acc = LogicalErrorAccumulator::new(1);
        // Decoder says no flip, ground truth says flip → error every shot
        acc.record(&make_result(0), 1);
        acc.record(&make_result(0), 1);
        assert!((acc.logical_error_rate(0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn half_error_rate() {
        let acc = LogicalErrorAccumulator::new(1);
        acc.record(&make_result(1), 1); // correct
        acc.record(&make_result(0), 1); // wrong
        assert!((acc.logical_error_rate(0) - 0.5).abs() < f64::EPSILON);
        assert_eq!(acc.total_shots(), 2);
    }

    #[test]
    fn mean_rate_multi_observable() {
        let acc = LogicalErrorAccumulator::new(2);
        // obs 0: 1 error in 2 shots = 0.5
        // obs 1: 0 errors in 2 shots = 0.0
        acc.record(&make_result(0b01), 0b00); // obs 0 wrong
        acc.record(&make_result(0b00), 0b00); // correct
        assert!((acc.mean_logical_error_rate() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_clears_counters() {
        let acc = LogicalErrorAccumulator::new(1);
        acc.record(&make_result(0), 1);
        acc.reset();
        assert_eq!(acc.total_shots(), 0);
        assert!((acc.logical_error_rate(0)).abs() < f64::EPSILON);
    }
}
