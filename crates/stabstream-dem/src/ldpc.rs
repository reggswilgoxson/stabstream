//! Bivariate Bicycle (BB) qLDPC code construction.
//!
//! Builds Hz and Hx check matrices for BB CSS codes over Z_l × Z_m
//! following Bravyi et al. 2024 (<https://arxiv.org/abs/2308.07915>):
//!
//!   Hz = [A | B^T],  Hx = [B | A^T]
//!
//! where A and B are circulant matrices defined by polynomials over F_2[Z_l × Z_m].

use base64::{engine::general_purpose::STANDARD, Engine};

/// Parameters for a Bivariate Bicycle CSS code over Z_l × Z_m.
#[derive(Debug, Clone)]
pub struct BbParams {
    /// Cyclic group order for the first variable (x direction).
    pub l: usize,
    /// Cyclic group order for the second variable (y direction).
    pub m: usize,
    /// Support of polynomial A as `(offset_l, offset_m)` pairs.
    pub poly_a: Vec<(usize, usize)>,
    /// Support of polynomial B as `(offset_l, offset_m)` pairs.
    pub poly_b: Vec<(usize, usize)>,
    /// Known code distance d.
    pub distance: u8,
    /// Number of encoded logical qubits k.
    pub logical_qubits: u16,
}

impl BbParams {
    /// BB[[144, 12, 12]]: Bravyi et al. 2024, (l=12, m=6).
    /// A = x³ + y + y², B = y³ + x + x²
    pub fn bb_144_12_12() -> Self {
        Self {
            l: 12,
            m: 6,
            poly_a: vec![(3, 0), (0, 1), (0, 2)],
            poly_b: vec![(0, 3), (1, 0), (2, 0)],
            distance: 12,
            logical_qubits: 12,
        }
    }

    /// BB[[72, 12, 6]]: Bravyi et al. 2024, (l=6, m=6).
    /// A = x³ + y + y², B = y³ + x + x²
    pub fn bb_72_12_6() -> Self {
        Self {
            l: 6,
            m: 6,
            poly_a: vec![(3, 0), (0, 1), (0, 2)],
            poly_b: vec![(0, 3), (1, 0), (2, 0)],
            distance: 6,
            logical_qubits: 12,
        }
    }

    /// Total data qubit count: n = 2 · l · m.
    pub fn n(&self) -> usize {
        2 * self.l * self.m
    }

    /// Ancilla count per syndrome round: 2 · l · m (l·m Z + l·m X ancillas).
    pub fn ancilla_count(&self) -> usize {
        2 * self.l * self.m
    }

    /// Encoding rate k/n.
    pub fn encoding_rate(&self) -> f64 {
        self.logical_qubits as f64 / self.n() as f64
    }

    fn flat_idx(&self, i: usize, j: usize) -> usize {
        i * self.m + j
    }

    /// Build `(hz_rows, hx_rows)`.
    ///
    /// Each returned `Vec<u16>` is a sorted list of column indices in
    /// `[0, 2·l·m)`.  The first `l·m` columns are "A-type" data qubits;
    /// the second `l·m` columns are "B-type" data qubits.
    pub fn build_check_rows(&self) -> (Vec<Vec<u16>>, Vec<Vec<u16>>) {
        let (l, m) = (self.l, self.m);
        let n_anc = l * m;

        let mut hz_rows: Vec<Vec<u16>> = Vec::with_capacity(n_anc);
        let mut hx_rows: Vec<Vec<u16>> = Vec::with_capacity(n_anc);

        for i in 0..l {
            for j in 0..m {
                let mut hz_row = Vec::with_capacity(6);
                let mut hx_row = Vec::with_capacity(6);

                // Hz left (A block): col = (i−di mod l, j−dj mod m)
                for &(di, dj) in &self.poly_a {
                    let ci = (i + l - di % l) % l;
                    let cj = (j + m - dj % m) % m;
                    hz_row.push(self.flat_idx(ci, cj) as u16);
                }
                // Hz right (B^T block): col = n_anc + (i+di mod l, j+dj mod m)
                for &(di, dj) in &self.poly_b {
                    let ci = (i + di) % l;
                    let cj = (j + dj) % m;
                    hz_row.push((n_anc + self.flat_idx(ci, cj)) as u16);
                }
                hz_row.sort_unstable();
                hz_rows.push(hz_row);

                // Hx left (B block): col = (i−di mod l, j−dj mod m)
                for &(di, dj) in &self.poly_b {
                    let ci = (i + l - di % l) % l;
                    let cj = (j + m - dj % m) % m;
                    hx_row.push(self.flat_idx(ci, cj) as u16);
                }
                // Hx right (A^T block): col = n_anc + (i+di mod l, j+dj mod m)
                for &(di, dj) in &self.poly_a {
                    let ci = (i + di) % l;
                    let cj = (j + dj) % m;
                    hx_row.push((n_anc + self.flat_idx(ci, cj)) as u16);
                }
                hx_row.sort_unstable();
                hx_rows.push(hx_row);
            }
        }

        (hz_rows, hx_rows)
    }
}

