//! Snapshot reports from the stabstream metrics layer.

use serde::{Deserialize, Serialize};

/// A point-in-time snapshot of logical error rate statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsReport {
    pub total_shots: u64,
    /// Logical error rate per observable (bit index).
    pub logical_error_rates: Vec<f64>,
    /// Logical error count per observable.
    pub logical_errors: Vec<u64>,
    /// Mean logical error rate across all observables.
    pub mean_logical_error_rate: f64,
}

impl MetricsReport {
    /// True when the mean logical error rate is below `threshold`.
    pub fn below_threshold(&self, threshold: f64) -> bool {
        self.mean_logical_error_rate < threshold
    }

    /// Format as a human-readable summary line.
    pub fn summary(&self) -> String {
        format!(
            "shots={} p_L_mean={:.4e} p_L_per_obs={:?}",
            self.total_shots,
            self.mean_logical_error_rate,
            self.logical_error_rates
                .iter()
                .map(|r| format!("{:.4e}", r))
                .collect::<Vec<_>>()
        )
    }
}

/// Full analysis report produced by replaying a QSSF recording through a decoder.
///
/// Includes logical error rates (when ground truth is available), decode latency
/// percentiles, per-ancilla fire frequency (for hardware debugging), and a
/// syndrome weight histogram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    /// Total frames read from the recording.
    pub frames_processed: u64,
    /// Number of full windows decoded (shots presented to the decoder).
    pub total_shots: u64,

    // --- Logical error rates ---
    /// Number of observables tracked.
    pub observable_count: usize,
    /// Per-observable logical error rate. All zeros when ground truth is absent.
    pub logical_error_rates: Vec<f64>,
    /// Mean logical error rate across all observables.
    pub mean_logical_error_rate: f64,
    /// Whether observable ground truth (QSSF tag 0x10) was present in the stream.
    pub ground_truth_available: bool,

    // --- Decode latency ---
    pub mean_decode_latency_ns: u64,
    pub p50_decode_latency_ns: u64,
    pub p99_decode_latency_ns: u64,
    pub max_decode_latency_ns: u64,

    // --- Hardware diagnostics ---
    /// Number of ancilla qubits observed in the recording.
    pub ancilla_count: usize,
    /// Fraction of frames in which each ancilla fired (index = ancilla id).
    pub per_ancilla_fire_frequency: Vec<f64>,
    /// Syndrome weight histogram: `syndrome_weight_histogram[w]` = number of
    /// frames in which exactly `w` ancillas fired.
    pub syndrome_weight_histogram: Vec<u64>,
}

impl AnalysisReport {
    pub fn summary(&self) -> String {
        let latency_str = if self.total_shots > 0 {
            format!(
                "latency_mean={} ns  p50={} ns  p99={} ns  max={} ns",
                self.mean_decode_latency_ns,
                self.p50_decode_latency_ns,
                self.p99_decode_latency_ns,
                self.max_decode_latency_ns,
            )
        } else {
            "latency=n/a".to_string()
        };

        let pl_str = if self.ground_truth_available {
            format!("p_L_mean={:.4e}", self.mean_logical_error_rate)
        } else {
            "p_L=n/a (no ground truth)".to_string()
        };

        format!(
            "frames={} shots={} {} {}",
            self.frames_processed, self.total_shots, pl_str, latency_str,
        )
    }
}
