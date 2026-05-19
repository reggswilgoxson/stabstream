use std::collections::BTreeMap;

use anyhow::Result;
use plotters::prelude::*;

use crate::data::DataPoint;

/// Write an SVG threshold plot to `path`.
///
/// X axis: physical error rate p. Y axis: logical error rate p_l.
/// One polyline per code distance. The diagonal y=x reference line marks the
/// region where the code is above threshold (p_l > p, decoder worse than no
/// correction).
pub fn write_plot(path: &str, points: &[DataPoint]) -> Result<()> {
    if points.is_empty() {
        anyhow::bail!("no data points to plot");
    }

    // Group by distance, sort each group by p_physical
    let mut by_distance: BTreeMap<u32, Vec<&DataPoint>> = BTreeMap::new();
    for dp in points {
        by_distance.entry(dp.distance).or_default().push(dp);
    }
    for series in by_distance.values_mut() {
        series.sort_by(|a, b| a.p_physical.partial_cmp(&b.p_physical).unwrap());
    }

    let x_min = points
        .iter()
        .map(|dp| dp.p_physical)
        .fold(f64::INFINITY, f64::min)
        .max(1e-10);
    let x_max = points
        .iter()
        .map(|dp| dp.p_physical)
        .fold(0.0_f64, f64::max);
    let y_max = points
        .iter()
        .map(|dp| (dp.p_l + dp.p_l_err).max(dp.p_physical))
        .fold(0.0_f64, f64::max)
        .min(1.0);

    let root = SVGBackend::new(path, (820, 600)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption("QEC Threshold Analysis (p_l vs p)", ("sans-serif", 18))
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(x_min..x_max, 0.0_f64..y_max)?;

    chart
        .configure_mesh()
        .x_desc("Physical Error Rate  p")
        .y_desc("Logical Error Rate  p_l")
        .x_label_formatter(&|v| format!("{:.3}", v))
        .y_label_formatter(&|v| format!("{:.3}", v))
        .draw()?;

    // Diagonal y=x reference — codes above this line are above threshold
    chart.draw_series(LineSeries::new(
        [(x_min, x_min), (x_max.min(y_max), x_max.min(y_max))],
        BLACK.mix(0.25).stroke_width(1),
    ))?;

    // One series per distance
    let palette: &[RGBColor] = &[
        RGBColor(0xE6, 0x19, 0x4B), // red
        RGBColor(0x43, 0x63, 0xD8), // blue
        RGBColor(0x3C, 0xB4, 0x4B), // green
        RGBColor(0xF5, 0x82, 0x31), // orange
        RGBColor(0x91, 0x1E, 0xB4), // purple
        RGBColor(0x42, 0xD4, 0xF4), // cyan
    ];

    for (idx, (&d, series)) in by_distance.iter().enumerate() {
        let color = palette[idx % palette.len()];

        // Main line
        chart
            .draw_series(LineSeries::new(
                series.iter().map(|dp| (dp.p_physical, dp.p_l)),
                color.stroke_width(2),
            ))?
            .label(format!("d = {d}"))
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
            });

        // Error-bar tick marks (vertical lines at each data point)
        for dp in series.iter() {
            let y_lo = (dp.p_l - dp.p_l_err).max(0.0);
            let y_hi = (dp.p_l + dp.p_l_err).min(y_max);
            chart.draw_series(LineSeries::new(
                [(dp.p_physical, y_lo), (dp.p_physical, y_hi)],
                color.mix(0.6).stroke_width(1),
            ))?;
        }

        // Data point markers
        chart.draw_series(
            series
                .iter()
                .map(|dp| Circle::new((dp.p_physical, dp.p_l), 4, color.filled())),
        )?;
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.9))
        .border_style(BLACK.mix(0.4))
        .position(SeriesLabelPosition::UpperLeft)
        .draw()?;

    root.present()?;
    Ok(())
}
