use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use stabstream_dem::DetectorErrorModel;
use stabstream_sim::{
    broadcast::{relay_to_socket, SimBroadcaster},
    shm::ShmProducer,
    serve_circuit_to_socket, serve_dem_to_socket, NoiseModel, DEFAULT_BROADCAST_CAPACITY,
};
use tokio::net::TcpListener;

#[derive(Debug, Clone, ValueEnum)]
enum SimulatorBackend {
    /// Spawn a `stim detect` subprocess (requires Stim on PATH).
    Stim,
    /// Sample directly from a DEM — no Stim required.
    Native,
}

#[derive(Debug, Clone, ValueEnum)]
enum Transport {
    /// One TCP connection per shot source (original behaviour).
    Direct,
    /// Broadcast: one shot source → all TCP clients receive every frame.
    ///
    /// Lagging clients skip frames but remain connected.  Use
    /// `--broadcast-capacity` to tune the backlog depth.
    Broadcast,
    /// Write frames to a POSIX SHM ring at `/dev/shm/<shm-name>`.
    ///
    /// No TCP server is started; decoders on the same host read frames
    /// directly from the ring with ~50–200 ns IPC latency.  Use
    /// `--shm-name` to configure the ring name.
    Shm,
}

#[derive(Parser, Debug)]
#[command(
    name = "stabstream-sim",
    about = "QSSF syndrome stream simulator.",
    long_about = "Serves QSSF syndrome frames over various transports.\n\n\
        TRANSPORTS\n\
        ----------\n\
        direct    (default) One TCP client per stim/native shot source.\n\
        broadcast One shot source shared across all TCP clients.\n\
        shm       Write frames to a POSIX SHM ring; no TCP server.\n\n\
        SIMULATOR BACKENDS\n\
        ------------------\n\
        stim    (default) Spawn a `stim detect` subprocess.\n\
        native  Sample directly from a DEM — no Stim required."
)]
struct Args {
    /// Stim circuit file (required for --simulator stim).
    #[arg(long, default_value = "circuit.stim")]
    circuit: String,

    /// Stim DEM file (required for --simulator native).
    #[arg(long)]
    dem: Option<String>,

    /// Simulator backend.
    #[arg(long, value_enum, default_value_t = SimulatorBackend::Stim)]
    simulator: SimulatorBackend,

    /// Transport layer.
    #[arg(long, value_enum, default_value_t = Transport::Direct)]
    transport: Transport,

    /// TCP port (broadcast and direct transports).
    #[arg(long, default_value_t = 9000)]
    port: u16,

    /// Number of syndrome shots to serve per connection (direct) or in total
    /// (broadcast / shm).
    #[arg(long, default_value_t = 10_000)]
    shots: u64,

    /// Broadcast channel backlog depth (broadcast transport only).
    #[arg(long, default_value_t = DEFAULT_BROADCAST_CAPACITY)]
    broadcast_capacity: usize,

    /// SHM ring name — ring appears at `/dev/shm/<name>` (shm transport only).
    #[arg(long, default_value = "stabstream")]
    shm_name: String,
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

    match args.transport {
        Transport::Direct => run_direct(&args, dem_arc).await,
        Transport::Broadcast => run_broadcast(&args, dem_arc).await,
        Transport::Shm => run_shm(&args, dem_arc).await,
    }
}

// ─── Direct (original one-to-one behaviour) ───────────────────────────────────

