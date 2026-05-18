use std::io;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use stabstream_core::frame::{FrameHeader, QSSF_MAGIC};
use stabstream_deserialize::stream::{QssfStream, StreamConfig};
use stabstream_deserialize::{parser::write_frame_header, rle::encode_detector_events};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

mod metrics;
mod ui;

use metrics::MetricsAggregator;

/// UUID written into the QSSF file header for Stim-sourced streams.
/// Not registered in any schema — used only as a stream identifier.
const STIM_GENERIC_UUID: &str = "00000000-5354-494d-0000-000000000001";

fn parse_source_arg() -> String {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|w| w[0] == "--source")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "tcp://localhost:9000".to_string())
}

fn parse_shots_arg() -> u64 {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|w| w[0] == "--shots")
        .and_then(|w| w[1].parse::<u64>().ok())
        .unwrap_or(10_000)
}

fn cluster_bucket(events: u32) -> u32 {
    match events {
        0..=1 => 1,
        2..=3 => 2,
        4..=5 => 3,
        _ => 4,
    }
}

/// Encode a single Stim detector-event line as a QSSF frame and write it to `writer`.
async fn encode_one_shot(
    line: &str,
    frame_id: u64,
    ancilla_count: u16,
    writer: &mut tokio::io::DuplexStream,
) -> Result<()> {
    let events: Vec<bool> = line.bytes().map(|b| b == b'1').collect();
    let de_rle = encode_detector_events(&events);
    let rle_len = de_rle.len();

    // +1 (0x01) for no event, -1 (0xFF) for event
    let meas: Vec<u8> = events
        .iter()
        .map(|&e| if e { 0xFF } else { 0x01 })
        .collect();

    let payload_len = (2 + rle_len + ancilla_count as usize) as u32;

    let timestamp_ns = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let hdr = FrameHeader {
        frame_id,
        round: frame_id as u32,
        timestamp_ns,
        qubit_count: 0, // not reported by stim detect
        ancilla_count,
        payload_len,
        code_type: 0x01, // SurfaceCode; no timing/parity optional sections
        distance: 0,     // not reported by stim detect
        flags: 0,
        crc32: 0, // filled by write_frame_header
    };
    let hdr_bytes = write_frame_header(&hdr);
    writer.write_all(&hdr_bytes).await?;

    writer.write_all(&(rle_len as u16).to_le_bytes()).await?;
    writer.write_all(&de_rle).await?;
    writer.write_all(&meas).await?;

    // Terminator: 0xFFFF sentinel + CRC32 of the 36-byte header
    writer.write_all(&0xFFFFu16.to_le_bytes()).await?;
    writer
        .write_all(&crc32fast::hash(&hdr_bytes).to_le_bytes())
        .await?;

    Ok(())
}

/// Reads detector-event lines from a Stim subprocess and encodes them as a
/// valid QSSF byte stream written to `writer`. Dropping `writer` signals EOF
/// to the `QssfStream` on the other end of the duplex pipe.
async fn run_stim_encoder(
    child_stdout: tokio::process::ChildStdout,
    mut writer: tokio::io::DuplexStream,
) -> Result<()> {
    // Write the 26-byte QSSF file header.
    let schema_id: Uuid = STIM_GENERIC_UUID.parse().unwrap();
    let mut file_hdr = [0u8; 26];
    file_hdr[0..4].copy_from_slice(&QSSF_MAGIC.to_le_bytes());
    file_hdr[4..6].copy_from_slice(&1u16.to_le_bytes());
    file_hdr[6..22].copy_from_slice(schema_id.as_bytes());
    file_hdr[22..26].copy_from_slice(&0u32.to_le_bytes()); // flags
    writer.write_all(&file_hdr).await?;

    let mut lines = BufReader::new(child_stdout).lines();

    // Read the first line to learn ancilla_count, then treat it as shot 0.
    let first_line = match lines.next_line().await? {
        Some(l) => l,
        None => return Ok(()), // Stim produced no output
    };
    let ancilla_count = first_line.len() as u16;
    encode_one_shot(&first_line, 0, ancilla_count, &mut writer).await?;

    let mut frame_id: u64 = 1;
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            continue; // skip blank trailing lines
        }
        encode_one_shot(&line, frame_id, ancilla_count, &mut writer).await?;
        frame_id += 1;
    }

    // writer dropped here → duplex EOF → QssfStream returns Ok(None)
    Ok(())
}