/// Encode a binary sparse matrix as a base64 CSR blob.
///
/// Wire format (all little-endian `u32`):
/// `[nrows][ncols][nnz][row_ptr × (nrows+1)][col_ind × nnz]`
///
/// Decodable in Python with:
/// ```python
/// import numpy as np, base64, struct
/// raw = base64.b64decode(s)
/// nrows, ncols, nnz = struct.unpack_from('<III', raw)
/// row_ptr = np.frombuffer(raw, dtype=np.uint32, count=nrows+1, offset=12)
/// col_ind = np.frombuffer(raw, dtype=np.uint32, count=nnz, offset=12+(nrows+1)*4)
/// ```
pub fn encode_csr_base64(rows: &[Vec<u16>], ncols: usize) -> String {
    let nrows = rows.len();
    let nnz: usize = rows.iter().map(|r| r.len()).sum();
    let byte_count = 12 + (nrows + 1) * 4 + nnz * 4;
    let mut buf: Vec<u8> = Vec::with_capacity(byte_count);

    buf.extend_from_slice(&(nrows as u32).to_le_bytes());
    buf.extend_from_slice(&(ncols as u32).to_le_bytes());
    buf.extend_from_slice(&(nnz as u32).to_le_bytes());

    let mut ptr = 0u32;
    buf.extend_from_slice(&ptr.to_le_bytes());
    for row in rows {
        ptr += row.len() as u32;
        buf.extend_from_slice(&ptr.to_le_bytes());
    }
    for row in rows {
        for &col in row {
            buf.extend_from_slice(&(col as u32).to_le_bytes());
        }
    }

    STANDARD.encode(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bb_72_row_weights() {
        let p = BbParams::bb_72_12_6();
        let (hz, hx) = p.build_check_rows();
        assert_eq!(hz.len(), 36);
        assert_eq!(hx.len(), 36);
        for row in hz.iter().chain(hx.iter()) {
            assert_eq!(row.len(), 6, "each stabilizer should have weight 6");
        }
    }

    #[test]
    fn bb_144_row_weights() {
        let p = BbParams::bb_144_12_12();
        let (hz, hx) = p.build_check_rows();
        assert_eq!(hz.len(), 72);
        assert_eq!(hx.len(), 72);
        for row in hz.iter().chain(hx.iter()) {
            assert_eq!(row.len(), 6);
        }
    }

    #[test]
    fn bb_72_css_commutativity() {
        // Hz · Hx^T = 0 mod 2 for every pair of rows
        let p = BbParams::bb_72_12_6();
        let (hz, hx) = p.build_check_rows();
        let n = p.n();
        for hz_row in &hz {
            for hx_row in &hx {
                let overlap = hz_row.iter().filter(|c| hx_row.contains(c)).count();
                assert_eq!(
                    overlap % 2,
                    0,
                    "Hz/Hx rows must overlap evenly (CSS commutativity)"
                );
            }
        }
        let _ = n;
    }

    #[test]
    fn encode_csr_roundtrip_dimensions() {
        let p = BbParams::bb_72_12_6();
        let (hz, _) = p.build_check_rows();
        let b64 = encode_csr_base64(&hz, p.n());
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .unwrap();
        let nrows = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize;
        let ncols = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
        assert_eq!(nrows, 36);
        assert_eq!(ncols, 72);
    }
}
