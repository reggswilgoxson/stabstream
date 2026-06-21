// Custom main — no criterion. Reads the saved criterion estimates produced by
// the other bench targets and prints a formatted pipeline-budget comparison.
// Listed last in Cargo.toml so `cargo bench -p stabstream-benches` prints this
// after all measurements are complete.

use std::path::{Path, PathBuf};

fn find_target_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..5 {
        let candidate = dir.join("target");
        if candidate.join("criterion").is_dir() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(p) => dir = p.to_owned(),
            None => break,
        }
    }
    None
}

fn read_median_ns(target: &Path, group: &str, bench: &str) -> Option<f64> {
    let path = target
        .join("criterion")
        .join(group)
        .join(bench)
        .join("new")
        .join("estimates.json");
    let content = std::fs::read_to_string(&path).ok()?;
    // Extract median.point_estimate. Values are stored in nanoseconds.
    // JSON layout: {"mean":{...},"median":{"confidence_interval":{...},"point_estimate":NN.N,...},...}
    let median_start = content.find("\"median\":")?;
    let after = &content[median_start..];
    let pe_start = after.find("\"point_estimate\":")?;
    let value_str = after[pe_start + 17..].trim_start_matches([' ', '\n', '\r', '\t']);
    let end = value_str
        .find(|c: char| {
            c != '.' && !c.is_ascii_digit() && c != 'e' && c != 'E' && c != '+' && c != '-'
        })
        .unwrap_or(value_str.len());
    value_str[..end].parse().ok()
}

fn fmt_ns(ns: f64) -> String {
    if ns >= 1_000.0 {
        format!("{:.2} µs", ns / 1_000.0)
    } else {
        format!("{:.0} ns", ns)
    }
}

fn print_row(name: &str, budget_ns: f64, measured: Option<f64>, is_subcost: bool) {
    let m_str = measured.map_or_else(|| "N/A".to_string(), |ns| fmt_ns(ns));
    let status = if is_subcost {
        "— included in parse above".to_string()
    } else {
        match measured {
            None => "not yet benchmarked".to_string(),
            Some(m) if m <= budget_ns => format!("✓  {:.1}× under budget", budget_ns / m),
            Some(m) => format!("✗  {:.1}× over budget", m / budget_ns),
        }
    };
    println!(
        "  {:<30}  {:>8}  {:>8}    {}",
        name,
        fmt_ns(budget_ns),
        m_str,
        status
    );
}

fn main() {
    let target = match find_target_dir() {
        Some(p) => p,
        None => {
            eprintln!(
                "\n[summary] Could not find target/criterion/. \
                 Run `cargo bench -p stabstream-benches` first.\n"
            );
            return;
        }
    };

    let parse_ns = read_median_ns(&target, "parse", "frame_header_sync");
    let crc_ns = read_median_ns(&target, "parse", "crc32_frame_header_32b");
    let slide_ns = read_median_ns(&target, "window_slide", "push_owned_steady_state_d5");

    const UF_BUDGET_NS: f64 = 400.0;
    let sep_wide: String = "═".repeat(72);
    let sep_thin: String = "─".repeat(72);

    println!();
    println!("{sep_wide}");
    println!("  stabstream · d=5 surface code · 24 ancillas · pipeline summary");
    println!("{sep_wide}");
    println!();
    println!(
        "  {:<30}  {:>8}  {:>8}    {}",
        "Stage", "Budget", "Measured", "Status"
    );
    println!("  {sep_thin}");

    print_row("Frame parse (inc. CRC)", 200.0, parse_ns, false);
    print_row("  └─ CRC hash (sub-cost)", 70.0, crc_ns, true);
    print_row("Window slide (push_owned)", 20.0, slide_ns, false);
    print_row("UF decode", UF_BUDGET_NS, None, false);

    println!("  {sep_thin}");

    // Totals: CRC is a sub-cost of parse, not additive.
    // Pipeline budget: parse(200) + slide(20) + decode(400) = 620 ns.
    let measured_subtotal = parse_ns.and_then(|p| slide_ns.map(|s| p + s));
    let estimated_total = measured_subtotal.map(|m| m + UF_BUDGET_NS);
    const PIPELINE_BUDGET_NS: f64 = 620.0; // 200 + 20 + 400

    if let Some(sub) = measured_subtotal {
        let status = if sub <= PIPELINE_BUDGET_NS {
            format!("✓  {:.1}× under budget", PIPELINE_BUDGET_NS / sub)
        } else {
            format!("✗  {:.1}× over budget", sub / PIPELINE_BUDGET_NS)
        };
        println!(
            "  {:<30}  {:>8}  {:>8}    {}",
            "Measured sub-total (no UF)",
            fmt_ns(PIPELINE_BUDGET_NS),
            fmt_ns(sub),
            status
        );
    }
    if let Some(est) = estimated_total {
        let status = if est < 1_000.0 {
            "✓  < 1 µs deadline".to_string()
        } else {
            format!("✗  {:.2} µs — exceeds deadline", est / 1_000.0)
        };
        println!(
            "  {:<30}  {:>8}  {:>8}    {}",
            "Est. total (with UF budget)",
            fmt_ns(PIPELINE_BUDGET_NS),
            fmt_ns(est),
            status
        );
    }

    println!();
    println!("{sep_wide}");

    // Warnings for stages over budget.
    let mut any_warn = false;
    if let Some(s) = slide_ns {
        if s > 20.0 {
            if !any_warn {
                println!();
            }
            println!(
                "  ⚠  window_slide is {:.1}× over its 20 ns budget.",
                s / 20.0
            );
            println!("     rebuild_matrix copies {} bools on every push.", 5 * 24);
            println!("     Incremental or lazy rebuild would recover this budget.");
            any_warn = true;
        }
    }
    if !any_warn {
        println!();
        println!("  All measured stages are within budget.");
    }
    println!();
}
