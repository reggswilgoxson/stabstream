/// Controls how aggressively the validator rejects frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidationPolicy {
    /// Enforce parity constraints, timing bounds, and CRC integrity. Recommended.
    #[default]
    StrictParity,
    /// Check CRC and payload length only; skip parity math. Useful for fast replay.
    CrcOnly,
    /// Pass all frames through without inspection. For benchmarking parse overhead.
    Disabled,
}
