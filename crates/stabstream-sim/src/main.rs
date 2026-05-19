use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use stabstream_dem::DetectorErrorModel;
use stabstream_sim::{serve_circuit_to_socket, serve_dem_to_socket};
use tokio::net::TcpListener;

#[derive(Debug, Clone, ValueEnum)]
enum SimulatorBackend {
    /// Spawn a `stim detect` subprocess (requires Stim on PATH).
    Stim,
    /// Sample directly from a DEM — no Stim required.
    Native,
}

#[derive(Parser, Debug)]
#[command(
    name = "stabstream-sim",
    about = "QSSF syndrome stream simulator. Serves frames over TCP.",
    long_about = "Serves QSSF syndrome frames to TCP clients.\n\n\
        --simulator stim   (default) spawns a 'stim detect' subprocess.\n\
        --simulator native samples directly from a DEM — no Stim on PATH needed.\n\
        Native mode requires --dem; stim mode requires --circuit."
)]
struct Args {
    /// Stim circuit file (required for --simulator stim).
    #[arg(long, default_value = "circuit.stim")]
    circuit: String,

    /// Stim DEM file (required for --simulator native).
    #[arg(long)]
    dem: Option<String>,

    /// Simulator backend: "stim" (default) or "native".
    #[arg(long, value_enum, default_value_t = SimulatorBackend::Stim)]
    simulator: SimulatorBackend,

    /// TCP port to listen on.
    #[arg(long, default_value_t = 9000)]
    port: u16,

    /// Number of syndrome shots to serve per client connection.
    #[arg(long, default_value_t = 10_000)]
    shots: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "stabstream_sim=info".into()),
        )
        .init();

    let args = Args::parse();

    // Validate and pre-load DEM for native mode so we fail fast before binding
    let dem_arc: Option<Arc<DetectorErrorModel>> = match args.simulator {
        SimulatorBackend::Native => {
            let path = args
                .dem
                .as_deref()
                .context("--dem <model.dem> is required for --simulator native")?;
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading DEM file '{path}'"))?;
            let dem = DetectorErrorModel::parse(&text).context("parsing DEM")?;
            tracing::info!(
                detectors = dem.detector_count,
                observables = dem.observable_count,
                errors = dem.errors.len(),
                "native simulator: DEM loaded"
            );
            Some(Arc::new(dem))
        }
        SimulatorBackend::Stim => None,
    };

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!(
        addr = %addr,
        simulator = ?args.simulator,
        shots = args.shots,
        "stabstream-sim listening"
    );

    loop {
        let (socket, peer) = listener.accept().await?;
        tracing::info!(%peer, "client connected");

        let shots = args.shots;

        match &dem_arc {
            Some(dem) => {
                let dem = Arc::clone(dem);
                tokio::spawn(async move {
                    match serve_dem_to_socket(&dem, shots, socket).await {
                        Ok(n) => tracing::info!(%peer, frames = n, "native stream complete"),
                        Err(e) => tracing::warn!(%peer, error = %e, "native stream error"),
                    }
                });
            }
            None => {
                let circuit = args.circuit.clone();
                tokio::spawn(async move {
                    match serve_circuit_to_socket(&circuit, shots, socket).await {
                        Ok(n) => tracing::info!(%peer, frames = n, "stim stream complete"),
                        Err(e) => tracing::warn!(%peer, error = %e, "stim stream error"),
                    }
                });
            }
        }
    }
}
