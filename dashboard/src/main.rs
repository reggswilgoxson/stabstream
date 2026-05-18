use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

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

    let mut metrics = MetricsAggregator::new();
    let schema_name = "unknown".to_string();
    let mut round: u64 = 0;
    let frame_duration = Duration::from_millis(1000 / 60); // 60 Hz

    // TODO: open QssfStream from `source` and drive the async pipeline
    // For now the TUI runs with placeholder metrics.
    loop {
        let tick_start = Instant::now();

        terminal.draw(|f| ui::render(f, &metrics, &schema_name, round))?;

        // Simulate receiving a frame
        metrics.record(
            (round % 10) as f64 + 1.0,
            5.0 + (round % 5) as f64,
            1 + (round % 4) as u32,
            42_000,
            false,
        );
        round += 1;

        // Poll for key events without blocking beyond the frame budget
        let elapsed = tick_start.elapsed();
        let timeout = frame_duration.saturating_sub(elapsed);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
