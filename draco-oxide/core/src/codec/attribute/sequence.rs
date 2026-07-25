use crate::mesh::ds::{GenericAttributeDs, GenericCornerTable};
use crate::types::{CornerIdx, FaceIdx, VecFaceIdx, VecVertexIdx, VertexIdx};

#[derive(Debug, Clone)]
pub struct Traverser<'a, D: GenericAttributeDs> {
    ads: &'a D,
    visited_vertices: VecVertexIdx<bool>,
    visited_faces: VecFaceIdx<bool>,
    corner_traversal_stack: Vec<CornerIdx>,
    out: Vec<CornerIdx>,
    /// The second corner emitted by a single traversal step, buffered for the
    /// next `Iterator::next` call. Only a start-of-component step emits two
    /// corners; every other step emits at most one.
    pending: Option<CornerIdx>,
}

impl<'a, D: GenericAttributeDs> Traverser<'a, D> {
    /// Creates a new `Traverser` instance.
    /// # Arguments
    /// * `ads` - A reference to the attribute data structure to traverse.
    /// * `corners_of_edgebreaker_traversal` - A vector of corner indices
    ///   representing the last-encoded corners for connected components in encoded order.
    pub fn new(ads: &'a D, corners_of_edgebreaker_traversal: Vec<CornerIdx>) -> Self {
        let num_faces = ads.num_faces();
        Self {
            visited_vertices: vec![false; ads.vertex_index_bound()].into(),
            visited_faces: vec![false; num_faces].into(),
            ads,
            corner_traversal_stack: corners_of_edgebreaker_traversal, // The last encoded connected component gets decoded first
            out: Vec::new(),
            pending: None,
        }
    }

    #[inline]
    fn is_vertex_visited(&self, v: VertexIdx) -> bool {
        unsafe { *self.visited_vertices.get_unchecked(v) }
    }

    /// Records `c` as the first corner reaching vertex `v`.
    #[inline]
    fn visit(&mut self, v: VertexIdx, c: CornerIdx) {
        if !self.is_vertex_visited(v) {
            self.out.push(c);
        }
        unsafe {
            *self.visited_vertices.get_unchecked_mut(v) = true;
        }
    }

    /// Traverses the mesh, filling `out` with the attribute sequence.
    fn drive(&mut self) {
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
            self.push_fan_neighbors(curr_corner, face_idx);
        }
    }

    /// Pushes the neighbouring corners of `curr_corner`'s face that still need
    /// traversing, in the order the draco reference walks them: the right corner
    /// is traversed before the left, so it is pushed last.
    #[inline]
    fn push_fan_neighbors(&mut self, curr_corner: CornerIdx, face_idx: FaceIdx) {
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

        if right_face.is_some() && unsafe { *self.visited_faces.get_unchecked(right_face.unwrap()) }
        {
            if left_face.is_some()
                && unsafe { *self.visited_faces.get_unchecked(left_face.unwrap()) }
            {
                // Both neighboring faces are visited, we can continue traversing. No update to the stack.
            } else if let Some(lc) = left_corner {
                self.corner_traversal_stack.push(lc);
            }
        } else if left_face.is_some()
            && unsafe { *self.visited_faces.get_unchecked(left_face.unwrap()) }
        {
            if let Some(rc) = right_corner {
                self.corner_traversal_stack.push(rc);
            }
        } else {
            if let Some(lc) = left_corner {
                self.corner_traversal_stack.push(lc);
            }
            if let Some(rc) = right_corner {
                self.corner_traversal_stack.push(rc);
            }
        }
    }

    /// Marks `v` visited, returning `c` the first time `v` is reached and `None`
    /// on any later visit. The lazy [`Iterator`] emits exactly the returned
    /// corners, in the same order as [`Self::drive`] fills `out`.
    #[inline]
    fn emit_if_unvisited(&mut self, v: VertexIdx, c: CornerIdx) -> Option<CornerIdx> {
        if self.is_vertex_visited(v) {
            None
        } else {
            unsafe {
                *self.visited_vertices.get_unchecked_mut(v) = true;
            }
            Some(c)
        }
    }

    /// Computes the attribute traversal sequence.
    pub fn compute_seqeunce(mut self) -> Vec<CornerIdx> {
        self.out.reserve(self.ads.vertex_index_bound());
        self.drive();
        self.out
    }
}

/// Yields the attribute traversal sequence one corner at a time, in the same
/// order as [`Traverser::compute_seqeunce`], letting a consumer fuse its own
/// per-corner work into the walk without materializing the sequence.
impl<D: GenericAttributeDs> Iterator for Traverser<'_, D> {
    type Item = CornerIdx;

    fn next(&mut self) -> Option<CornerIdx> {
        if let Some(c) = self.pending.take() {
            return Some(c);
        }
        while let Some(curr_corner) = self.corner_traversal_stack.pop() {
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
                // Start of a component fan: emit the next and previous corners
                // (in that order) and re-push the current corner for later.
                self.corner_traversal_stack.push(curr_corner);
                let first = self.emit_if_unvisited(next_v, next_c);
                let second = self.emit_if_unvisited(prev_v, prev_c);
                match (first, second) {
                    (Some(a), Some(b)) => {
                        self.pending = Some(b);
                        return Some(a);
                    }
                    (Some(a), None) | (None, Some(a)) => return Some(a),
                    // The branch condition guarantees at least one emit.
                    (None, None) => continue,
                }
            }

            unsafe {
                *self.visited_faces.get_unchecked_mut(face_idx) = true;
            }

            let emitted = if !self.is_vertex_visited(v) {
                let e = self.emit_if_unvisited(v, curr_corner);
                if !self.ads.is_on_boundary(v) {
                    self.corner_traversal_stack.push(
                        self.ads
                            .corner_table()
                            .get_right_corner_with_face_idx(curr_corner, face_idx)
                            .unwrap(), // Guaranteed to exist: unvisited, non-boundary vertex.
                    );
                    return e;
                }
                e
            } else {
                None
            };

            self.push_fan_neighbors(curr_corner, face_idx);

            if let Some(c) = emitted {
                return Some(c);
            }
        }
        None
    }
}
