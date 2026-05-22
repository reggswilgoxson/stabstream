use std::fs::File;
use std::io::{BufReader, Write};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};
use stabstream_convert::{
    export_owned_frame, QssfExporter, StimImporter, StimWithObsImporter, STIM_GENERIC_UUID,
};
use stabstream_dem::parser::DetectorErrorModel;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(
    name = "stabstream-convert",
    about = "Convert syndrome data between formats and QSSF binary",
    long_about = "Convert syndrome data between formats and QSSF binary.\n\n\
        SUBCOMMANDS\n\
        -----------\n\
        stim-to-qssf   Run `stim detect` on a circuit file and write QSSF output.\n\
                       Optionally embeds observable ground truth (QSSF tag 0x10)\n\
                       for offline threshold analysis.\n\
        from-file      Convert a pre-generated Stim 01 detector-events text file\n\
                       to QSSF binary."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run `stim detect` on a circuit and write QSSF output (requires Stim on PATH).
    StimToQssf(StimToQssfArgs),
    /// Convert a pre-generated Stim 01 detector-events file to QSSF.
    FromFile(FromFileArgs),
    /// Sample shots directly from a DEM and write an ML training dataset.
    DemToDataset(DemToDatasetArgs),
}

#[derive(Parser, Debug)]
pub struct StimToQssfArgs {
    /// Stim circuit file (.stim).
    #[arg(long)]
    circuit: String,

    /// Number of syndrome shots to generate.
    #[arg(long, default_value_t = 10_000)]
    shots: u64,

    /// Embed observable ground truth in QSSF metadata tag 0x10.
    ///
    /// Requires Stim ≥ 1.13 (--obs-out-format support).
    #[arg(long, default_value_t = false)]
    with_observables: bool,

    /// Output QSSF file path.
    #[arg(long = "out", short = 'o')]
    output: String,

    /// Schema UUID to embed in the QSSF file header.
    #[arg(long, default_value = STIM_GENERIC_UUID)]
    schema_id: String,
}

#[derive(Parser, Debug)]
pub struct FromFileArgs {
    /// Input Stim detector-events file (01 text format, one shot per line).
    #[arg(long, short = 'i')]
    input: String,

    /// Output QSSF file path.
    #[arg(long, short = 'o')]
    output: String,

    /// Schema UUID to embed in the QSSF file header.
    #[arg(long, default_value = STIM_GENERIC_UUID)]
    schema_id: String,
}

#[derive(Parser, Debug)]
pub struct DemToDatasetArgs {
    /// Stim Detector Error Model file (.dem).
    #[arg(long)]
    dem: String,

    /// Number of syndrome shots to sample.
    #[arg(long, default_value_t = 100_000)]
    shots: u64,

    /// Output dataset file path (SSDS binary format, readable by stabstream.io.load_dataset).
    #[arg(long = "out", short = 'o')]
    output: String,

    /// Random seed (omit for entropy-seeded sampling).
    #[arg(long)]
    seed: Option<u64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::StimToQssf(args) => run_stim_to_qssf(args),
        Commands::FromFile(args) => run_from_file(args),
        Commands::DemToDataset(args) => run_dem_to_dataset(args),
    }
}

// ─── stim-to-qssf ────────────────────────────────────────────────────────────

fn run_stim_to_qssf(args: StimToQssfArgs) -> Result<()> {
    let schema_id: Uuid = args.schema_id.parse().context("invalid schema UUID")?;
    let output_file =
        File::create(&args.output).with_context(|| format!("cannot create '{}'", args.output))?;
    let mut exporter = QssfExporter::new(output_file, schema_id);

    let circuit_file = File::open(&args.circuit)
        .with_context(|| format!("cannot open circuit '{}'", args.circuit))?;

    if args.with_observables {
        // Run stim detect with observable output written to a temp file.
        // We use .output() to collect all stdout in memory, then open the obs
        // tempfile after stim exits (guaranteeing the file is fully written).
        let obs_tmp = tempfile_path();
        let output = Command::new("stim")
            .args([
                "detect",
                "--shots",
                &args.shots.to_string(),
                "--obs-out-format",
                "01",
                "--obs-out",
                &obs_tmp,
            ])
            .stdin(Stdio::from(circuit_file))
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow::anyhow!("'stim' not found on PATH — install via `pip install stim`")
                } else {
                    anyhow::anyhow!("failed to run stim: {e}")
                }
            })?;

        if !output.status.success() {
            bail!("stim exited with status {}", output.status);
        }

        let det_cursor = std::io::Cursor::new(output.stdout);
        let obs_file = File::open(&obs_tmp)
            .with_context(|| format!("cannot open stim obs output '{obs_tmp}'"))?;

        let mut importer =
            StimWithObsImporter::new(BufReader::new(det_cursor), BufReader::new(obs_file));

        let mut count = 0u64;
        while let Some(frame) = importer.next_frame()? {
            export_owned_frame(&mut exporter, &frame)?;
            count += 1;
        }
        let _ = std::fs::remove_file(&obs_tmp);
        exporter.flush()?;
        eprintln!(
            "Converted {count} shots with observable ground truth → {}",
            args.output
        );
    } else {
        // No observables — stream stim stdout directly.
        let mut child = Command::new("stim")
            .args(["detect", "--shots", &args.shots.to_string()])
            .stdin(Stdio::from(circuit_file))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow::anyhow!("'stim' not found on PATH — install via `pip install stim`")
                } else {
                    anyhow::anyhow!("failed to spawn stim: {e}")
                }
            })?;

        let stdout = child.stdout.take().context("stim stdout unavailable")?;
        let mut importer = StimImporter::new(BufReader::new(stdout));

        let mut count = 0u64;
        while let Some(frame) = importer.next_frame()? {
            export_owned_frame(&mut exporter, &frame)?;
            count += 1;
        }

        let status = child.wait().context("waiting for stim")?;
        if !status.success() {
            bail!("stim exited with status {status}");
        }

        exporter.flush()?;
        eprintln!("Converted {count} shots → {}", args.output);
    }

    Ok(())
}

