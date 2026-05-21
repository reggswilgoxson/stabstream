use std::fs::File;
use std::io::BufReader;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use stabstream_convert::{
    export_owned_frame, QssfExporter, StimImporter, StimWithObsImporter, STIM_GENERIC_UUID,
};
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::StimToQssf(args) => run_stim_to_qssf(args),
        Commands::FromFile(args) => run_from_file(args),
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

// ─── helpers ─────────────────────────────────────────────────────────────────

fn tempfile_path() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("/tmp/stabstream_obs_{ts}.01")
}
