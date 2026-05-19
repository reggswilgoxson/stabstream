use anyhow::Result;
use clap::{Parser, Subcommand};

mod compare;
mod data;
mod plot;
mod run;

#[derive(Parser)]
#[command(
    name = "stabstream-threshold",
    about = "QEC threshold benchmarking: sweep physical error rate vs code distance, compute p_L.",
    long_about = "Generates threshold plots by sweeping over (distance, p_physical) pairs.\n\n\
        WORKFLOW\n\
        1. Generate one DEM per (distance, error_rate) operating point using Stim or\n\
           another circuit simulator.\n\
        2. Run `stabstream-threshold run --dem d=3:d3_p001.dem ...` to sample shots\n\
           and compute p_L for each point in parallel.\n\
        3. Run `stabstream-threshold compare --input run1.csv --input run2.csv\n\
           --plot threshold.svg` to overlay multiple sweeps and visualise the threshold.\n\n\
        PARALLELISM\n\
        The `run` subcommand distributes shots across all available CPUs using rayon.\n\
        Each thread maintains its own RNG (SmallRng, xorshift128+) and decoder\n\
        instance, so there is no cross-thread synchronisation in the hot sampling\n\
        path beyond the AtomicU64 error counters."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sample shots for each (distance, DEM) pair and compute logical error rates.
    Run(run::RunArgs),
    /// Merge and plot threshold data from one or more `run` output files.
    Compare(compare::CompareArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => run::run(args),
        Commands::Compare(args) => compare::compare(args),
    }
}
