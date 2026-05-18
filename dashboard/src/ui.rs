use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{BarChart, Block, Borders, Paragraph, Sparkline},
    Frame,
};

use crate::metrics::MetricsAggregator;

/// Render the full TUI to `frame`.
pub fn render(frame: &mut Frame, metrics: &MetricsAggregator, schema_name: &str, round: u64) {
    let area = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(8), // stats panel
            Constraint::Length(5), // sparkline
            Constraint::Min(5),    // cluster bar chart
        ])
        .split(area);

    // Header
    let header = Paragraph::new(Span::styled(
        format!(" stabstream  |  schema: {schema_name}  |  round: {round}"),
        Style::default().fg(Color::Cyan),
    ))
    .block(Block::default().borders(Borders::ALL).title("stabstream"));
    frame.render_widget(header, chunks[0]);

    // Stats panel
    let stats_text = format!(
        "Syndrome rate : {:.2} events/round\n\
         Detector fire : {:.1} %\n\
         Parse p50     : {} ns\n\
         Parse p99     : {} ns\n\
         Drop rate     : {:.3} %\n\
         Total frames  : {}",
        metrics.latest_syndrome_rate(),
        metrics.latest_fire_rate_pct(),
        metrics.latency_p50_ns(),
        metrics.latency_p99_ns(),
        metrics.drop_rate() * 100.0,
        metrics.total_frames(),
    );
    let stats =
        Paragraph::new(stats_text).block(Block::default().borders(Borders::ALL).title("Metrics"));
    frame.render_widget(stats, chunks[1]);

    // Syndrome rate sparkline
    let sparkline_data = metrics.syndrome_rate_window();
    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Syndrome rate (last 100 rounds)"),
        )
        .data(&sparkline_data)
        .style(Style::default().fg(Color::Green));
    frame.render_widget(sparkline, chunks[2]);

    // Cluster size bar chart
    let histogram = metrics.cluster_histogram();
    let bar_data = [
        ("1", histogram[0]),
        ("2", histogram[1]),
        ("3", histogram[2]),
        ("4+", histogram[3]),
    ];
    let bar_chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Cluster size distribution"),
        )
        .data(&bar_data)
        .bar_width(5)
        .bar_style(Style::default().fg(Color::Yellow))
        .value_style(Style::default().fg(Color::White));
    frame.render_widget(bar_chart, chunks[3]);
}
