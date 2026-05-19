use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One (distance, p_physical, p_l) data point from a threshold sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Code distance d.
    pub distance: u32,
    /// Physical (gate-level) error rate — either user-supplied or inferred from the DEM.
    pub p_physical: f64,
    /// Mean logical error rate across all observables.
    pub p_l: f64,
    /// Binomial standard error: sqrt(p_l * (1-p_l) / shots).
    pub p_l_err: f64,
    /// Number of shots used to compute p_l.
    pub shots: u64,
    /// Total logical errors across all observables.
    pub logical_errors: u64,
}

impl DataPoint {
    pub fn compute_stderr(p_l: f64, shots: u64) -> f64 {
        if shots == 0 {
            0.0
        } else {
            (p_l * (1.0 - p_l) / shots as f64).sqrt()
        }
    }
}

/// Full output from a `run` invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutput {
    pub decoder: String,
    pub shots_per_point: u64,
    pub data_points: Vec<DataPoint>,
}

// ---------------------------------------------------------------------------
// CSV I/O
// ---------------------------------------------------------------------------

pub fn write_csv(path: &str, points: &[DataPoint]) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path).with_context(|| format!("creating '{path}'"))?;
    writeln!(f, "distance,p_physical,p_l,p_l_err,shots,logical_errors")?;
    for dp in points {
        writeln!(
            f,
            "{},{:.10e},{:.10e},{:.10e},{},{}",
            dp.distance, dp.p_physical, dp.p_l, dp.p_l_err, dp.shots, dp.logical_errors
        )?;
    }
    Ok(())
}

pub fn read_csv(path: &str) -> Result<Vec<DataPoint>> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading '{path}'"))?;
    let mut points = Vec::new();
    let mut lines = content.lines();
    lines.next(); // skip header
    for (lineno, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 6 {
            anyhow::bail!(
                "{path}:{}: expected 6 CSV columns, got {}",
                lineno + 2,
                fields.len()
            );
        }
        points.push(DataPoint {
            distance: fields[0]
                .parse()
                .with_context(|| format!("{path}:{}: distance", lineno + 2))?,
            p_physical: fields[1]
                .parse()
                .with_context(|| format!("{path}:{}: p_physical", lineno + 2))?,
            p_l: fields[2]
                .parse()
                .with_context(|| format!("{path}:{}: p_l", lineno + 2))?,
            p_l_err: fields[3]
                .parse()
                .with_context(|| format!("{path}:{}: p_l_err", lineno + 2))?,
            shots: fields[4]
                .parse()
                .with_context(|| format!("{path}:{}: shots", lineno + 2))?,
            logical_errors: fields[5]
                .parse()
                .with_context(|| format!("{path}:{}: logical_errors", lineno + 2))?,
        });
    }
    Ok(points)
}

// ---------------------------------------------------------------------------
// JSON I/O
// ---------------------------------------------------------------------------

pub fn write_json(path: &str, output: &RunOutput) -> Result<()> {
    let json = serde_json::to_string_pretty(output)?;
    std::fs::write(path, json).with_context(|| format!("writing '{path}'"))?;
    Ok(())
}

pub fn read_json(path: &str) -> Result<Vec<DataPoint>> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading '{path}'"))?;
    // Try RunOutput first, fall back to bare Vec<DataPoint>
    if let Ok(output) = serde_json::from_str::<RunOutput>(&content) {
        return Ok(output.data_points);
    }
    serde_json::from_str(&content).with_context(|| format!("parsing JSON from '{path}'"))
}