// ─── from-file ───────────────────────────────────────────────────────────────

fn run_from_file(args: FromFileArgs) -> Result<()> {
    let schema_id: Uuid = args.schema_id.parse().context("invalid schema UUID")?;
    let input_file =
        File::open(&args.input).with_context(|| format!("cannot open '{}'", args.input))?;
    let output_file =
        File::create(&args.output).with_context(|| format!("cannot create '{}'", args.output))?;

    let mut importer = StimImporter::new(BufReader::new(input_file));
    let mut exporter = QssfExporter::new(output_file, schema_id);

    let mut count = 0u64;
    while let Some(frame) = importer.next_frame()? {
        export_owned_frame(&mut exporter, &frame)?;
        count += 1;
    }

    exporter.flush()?;
    eprintln!("Converted {count} frames → {}", args.output);
    Ok(())
}

// ─── dem-to-dataset ──────────────────────────────────────────────────────────

fn run_dem_to_dataset(args: DemToDatasetArgs) -> Result<()> {
    let dem_text = std::fs::read_to_string(&args.dem)
        .with_context(|| format!("cannot read DEM file '{}'", args.dem))?;
    let dem = DetectorErrorModel::parse(&dem_text)
        .map_err(|e| anyhow::anyhow!("failed to parse DEM: {e}"))?;

    let sampler = InlineDemSampler::from_dem(&dem);
    let det = sampler.detector_count;
    let obs = sampler.observable_count;
    let shots = args.shots as usize;

    let mut rng: SmallRng = match args.seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    };

    // Pre-allocate output buffers.
    let mut x_buf: Vec<u8> = Vec::with_capacity(shots * det);
    let mut y_buf: Vec<u64> = Vec::with_capacity(shots);

    let mut events = vec![false; det];
    for _ in 0..shots {
        for v in events.iter_mut() {
            *v = false;
        }
        let mut obs_flip = 0u64;
        for error in &sampler.errors {
            if rng.next_u64() <= error.threshold {
                for &d in &error.detectors {
                    events[d as usize] ^= true;
                }
                obs_flip ^= error.obs_bitmask;
            }
        }
        for &e in &events {
            x_buf.push(e as u8);
        }
        y_buf.push(obs_flip);
    }

    // Write SSDS binary format.
    let mut out = File::create(&args.output)
        .with_context(|| format!("cannot create '{}'", args.output))?;

    out.write_all(b"SSDS")?;                              // magic
    out.write_all(&[1u8])?;                               // version
    out.write_all(&(shots as u64).to_le_bytes())?;        // shots
    out.write_all(&(det as u32).to_le_bytes())?;          // detector_count
    out.write_all(&(obs as u32).to_le_bytes())?;          // observable_count
    out.write_all(&x_buf)?;                               // X (uint8)
    for &y in &y_buf {
        out.write_all(&y.to_le_bytes())?;
    }
    out.flush()?;

    eprintln!(
        "Sampled {shots} shots ({det} detectors, {obs} observables) → {}",
        args.output
    );
    Ok(())
}

// ─── inline DEM sampler (avoids circular dependency with stabstream-sim) ─────

struct InlineCompiledError {
    threshold: u64,
    detectors: Vec<u32>,
    obs_bitmask: u64,
}

struct InlineDemSampler {
    errors: Vec<InlineCompiledError>,
    detector_count: usize,
    observable_count: usize,
}

impl InlineDemSampler {
    fn from_dem(dem: &DetectorErrorModel) -> Self {
        let errors = dem
            .errors
            .iter()
            .map(|e| {
                let p = e.probability;
                let threshold = if p >= 1.0 {
                    u64::MAX
                } else if p <= 0.0 {
                    0
                } else {
                    (p * u64::MAX as f64) as u64
                };
                let obs_bitmask = e.observables.iter().fold(0u64, |acc, &o| acc | (1u64 << o));
                InlineCompiledError {
                    threshold,
                    detectors: e.detectors.clone(),
                    obs_bitmask,
                }
            })
            .collect();
        Self {
            errors,
            detector_count: dem.detector_count,
            observable_count: dem.observable_count,
        }
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn tempfile_path() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("/tmp/stabstream_obs_{ts}.01")
}
