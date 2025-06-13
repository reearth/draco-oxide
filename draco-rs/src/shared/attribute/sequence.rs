use crate::core::{corner_table::GenericCornerTable, shared::{CornerIdx, VertexIdx}};

pub(crate) trait Sequencer {
    fn next(&mut self) -> Option<VertexIdx>;
}

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
    ///   representing the first corners for connected components in encoded order.
    fn new(
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

impl<'ct, T> Sequencer for Traverser<'ct, T> 
    where T: GenericCornerTable
{
    fn next(&mut self) -> Option<VertexIdx> {
        // vertices in the next_outputs_stack must be processed first, if any.
        if let Some(v) = self.next_outputs_stack.pop() {
            return Some(v);
        }

        let curr_corner = if let Some(c) = self.corner_traversal_stack.pop() {
            c
        } else {
            return None;  // No more corners to traverse.
        };

        // Coming here means that we are visiting a new face.
        let face_idx = self.corner_table.face_idx_containing(curr_corner);
        debug_assert!(self.visited_faces[face_idx] == false, "Face {} has already been visited, but it should not have.", face_idx);
        self.visited_faces[face_idx] = true;

        // If the face is have not yet been visited, then the 
        // other vertices of the face are not visited yet. If this is the case, then
        // we need to store them in self.next_outputs_stack so that they will get processed first.
        let next_v = self.corner_table.vertex_idx(self.corner_table.next(curr_corner));
        let prev_v = self.corner_table.vertex_idx(self.corner_table.previous(curr_corner));
        if !self.visited_vertices[next_v] {
            self.visited_vertices[next_v] = true;
            self.next_outputs_stack.push(next_v);
        }
        if !self.visited_vertices[prev_v] {
            self.visited_vertices[prev_v] = true;
            self.next_outputs_stack.push(prev_v);
        }
        if !self.next_outputs_stack.is_empty() {
            return Some(curr_corner);
        }

        // If we have not yet visited the vertex of the current corner and if it is not on a boundary then we can simply return it.
        let v = self.corner_table.vertex_idx(curr_corner);
        if !self.visited_vertices[v] {
            self.visited_vertices[v] = true;
            if !self.corner_table.is_on_boundary(v) {
                self.corner_traversal_stack.push(
                    self.corner_table.get_right_corner(curr_corner).unwrap() // It is guaranteed to exist because current corner is unvisited and not on a boundary
                );
                return Some(v);
            }
        }

        let right_corner = self.corner_table.get_right_corner(curr_corner);
        let left_corner = self.corner_table.get_left_corner(curr_corner);
        let right_face = right_corner.map(|c| self.corner_table.face_idx_containing(c));
        let left_face = left_corner.map(|c| self.corner_table.face_idx_containing(c));

        if right_face.is_some() && self.visited_faces[right_face.unwrap()] {
            // Right face has been visited
            if left_face.is_some() && self.visited_faces[left_face.unwrap()] {
                // Both neighboring faces are visited, we can continue traversing. No update to the stack.
                return Some(curr_corner);
            } else {
                // Left face is unvisited, we need to traverse it.
                self.corner_traversal_stack.push(left_corner.unwrap());
                return Some(curr_corner);
            }
        } else {
            // Right face is unvisited or does not exist.
            if left_face.is_some() && self.visited_faces[left_face.unwrap()] {
                // Left face is visited, we need to traverse the right one.
                self.corner_traversal_stack.push(right_corner.unwrap());
                return Some(curr_corner);
            } else {
                // Both neighboring faces are unvisited, we need to traverse both.
                // The right corner must be traversed first.
                self.corner_traversal_stack.push(left_corner.unwrap());
                self.corner_traversal_stack.push(right_corner.unwrap());
                return Some(curr_corner);
            }
        }
    }
}
