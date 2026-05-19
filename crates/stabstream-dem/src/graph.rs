//! Weighted spacetime graph built from a `DetectorErrorModel`.
//!
//! Each detector becomes a node; each error mechanism becomes a weighted edge.
//! A virtual boundary node is appended last so that open-boundary codes
//! (surface codes) can pair syndromes with the boundary rather than requiring
//! an even number of active syndrome nodes.

use crate::parser::DetectorErrorModel;

/// A node in the spacetime syndrome graph.
#[derive(Debug, Clone)]
pub struct SpacetimeNode {
    /// Detector id, or `usize::MAX` for the boundary node.
    pub id: usize,
    /// Optional spacetime coordinates [x, y, t].
    pub coords: Option<[f64; 3]>,
}

/// A weighted edge between two spacetime nodes.
///
/// Corresponds to a single error mechanism in the DEM.
#[derive(Debug, Clone)]
pub struct SpacetimeEdge {
    /// Source node index into `SpacetimeGraph::nodes`.
    pub u: u32,
    /// Destination node index into `SpacetimeGraph::nodes`.
    pub v: u32,
    /// Matching weight: `-ln(p / (1 - p))`. Smaller = more likely error.
    pub weight: f32,
    /// Observable indices flipped when this edge is part of a correction.
    /// `SmallVec`-style: stored inline for ≤4 observables (the common case).
    pub fault_ids: Vec<u8>,
}

/// Immutable weighted graph over syndrome spacetime nodes.
///
/// Constructed once from a `DetectorErrorModel` and shared across decode
/// threads via `Arc<SpacetimeGraph>`. All hot-path decode operations take
/// only a shared reference.
pub struct SpacetimeGraph {
    pub nodes: Vec<SpacetimeNode>,
    pub edges: Vec<SpacetimeEdge>,
    /// Index of the virtual boundary node in `nodes`.
    pub boundary_node: usize,
}

impl SpacetimeGraph {
    /// Build a `SpacetimeGraph` from a parsed `DetectorErrorModel`.
    ///
    /// The graph has `dem.detector_count + 1` nodes (the extra one is the
    /// boundary). Edges are derived from `dem.errors`; degenerate
    /// zero-probability errors are skipped.
    pub fn from_dem(dem: &DetectorErrorModel) -> Self {
        // Build nodes for each detector
        let mut nodes: Vec<SpacetimeNode> = (0..dem.detector_count)
            .map(|id| SpacetimeNode {
                id,
                coords: dem.detector_coords(id as u32),
            })
            .collect();

        // Boundary node (last)
        let boundary_node = nodes.len();
        nodes.push(SpacetimeNode {
            id: usize::MAX,
            coords: None,
        });

        let mut edges: Vec<SpacetimeEdge> = Vec::with_capacity(dem.errors.len());

        for err in &dem.errors {
            let p = err.probability;
            // Skip zero-probability and probability-1 errors (infinite weight)
            if p <= 0.0 || p >= 1.0 {
                continue;
            }
            let weight = -(p / (1.0 - p)).ln() as f32;
            let fault_ids = err.observables.clone();

            match err.detectors.len() {
                0 => {
                    // Observable-only flip — boundary-to-boundary (no-op in graph)
                }
                1 => {
                    // Single-detector error: connect to boundary
                    let u = err.detectors[0];
                    let v = boundary_node as u32;
                    edges.push(SpacetimeEdge {
                        u,
                        v,
                        weight,
                        fault_ids,
                    });
                }
                2 => {
                    let u = err.detectors[0];
                    let v = err.detectors[1];
                    edges.push(SpacetimeEdge {
                        u,
                        v,
                        weight,
                        fault_ids,
                    });
                }
                _ => {
                    // Hyperedge: decompose into pairs (approximation valid for
                    // surface code distance-3+ where hyperedges are rare)
                    let dets = &err.detectors;
                    for k in 0..dets.len() - 1 {
                        edges.push(SpacetimeEdge {
                            u: dets[k],
                            v: dets[k + 1],
                            weight,
                            fault_ids: fault_ids.clone(),
                        });
                    }
                }
            }
        }

        Self {
            nodes,
            edges,
            boundary_node,
        }
    }

    pub fn detector_count(&self) -> usize {
        // nodes includes the boundary node
        self.nodes.len().saturating_sub(1)
    }
}
