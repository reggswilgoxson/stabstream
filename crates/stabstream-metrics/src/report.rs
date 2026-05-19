//! Snapshot report from `LogicalErrorAccumulator`.

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
