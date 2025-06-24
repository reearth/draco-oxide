use crate::core::{corner_table::GenericCornerTable, shared::{CornerIdx, VertexIdx}};

#[derive(Debug, Clone)]
pub(crate) struct Traverser<'ct, CornerTableType> 
    where CornerTableType: GenericCornerTable
{
    corner_table: &'ct CornerTableType,
    visited_vertices: Vec<bool>,
    visited_faces: Vec<bool>,
    corner_traversal_stack: Vec<CornerIdx>,
    next_outputs_stack: Vec<VertexIdx>,
}

impl<'ct, T> Traverser<'ct, T> 
    where T: GenericCornerTable
{
    /// Creates a new `Traverser` instance.
    /// # Arguments
    /// * `corner_table` - A reference to the corner table to traverse.
    /// * `first_corners_for_connected_components_encoded_order` - A vector of corner indices
    ///   representing the last-encoded corners for connected components in encoded order.
    pub(crate) fn new(
        corner_table: &'ct T,
        first_corners_for_connected_components_encoded_order: Vec<CornerIdx>
    ) -> Self {
        Self {
            visited_vertices: vec![false; corner_table.num_vertices()],
            visited_faces: vec![false; corner_table.num_faces()],
            corner_table,
            corner_traversal_stack: first_corners_for_connected_components_encoded_order, // The last encoded connected component gets decoded first
            next_outputs_stack: Vec::new(),
        }
    }
}

impl<'ct, T> Iterator for Traverser<'ct, T> 
    where T: GenericCornerTable
{
    type Item = CornerIdx;
    fn next(&mut self) -> Option<CornerIdx> {
        // corners in the next_outputs_stack must be processed first, if any.
        if let Some(c) = self.next_outputs_stack.pop() {
            return Some(c);
        }

        let curr_corner = if let Some(c) = self.corner_traversal_stack.pop() {
            c
        } else {
            return None;  // No more corners to traverse.
        };

        // If the face has not yet been visited, then the 
        // other vertices of the face are not visited yet either. If this is the case, then
        // we need to store them in self.next_outputs_stack so that they will get processed first.
        let next_c = self.corner_table.next(curr_corner);
        let next_v = self.corner_table.vertex_idx(next_c);
        let prev_c = self.corner_table.previous(curr_corner);
        let prev_v = self.corner_table.vertex_idx(prev_c);
        if !self.visited_vertices[next_v] {
            debug_assert!(!self.visited_vertices[prev_v], "Previous vertex {} has already been visited, but it should not have.", prev_v);
            self.visited_vertices[next_v] = true;
            self.visited_vertices[prev_v] = true;
            // We need to return the next corners first, then the previous vertex, and finally the current corner.
            // This order is determined by the draco library.
            self.corner_traversal_stack.push(curr_corner);
            self.next_outputs_stack.push(prev_c);
            return Some(next_c);
        }

        let v = self.corner_table.vertex_idx(curr_corner);

        // Coming here means that we are visiting a new face.
        let face_idx = self.corner_table.face_idx_containing(curr_corner);
        // debug_assert!(!self.visited_faces[face_idx], "Face {} has already been visited, but it should not have. was visiting corner: {:?}, vertex: {}", face_idx, curr_corner, v);
        self.visited_faces[face_idx] = true;

        // If we have not yet visited the vertex of the current corner and if it is not on a boundary then we can simply return it.
        let mut on_boundary = false;
        if !self.visited_vertices[v] {
            self.visited_vertices[v] = true;
            if !self.corner_table.is_on_boundary(v) {
                self.corner_traversal_stack.push(
                    self.corner_table.get_right_corner(curr_corner).unwrap() // It is guaranteed to exist because the current corner is unvisited and not on a boundary
                );
                return Some(curr_corner);
            }
            on_boundary = true;
        }

        let right_corner = self.corner_table.get_right_corner(curr_corner);
        let left_corner = self.corner_table.get_left_corner(curr_corner);
        let right_face = right_corner.map(|c| self.corner_table.face_idx_containing(c));
        let left_face = left_corner.map(|c| self.corner_table.face_idx_containing(c));

        if right_face.is_some() && self.visited_faces[right_face.unwrap()] {
            // Right face has been visited
            if left_face.is_some() && self.visited_faces[left_face.unwrap()] {
                // Both neighboring faces are visited, we can continue traversing. No update to the stack.
                // check whether the left or right face is a handle.
                for i in (0..self.corner_traversal_stack.len()).rev() {
                    let c = self.corner_traversal_stack[i];
                    if self.corner_table.face_idx_containing(c) == face_idx {
                        self.corner_traversal_stack.remove(i);
                    }
                }
                self.next()
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
                self.next()
            }
        } else {
            // Right face is unvisited or does not exist.
            if left_face.is_some() && self.visited_faces[left_face.unwrap()] {
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
                self.next()
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

                if on_boundary {
                    // If the current corner is on a boundary, we need to return it.
                    Some(curr_corner)
                } else {
                    // Otherwise, we can continue traversing.
                    self.next()
                }
            }
        }
    }
}