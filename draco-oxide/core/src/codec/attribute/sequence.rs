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


/// Computes the attribute traversal order for the default
/// `MESH_TRAVERSAL_DEPTH_FIRST` method and returns the per-data-value corner
/// map (`out[p]` = corner at which the `p`-th value's vertex was first
/// visited). Equivalent to [`Traverser::compute_sequence`] but a direct,
/// **O(faces)** port of Google's `DepthFirstTraverser` driven by
/// `MeshTraversalSequencer` (seed `traverse_from_corner(3*i)` for every face
/// in order). The older `Traverser` does the same walk but rescans the whole
/// traversal stack per face for handle cleanup, which is O(faces²); this is
/// the hot path for large meshes, so we use the linear version everywhere in
/// the decoder.
pub fn compute_sequence_depth_first<T>(ct: &T) -> Vec<CornerIdx>
where
    T: GenericCornerTable,
{
    let num_faces = ct.num_faces();
    let num_vertices = ct.num_vertices();
    let mut out: Vec<CornerIdx> = Vec::with_capacity(num_vertices);
    if num_faces == 0 {
        return out;
    }

    let mut is_face_visited = vec![false; num_faces];
    let mut is_vertex_visited = vec![false; num_vertices];
    let mut stack: Vec<CornerIdx> = Vec::new();
    let mut num_visited_faces = 0usize;

    let face_of = |c: CornerIdx| usize::from(c) / 3;

    for seed in 0..num_faces {
        if num_visited_faces >= num_faces {
            break;
        }
        let start = CornerIdx::from(3 * seed);
        if is_face_visited[face_of(start)] {
            continue;
        }

        stack.clear();
        stack.push(start);

        // For the seed face the other two corners may not be processed yet.
        let next_c = ct.next(start);
        let prev_c = ct.previous(start);
        let nv = usize::from(ct.vertex_idx(next_c));
        let pv = usize::from(ct.vertex_idx(prev_c));
        if !is_vertex_visited[nv] {
            is_vertex_visited[nv] = true;
            out.push(next_c);
        }
        if !is_vertex_visited[pv] {
            is_vertex_visited[pv] = true;
            out.push(prev_c);
        }

        while let Some(&top) = stack.last() {
            let mut corner_id = top;
            if is_face_visited[face_of(corner_id)] {
                stack.pop();
                continue;
            }

            loop {
                is_face_visited[face_of(corner_id)] = true;
                num_visited_faces += 1;

                let vid = usize::from(ct.vertex_idx(corner_id));
                if !is_vertex_visited[vid] {
                    let on_boundary = ct.is_on_boundary(ct.vertex_idx(corner_id));
                    is_vertex_visited[vid] = true;
                    out.push(corner_id);
                    if !on_boundary {
                        // Interior vertex: walk straight to the right corner.
                        corner_id = ct.opposite(ct.next(corner_id)).unwrap();
                        continue;
                    }
                }

                // Current vertex already visited, or on a boundary.
                let right = ct.opposite(ct.next(corner_id));
                let left = ct.opposite(ct.previous(corner_id));
                let right_visited = right.is_none_or(|c| is_face_visited[face_of(c)]);
                let left_visited = left.is_none_or(|c| is_face_visited[face_of(c)]);

                if right_visited {
                    if left_visited {
                        stack.pop();
                        break;
                    }
                    corner_id = left.unwrap();
                } else if left_visited {
                    corner_id = right.unwrap();
                } else {
                    // Both neighbours unvisited: continue left now, resume right later.
                    *stack.last_mut().unwrap() = left.unwrap();
                    stack.push(right.unwrap());
                    break;
                }
            }
        }
    }

    out
}

/// Number of priority buckets used by the max-prediction-degree traversal.
const MPD_MAX_PRIORITY: usize = 3;

/// Priority of traversing the edge that lands on `corner`'s tip vertex, and
/// (as a side effect) bumps that vertex's accumulated prediction degree.
/// Mirrors `MaxPredictionDegreeTraverser::ComputePriority`: an already-visited
/// tip gives priority 0; otherwise the degree is incremented and the priority
/// is 1 when the degree is now > 1, else 2.
fn mpd_compute_priority<T: GenericCornerTable>(
    ct: &T,
    corner: CornerIdx,
    is_vertex_visited: &[bool],
    prediction_degree: &mut [i32],
) -> usize {
    let v_tip = usize::from(ct.vertex_idx(corner));
    let mut priority = 0usize;
    if !is_vertex_visited[v_tip] {
        prediction_degree[v_tip] += 1;
        priority = if prediction_degree[v_tip] > 1 { 1 } else { 2 };
    }
    if priority >= MPD_MAX_PRIORITY {
        priority = MPD_MAX_PRIORITY - 1;
    }
    priority
}

