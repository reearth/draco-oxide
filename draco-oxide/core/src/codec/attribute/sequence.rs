use crate::corner_table::GenericCornerTable;
use crate::types::{CornerIdx, VertexIdx};

#[derive(Debug, Clone)]
pub struct Traverser<'ct, CornerTableType>
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
    pub fn new(corner_table: &'ct T, corners_of_edgebreaker_traversal: Vec<CornerIdx>) -> Self {
        Self {
            visited_vertices: vec![false; corner_table.num_vertices()],
            visited_faces: vec![false; corner_table.num_faces()],
            corner_table,
            corner_traversal_stack: corners_of_edgebreaker_traversal, // The last encoded connected component gets decoded first
            out: Vec::with_capacity(corner_table.num_corners()),
        }
    }

    pub fn is_vertex_visited(&self, v: VertexIdx) -> bool {
        self.visited_vertices[usize::from(v)]
    }

    pub fn visit(&mut self, v: VertexIdx, c: CornerIdx) {
        if !self.visited_vertices[usize::from(v)] {
            self.out.push(c);
        }
        self.visited_vertices[usize::from(v)] = true;
    }

    pub fn compute_seqeunce(mut self) -> Vec<CornerIdx> {
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
