//! stabstream-analyze — offline QEC decoding analysis of QSSF recordings.
//!
//! Usage:
//!   stabstream-analyze --input recording.qssf [--dem model.dem]
//!                      [--decoder union-find] [--window-depth 5]
//!                      [--observable-count 1] [--output report.json]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use stabstream_decoder::union_find::UnionFindDecoder;
use stabstream_decoder::NullDecoder;
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};
use stabstream_replay::{analyze_file, AnalysisConfig};

#[derive(Parser)]
#[command(
    name = "stabstream-analyze",
    about = "Analyze a QSSF syndrome recording through a QEC decoder",
    long_about = "Reads a QSSF recording (plain or zstd-compressed), slides a SyndromeWindow \
through the frames, decodes each full window, and reports logical error rates, \
decode latency percentiles, per-ancilla fire frequencies, and syndrome weight \
distributions."
)]
struct Args {
    /// Input QSSF file (plain or .zst compressed)
    #[arg(long, short, value_name = "FILE")]
    input: PathBuf,

    /// Stim DEM model file (required for union-find decoder)
    #[arg(long, value_name = "FILE")]
    dem: Option<PathBuf>,

    /// Decoder to use: "union-find" (default) or "null"
    #[arg(long, default_value = "union-find")]
    decoder: String,

    /// Number of syndrome rounds to hold in the sliding window
    #[arg(long, default_value_t = 5)]
    window_depth: usize,

    /// Number of logical observables to track
    #[arg(long, default_value_t = 1)]
    observable_count: usize,

    /// Write JSON report to this file (default: print to stdout)
    #[arg(long, short, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Print a human-readable summary even when --output is set
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let config = AnalysisConfig {
        window_depth: args.window_depth,
        observable_count: args.observable_count,
    };

    let report = match args.decoder.as_str() {
        "null" => {
            let dec = NullDecoder;
            analyze_file(&args.input, &dec, config)
                .with_context(|| format!("analyzing {:?}", args.input))?
        }
        "union-find" | "uf" => {
            let dem_path = args
                .dem
                .as_ref()
                .with_context(|| "union-find decoder requires --dem <model.dem>")?;
            let dem_text = std::fs::read_to_string(dem_path)
                .with_context(|| format!("reading DEM {:?}", dem_path))?;
            let dem = DetectorErrorModel::parse(&dem_text).with_context(|| "parsing DEM")?;
            let graph = Arc::new(SpacetimeGraph::from_dem(&dem));
            let dec = UnionFindDecoder::new(Arc::clone(&graph));
            analyze_file(&args.input, &dec, config)
                .with_context(|| format!("analyzing {:?}", args.input))?
        }
        other => anyhow::bail!("unknown decoder '{}' — use 'union-find' or 'null'", other),
    };

    let json = serde_json::to_string_pretty(&report).context("serializing report to JSON")?;

    match &args.output {
        Some(path) => {
            std::fs::write(path, &json).with_context(|| format!("writing report to {:?}", path))?;
            if args.verbose {
                eprintln!("{}", report.summary());
            }
            eprintln!("Report written to {:?}", path);
        }
        None => {
            println!("{}", json);
            eprintln!("{}", report.summary());
        }
    }

    Ok(())
}