/// Computes the attribute traversal order for the
/// `MESH_TRAVERSAL_PREDICTION_DEGREE` method (used by Draco at higher
/// compression levels — e.g. positions encoded with constrained-multi-
/// parallelogram). Returns the per-data-value corner map: `out[p]` is the
/// corner at which the `p`-th attribute value's vertex was first visited,
/// i.e. Google's `encoded_attribute_value_index_to_corner_map`.
///
/// Direct port of `compression/mesh/traverser/max_prediction_degree_traverser.h`
/// driven by `MeshTraversalSequencer` (which seeds `traverse_from_corner(3*i)`
/// for every face in order until all faces are visited).
pub fn compute_sequence_max_prediction_degree<T>(ct: &T) -> Vec<CornerIdx>
where
    T: GenericCornerTable,
{
    let num_faces = ct.num_faces();
    let num_vertices = ct.num_vertices();
    let mut out: Vec<CornerIdx> = Vec::with_capacity(num_vertices);
    if num_vertices == 0 {
        return out;
    }

    let mut is_face_visited = vec![false; num_faces];
    let mut is_vertex_visited = vec![false; num_vertices];
    let mut prediction_degree = vec![0i32; num_vertices];
    let mut stacks: [Vec<CornerIdx>; MPD_MAX_PRIORITY] = Default::default();
    let mut num_visited_faces = 0usize;

    let face_of = |c: CornerIdx| usize::from(c) / 3;

    for i in 0..num_faces {
        if num_visited_faces >= num_faces {
            break;
        }
        let start = CornerIdx::from(3 * i);

        // Seed: stage the start corner and visit the first face's three
        // vertices (next, prev, tip — in that order) up front.
        stacks[0].push(start);
        let mut best_priority = 0usize;
        let first_next = ct.next(start);
        let first_prev = ct.previous(start);
        for (corner, vtx) in [
            (first_next, usize::from(ct.vertex_idx(first_next))),
            (first_prev, usize::from(ct.vertex_idx(first_prev))),
            (start, usize::from(ct.vertex_idx(start))),
        ] {
            if !is_vertex_visited[vtx] {
                is_vertex_visited[vtx] = true;
                out.push(corner);
            }
        }

        // Drain the priority buckets.
        loop {
            // Pop the highest-priority (lowest bucket index) available corner.
            let mut popped = None;
            let mut p = best_priority;
            while p < MPD_MAX_PRIORITY {
                if let Some(c) = stacks[p].pop() {
                    best_priority = p;
                    popped = Some(c);
                    break;
                }
                p += 1;
            }
            let Some(mut corner_id) = popped else { break };
            if is_face_visited[face_of(corner_id)] {
                continue;
            }

            loop {
                is_face_visited[face_of(corner_id)] = true;
                num_visited_faces += 1;

                let vid = usize::from(ct.vertex_idx(corner_id));
                if !is_vertex_visited[vid] {
                    is_vertex_visited[vid] = true;
                    out.push(corner_id);
                }

                // right = opposite(next(c)); left = opposite(prev(c)).
                let right = ct.opposite(ct.next(corner_id));
                let left = ct.opposite(ct.previous(corner_id));
                let right_visited = right.is_none_or(|rc| is_face_visited[face_of(rc)]);
                let left_visited = left.is_none_or(|lc| is_face_visited[face_of(lc)]);

                let mut advanced = false;
                if !left_visited {
                    let lc = left.unwrap();
                    let priority =
                        mpd_compute_priority(ct, lc, &is_vertex_visited, &mut prediction_degree);
                    if right_visited && priority <= best_priority {
                        corner_id = lc;
                        advanced = true;
                    } else {
                        stacks[priority].push(lc);
                        if priority < best_priority {
                            best_priority = priority;
                        }
                    }
                }
                if !advanced && !right_visited {
                    let rc = right.unwrap();
                    let priority =
                        mpd_compute_priority(ct, rc, &is_vertex_visited, &mut prediction_degree);
                    if priority <= best_priority {
                        corner_id = rc;
                        advanced = true;
                    } else {
                        stacks[priority].push(rc);
                        if priority < best_priority {
                            best_priority = priority;
                        }
                    }
                }

                if !advanced {
                    break;
                }
            }
        }
    }

    out
}
