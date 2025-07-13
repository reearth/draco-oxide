use faer::{traits::pulp::num_complex::ComplexFloat, Mat};

#[allow(unused)]
pub(crate) fn weak_eq_by_laplacian(x: &[[usize; 3]], y: &[[usize; 3]]) -> Option<bool> {
    // Check if the two meshes have the same number of faces
    if x.len() != y.len() {
        return Some(false);
    }

    // Check if the two meshes have the same number of vertices
    let n_vertices = x.iter().flatten().max().unwrap()+1;
    if n_vertices != y.iter().flatten().max().unwrap()+1 {
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
    
    
    // laplacian matrix
    let l1_x = compute_l1(x);
    let n_edges = l1_x.nrows();
    // compute the eigenvalues and eigenvectors of the laplacian matrix
    let l1_x_eigen = l1_x.clone().eigen().unwrap();
    let mut l1_x_eigen = (0..n_edges).map(|i| l1_x_eigen.S()  [i].re()).collect::<Vec<_>>();
    l1_x_eigen.sort_by(|a, b| a.partial_cmp(b).unwrap());
    
    // laplacian matrix
    let l1_y = compute_l1(y);
    // compute the eigenvalues and eigenvectors of the laplacian matrix
    let l1_y_eigen = l1_y.eigen().unwrap();
    let mut l1_y_eigen = (0..n_edges).map(|i| l1_y_eigen.S()[i].re()).collect::<Vec<_>>();
    l1_y_eigen.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let err = l1_x_eigen.iter().zip(l1_y_eigen.iter()).map(|(a, b)| (a-b).abs()).sum::<f64>();
    Some(
        err < 1e-6
    )
}


fn compute_l1(x: &[[usize; 3]]) -> Mat<f64> {
    let mut x = x.to_vec();
    x.iter_mut().for_each(|face| face.sort());
    x.sort();

    // compute edges 
    let edges = {
        let mut edges = x.iter().flat_map(|face| [
            [face[0], face[1]],
            [face[1], face[2]],
            [face[2], face[0]],
        ]).collect::<Vec<_>>();
        edges.iter_mut().for_each(|e| {
            e.sort();
        });
        edges.sort();
        edges.dedup();
        edges
    };

    let mut l1_up = Mat::<f64>::zeros(edges.len(), edges.len());
    for face in x.iter() {
        let mut face = *face;
        face.sort();
        let e1 = edges.binary_search(&[face[0], face[1]]).unwrap();
        let e2 = edges.binary_search(&[face[1], face[2]]).unwrap();
        let e3 = edges.binary_search(&[face[0], face[2]]).unwrap();

        l1_up[(e1, e1)] += 1.0;
        l1_up[(e2, e2)] += 1.0;
        l1_up[(e3, e3)] += 1.0;
        
        l1_up[(e1, e2)] += 1.0;
        l1_up[(e2, e1)] += 1.0;

        l1_up[(e2, e3)] -= 1.0;
        l1_up[(e3, e2)] -= 1.0;

        l1_up[(e3, e1)] -= 1.0;
        l1_up[(e1, e3)] -= 1.0;
    }

    let mut l1_down = Mat::<f64>::zeros(edges.len(), edges.len());
    for i in 0..edges.len() {
        l1_down[(i, i)] += 2.0;
        for j in i+1..edges.len() {
            let e1 = edges[i];
            let e2 = edges[j];
            if let Some(&v) = e1.iter().find(|v| e2.contains(v)) {
                if (e1[0]==v && e2[0]==v) || (e1[1]==v && e2[1]==v) {
                    // if v has the same sign with both edges...
                    l1_down[(i, j)] += 1.0;
                    l1_down[(j, i)] += 1.0;
                } else {
                    l1_down[(i, j)] -= 1.0;
                    l1_down[(j, i)] -= 1.0;
                }
            } else {
                continue;
            }
        }
    }

    l1_down + l1_up
}

#[cfg(test)]
mod tests {
    use super::*;
    use faer::mat;
    use faer::traits::pulp::num_complex::Complex64;

    #[test]
    fn test_compute_l1() {
        let x = [[0, 1, 2], [1, 2, 3]];
        let l1 = compute_l1(&x);
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

        let torus1 = vec!{
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        };

        let num_vertices = torus1.iter().flat_map(|face| face).max().unwrap() + 1;
        // create permutation for the vertices
        let p = (0..num_vertices)
            .map(|i| (i * (num_vertices-1))%num_vertices)
            .collect::<Vec<_>>();
        let mut torus2 = torus1.clone();
        for face in torus2.iter_mut() {
            for i in 0..3 {
                face[i] = p[face[i]];
            }
        }

        assert_eq!(weak_eq_by_laplacian(&torus1, &torus2), Some(true));
    }

    #[test]
    fn test_faer () {
        let m = mat![
            [2.0, 1.0, 0.0],
            [1.0, 2.0, 1.0],
            [0.0, 1.0, 2.0]
        ];

        let eigen = m.eigen().unwrap();
        let eigen = eigen.S();
        assert!(((eigen[0]-Complex64::from(2_f64-2_f64.sqrt())) as Complex64).abs() < 1e-6);
        assert!(((eigen[1]-Complex64::from(2_f64)) as Complex64).abs() < 1e-6);
        assert!(((eigen[2]-Complex64::from(2_f64+2_f64.sqrt())) as Complex64).abs() < 1e-6);
    }
}