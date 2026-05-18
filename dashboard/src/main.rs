use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use stabstream_deserialize::stream::{QssfStream, StreamConfig};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::AsyncRead;

mod metrics;
mod ui;

use metrics::MetricsAggregator;

fn parse_source_arg() -> String {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|w| w[0] == "--source")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "tcp://localhost:9000".to_string())
}

fn cluster_bucket(events: u32) -> u32 {
    match events {
        0..=1 => 1,
        2..=3 => 2,
        4..=5 => 3,
        _ => 4,
    }
}

async fn run_stream_loop<R: AsyncRead + Unpin>(
    mut stream: QssfStream<R>,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
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

    Ok(())
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

    let result = if source.starts_with("tcp://") {
        let addr = source.trim_start_matches("tcp://");
        match tokio::net::TcpStream::connect(addr).await {
            Ok(tcp) => run_stream_loop(QssfStream::new(tcp, config), &mut terminal).await,
            Err(e) => Err(e.into()),
        }
    } else {
        match tokio::fs::File::open(&source).await {
            Ok(file) => {
                let reader = tokio::io::BufReader::new(file);
                run_stream_loop(QssfStream::new(reader, config), &mut terminal).await
            }
            Err(e) => Err(e.into()),
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}
