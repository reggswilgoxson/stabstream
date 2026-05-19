/// ldpc-to-schema — generate a HardwareSchema JSON from LDPC code parameters.
///
/// # Usage
///
/// ```bash
/// # Named BB codes
/// ldpc-to-schema --bb bb-144-12-12 --out schemas/bb_144.json
/// ldpc-to-schema --bb bb-72-12-6  --out schemas/bb_72.json
///
/// # Custom BB code from polynomial parameters
/// ldpc-to-schema --bb custom --l 12 --m 6 \
///     --poly-a "3,0 0,1 0,2" --poly-b "0,3 1,0 2,0" \
///     --distance 12 --logical-qubits 12 \
///     --name "my_bb_144" --out my_bb_144.json
/// ```
use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use stabstream_dem::{
    ldpc::BbParams,
    schema_gen::schema_from_bb,
};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, ValueEnum)]
enum BbPreset {
    #[value(name = "bb-144-12-12")]
    Bb144_12_12,
    #[value(name = "bb-72-12-6")]
    Bb72_12_6,
    Custom,
}

#[derive(Parser, Debug)]
#[command(
    name = "ldpc-to-schema",
    about = "Generate a HardwareSchema JSON from LDPC (Bivariate Bicycle) code parameters",
    long_about = None
)]
struct Args {
    /// Named BB code preset or 'custom' for user-specified parameters.
    #[arg(long, value_enum)]
    bb: BbPreset,

    /// Cyclic group order l (required for --bb custom).
    #[arg(long)]
    l: Option<usize>,

    /// Cyclic group order m (required for --bb custom).
    #[arg(long)]
    m: Option<usize>,

    /// Polynomial A support as space-separated "di,dj" pairs, e.g. "3,0 0,1 0,2".
    #[arg(long)]
    poly_a: Option<String>,

    /// Polynomial B support as space-separated "di,dj" pairs, e.g. "0,3 1,0 2,0".
    #[arg(long)]
    poly_b: Option<String>,

    /// Known code distance (required for --bb custom).
    #[arg(long)]
    distance: Option<u8>,

    /// Number of logical qubits k (required for --bb custom).
    #[arg(long)]
    logical_qubits: Option<u16>,

    /// Schema name (defaults to a generated name).
    #[arg(long)]
    name: Option<String>,

    /// Output JSON file path. Writes to stdout if omitted.
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,

    /// Pretty-print the JSON output.
    #[arg(long, default_value_t = true)]
    pretty: bool,
}

fn parse_poly(s: &str) -> Result<Vec<(usize, usize)>> {
    s.split_whitespace()
        .map(|pair| {
            let mut it = pair.splitn(2, ',');
            let di: usize = it.next().context("missing di")?.parse().context("di not a number")?;
            let dj: usize = it.next().context("missing dj")?.parse().context("dj not a number")?;
            Ok((di, dj))
        })
        .collect()
}

fn main() -> Result<()> {
    let args = Args::parse();

    let params = match args.bb {
        BbPreset::Bb144_12_12 => BbParams::bb_144_12_12(),
        BbPreset::Bb72_12_6 => BbParams::bb_72_12_6(),
        BbPreset::Custom => {
            let l = args.l.context("--l is required for --bb custom")?;
            let m = args.m.context("--m is required for --bb custom")?;
            let poly_a = parse_poly(
                args.poly_a.as_deref().context("--poly-a is required for --bb custom")?,
            )?;
            let poly_b = parse_poly(
                args.poly_b.as_deref().context("--poly-b is required for --bb custom")?,
            )?;
            let distance = args.distance.context("--distance is required for --bb custom")?;
            let logical_qubits =
                args.logical_qubits.context("--logical-qubits is required for --bb custom")?;
            if poly_a.is_empty() || poly_b.is_empty() {
                bail!("polynomial support must be non-empty");
            }
            BbParams { l, m, poly_a, poly_b, distance, logical_qubits }
        }
    };

    let n = params.n();
    let name = args.name.unwrap_or_else(|| {
        format!(
            "bivariate_bicycle_{}_{}_{}", n, params.logical_qubits, params.distance
        )
    });

    let schema = schema_from_bb(&params, &name);

    let json = if args.pretty {
        serde_json::to_string_pretty(&schema)?
    } else {
        serde_json::to_string(&schema)?
    };

    match args.out {
        Some(path) => {
            std::fs::write(&path, json.as_bytes())
                .with_context(|| format!("cannot write to {}", path.display()))?;
            eprintln!(
                "Wrote {} — {} data qubits, {} ancillas, k={}, d={}, rate={:.4}",
                path.display(),
                n,
                params.ancilla_count(),
                params.logical_qubits,
                params.distance,
                params.encoding_rate()
            );
        }
        None => {
            io::stdout().write_all(json.as_bytes())?;
            println!();
        }
    }

    Ok(())
}
