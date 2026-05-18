use anyhow::Result;
use clap::Parser;
use stabstream_sim::serve_circuit_to_socket;
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(
    name = "stabstream-sim",
    about = "Stim-backed QSSF syndrome stream simulator. Serves one client per connection."
)]
struct Args {
    /// Path to the Stim circuit file.
    #[arg(long, default_value = "circuit.stim")]
    circuit: String,

    /// TCP port to listen on.
    #[arg(long, default_value_t = 9000)]
    port: u16,

    /// Number of detector-event shots per connection.
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
    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!(addr = %addr, circuit = %args.circuit, shots = args.shots, "stabstream-sim listening");

    loop {
        let (socket, peer) = listener.accept().await?;
        tracing::info!(%peer, "client connected");

        let circuit = args.circuit.clone();
        let shots = args.shots;

        tokio::spawn(async move {
            match serve_circuit_to_socket(&circuit, shots, socket).await {
                Ok(n) => tracing::info!(%peer, frames = n, "stream complete"),
                Err(e) => tracing::warn!(%peer, error = %e, "stream error"),
            }
        });
    }
}
