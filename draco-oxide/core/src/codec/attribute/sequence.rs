use crate::mesh::ds::{AttributeDS, GenericCornerTable};
use crate::types::{CornerIdx, VecFaceIdx, VecVertexIdx, VertexIdx};

#[derive(Debug, Clone)]
pub struct Traverser<'a> {
    ads: &'a AttributeDS<'a>,
    visited_vertices: VecVertexIdx<bool>,
    visited_faces: VecFaceIdx<bool>,
    corner_traversal_stack: Vec<CornerIdx>,
    out: Vec<CornerIdx>,
}

impl<'a> Traverser<'a> {
    /// Creates a new `Traverser` instance.
    /// # Arguments
    /// * `ads` - A reference to the attribute data structure to traverse.
    /// * `corners_of_edgebreaker_traversal` - A vector of corner indices
    ///   representing the last-encoded corners for connected components in encoded order.
    pub fn new(ads: &'a AttributeDS, corners_of_edgebreaker_traversal: Vec<CornerIdx>) -> Self {
        let num_faces = ads.global_ds().num_faces();
        Self {
            visited_vertices: vec![false; ads.num_vertices()].into(),
            visited_faces: vec![false; num_faces].into(),
            ads,
            corner_traversal_stack: corners_of_edgebreaker_traversal, // The last encoded connected component gets decoded first
            out: Vec::with_capacity(num_faces * 3),
        }
    }

    #[inline]
    fn is_vertex_visited(&self, v: VertexIdx) -> bool {
        unsafe { *self.visited_vertices.get_unchecked(v) }
    }

    #[inline]
    pub fn visit(&mut self, v: VertexIdx, c: CornerIdx) {
        if !self.is_vertex_visited(v) {
            self.out.push(c);
        }
        unsafe {
            *self.visited_vertices.get_unchecked_mut(v) = true;
        }
    }

    pub fn compute_seqeunce(mut self) -> Vec<CornerIdx> {
        while let Some(curr_corner) = self.corner_traversal_stack.pop() {
            // If the face has not yet been visited, then the
            // other vertices of the face are not visited yet either. If this is the case, then
            // we need to store them in self.next_outputs_stack so that they will get processed first.
            let face_idx = curr_corner.face_idx();
            if unsafe { *self.visited_faces.get_unchecked(face_idx) } {
                continue;
            }
            let v = self.ads.vertex_idx(curr_corner);
            let next_c = curr_corner.next_with_face_idx(face_idx);
            let next_v = self.ads.vertex_idx(next_c);
            let prev_c = curr_corner.previous_with_face_idx(face_idx);
            let prev_v = self.ads.vertex_idx(prev_c);
            if !self.is_vertex_visited(next_v) || !self.is_vertex_visited(prev_v) {
                // We need to return the next corner first, then the previous corner, and finally the current corner.
                // This order is determined by the draco library.
                self.visit(next_v, next_c);
                self.visit(prev_v, prev_c);
                self.corner_traversal_stack.push(curr_corner);
                continue;
            }

            // Coming here means that we are visiting a new face.
            unsafe {
                *self.visited_faces.get_unchecked_mut(face_idx) = true;
            }
            // Once a face is marked visited it is never unmarked, and the pop
            // loop above skips any corner whose face is already visited. So stale
            // corners of this face still left on the stack (the handle case) are
            // harmlessly skipped when popped; we no longer scan-and-remove them.

            // If we have not yet visited the vertex of the current corner and if it is not on a boundary then we can simply return it.
            if !self.is_vertex_visited(v) {
                self.visit(v, curr_corner);
                if !self.ads.is_on_boundary(v) {
                    self.corner_traversal_stack.push(
                        self.ads
                            .corner_table()
                            .get_right_corner_with_face_idx(curr_corner, face_idx)
                            .unwrap(), // It is guaranteed to exist because the current corner is unvisited and not on a boundary
                    );
                    continue;
                }
            }

            self.visit(v, curr_corner);

            let right_corner = self
                .ads
                .corner_table()
                .get_right_corner_with_face_idx(curr_corner, face_idx);
            let left_corner = self
                .ads
                .corner_table()
                .get_left_corner_with_face_idx(curr_corner, face_idx);
            let right_face = right_corner.map(|c| c.face_idx());
            let left_face = left_corner.map(|c| c.face_idx());

            if right_face.is_some()
                && unsafe { *self.visited_faces.get_unchecked(right_face.unwrap()) }
            {
                // Right face has been visited
                if left_face.is_some()
                    && unsafe { *self.visited_faces.get_unchecked(left_face.unwrap()) }
                {
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
                if left_face.is_some()
                    && unsafe { *self.visited_faces.get_unchecked(left_face.unwrap()) }
                {
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
