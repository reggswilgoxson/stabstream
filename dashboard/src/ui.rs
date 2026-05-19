use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{BarChart, Block, Borders, Gauge, Paragraph, Sparkline},
    Frame,
};

use crate::metrics::MetricsAggregator;

/// Render the full TUI to `frame`.
pub fn render(
    frame: &mut Frame,
    metrics: &MetricsAggregator,
    schema_name: &str,
    round: u64,
    export_pending: bool,
) {
    let area = frame.size();

    let has_p_l = metrics.p_l().is_some();
    let has_heatmap = metrics.ancilla_count > 0;
    let heatmap_rows = heatmap_height(metrics.ancilla_count);

    let mut constraints = vec![
        Constraint::Length(3), // header
        Constraint::Length(1), // status line
        Constraint::Length(7), // metrics text
        Constraint::Length(4), // sparkline
    ];
    if has_p_l {
        constraints.push(Constraint::Length(3));
    }
    if has_heatmap {
        constraints.push(Constraint::Length(heatmap_rows));
    }
    constraints.push(Constraint::Min(5)); // cluster bar chart

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut idx = 0;

    // ── Header ──────────────────────────────────────────────────────────────
    let header = Paragraph::new(Span::styled(
        format!(" stabstream  │  schema: {schema_name}  │  round: {round}"),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ))
    .block(Block::default().borders(Borders::ALL).title(" stabstream "));
    frame.render_widget(header, chunks[idx]);
    idx += 1;

    // ── Status line ──────────────────────────────────────────────────────────
    let behind = metrics.frames_behind(round);
    let behind_str = if behind > 0 {
        format!("{behind}↓")
    } else if behind < 0 {
        format!("{}↑", -behind)
    } else {
        "0".to_string()
    };
    let export_hint = if export_pending { " [exporting…]" } else { "  [e] export" };
    let status_line = Line::from(vec![
        Span::raw("  decoder: "),
        Span::styled(&metrics.decoder_name, Style::default().fg(Color::Yellow)),
        Span::raw("   p99: "),
        Span::styled(
            format!("{} ns", metrics.latency_p99_ns()),
            Style::default().fg(Color::Green),
        ),
        Span::raw("   frames behind: "),
        Span::styled(
            behind_str,
            Style::default().fg(if behind > 50 { Color::Red } else { Color::White }),
        ),
        Span::styled(export_hint, Style::default().fg(Color::DarkGray)),
        Span::raw("  [q] quit"),
    ]);
    frame.render_widget(Paragraph::new(status_line), chunks[idx]);
    idx += 1;

    // ── Metrics text ─────────────────────────────────────────────────────────
    let stats_text = format!(
        "Syndrome rate : {:.2} events/round   (mean: {:.2})\n\
         Detector fire : {:.1}%               (mean: {:.1}%)\n\
         Parse latency : p50 {} ns   p99 {} ns\n\
         Drop rate     : {:.3}%\n\
         Total frames  : {}",
        metrics.latest_syndrome_rate(),
        metrics.mean_syndrome_rate(),
        metrics.latest_fire_rate_pct(),
        metrics.mean_fire_rate_pct(),
        metrics.latency_p50_ns(),
        metrics.latency_p99_ns(),
        metrics.drop_rate() * 100.0,
        metrics.total_frames(),
    );
    let stats = Paragraph::new(stats_text)
        .block(Block::default().borders(Borders::ALL).title(" Metrics "));
    frame.render_widget(stats, chunks[idx]);
    idx += 1;

    // ── Syndrome rate sparkline ───────────────────────────────────────────────
    let sparkline_data = metrics.syndrome_rate_window();
    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Syndrome rate (last 100 rounds) "),
        )
        .data(&sparkline_data)
        .style(Style::default().fg(Color::Green));
    frame.render_widget(sparkline, chunks[idx]);
    idx += 1;

    // ── p_L gauge ────────────────────────────────────────────────────────────
    if has_p_l {
        if let Some(p_l) = metrics.p_l() {
            render_p_l_gauge(frame, chunks[idx], p_l, metrics.obs_total());
        }
        idx += 1;
    }

    // ── Ancilla heatmap ───────────────────────────────────────────────────────
    if has_heatmap {
        render_ancilla_heatmap(frame, chunks[idx], metrics);
        idx += 1;
    }

    // ── Cluster bar chart ─────────────────────────────────────────────────────
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
                .title(" Cluster size distribution "),
        )
        .data(&bar_data)
        .bar_width(5)
        .bar_style(Style::default().fg(Color::Yellow))
        .value_style(Style::default().fg(Color::White));
    frame.render_widget(bar_chart, chunks[idx]);
}

fn render_p_l_gauge(frame: &mut Frame, area: Rect, p_l: f64, obs_total: u64) {
    let color = if p_l < 0.01 {
        Color::Green
    } else if p_l < 0.05 {
        Color::Yellow
    } else {
        Color::Red
    };
    // 5% fills the bar — gives useful visual range for typical operating conditions
    let ratio = (p_l * 20.0).min(1.0);
    let label = format!("p_L = {p_l:.2e}  ({obs_total} shots with ground truth)");
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Logical error rate  (observable_flips tag 0x10) "),
        )
        .gauge_style(Style::default().fg(color).bg(Color::Black))
        .ratio(ratio)
        .label(label);
    frame.render_widget(gauge, area);
}

fn render_ancilla_heatmap(frame: &mut Frame, area: Rect, metrics: &MetricsAggregator) {
    let fire_rates = metrics.per_ancilla_fire_rates();
    let n = fire_rates.len();
    if n == 0 {
        return;
    }

    let mean = fire_rates.iter().copied().sum::<f64>() / n as f64;
    let var = fire_rates.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / n as f64;
    let std = var.sqrt();

    // 2 chars per cell ("█ "), fit within inner width
    let inner_width = area.width.saturating_sub(2) as usize;
    let cols = (inner_width / 2).max(1);

    let mut lines: Vec<Line> = Vec::new();
    let mut current: Vec<Span> = Vec::new();

    for (i, &rate) in fire_rates.iter().enumerate() {
        let z = if std > 1e-9 { (rate - mean) / std } else { 0.0 };
        let color = heatmap_color(rate, z);
        current.push(Span::styled("█", Style::default().fg(color)));
        current.push(Span::raw(" "));
        if (i + 1) % cols == 0 || i + 1 == n {
            lines.push(Line::from(std::mem::take(&mut current)));
        }
    }

    let title = format!(
        " Ancilla fire frequency  ({n} ancillas, mean {:.3}) ",
        mean
    );
    let heatmap =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(heatmap, area);
}

/// Map (rate, z-score) → TUI color.
fn heatmap_color(rate: f64, z: f64) -> Color {
    if z < -2.5 {
        Color::Blue // significantly underactive — possible readout failure
    } else if z > 3.0 {
        Color::Red // severely overactive — leakage or crosstalk
    } else if z > 2.0 {
        Color::Rgb(220, 80, 20)
    } else if rate > 0.15 {
        Color::Rgb(200, 150, 0)
    } else if rate > 0.08 {
        Color::Yellow
    } else if rate < 0.001 {
        Color::Rgb(80, 80, 200)
    } else {
        Color::Green
    }
}

fn heatmap_height(ancilla_count: u16) -> u16 {
    let content_rows = ancilla_count.saturating_add(39) / 40;
    (content_rows + 2).max(4).min(10)
}
