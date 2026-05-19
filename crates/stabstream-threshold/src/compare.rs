use std::collections::BTreeMap;

use anyhow::{Context, Result};
use clap::Parser;

use crate::data::{read_csv, read_json, write_csv, DataPoint};
use crate::plot::write_plot;

#[derive(Parser, Debug)]
pub struct CompareArgs {
    /// Input files from `stabstream-threshold run` (CSV or JSON). Repeat for multiple.
    #[arg(long, short, required = true, value_name = "FILE")]
    pub input: Vec<String>,

    /// Labels for each input file (displayed in the legend). Defaults to filename.
    #[arg(long, value_name = "LABEL")]
    pub label: Vec<String>,

    /// SVG output path for the threshold plot.
    #[arg(long, value_name = "FILE")]
    pub plot: Option<String>,

    /// Merged CSV output path (all data points from all inputs combined).
    #[arg(long, short, value_name = "FILE")]
    pub out: Option<String>,

    /// Print a summary table to stdout.
    #[arg(long, short)]
    pub verbose: bool,
}

pub fn compare(args: CompareArgs) -> Result<()> {
    let mut all_points: Vec<DataPoint> = Vec::new();

    for (i, path) in args.input.iter().enumerate() {
        let label = args
            .label
            .get(i)
            .map(String::as_str)
            .unwrap_or(path.as_str());
        let points = load_any(path)?;
        eprintln!(
            "Loaded {} data points from '{}' ({})",
            points.len(),
            path,
            label
        );
        all_points.extend(points);
    }

    if all_points.is_empty() {
        anyhow::bail!("no data points loaded from any input file");
    }

    // Print summary grouped by distance
    if args.verbose {
        print_summary_table(&all_points);
    }

    // Write merged CSV
    if let Some(ref out) = args.out {
        write_csv(out, &all_points)?;
        eprintln!("Merged CSV: {out}");
    }

    // Write SVG plot
    if let Some(ref plot_path) = args.plot {
        write_plot(plot_path, &all_points)?;
        eprintln!("Plot: {plot_path}");
    }

    // Always print the threshold estimate
    estimate_threshold(&all_points);

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_any(path: &str) -> Result<Vec<DataPoint>> {
    if path.ends_with(".json") {
        read_json(path).with_context(|| format!("loading '{path}'"))
    } else {
        read_csv(path).with_context(|| format!("loading '{path}'"))
    }
}

fn print_summary_table(points: &[DataPoint]) {
    let mut by_distance: BTreeMap<u32, Vec<&DataPoint>> = BTreeMap::new();
    for dp in points {
        by_distance.entry(dp.distance).or_default().push(dp);
    }
    println!(
        "\n{:>8}  {:>12}  {:>12}  {:>12}  {:>10}",
        "distance", "p_physical", "p_l", "±stderr", "shots"
    );
    for (&d, series) in &by_distance {
        let mut sorted = series.clone();
        sorted.sort_by(|a, b| a.p_physical.partial_cmp(&b.p_physical).unwrap());
        for dp in sorted {
            println!(
                "{:>8}  {:>12.4e}  {:>12.4e}  {:>12.4e}  {:>10}",
                d, dp.p_physical, dp.p_l, dp.p_l_err, dp.shots
            );
        }
    }
    println!();
}

/// Heuristic threshold estimate: the crossing of adjacent-distance p_l curves.
///
/// For each pair of consecutive distances (d, d+Δ), finds the p_physical value
/// where p_l(d) ≈ p_l(d+Δ) by linear interpolation between adjacent points.
fn estimate_threshold(points: &[DataPoint]) {
    let mut by_distance: BTreeMap<u32, Vec<&DataPoint>> = BTreeMap::new();
    for dp in points {
        by_distance.entry(dp.distance).or_default().push(dp);
    }

    let distances: Vec<u32> = by_distance.keys().cloned().collect();
    if distances.len() < 2 {
        return;
    }

    let mut crossings: Vec<f64> = Vec::new();

    for pair in distances.windows(2) {
        let d_lo = pair[0];
        let d_hi = pair[1];
        let s_lo = {
            let mut v = by_distance[&d_lo].clone();
            v.sort_by(|a, b| a.p_physical.partial_cmp(&b.p_physical).unwrap());
            v
        };
        let s_hi = {
            let mut v = by_distance[&d_hi].clone();
            v.sort_by(|a, b| a.p_physical.partial_cmp(&b.p_physical).unwrap());
            v
        };

        // For each consecutive pair of p values where the sign of (p_l_lo - p_l_hi) flips
        for i in 0..s_lo.len().saturating_sub(1) {
            let p_a = s_lo[i].p_physical;
            let p_b = s_lo[i + 1].p_physical;
            // Interpolate p_l for d_hi at p_a and p_b
            let hi_a = interp_at(s_hi.as_slice(), p_a);
            let hi_b = interp_at(s_hi.as_slice(), p_b);
            if let (Some(hi_a), Some(hi_b)) = (hi_a, hi_b) {
                let diff_a = s_lo[i].p_l - hi_a;
                let diff_b = s_lo[i + 1].p_l - hi_b;
                if diff_a * diff_b < 0.0 {
                    // Linear interpolation of zero crossing
                    let t = diff_a / (diff_a - diff_b);
                    let p_cross = p_a + t * (p_b - p_a);
                    crossings.push(p_cross);
                }
            }
        }

        if !crossings.is_empty() {
            let mean = crossings.iter().sum::<f64>() / crossings.len() as f64;
            println!(
                "Estimated threshold (d={} vs d={}): p_th ≈ {:.4e}",
                d_lo, d_hi, mean
            );
        }
    }

    if crossings.len() > 1 {
        let mean = crossings.iter().sum::<f64>() / crossings.len() as f64;
        println!("Overall threshold estimate: p_th ≈ {:.4e}", mean);
    }
}

/// Linear interpolation of p_l at `p` from a sorted series of data points.
fn interp_at(series: &[&DataPoint], p: f64) -> Option<f64> {
    if series.is_empty() {
        return None;
    }
    if p <= series[0].p_physical {
        return Some(series[0].p_l);
    }
    if p >= series[series.len() - 1].p_physical {
        return Some(series[series.len() - 1].p_l);
    }
    for i in 0..series.len() - 1 {
        let p0 = series[i].p_physical;
        let p1 = series[i + 1].p_physical;
        if p >= p0 && p <= p1 {
            let t = (p - p0) / (p1 - p0);
            return Some(series[i].p_l + t * (series[i + 1].p_l - series[i].p_l));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::DataPoint;

    fn make_point(d: u32, p: f64, pl: f64) -> DataPoint {
        DataPoint {
            distance: d,
            p_physical: p,
            p_l: pl,
            p_l_err: 0.001,
            shots: 10000,
            logical_errors: (pl * 10000.0) as u64,
        }
    }

    #[test]
    fn interp_at_midpoint() {
        let p0 = make_point(3, 0.0, 0.0);
        let p1 = make_point(3, 1.0, 1.0);
        let series: Vec<&DataPoint> = vec![&p0, &p1];
        let v = interp_at(&series, 0.5).unwrap();
        assert!((v - 0.5).abs() < 1e-10);
    }

    #[test]
    fn interp_at_boundary() {
        let p0 = make_point(3, 0.1, 0.05);
        let p1 = make_point(3, 0.2, 0.15);
        let series: Vec<&DataPoint> = vec![&p0, &p1];
        assert!((interp_at(&series, 0.05).unwrap() - 0.05).abs() < 1e-10);
        assert!((interp_at(&series, 0.25).unwrap() - 0.15).abs() < 1e-10);
    }
}
