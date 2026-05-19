use std::collections::VecDeque;
use std::time::Instant;

const WINDOW: usize = 1000;
const EXPORT_WINDOW: usize = 2000;

/// Rolling statistics over the last [`WINDOW`] frames.
pub struct MetricsAggregator {
    // existing rolling windows
    syndrome_rates: VecDeque<f64>,
    fire_rates: VecDeque<f64>,
    cluster_size_1: u64,
    cluster_size_2: u64,
    cluster_size_3: u64,
    cluster_size_4plus: u64,
    parse_latencies_ns: VecDeque<u64>,
    total_frames: u64,
    dropped_frames: u64,

    // per-ancilla heatmap (cumulative fire counts + total frames seen)
    per_ancilla_fires: Vec<u64>,
    heatmap_frames: u64,
    pub ancilla_count: u16,

    // logical error rate from observable_flips tag 0x10
    obs_errors: u64,  // frames where observable_flips != 0
    obs_total: u64,   // frames where metadata.observable_flips was Some

    // frames-behind-real-time
    start: Instant,
    frame_period_ns: u64, // expected syndrome cycle

    // export ring buffer: last EXPORT_WINDOW (detector_events, frame_id) pairs
    export_buf: VecDeque<(Vec<bool>, u64)>,

    // decoder name shown in status line
    pub decoder_name: String,
}

impl Default for MetricsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsAggregator {
    pub fn new() -> Self {
        Self {
            syndrome_rates: VecDeque::new(),
            fire_rates: VecDeque::new(),
            cluster_size_1: 0,
            cluster_size_2: 0,
            cluster_size_3: 0,
            cluster_size_4plus: 0,
            parse_latencies_ns: VecDeque::new(),
            total_frames: 0,
            dropped_frames: 0,
            per_ancilla_fires: Vec::new(),
            heatmap_frames: 0,
            ancilla_count: 0,
            obs_errors: 0,
            obs_total: 0,
            start: Instant::now(),
            frame_period_ns: 1_100,
            export_buf: VecDeque::new(),
            decoder_name: "none".to_string(),
        }
    }

    pub fn with_decoder(mut self, name: &str) -> Self {
        self.decoder_name = name.to_string();
        self
    }

    pub fn with_frame_period_ns(mut self, ns: u64) -> Self {
        self.frame_period_ns = ns;
        self
    }

    /// Record one parsed frame (basic counters).
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

    /// Record per-ancilla detector events and optional observable_flips.
    pub fn record_ancilla_events(
        &mut self,
        events: &[bool],
        frame_id: u64,
        obs_flips: Option<u64>,
    ) {
        let n = events.len();
        if n == 0 {
            return;
        }

        // Resize heatmap if ancilla count changed
        if self.per_ancilla_fires.len() != n {
            self.per_ancilla_fires = vec![0u64; n];
            self.heatmap_frames = 0;
            self.ancilla_count = n as u16;
        }
        self.heatmap_frames += 1;

        for (i, &fired) in events.iter().enumerate() {
            if fired {
                self.per_ancilla_fires[i] += 1;
            }
        }

        // p_L tracking
        if let Some(flips) = obs_flips {
            self.obs_total += 1;
            if flips != 0 {
                self.obs_errors += 1;
            }
        }

        // export ring buffer
        if self.export_buf.len() >= EXPORT_WINDOW {
            self.export_buf.pop_front();
        }
        self.export_buf.push_back((events.to_vec(), frame_id));
    }

    // ---- heatmap ----

    /// Per-ancilla fire rate in [0, 1] over all recorded frames.
    pub fn per_ancilla_fire_rates(&self) -> Vec<f64> {
        if self.heatmap_frames == 0 {
            return vec![0.0; self.per_ancilla_fires.len()];
        }
        self.per_ancilla_fires
            .iter()
            .map(|&c| c as f64 / self.heatmap_frames as f64)
            .collect()
    }

    // ---- p_L ----

    /// Logical error rate from observable_flips metadata (None if no obs data yet).
    pub fn p_l(&self) -> Option<f64> {
        if self.obs_total == 0 {
            None
        } else {
            Some(self.obs_errors as f64 / self.obs_total as f64)
        }
    }

    pub fn obs_total(&self) -> u64 {
        self.obs_total
    }

    // ---- frames behind real-time ----

    /// How many frames behind real-time based on wall clock and expected cycle.
    /// Positive = behind, negative = ahead.
    pub fn frames_behind(&self, actual_frames: u64) -> i64 {
        let elapsed_ns = self.start.elapsed().as_nanos() as u64;
        let expected = elapsed_ns / self.frame_period_ns.max(1);
        expected as i64 - actual_frames as i64
    }

    // ---- export ----

    /// Snapshot of the export ring buffer for 'e' key export.
    pub fn export_snapshot(&self) -> (Vec<(Vec<bool>, u64)>, u16) {
        (
            self.export_buf.iter().cloned().collect(),
            self.ancilla_count,
        )
    }

    // ---- existing accessors ----

    pub fn latest_syndrome_rate(&self) -> f64 {
        self.syndrome_rates.back().copied().unwrap_or(0.0)
    }

    pub fn latest_fire_rate_pct(&self) -> f64 {
        self.fire_rates.back().copied().unwrap_or(0.0)
    }

    pub fn syndrome_rate_window(&self) -> Vec<u64> {
        self.syndrome_rates
            .iter()
            .rev()
            .take(100)
            .rev()
            .map(|&v| v as u64)
            .collect()
    }

    pub fn cluster_histogram(&self) -> [u64; 4] {
        [
            self.cluster_size_1,
            self.cluster_size_2,
            self.cluster_size_3,
            self.cluster_size_4plus,
        ]
    }

    pub fn latency_p50_ns(&self) -> u64 {
        percentile(&self.parse_latencies_ns, 50)
    }

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
