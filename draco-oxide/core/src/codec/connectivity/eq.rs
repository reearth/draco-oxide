//! Weak connectivity-isomorphism test via the edge (1-)Laplacian spectrum.

use faer::{traits::pulp::num_complex::ComplexFloat, Mat};

/// How many eigenvalues are compared from each end of the spectrum.
const NUM_EXTREMES: usize = 5;

/// Per-eigenvalue tolerance for the spectrum comparison.
const EIGEN_TOL: f64 = 1e-6;

/// Compares two triangle meshes by the smallest and largest [`NUM_EXTREMES`]
/// distinct eigenvalues of their edge Laplacians (distinct because a
/// single-vector Krylov method cannot resolve multiplicities). The spectrum
/// is a relabeling invariant but not a complete one, hence "weak". `None`
/// when a mesh has unconnected vertices.
pub fn weak_eq_by_laplacian(x: &[[usize; 3]], y: &[[usize; 3]]) -> Option<bool> {
    // Check if the two meshes have the same number of faces
    if x.len() != y.len() {
        return Some(false);
    }

    // Check if the two meshes have the same number of vertices
    let n_vertices = x.iter().flatten().max().unwrap() + 1;
    if n_vertices != y.iter().flatten().max().unwrap() + 1 {
        return Some(false);
    }

    // Check if the two meshes have no unconnected vertices
    let mut x_v_set = vec![false; n_vertices];
    for v in x.iter().flatten() {
        x_v_set[*v] = true;
    }
    if x_v_set.iter().any(|&v| !v) {
        return None;
    }

    let mut y_v_set = vec![false; n_vertices];
    for v in y.iter().flatten() {
        y_v_set[*v] = true;
    }
    if y_v_set.iter().any(|&v| !v) {
        return None;
    }

    let l1_x = SparseL1::build(x);
    let l1_y = SparseL1::build(y);
    if l1_x.n != l1_y.n {
        return Some(false);
    }

    let ex = extreme_eigenvalues(&l1_x);
    let ey = extreme_eigenvalues(&l1_y);
    Some(ex.len() == ey.len() && ex.iter().zip(&ey).all(|(a, b)| (a - b).abs() < EIGEN_TOL))
}

/// The edge Laplacian (up plus down) in compressed sparse rows.
struct SparseL1 {
    n: usize,
    row_start: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<f64>,
}

