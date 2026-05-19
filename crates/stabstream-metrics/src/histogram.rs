//! Simple power-of-2 bucket histogram for decode latency and syndrome weights.

/// A fixed-bucket histogram with `N` power-of-2 buckets.
///
/// Bucket `i` covers `[2^i, 2^(i+1))`. Values ≥ 2^N land in the last bucket.
pub struct Histogram {
    buckets: Vec<u64>,
    total: u64,
}

impl Histogram {
    pub fn new(bucket_count: usize) -> Self {
        Self {
            buckets: vec![0; bucket_count.max(1)],
            total: 0,
        }
    }

    /// Record a value into the appropriate bucket.
    pub fn record(&mut self, value: u64) {
        self.total += 1;
        let idx = if value == 0 {
            0
        } else {
            let bit = 63 - value.leading_zeros() as usize;
            bit.min(self.buckets.len() - 1)
        };
        self.buckets[idx] += 1;
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// Count of values in bucket `i` (covering `[2^i, 2^(i+1))`).
    pub fn bucket(&self, i: usize) -> u64 {
        self.buckets.get(i).copied().unwrap_or(0)
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_lands_in_bucket_zero() {
        let mut h = Histogram::new(8);
        h.record(0);
        assert_eq!(h.bucket(0), 1);
    }

    #[test]
    fn powers_of_two_land_in_correct_buckets() {
        let mut h = Histogram::new(16);
        h.record(1); // 2^0 → bucket 0
        h.record(2); // 2^1 → bucket 1
        h.record(4); // 2^2 → bucket 2
        h.record(8); // 2^3 → bucket 3
        assert_eq!(h.bucket(0), 1);
        assert_eq!(h.bucket(1), 1);
        assert_eq!(h.bucket(2), 1);
        assert_eq!(h.bucket(3), 1);
    }
}
