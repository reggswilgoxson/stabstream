use std::collections::VecDeque;

const WINDOW: usize = 1000;

/// Rolling statistics over the last [`WINDOW`] frames.
#[derive(Debug, Default)]
pub struct MetricsAggregator {
    syndrome_rates: VecDeque<f64>,
    fire_rates: VecDeque<f64>,
    cluster_size_1: u64,
    cluster_size_2: u64,
    cluster_size_3: u64,
    cluster_size_4plus: u64,
    parse_latencies_ns: VecDeque<u64>,
    total_frames: u64,
    dropped_frames: u64,
}

impl MetricsAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one parsed frame.
    pub fn record(
        &mut self,
        syndrome_rate: f64,
        fire_rate_pct: f64,
        cluster_size: u32,
        parse_latency_ns: u64,
        dropped: bool,
    ) {
        self.total_frames += 1;
        if dropped {
            self.dropped_frames += 1;
        }

        push_window(&mut self.syndrome_rates, syndrome_rate);
        push_window(&mut self.fire_rates, fire_rate_pct);
        push_window(&mut self.parse_latencies_ns, parse_latency_ns);

        match cluster_size {
            1 => self.cluster_size_1 += 1,
            2 => self.cluster_size_2 += 1,
            3 => self.cluster_size_3 += 1,
            _ => self.cluster_size_4plus += 1,
        }
    }

    pub fn latest_syndrome_rate(&self) -> f64 {
        self.syndrome_rates.back().copied().unwrap_or(0.0)
    }

    pub fn latest_fire_rate_pct(&self) -> f64 {
        self.fire_rates.back().copied().unwrap_or(0.0)
    }

    /// Returns the last 100 syndrome-rate samples (for sparkline rendering).
    pub fn syndrome_rate_window(&self) -> Vec<u64> {
        self.syndrome_rates
            .iter()
            .rev()
            .take(100)
            .rev()
            .map(|&v| v as u64)
            .collect()
    }

    /// Returns cluster-size counts as `[size_1, size_2, size_3, size_4+]`.
    pub fn cluster_histogram(&self) -> [u64; 4] {
        [
            self.cluster_size_1,
            self.cluster_size_2,
            self.cluster_size_3,
            self.cluster_size_4plus,
        ]
    }

    /// p50 parse latency in nanoseconds.
    pub fn latency_p50_ns(&self) -> u64 {
        percentile(&self.parse_latencies_ns, 50)
    }

    /// p99 parse latency in nanoseconds.
    pub fn latency_p99_ns(&self) -> u64 {
        percentile(&self.parse_latencies_ns, 99)
    }

    pub fn drop_rate(&self) -> f64 {
        if self.total_frames == 0 {
            0.0
        } else {
            self.dropped_frames as f64 / self.total_frames as f64
        }
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    pub fn mean_syndrome_rate(&self) -> f64 {
        if self.syndrome_rates.is_empty() {
            return 0.0;
        }
        self.syndrome_rates.iter().sum::<f64>() / self.syndrome_rates.len() as f64
    }

    pub fn mean_fire_rate_pct(&self) -> f64 {
        if self.fire_rates.is_empty() {
            return 0.0;
        }
        self.fire_rates.iter().sum::<f64>() / self.fire_rates.len() as f64
    }
}

fn push_window<T>(queue: &mut VecDeque<T>, value: T) {
    if queue.len() >= WINDOW {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn percentile(queue: &VecDeque<u64>, pct: usize) -> u64 {
    if queue.is_empty() {
        return 0;
    }
    let mut sorted: Vec<u64> = queue.iter().copied().collect();
    sorted.sort_unstable();
    let idx = (sorted.len() * pct / 100).min(sorted.len() - 1);
    sorted[idx]
}