impl SparseL1 {
    fn build(faces: &[[usize; 3]]) -> Self {
        let mut faces = faces.to_vec();
        faces.iter_mut().for_each(|face| face.sort());
        faces.sort();

        // compute edges
        let edges = {
            let mut edges = faces
                .iter()
                .flat_map(|face| [[face[0], face[1]], [face[1], face[2]], [face[2], face[0]]])
                .collect::<Vec<_>>();
            edges.iter_mut().for_each(|e| {
                e.sort();
            });
            edges.sort();
            edges.dedup();
            edges
        };
        let n = edges.len();

        let mut triplets: Vec<(usize, usize, f64)> = Vec::with_capacity(faces.len() * 9 + n * 8);

        // The up Laplacian: one block per face, signs from the induced edge
        // orientations.
        for face in &faces {
            let e1 = edges.binary_search(&[face[0], face[1]]).unwrap();
            let e2 = edges.binary_search(&[face[1], face[2]]).unwrap();
            let e3 = edges.binary_search(&[face[0], face[2]]).unwrap();
            triplets.extend([
                (e1, e1, 1.0),
                (e2, e2, 1.0),
                (e3, e3, 1.0),
                (e1, e2, 1.0),
                (e2, e1, 1.0),
                (e2, e3, -1.0),
                (e3, e2, -1.0),
                (e3, e1, -1.0),
                (e1, e3, -1.0),
            ]);
        }

        // The down Laplacian: 2 on the diagonal; for each pair of edges
        // sharing a vertex, +1 when the shared vertex has the same sign in
        // both, -1 otherwise. Distinct edges share at most one vertex, so
        // walking each vertex's incident edges hits every pair once.
        for i in 0..n {
            triplets.push((i, i, 2.0));
        }
        let n_vertices = edges.iter().flatten().max().map_or(0, |v| v + 1);
        let mut incident: Vec<Vec<usize>> = vec![Vec::new(); n_vertices];
        for (i, e) in edges.iter().enumerate() {
            incident[e[0]].push(i);
            incident[e[1]].push(i);
        }
        for (v, list) in incident.iter().enumerate() {
            for (a, &i) in list.iter().enumerate() {
                for &j in &list[a + 1..] {
                    let (e1, e2) = (edges[i], edges[j]);
                    let s = if (e1[0] == v && e2[0] == v) || (e1[1] == v && e2[1] == v) {
                        1.0
                    } else {
                        -1.0
                    };
                    triplets.push((i, j, s));
                    triplets.push((j, i, s));
                }
            }
        }

        // Triplets to CSR, summing duplicates.
        triplets.sort_by_key(|&(r, c, _)| (r, c));
        let mut row_start = vec![0usize; n + 1];
        let mut cols: Vec<usize> = Vec::with_capacity(triplets.len());
        let mut vals: Vec<f64> = Vec::with_capacity(triplets.len());
        let mut last: Option<(usize, usize)> = None;
        for &(r, c, v) in &triplets {
            if last == Some((r, c)) {
                *vals.last_mut().unwrap() += v;
            } else {
                cols.push(c);
                vals.push(v);
                last = Some((r, c));
            }
            row_start[r + 1] = cols.len();
        }
        // Rows with no entries inherit the previous offset.
        for r in 0..n {
            row_start[r + 1] = row_start[r + 1].max(row_start[r]);
        }

        SparseL1 {
            n,
            row_start,
            cols,
            vals,
        }
    }

    fn matvec(&self, x: &[f64], out: &mut [f64]) {
        for (r, o) in out.iter_mut().enumerate().take(self.n) {
            let mut acc = 0.0;
            for k in self.row_start[r]..self.row_start[r + 1] {
                acc += self.vals[k] * x[self.cols[k]];
            }
            *o = acc;
        }
    }

    fn to_dense(&self) -> Mat<f64> {
        let mut m = Mat::<f64>::zeros(self.n, self.n);
        for r in 0..self.n {
            for k in self.row_start[r]..self.row_start[r + 1] {
                m[(r, self.cols[k])] += self.vals[k];
            }
        }
        m
    }
}