async fn run_direct(args: &Args, dem_arc: Option<Arc<DetectorErrorModel>>) -> Result<()> {
    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!(addr = %addr, transport = "direct", "stabstream-sim listening");

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

// ─── Broadcast (one source → N TCP clients) ───────────────────────────────────

async fn run_broadcast(args: &Args, dem_arc: Option<Arc<DetectorErrorModel>>) -> Result<()> {
    let broadcaster = SimBroadcaster::new(args.broadcast_capacity);
    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!(
        addr = %addr,
        transport = "broadcast",
        capacity = args.broadcast_capacity,
        "stabstream-sim listening"
    );

    // Pre-build the file header (same for all clients).
    use stabstream_core::frame::QSSF_MAGIC;
    use uuid::Uuid;
    let schema_id: Uuid = stabstream_sim::STIM_GENERIC_UUID.parse().unwrap();
    let mut file_header = [0u8; 26];
    file_header[0..4].copy_from_slice(&QSSF_MAGIC.to_le_bytes());
    file_header[4..6].copy_from_slice(&1u16.to_le_bytes());
    file_header[6..22].copy_from_slice(schema_id.as_bytes());

    // Producer task: sample shots and broadcast frames.
    let bc_producer = Arc::clone(&broadcaster);
    let shots = args.shots;
    let dem_for_producer = dem_arc.clone();
    let circuit = args.circuit.clone();

    tokio::spawn(async move {
        let count = run_broadcast_producer(bc_producer, dem_for_producer, &circuit, shots).await;
        tracing::info!(frames = count, "broadcast producer finished");
    });

    // Accept loop: relay broadcast channel to each TCP client.
    loop {
        let (socket, peer) = listener.accept().await?;
        tracing::info!(%peer, "broadcast client connected");

        let rx = broadcaster.subscribe();
        let bc = Arc::clone(&broadcaster);
        let hdr = file_header;

        tokio::spawn(async move {
            if let Err(e) = relay_to_socket(socket, rx, hdr, bc).await {
                tracing::warn!(%peer, error = %e, "relay error");
            }
            tracing::info!(%peer, "broadcast client disconnected");
        });
    }
}

/// Generate frames in a blocking thread and push them to the broadcaster.
async fn run_broadcast_producer(
    broadcaster: Arc<SimBroadcaster>,
    dem_arc: Option<Arc<DetectorErrorModel>>,
    circuit: &str,
    shots: u64,
) -> u64 {
    use stabstream_sim::{DemSampler, NoiseModel as _};

    match dem_arc {
        Some(dem) => {
            // CPU-bound native sampling: run in a blocking thread.
            let bc = Arc::clone(&broadcaster);
            let dem_clone = Arc::clone(&dem);
            let count = tokio::task::spawn_blocking(move || {
                use rand::SeedableRng;
                let mut rng = rand::rngs::SmallRng::from_entropy();
                let sampler = DemSampler::from_dem(&dem_clone);
                let ancilla_count = dem_clone.detector_count as u16;
                let mut n: u64 = 0;
                for frame_id in 0..shots {
                    let shot = sampler.sample(&mut rng);
                    let frame_bytes = crate_encode_shot_frame(
                        &shot.detector_events,
                        shot.observable_flips,
                        frame_id,
                        ancilla_count,
                    );
                    bc.send(std::sync::Arc::new(frame_bytes));
                    n += 1;
                }
                n
            })
            .await
            .unwrap_or(0);
            count
        }
        None => {
            // Stim subprocess: async I/O, runs on the tokio runtime.
            use std::process::Stdio;
            use tokio::{
                io::{AsyncBufReadExt, BufReader},
                process::Command,
            };

            let Ok(circuit_file) = std::fs::File::open(circuit) else {
                tracing::error!(circuit, "cannot open circuit file");
                return 0;
            };
            let Ok(mut child) = Command::new("stim")
                .args(["detect", "--shots", &shots.to_string()])
                .stdin(Stdio::from(circuit_file))
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
            else {
                tracing::error!("cannot spawn stim subprocess");
                return 0;
            };

            let stdout = child.stdout.take().expect("stdout piped");
            let mut lines = BufReader::new(stdout).lines();
            let mut frame_id: u64 = 0;
            let mut ancilla_count: Option<u16> = None;

            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let ac = *ancilla_count.get_or_insert(trimmed.len() as u16);
                let events: Vec<bool> = trimmed.bytes().map(|b| b == b'1').collect();
                let frame_bytes =
                    crate_encode_shot_frame(&events, 0, frame_id, ac);
                broadcaster.send(std::sync::Arc::new(frame_bytes));
                frame_id += 1;
            }
            let _ = child.wait().await;
            frame_id
        }
    }
}

// ─── SHM (write to shared memory ring, no TCP) ────────────────────────────────

async fn run_shm(args: &Args, dem_arc: Option<Arc<DetectorErrorModel>>) -> Result<()> {
    let name = args.shm_name.clone();
    let shots = args.shots;

    tracing::info!(
        shm = %format!("/dev/shm/{name}"),
        transport = "shm",
        shots,
        "stabstream-sim writing SHM ring"
    );

    // SHM I/O and sampling are both blocking — run entirely in spawn_blocking.
    let dem_for_shm = dem_arc.clone();
    tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut ring = ShmProducer::create(&name)?;

        match dem_for_shm {
            Some(dem) => {
                use stabstream_sim::DemSampler;
                use rand::SeedableRng;
                let mut rng = rand::rngs::SmallRng::from_entropy();
                let sampler = DemSampler::from_dem(&dem);
                let ancilla_count = dem.detector_count as u16;

                for frame_id in 0..shots {
                    let shot = sampler.sample(&mut rng);
                    let frame_bytes = crate_encode_shot_frame(
                        &shot.detector_events,
                        shot.observable_flips,
                        frame_id,
                        ancilla_count,
                    );
                    ring.write_frame(&frame_bytes)?;
                }
                Ok(shots)
            }
            None => {
                anyhow::bail!("--transport shm with --simulator stim requires a DEM; use --simulator native instead");
            }
        }
    })
    .await??;

    tracing::info!(shm_name = %args.shm_name, frames = shots, "SHM producer finished");
    Ok(())
}

// ─── Frame encoding helper (mirrors stabstream_sim::encode_shot_frame) ─────────

fn crate_encode_shot_frame(
    detector_events: &[bool],
    _observable_flips: u64,
    frame_id: u64,
    ancilla_count: u16,
) -> Vec<u8> {
    use stabstream_core::frame::FrameHeader;
    use stabstream_deserialize::{parser::write_frame_header, rle::encode_detector_events};
    use std::time::{SystemTime, UNIX_EPOCH};

    let de_rle = encode_detector_events(detector_events);
    let meas: Vec<u8> = detector_events
        .iter()
        .map(|&e| if e { 0xFF } else { 0x01 })
        .collect();

    let timestamp_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let hdr = FrameHeader {
        frame_id,
        round: frame_id as u32,
        timestamp_ns,
        qubit_count: 0,
        ancilla_count,
        payload_len: (2 + de_rle.len() + ancilla_count as usize) as u32,
        code_type: 0x01,
        distance: 0,
        flags: 0,
        crc32: 0,
    };
    let hdr_bytes = write_frame_header(&hdr);

    let mut out = Vec::with_capacity(36 + 2 + de_rle.len() + meas.len() + 6);
    out.extend_from_slice(&hdr_bytes);
    out.extend_from_slice(&(de_rle.len() as u16).to_le_bytes());
    out.extend_from_slice(&de_rle);
    out.extend_from_slice(&meas);
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&hdr_bytes).to_le_bytes());
    out
}
