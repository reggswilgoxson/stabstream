//! Stim Detector Error Model (DEM) parser and spacetime syndrome graph.
//!
//! A `.dem` file encodes a bipartite hypergraph where detector nodes are
//! connected by error mechanisms with associated flip probabilities. Every real
//! MWPM or Union-Find decoder operates on this graph. This crate parses the
//! text DEM format and constructs the weighted `SpacetimeGraph` that decoders
//! consume.

pub mod graph;
pub mod parser;
pub mod schema_gen;

pub use graph::{SpacetimeEdge, SpacetimeGraph, SpacetimeNode};
pub use parser::{DetectorErrorModel, DemDetector, DemError, ParseError};

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_DEM: &str = r#"error(0.1) D0 D1 ^ L0
error(0.05) D1 D2
detector(0, 1, 0) D0
detector(1, 1, 0) D1
detector(2, 1, 0) D2
logical_observable(0) L0
"#;

    #[test]
    fn parse_simple_dem() {
        let dem = DetectorErrorModel::parse(SIMPLE_DEM).unwrap();
        assert_eq!(dem.detector_count, 3);
        assert_eq!(dem.observable_count, 1);
        assert_eq!(dem.errors.len(), 2);
        assert!((dem.errors[0].probability - 0.1).abs() < 1e-10);
        assert_eq!(dem.errors[0].detectors, vec![0, 1]);
        assert_eq!(dem.errors[0].observables, vec![0]);
        assert!(dem.errors[1].observables.is_empty());
    }

    #[test]
    fn build_spacetime_graph() {
        let dem = DetectorErrorModel::parse(SIMPLE_DEM).unwrap();
        let graph = SpacetimeGraph::from_dem(&dem);
        // 3 detectors + 1 boundary node
        assert_eq!(graph.nodes.len(), 4);
        // 2 error mechanisms → 2 edges; error[1] has no observable so doesn't
        // need boundary split, still 2 edges
        assert_eq!(graph.edges.len(), 2);
        // weights = -ln(p/(1-p))
        let w0 = -(0.1_f64 / 0.9).ln() as f32;
        assert!((graph.edges[0].weight - w0).abs() < 1e-4);
    }

    #[test]
    fn repeat_block_expands() {
        let dem = "repeat 3 {\nerror(0.01) D0 D1\n}\n";
        let model = DetectorErrorModel::parse(dem).unwrap();
        assert_eq!(model.errors.len(), 3);
    }

    #[test]
    fn detector_coords_parsed() {
        let dem = "detector(1.5, 2.5, 3.0) D0\nerror(0.01) D0\n";
        let model = DetectorErrorModel::parse(dem).unwrap();
        let det = model.detectors.iter().find(|d| d.id == 0).unwrap();
        let coords = det.coords.unwrap();
        assert!((coords[0] - 1.5).abs() < 1e-9);
        assert!((coords[1] - 2.5).abs() < 1e-9);
        assert!((coords[2] - 3.0).abs() < 1e-9);
    }
}
