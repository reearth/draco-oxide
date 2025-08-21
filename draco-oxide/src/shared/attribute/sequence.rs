use crate::core::corner_table::GenericCornerTable;
use crate::core::shared::{CornerIdx, VertexIdx};

#[derive(Debug, Clone)]
pub(crate) struct Traverser<'ct, CornerTableType> 
    where CornerTableType: GenericCornerTable
{
    corner_table: &'ct CornerTableType,
    visited_vertices: Vec<bool>,
    visited_faces: Vec<bool>,
    corner_traversal_stack: Vec<CornerIdx>,
    out: Vec<CornerIdx>,
}

impl<'ct, T> Traverser<'ct, T> 
    where T: GenericCornerTable
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

            // If we have not yet visited the vertex of the current corner and if it is not on a boundary then we can simply return it.
            if !self.is_vertex_visited(v) {
                self.visit(v, curr_corner);
                if !self.corner_table.is_on_boundary(v) {
                    self.corner_traversal_stack.push(
                        self.corner_table.get_right_corner(curr_corner).unwrap() // It is guaranteed to exist because the current corner is unvisited and not on a boundary
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
                    // check whether the left or right face is a handle.
                    for i in (0..self.corner_traversal_stack.len()).rev() {
                        let c = self.corner_traversal_stack[i];
                        if self.corner_table.face_idx_containing(c) == face_idx {
                            self.corner_traversal_stack.remove(i);
                        }
                    }
                } else {
                    // Left face is unvisited or does not exist. 
                    // check whether the left face is a handle.
                    for i in (0..self.corner_traversal_stack.len()).rev() {
                        let c = self.corner_traversal_stack[i];
                        if self.corner_table.face_idx_containing(c) == face_idx {
                            self.corner_traversal_stack.remove(i);
                            // ToDo: Consider adding break here
                        }
                    }
                    
                    // We need to traverse the left face if it exists.
                    if let Some(lc) = left_corner {
                        self.corner_traversal_stack.push(lc);
                    }
                }
            } else {
                // Right face is unvisited or does not exist.
                if left_face.is_some() && self.visited_faces[usize::from(left_face.unwrap())] {
                    // Left face is visited.
                    // check whether the left face is a handle.
                    for i in (0..self.corner_traversal_stack.len()).rev() {
                        let c = self.corner_traversal_stack[i];
                        if self.corner_table.face_idx_containing(c) == face_idx {
                            self.corner_traversal_stack.remove(i);
                            // ToDo: Consider adding break here
                        }
                    }

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
    use crate::encode::connectivity::ConnectivityEncoderOutput;
    use crate::{encode::connectivity::encode_connectivity, io::obj::load_obj};
    use crate::core::shared::ConfigType;

    #[test]
    fn test_traverser() {
        let mut mesh = load_obj("tests/data/tetrahedron.obj").unwrap();
        let out: crate::encode::connectivity::ConnectivityEncoderOutput<'_> = encode_connectivity(
            &mesh.faces, 
            &mut mesh.attributes, 
            &mut Vec::new(), 
            &crate::encode::Config::default()
        ).unwrap();

        let (ct, corners) = if let ConnectivityEncoderOutput::Edgebreaker(edgebreaker_out) = out {
            (edgebreaker_out.corner_table, edgebreaker_out.corners_of_edgebreaker)
        } else {
            panic!("Expected Edgebreaker Output");
        };

        let ct_pos = ct.universal_corner_table();
        let sequence_points = Traverser::new(
            ct_pos,
            corners.clone(),
        ).compute_seqeunce().iter().map(|c| ct_pos.point_idx(*c)).collect::<Vec<_>>();
        assert_eq!(
            sequence_points.into_iter().map(|c| usize::from(c)).collect::<Vec<_>>(), 
            vec![3,1,0,2]
        );

        let ct_nor = &ct.attribute_corner_table(1).unwrap();
        let sequence_normals = Traverser::new(
            ct_nor,
            corners.clone(),
        ).compute_seqeunce().iter().map(|c| ct_nor.point_idx(*c)).collect::<Vec<_>>();
        assert_eq!(
            sequence_normals.into_iter().map(|c| usize::from(c)).collect::<Vec<_>>(), 
            vec![3,1,0,2]
        );

        let ct_tex = &ct.attribute_corner_table(2).unwrap();
        let sequence_tex_coords = Traverser::new(
            ct_tex,
            corners,
        ).compute_seqeunce().iter().map(|c| ct_tex.point_idx(*c)).collect::<Vec<_>>();
        assert_eq!(
            sequence_tex_coords.into_iter().map(|c| usize::from(c)).collect::<Vec<_>>(), 
            vec![3,1,0,2,5,4]
        );
    }
}
