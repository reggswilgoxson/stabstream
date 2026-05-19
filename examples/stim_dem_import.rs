//! Example: Parse a Stim detector error model (`.dem`) and build a spacetime graph.
//!
//! Usage:
//!   cargo run --example stim_dem_import -- path/to/model.dem

use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dem_path = args.get(1).map(String::as_str).unwrap_or("model.dem");

    println!("Loading Stim DEM from: {dem_path}");
    let source = match std::fs::read_to_string(dem_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {dem_path}: {e}");
            std::process::exit(1);
        }
    };

    let dem = match DetectorErrorModel::parse(&source) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("DEM parse error: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "Parsed DEM: {} detectors, {} observables, {} error mechanisms",
        dem.detector_count,
        dem.observable_count,
        dem.errors.len(),
    );

    let graph = SpacetimeGraph::from_dem(&dem);
    println!(
        "SpacetimeGraph: {} nodes ({} detectors + 1 boundary), {} edges",
        graph.nodes.len(),
        graph.detector_count(),
        graph.edges.len(),
    );

    // Print a sample of edges with their weights
    println!("\nFirst 5 edges:");
    for edge in graph.edges.iter().take(5) {
        println!(
            "  D{} ↔ {} weight={:.4}  fault_ids={:?}",
            edge.u,
            if edge.v as usize == graph.boundary_node {
                "boundary".to_string()
            } else {
                format!("D{}", edge.v)
            },
            edge.weight,
            edge.fault_ids,
        );
    }

    // Optionally generate a schema JSON
    if args.iter().any(|a| a == "--gen-schema") {
        let schema = stabstream_dem::schema_gen::schema_from_dem(&dem, dem_path);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        println!("\nGenerated schema JSON:\n{json}");
    }
}
