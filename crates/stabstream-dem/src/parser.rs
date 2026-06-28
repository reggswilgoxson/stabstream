//! Stim DEM text-format parser.
//!
//! Handles `error`, `detector`, `logical_observable`, and `repeat` blocks.
//! The format is line-oriented: each instruction occupies one line (or a
//! `repeat N { ... }` block spanning multiple lines).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid probability '{0}' in error instruction")]
    InvalidProbability(String),
    #[error("malformed target '{0}'")]
    MalformedTarget(String),
    #[error("unterminated repeat block")]
    UnterminatedRepeat,
    #[error("invalid repeat count '{0}'")]
    InvalidRepeatCount(String),
    #[error("malformed detector coords '{0}'")]
    MalformedCoords(String),
}

/// A single error mechanism in the DEM.
#[derive(Debug, Clone)]
pub struct DemError {
    /// Bernoulli probability of this error mechanism firing.
    pub probability: f64,
    /// Detector indices touched by this mechanism.
    pub detectors: Vec<u32>,
    /// Observable indices flipped when this mechanism fires.
    pub observables: Vec<u8>,
}

/// Detector node with optional spacetime coordinates.
#[derive(Debug, Clone)]
pub struct DemDetector {
    pub id: u32,
    /// Spacetime position [x, y, t] — used to build geometry-aware graphs.
    pub coords: Option<[f64; 3]>,
}

/// Parsed Stim Detector Error Model.
#[derive(Debug)]
pub struct DetectorErrorModel {
    pub detector_count: usize,
    pub observable_count: usize,
    pub errors: Vec<DemError>,
    pub detectors: Vec<DemDetector>,
}

impl DetectorErrorModel {
    /// Parse a Stim DEM from its text representation.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let mut errors: Vec<DemError> = Vec::new();
        let mut detectors: Vec<DemDetector> = Vec::new();
        let mut max_detector: i64 = -1;
        let mut max_observable: i64 = -1;

        let lines: Vec<&str> = input.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            i += 1;

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with("repeat ") {
                // repeat N {
                let count = parse_repeat_count(line)?;
                // Collect lines until closing '}'
                let mut block_lines: Vec<&str> = Vec::new();
                loop {
                    if i >= lines.len() {
                        return Err(ParseError::UnterminatedRepeat);
                    }
                    let bl = lines[i].trim();
                    i += 1;
                    if bl == "}" {
                        break;
                    }
                    block_lines.push(bl);
                }
                let block_src = block_lines.join("\n");
                let inner = Self::parse(&block_src)?;
                // Expand the repeat block count times
                for _ in 0..count {
                    for e in &inner.errors {
                        errors.push(e.clone());
                    }
                }
                // Update detector/observable max from inner
                for e in &inner.errors {
                    for &d in &e.detectors {
                        if d as i64 > max_detector {
                            max_detector = d as i64;
                        }
                    }
                    for &o in &e.observables {
                        if o as i64 > max_observable {
                            max_observable = o as i64;
                        }
                    }
                }
                for det in &inner.detectors {
                    let id = det.id;
                    if id as i64 > max_detector {
                        max_detector = id as i64;
                    }
                    if !detectors.iter().any(|d| d.id == id) {
                        detectors.push(det.clone());
                    }
                }
                continue;
            }

            if line.starts_with("error(") {
                let e = parse_error_line(line)?;
                for &d in &e.detectors {
                    if d as i64 > max_detector {
                        max_detector = d as i64;
                    }
                }
                for &o in &e.observables {
                    if o as i64 > max_observable {
                        max_observable = o as i64;
                    }
                }
                errors.push(e);
                continue;
            }

            if line.starts_with("detector") {
                let det = parse_detector_line(line)?;
                if det.id as i64 > max_detector {
                    max_detector = det.id as i64;
                }
                if !detectors.iter().any(|d| d.id == det.id) {
                    detectors.push(det);
                }
                continue;
            }

            if line.starts_with("logical_observable") {
                if let Some(id) = parse_observable_id(line) {
                    if id as i64 > max_observable {
                        max_observable = id as i64;
                    }
                }
                continue;
            }

