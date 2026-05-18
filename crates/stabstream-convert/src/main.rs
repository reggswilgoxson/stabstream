use std::fs::File;
use std::io::BufReader;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use stabstream_convert::{export_owned_frame, QssfExporter, StimImporter, STIM_GENERIC_UUID};
use uuid::Uuid;

#[derive(Debug, Clone, ValueEnum)]
enum FromFormat {
    /// Stim detector-event 01 text format (one shot per line).
    Stim,
}

#[derive(Parser, Debug)]
#[command(
    name = "stabstream-convert",
    about = "Convert syndrome data between formats and QSSF binary"
)]
struct Args {
    /// Source format.
    #[arg(long, value_enum)]
    from: FromFormat,

    /// Input file path.
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
    let args = Args::parse();

    let schema_id: Uuid = args
        .schema_id
        .parse()
        .context("invalid schema UUID")?;

    match args.from {
        FromFormat::Stim => convert_stim(&args.input, &args.output, schema_id),
    }
}

fn convert_stim(input_path: &str, output_path: &str, schema_id: Uuid) -> Result<()> {
    let input_file =
        File::open(input_path).with_context(|| format!("cannot open '{input_path}'"))?;
    let output_file =
        File::create(output_path).with_context(|| format!("cannot create '{output_path}'"))?;

    let mut importer = StimImporter::new(BufReader::new(input_file));
    let mut exporter = QssfExporter::new(output_file, schema_id);

    let mut count = 0u64;
    while let Some(frame) = importer.next_frame()? {
        export_owned_frame(&mut exporter, &frame)?;
        count += 1;
    }

    exporter.flush()?;
    eprintln!("Converted {count} frames → {output_path}");
    Ok(())
}