/// The compared eigenvalues: the deduplicated spectrum for small matrices,
/// otherwise the extreme [`NUM_EXTREMES`] distinct Lanczos Ritz values. Only
/// residual-certified values are kept, which excludes the slowly re-emerging
/// copies of multiple eigenvalues; survivors are converged to machine
/// precision, so the dedup threshold sits far below [`EIGEN_TOL`].
fn extreme_eigenvalues(l: &SparseL1) -> Vec<f64> {
    /// Lanczos steps; the compared extremes are converged far earlier.
    const STEPS: usize = 150;
    /// Certification bound: for a symmetric matrix the eigenvalue error is at
    /// most the Ritz residual, so this keeps values accurate below EIGEN_TOL.
    const RESIDUAL_TOL: f64 = 1e-8;

    if l.n <= STEPS {
        let mut ev = dense_eigenvalues(&l.to_dense());
        ev.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ev.dedup_by(|a, b| (*a - *b).abs() < EIGEN_TOL / 10.0);
        let start = ev.len().saturating_sub(NUM_EXTREMES);
        let mut out = ev[..NUM_EXTREMES.min(ev.len())].to_vec();
        out.extend_from_slice(&ev[start.max(NUM_EXTREMES.min(ev.len()))..]);
        return out;
    }

    let n = l.n;
    let mut vs: Vec<Vec<f64>> = Vec::with_capacity(STEPS);
    let mut alphas: Vec<f64> = Vec::with_capacity(STEPS);
    let mut betas: Vec<f64> = Vec::with_capacity(STEPS);
    vs.push(deterministic_unit(n, 1));

    let mut w = vec![0.0; n];
    let mut final_beta = 0.0;
    for j in 0..STEPS {
        l.matvec(&vs[j], &mut w);
        let alpha = dot(&w, &vs[j]);
        alphas.push(alpha);
        // Full reorthogonalization, twice; the residual certification below
        // is only a bound while the basis stays orthogonal, so the second
        // pass is load-bearing.
        for _ in 0..2 {
            for u in &vs {
                let c = dot(&w, u);
                for (wi, ui) in w.iter_mut().zip(u) {
                    *wi -= c * ui;
                }
            }
        }
        let beta = dot(&w, &w).sqrt();
        if j + 1 == STEPS {
            final_beta = beta;
            break;
        }
        if beta > 1e-10 {
            let inv = 1.0 / beta;
            betas.push(beta);
            vs.push(w.iter().map(|x| x * inv).collect());
        } else {
            // Invariant subspace found: continue in its orthogonal complement
            // with a fresh start vector (a zero coupling keeps T's spectrum
            // the union of the blocks').
            let mut fresh = Vec::new();
            for seed in 2..(n as u64 + 2) {
                fresh = deterministic_unit(n, seed);
                for u in &vs {
                    let c = dot(&fresh, u);
                    for (fi, ui) in fresh.iter_mut().zip(u) {
                        *fi -= c * ui;
                    }
                }
                if dot(&fresh, &fresh).sqrt() > 1e-8 {
                    break;
                }
                fresh.clear();
            }
            if fresh.is_empty() {
                final_beta = 0.0;
                break;
            }
            let inv = 1.0 / dot(&fresh, &fresh).sqrt();
            fresh.iter_mut().for_each(|x| *x *= inv);
            betas.push(0.0);
            vs.push(fresh);
        }
    }

    // Certified Ritz values of the tridiagonal projection.
    let m = alphas.len();
    let mut t = Mat::<f64>::zeros(m, m);
    for (i, &a) in alphas.iter().enumerate() {
        t[(i, i)] = a;
    }
    for (i, &b) in betas.iter().enumerate() {
        t[(i, i + 1)] = b;
        t[(i + 1, i)] = b;
    }
    let eigen = t.eigen().unwrap();
    let mut certified: Vec<f64> = (0..m)
        .filter_map(|i| {
            let theta = eigen.S()[i].re();
            let residual = (final_beta * eigen.U()[(m - 1, i)].re()).abs();
            (residual < RESIDUAL_TOL).then_some(theta)
        })
        .collect();
    certified.sort_by(|a, b| a.partial_cmp(b).unwrap());
    certified.dedup_by(|a, b| (*a - *b).abs() < EIGEN_TOL / 10.0);

    let k = NUM_EXTREMES.min(certified.len());
    let mut out = certified[..k].to_vec();
    let start = certified.len().saturating_sub(NUM_EXTREMES).max(k);
    out.extend_from_slice(&certified[start..]);
    out
}