async fn run_stream_loop<R: AsyncRead + Unpin>(
    mut stream: QssfStream<R>,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<MetricsAggregator> {
    let mut metrics = MetricsAggregator::new();
    let schema_name = "unknown".to_string();
    let mut round: u64 = 0;

    loop {
        let t0 = Instant::now();
        let frame_result =
            tokio::time::timeout(Duration::from_millis(16), stream.next_frame()).await;

        match frame_result {
            Ok(Ok(Some(frame))) => {
                let latency = t0.elapsed().as_nanos() as u64;
                let events = frame.detector_event_count();
                let ancilla = frame.header.ancilla_count;
                let fire_pct = if ancilla > 0 {
                    events as f64 / ancilla as f64 * 100.0
                } else {
                    0.0
                };
                metrics.record(
                    events as f64,
                    fire_pct,
                    cluster_bucket(events),
                    latency,
                    false,
                );
                round += 1;
            }
            Ok(Ok(None)) => break, // clean EOF
            Ok(Err(e)) => return Err(e.into()),
            Err(_timeout) => {} // no frame this tick — just redraw
        }

        terminal.draw(|f| ui::render(f, &metrics, &schema_name, round))?;

        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
    }

    Ok(metrics)
}

fn print_run_summary(metrics: &MetricsAggregator, elapsed: Duration, source: &str) {
    let frames = metrics.total_frames();
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        frames as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    let [c1, c2, c3, c4] = metrics.cluster_histogram();
    let cluster_total = c1 + c2 + c3 + c4;
    let pct = |n: u64| {
        if cluster_total > 0 {
            n as f64 / cluster_total as f64 * 100.0
        } else {
            0.0
        }
    };

    let sep = "─".repeat(56);
    println!("\n── stabstream run summary {sep}");
    println!("  source      {source}");
    println!(
        "  frames      {:>10}    elapsed  {:.2} s    throughput  {:.0} f/s",
        frames,
        elapsed.as_secs_f64(),
        throughput
    );
    println!("{sep}──");
    println!(
        "  syndrome    mean {:>6.2}    latest {:>6.2}",
        metrics.mean_syndrome_rate(),
        metrics.latest_syndrome_rate()
    );
    println!(
        "  fire rate   mean {:>5.1}%    latest {:>5.1}%",
        metrics.mean_fire_rate_pct(),
        metrics.latest_fire_rate_pct()
    );
    println!(
        "  latency     p50  {:>5} µs   p99    {:>5} µs",
        metrics.latency_p50_ns() / 1_000,
        metrics.latency_p99_ns() / 1_000,
    );
    println!("  drop rate   {:.2}%", metrics.drop_rate() * 100.0);
    println!("{sep}──");
    println!("  cluster histogram");
    println!("    size 1    {:>7}  {:>5.1}%", c1, pct(c1));
    println!("    size 2    {:>7}  {:>5.1}%", c2, pct(c2));
    println!("    size 3    {:>7}  {:>5.1}%", c3, pct(c3));
    println!("    size 4+   {:>7}  {:>5.1}%", c4, pct(c4));
    println!("{sep}──");
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "stabstream=info".into()),
        )
        .init();

    let source = parse_source_arg();
    tracing::info!(source = %source, "opening QSSF source");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let config = StreamConfig {
        validation: ValidationPolicy::Disabled,
        ..Default::default()
    };

    let t_start = Instant::now();
    let result = if source.starts_with("tcp://") {
        let addr = source.trim_start_matches("tcp://");
        match tokio::net::TcpStream::connect(addr).await {
            Ok(tcp) => run_stream_loop(QssfStream::new(tcp, config), &mut terminal).await,
            Err(e) => Err(e.into()),
        }
    } else if source.starts_with("stim:") {
        let circuit_path = source.trim_start_matches("stim:");
        let shots = parse_shots_arg();

        let circuit_file = match std::fs::File::open(circuit_path) {
            Ok(f) => f,
            Err(e) => {
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                terminal.show_cursor()?;
                return Err(anyhow::anyhow!(
                    "cannot open circuit file '{}': {}",
                    circuit_path,
                    e
                ));
            }
        };

        let mut child = match Command::new("stim")
            .arg("detect")
            .arg("--shots")
            .arg(shots.to_string())
            .stdin(Stdio::from(circuit_file))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                terminal.show_cursor()?;
                return Err(anyhow::anyhow!(
                    "'stim' not found on PATH — install via `pip install stim`: {}",
                    e
                ));
            }
            Err(e) => return Err(e.into()),
        };

        let child_stdout = child.stdout.take().expect("stdout was piped");
        // 64 KiB buffer: ~32 frames of headroom for a d=5 surface code circuit
        let (reader, writer) = tokio::io::duplex(64 * 1024);

        tokio::spawn(async move {
            if let Err(e) = run_stim_encoder(child_stdout, writer).await {
                tracing::error!("stim encoder error: {e}");
            }
            let _ = child.wait().await;
        });

        run_stream_loop(QssfStream::new(reader, config), &mut terminal).await
    } else {
        match tokio::fs::File::open(&source).await {
            Ok(file) => {
                let reader = tokio::io::BufReader::new(file);
                run_stream_loop(QssfStream::new(reader, config), &mut terminal).await
            }
            Err(e) => Err(e.into()),
        }
    };

    let elapsed = t_start.elapsed();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match result {
        Ok(ref metrics) if metrics.total_frames() > 0 => {
            print_run_summary(metrics, elapsed, &source);
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}
