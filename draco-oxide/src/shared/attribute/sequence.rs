use crate::core::corner_table::GenericCornerTable;
use crate::core::shared::{CornerIdx, VertexIdx};

#[derive(Debug, Clone)]
pub(crate) struct Traverser<'ct, CornerTableType>
where
    CornerTableType: GenericCornerTable,
{
    corner_table: &'ct CornerTableType,
    visited_vertices: Vec<bool>,
    visited_faces: Vec<bool>,
    corner_traversal_stack: Vec<CornerIdx>,
    out: Vec<CornerIdx>,
}

impl<'ct, T> Traverser<'ct, T>
where
    T: GenericCornerTable,
{
    /// Creates a new `Traverser` instance.
    /// # Arguments
    /// * `corner_table` - A reference to the corner table to traverse.
    /// * `corners_of_edgebreaker_traversal` - A vector of corner indices
    ///   representing the last-encoded corners for connected components in encoded order.
    pub(crate) fn new(
        corner_table: &'ct T,
        corners_of_edgebreaker_traversal: Vec<CornerIdx>,
    ) -> Self {
        Self {
            visited_vertices: vec![false; corner_table.num_vertices()],
            visited_faces: vec![false; corner_table.num_faces()],
            corner_table,
            corner_traversal_stack: corners_of_edgebreaker_traversal, // The last encoded connected component gets decoded first
            out: Vec::with_capacity(corner_table.num_corners()),
        }
    }

    pub(crate) fn is_vertex_visited(&self, v: VertexIdx) -> bool {
        self.visited_vertices[usize::from(v)]
    }

    pub(crate) fn visit(&mut self, v: VertexIdx, c: CornerIdx) {
        if !self.visited_vertices[usize::from(v)] {
            self.out.push(c);
        }
        self.visited_vertices[usize::from(v)] = true;
    }

    pub(crate) fn compute_seqeunce(mut self) -> Vec<CornerIdx> {
        while let Some(curr_corner) = self.corner_traversal_stack.pop() {
            // If the face has not yet been visited, then the
            // other vertices of the face are not visited yet either. If this is the case, then
            // we need to store them in self.next_outputs_stack so that they will get processed first.
            let v = self.corner_table.vertex_idx(curr_corner);
            if self.visited_faces[usize::from(self.corner_table.face_idx_containing(curr_corner))] {
                continue;
            }
            let next_c = self.corner_table.next(curr_corner);
            let next_v = self.corner_table.vertex_idx(next_c);
            let prev_c = self.corner_table.previous(curr_corner);
            let prev_v = self.corner_table.vertex_idx(prev_c);
            if !self.is_vertex_visited(next_v) || !self.is_vertex_visited(prev_v) {
                // We need to return the next corner first, then the previous corner, and finally the current corner.
                // This order is determined by the draco library.
                self.visit(next_v, next_c);
                self.visit(prev_v, prev_c);
                self.corner_traversal_stack.push(curr_corner);
                continue;
            }

            // Coming here means that we are visiting a new face.
            let face_idx = self.corner_table.face_idx_containing(curr_corner);
            self.visited_faces[usize::from(face_idx)] = true;
            // Once a face is marked visited it is never unmarked, and the pop
            // loop above skips any corner whose face is already visited. So stale
            // corners of this face still left on the stack (the handle case) are
            // harmlessly skipped when popped; we no longer scan-and-remove them.

            // If we have not yet visited the vertex of the current corner and if it is not on a boundary then we can simply return it.
            if !self.is_vertex_visited(v) {
                self.visit(v, curr_corner);
                if !self.corner_table.is_on_boundary(v) {
                    self.corner_traversal_stack.push(
                        self.corner_table.get_right_corner(curr_corner).unwrap(), // It is guaranteed to exist because the current corner is unvisited and not on a boundary
                    );
                    continue;
                }
            }

            self.visit(v, curr_corner);

            let right_corner = self.corner_table.get_right_corner(curr_corner);
            let left_corner = self.corner_table.get_left_corner(curr_corner);
            let right_face = right_corner.map(|c| self.corner_table.face_idx_containing(c));
            let left_face = left_corner.map(|c| self.corner_table.face_idx_containing(c));

            if right_face.is_some() && self.visited_faces[usize::from(right_face.unwrap())] {
                // Right face has been visited
                if left_face.is_some() && self.visited_faces[usize::from(left_face.unwrap())] {
                    // Both neighboring faces are visited, we can continue traversing. No update to the stack.
                } else {
                    // Left face is unvisited or does not exist.
                    // We need to traverse the left face if it exists.
                    if let Some(lc) = left_corner {
                        self.corner_traversal_stack.push(lc);
                    }
                }
            } else {
                // Right face is unvisited or does not exist.
                if left_face.is_some() && self.visited_faces[usize::from(left_face.unwrap())] {
                    // Left face is visited.
                    // we need to traverse the right face if it exists.
                    if let Some(rc) = right_corner {
                        self.corner_traversal_stack.push(rc);
                    }
                } else {
                    // Both neighboring faces are unvisited, or the neighborig faces may not exist.
                    // If there are neighboring faces, then we need to traverse them.
                    // The right corner must be traversed first.
                    if let Some(lc) = left_corner {
                        self.corner_traversal_stack.push(lc);
                    }
                    if let Some(rc) = right_corner {
                        self.corner_traversal_stack.push(rc);
                    }
                }
            }
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::shared::ConfigType;
    use crate::encode::connectivity::ConnectivityEncoderOutput;
    use crate::{encode::connectivity::encode_connectivity, io::obj::load_obj};

    #[test]
    fn test_traverser() {
        let mut mesh = load_obj("tests/data/tetrahedron.obj").unwrap();
        let out: crate::encode::connectivity::ConnectivityEncoderOutput<'_> = encode_connectivity(
            &mesh.faces,
            &mut mesh.attributes,
            &mut Vec::new(),
            &crate::encode::Config::default(),
        )
        .unwrap();

        let (ct, corners) = if let ConnectivityEncoderOutput::Edgebreaker(edgebreaker_out) = out {
            (
                edgebreaker_out.corner_table,
                edgebreaker_out.corners_of_edgebreaker,
            )
        } else {
            panic!("Expected Edgebreaker Output");
        };

        let ct_pos = ct.universal_corner_table();
        let sequence_points = Traverser::new(ct_pos, corners.clone())
            .compute_seqeunce()
            .iter()
            .map(|c| ct_pos.point_idx(*c))
            .collect::<Vec<_>>();
        assert_eq!(
            sequence_points
                .into_iter()
                .map(|c| usize::from(c))
                .collect::<Vec<_>>(),
            vec![3, 1, 0, 2]
        );

        let ct_nor = &ct.attribute_corner_table(1).unwrap();
        let sequence_normals = Traverser::new(ct_nor, corners.clone())
            .compute_seqeunce()
            .iter()
            .map(|c| ct_nor.point_idx(*c))
            .collect::<Vec<_>>();
        assert_eq!(
            sequence_normals
                .into_iter()
                .map(|c| usize::from(c))
                .collect::<Vec<_>>(),
            vec![3, 1, 0, 2]
        );

        let ct_tex = &ct.attribute_corner_table(2).unwrap();
        let sequence_tex_coords = Traverser::new(ct_tex, corners)
            .compute_seqeunce()
            .iter()
            .map(|c| ct_tex.point_idx(*c))
            .collect::<Vec<_>>();
        assert_eq!(
            sequence_tex_coords
                .into_iter()
                .map(|c| usize::from(c))
                .collect::<Vec<_>>(),
            vec![3, 1, 0, 2, 5, 4]
        );
    }

    /// FNV-1a over the little-endian bytes of the point-index sequence.
    /// Deterministic and toolchain-independent, unlike DefaultHasher.
    fn digest(seq: &[usize]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &v in seq {
            for b in (v as u64).to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// Computes (attr_idx, sequence_len, digest) for the universal corner table
    /// (attr 0) and every attribute corner table of `mesh`. The digest captures
    /// the exact `Vec<CornerIdx>` traversal order via point indices — this is the
    /// shared encoder/decoder symmetry that must stay byte-identical.
    fn sequence_fingerprints(path: &str) -> Vec<(usize, usize, u64)> {
        let mut mesh = load_obj(path).unwrap();
        let out = encode_connectivity(
            &mesh.faces,
            &mut mesh.attributes,
            &mut Vec::new(),
            &crate::encode::Config::default(),
        )
        .unwrap();

        let (ct, corners) = if let ConnectivityEncoderOutput::Edgebreaker(eb) = out {
            (eb.corner_table, eb.corners_of_edgebreaker)
        } else {
            panic!("Expected Edgebreaker Output for {path}");
        };

        let mut fps = Vec::new();

        let ct_pos = ct.universal_corner_table();
        let seq: Vec<usize> = Traverser::new(ct_pos, corners.clone())
            .compute_seqeunce()
            .iter()
            .map(|c| usize::from(ct_pos.point_idx(*c)))
            .collect();
        fps.push((0, seq.len(), digest(&seq)));

        let mut attr_idx = 1;
        while let Some(ct_attr) = ct.attribute_corner_table(attr_idx) {
            let seq: Vec<usize> = Traverser::new(&ct_attr, corners.clone())
                .compute_seqeunce()
                .iter()
                .map(|c| usize::from(ct_attr.point_idx(*c)))
                .collect();
            fps.push((attr_idx, seq.len(), digest(&seq)));
            attr_idx += 1;
        }

        fps
    }

    /// Byte-identical oracle for `compute_sequence`. The expected fingerprints
    /// were captured from the pre-optimization implementation; any change that
    /// alters the traversal order on these meshes (boundaries, handles) trips
    /// this test. torus.obj carries topological handles, which is exactly the
    /// case the handle-detection scan-and-remove blocks exist to handle.
    #[test]
    fn oracle_compute_sequence() {
        let cases: &[(&str, &[(usize, usize, u64)])] = &[
            ("tests/data/tetrahedron.obj", EXPECT_TETRAHEDRON),
            ("tests/data/sphere.obj", EXPECT_SPHERE),
            ("tests/data/punctured_sphere.obj", EXPECT_PUNCTURED_SPHERE),
            ("tests/data/torus.obj", EXPECT_TORUS),
            ("tests/data/bunny.obj", EXPECT_BUNNY),
        ];

        let dump = std::env::var("DUMP_FINGERPRINTS").is_ok();
        for (path, expected) in cases {
            let got = sequence_fingerprints(path);
            if dump {
                eprintln!("{path} => {got:?}");
                continue;
            }
            assert_eq!(
                &got[..],
                *expected,
                "compute_sequence output changed for {path}"
            );
        }
    }

    // Captured from the pre-optimization implementation. Format: (attr_idx, len, fnv1a_digest).
    const EXPECT_TETRAHEDRON: &[(usize, usize, u64)] = &[
        (0, 4, 18054049684469353541),
        (1, 4, 18054049684469353541),
        (2, 6, 3159456026337658052),
    ];
    const EXPECT_SPHERE: &[(usize, usize, u64)] =
        &[(0, 114, 17737425019064467876), (1, 114, 17737425019064467876)];
    const EXPECT_PUNCTURED_SPHERE: &[(usize, usize, u64)] =
        &[(0, 114, 17132826066695074116), (1, 114, 17132826066695074116)];
    const EXPECT_TORUS: &[(usize, usize, u64)] = &[(0, 2051, 930682351741064974)];
    const EXPECT_BUNNY: &[(usize, usize, u64)] =
        &[(0, 34834, 3080192193140594432), (1, 34834, 3080192193140594432)];
}