fn dense_eigenvalues(m: &Mat<f64>) -> Vec<f64> {
    let eigen = m.eigen().unwrap();
    (0..m.nrows()).map(|i| eigen.S()[i].re()).collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// A deterministic pseudo-random unit vector (xorshift64), so runs are
/// reproducible without an RNG dependency.
fn deterministic_unit(n: usize, seed: u64) -> Vec<f64> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut v: Vec<f64> = (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state as f64 / u64::MAX as f64) - 0.5
        })
        .collect();
    let inv = 1.0 / dot(&v, &v).sqrt();
    v.iter_mut().for_each(|x| *x *= inv);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use faer::mat;
    use faer::traits::pulp::num_complex::Complex64;

    #[test]
    fn test_compute_l1() {
        let x = [[0, 1, 2], [1, 2, 3]];
        let l1 = SparseL1::build(&x).to_dense();
        let expected =
            // l1 up
            mat![
                [ 1.0, -1.0,  1.0,  0.0,  0.0], // [0,1]
                [-1.0,  1.0, -1.0,  0.0,  0.0], // [0,2]
                [ 1.0, -1.0,  2.0, -1.0,  1.0], // [1,2]
                [ 0.0,  0.0, -1.0,  1.0, -1.0], // [1,3]
                [ 0.0,  0.0,  1.0, -1.0,  1.0], // [2,3]
            ]
            +
            mat![
                [ 2.0,  1.0, -1.0, -1.0,  0.0], // [0,1]
                [ 1.0,  2.0,  1.0,  0.0, -1.0], // [0,2]
                [-1.0,  1.0,  2.0,  1.0, -1.0], // [1,2]
                [-1.0,  0.0,  1.0,  2.0,  1.0], // [1,3]
                [ 0.0, -1.0, -1.0,  1.0,  2.0], // [2,3]
            ];
        assert_eq!(l1, expected, "l1={:?}, expected={:?}", l1, expected);
    }

    #[test]
    fn test_weak_eq_by_laplacian() {
        let x = [[0, 1, 2], [1, 2, 3]];
        let y = [[0, 1, 2], [0, 1, 3]];
        assert_eq!(weak_eq_by_laplacian(&x, &y), Some(true));

        let torus1 = vec![
            [9, 12, 13],
            [8, 9, 13],
            [8, 9, 10],
            [1, 8, 10],
            [1, 10, 11],
            [1, 2, 11],
            [2, 11, 12],
            [2, 12, 13],
            [8, 13, 14],
            [7, 8, 14],
            [1, 7, 8],
            [0, 1, 7],
            [0, 1, 2],
            [0, 2, 3],
            [2, 3, 13],
            [3, 13, 14],
            [7, 14, 15],
            [6, 7, 15],
            [0, 6, 7],
            [0, 5, 6],
            [0, 3, 5],
            [3, 4, 5],
            [3, 4, 14],
            [4, 14, 15],
            [6, 12, 15],
            [6, 9, 12],
            [5, 6, 9],
            [5, 9, 10],
            [4, 5, 10],
            [4, 10, 11],
            [4, 11, 15],
            [11, 12, 15],
        ];

        let num_vertices = torus1.iter().flatten().max().unwrap() + 1;
        // create permutation for the vertices
        let p = (0..num_vertices)
            .map(|i| (i * (num_vertices - 1)) % num_vertices)
            .collect::<Vec<_>>();
        let mut torus2 = torus1.clone();
        for face in torus2.iter_mut() {
            for i in 0..3 {
                face[i] = p[face[i]];
            }
        }

        assert_eq!(weak_eq_by_laplacian(&torus1, &torus2), Some(true));

        // An edge flip preserves the vertex, face, and edge counts but changes
        // the connectivity; the spectra must differ.
        let mut torus3 = torus1.clone();
        assert_eq!(torus3[0], [9, 12, 13]);
        assert_eq!(torus3[1], [8, 9, 13]);
        torus3[0] = [8, 12, 13];
        torus3[1] = [8, 9, 12];
        assert_eq!(weak_eq_by_laplacian(&torus1, &torus3), Some(false));
    }

    #[test]
    fn test_faer() {
        let m = mat![[2.0, 1.0, 0.0], [1.0, 2.0, 1.0], [0.0, 1.0, 2.0]];

        let eigen = m.eigen().unwrap();
        let eigen = eigen.S();
        assert!(((eigen[0] - Complex64::from(2_f64 - 2_f64.sqrt())) as Complex64).abs() < 1e-6);
        assert!(((eigen[1] - Complex64::from(2_f64)) as Complex64).abs() < 1e-6);
        assert!(((eigen[2] - Complex64::from(2_f64 + 2_f64.sqrt())) as Complex64).abs() < 1e-6);
    }
}