            // Unknown instruction — skip gracefully (forward compat)
        }

        let detector_count = if max_detector >= 0 {
            (max_detector + 1) as usize
        } else {
            0
        };
        let observable_count = if max_observable >= 0 {
            (max_observable + 1) as usize
        } else {
            0
        };

        Ok(Self {
            detector_count,
            observable_count,
            errors,
            detectors,
        })
    }

    /// Return the spacetime coordinates for detector `id`, if available.
    pub fn detector_coords(&self, id: u32) -> Option<[f64; 3]> {
        self.detectors.iter().find(|d| d.id == id)?.coords
    }
}

// ---------------------------------------------------------------------------
// Line parsers
// ---------------------------------------------------------------------------

fn parse_error_line(line: &str) -> Result<DemError, ParseError> {
    // error(0.001) D0 D1 ^ L0 L1
    let rest = line.strip_prefix("error(").unwrap();
    let (prob_str, rest) = rest
        .split_once(')')
        .ok_or_else(|| ParseError::InvalidProbability(line.to_string()))?;
    let probability = prob_str
        .trim()
        .parse::<f64>()
        .map_err(|_| ParseError::InvalidProbability(prob_str.to_string()))?;

    let mut detectors = Vec::new();
    let mut observables = Vec::new();
    let mut after_caret = false;

    for token in rest.split_whitespace() {
        // ^ is a decomposition separator in Stim DEM (not an observable marker).
        // Classify every target by prefix: D<id> → detector, L<id> → observable.
        if token == "^" {
            after_caret = true;
            continue;
        }
        if let Some(id) = token.strip_prefix('L').and_then(|n| n.parse::<u8>().ok()) {
            observables.push(id);
        } else if let Some(id) = token.strip_prefix('D').and_then(|n| n.parse::<u32>().ok()) {
            detectors.push(id);
        } else {
            return Err(ParseError::MalformedTarget(token.to_string()));
        }
    }

    Ok(DemError {
        probability,
        detectors,
        observables,
    })
}

fn parse_detector_line(line: &str) -> Result<DemDetector, ParseError> {
    // detector[(x, y, t)] D<id>   — coords are optional
    // detector D<id>
    let (coords_part, rest) = if line.contains('(') {
        let open = line.find('(').unwrap();
        let close = line
            .find(')')
            .ok_or_else(|| ParseError::MalformedCoords(line.to_string()))?;
        let coords_str = &line[open + 1..close];
        let after = &line[close + 1..];
        (Some(coords_str), after)
    } else {
        let rest = line.strip_prefix("detector").unwrap_or(line);
        (None, rest)
    };

    let coords = if let Some(s) = coords_part {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let x = parts
            .first()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let y = parts
            .get(1)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let t = parts
            .get(2)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        Some([x, y, t])
    } else {
        None
    };

    // Find the D<id> token in the remainder
    let id = rest
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix('D').and_then(|n| n.parse::<u32>().ok()))
        .ok_or_else(|| ParseError::MalformedTarget(line.to_string()))?;

    Ok(DemDetector { id, coords })
}

fn parse_observable_id(line: &str) -> Option<u8> {
    // logical_observable[(x, y)] L<id>
    line.split_whitespace()
        .find_map(|tok| tok.strip_prefix('L').and_then(|n| n.parse::<u8>().ok()))
}

fn parse_detector_target(token: &str) -> Result<u32, ParseError> {
    token
        .strip_prefix('D')
        .and_then(|n| n.parse::<u32>().ok())
        .ok_or_else(|| ParseError::MalformedTarget(token.to_string()))
}

fn parse_observable_target(token: &str) -> Result<u8, ParseError> {
    token
        .strip_prefix('L')
        .and_then(|n| n.parse::<u8>().ok())
        .ok_or_else(|| ParseError::MalformedTarget(token.to_string()))
}

fn parse_repeat_count(line: &str) -> Result<u64, ParseError> {
    // repeat N {
    let rest = line.strip_prefix("repeat").unwrap().trim();
    let count_str = rest.split_whitespace().next().unwrap_or("");
    count_str
        .parse::<u64>()
        .map_err(|_| ParseError::InvalidRepeatCount(count_str.to_string()))
}
