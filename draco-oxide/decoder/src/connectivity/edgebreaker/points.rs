//! Point-id assignment and per-attribute vertex maps, derived from a single
//! walk over the position fans shared by every attribute.

use draco_oxide_core::mesh::ds::{CornerTable, GenericCornerTable};
use draco_oxide_core::types::{CornerIdx, PointIdx, VecCornerIdx};

/// The seam-separated sectors of the position fans for one seam set: per corner
/// its vertex id, and per vertex its left-most corner. Vertices mirror the
/// encoder's attribute vertex construction, so both sides agree on which
/// corners share an attribute value.
pub(crate) struct FanVertices {
    pub corner_to_vertex: Vec<usize>,
    pub vertex_to_left_most_corner: Vec<CornerIdx>,
}

impl FanVertices {
    fn with_capacity(num_corners: usize) -> Self {
        Self {
            corner_to_vertex: vec![usize::MAX; num_corners],
            vertex_to_left_most_corner: Vec::new(),
        }
    }
}

/// Splits every position fan into the sectors separated by each seam set, all
/// from one topology walk per fan. `seam_sets[k]` is the per-corner
/// `is_edge_on_seam` flag of output `k`; the returned vector is parallel to it.
///
/// Each fan is walked once through the corner table (left-most search plus one
/// right sweep, buffering the corner sequence); every output's sectors and
/// vertex numbering are then derived from the buffer with plain array reads,
/// reproducing the per-attribute walk order exactly: sectors are numbered from
/// the sector-left-most corner rightward, and on a seamless closed fan the
/// start corner is the right neighbor of the position-left-most corner.
pub(crate) fn fan_vertices(
    pos_ct: &CornerTable,
    seam_sets: &[&[bool]],
    num_corners: usize,
) -> Vec<FanVertices> {
    let mut outputs: Vec<FanVertices> = seam_sets
        .iter()
        .map(|_| FanVertices::with_capacity(num_corners))
        .collect();

    let mut visited = vec![false; num_corners];
    // Scratch buffers reused across fans: the fan's corners in right-sweep
    // order from the position-left-most corner, and per corner the edge index
    // consulted for a seam crossing (the edge swung across to reach it).
    let mut fan: Vec<CornerIdx> = Vec::new();
    let mut crossed_edge: Vec<usize> = Vec::new();

    for start in 0..num_corners {
        if visited[start] {
            continue;
        }
        let start = CornerIdx::from(start);

        // Walk to the position-fan left-most corner (boundary or full circle).
        let mut pos_left_most = start;
        let mut closed = false;
        while let Some(l) = pos_ct.swing_left(pos_left_most) {
            if l == start {
                closed = true;
                break;
            }
            pos_left_most = l;
        }

        // Sweep right once, buffering the whole fan.
        fan.clear();
        crossed_edge.clear();
        fan.push(pos_left_most);
        visited[usize::from(pos_left_most)] = true;
        let mut prev = pos_left_most;
        while let Some(curr) = pos_ct.swing_right(prev) {
            if curr == pos_left_most {
                break;
            }
            fan.push(curr);
            visited[usize::from(curr)] = true;
            prev = curr;
        }
        for &c in fan.iter() {
            crossed_edge.push(usize::from(c.next()));
        }
        let m = fan.len();

        for (seams, out) in seam_sets.iter().zip(outputs.iter_mut()) {
            // The sector-left-most corner: on an open fan the position-left-most
            // corner itself; on a closed fan, walk left until a seam or until the
            // next step would complete the circle.
            let start_idx = if !closed {
                0
            } else {
                let mut i = 0;
                loop {
                    if seams[crossed_edge[i]] {
                        break;
                    }
                    let l = (i + m - 1) % m;
                    if l == 0 {
                        break;
                    }
                    i = l;
                }
                i
            };

            let mut vert = out.vertex_to_left_most_corner.len();
            out.vertex_to_left_most_corner.push(fan[start_idx]);
            out.corner_to_vertex[usize::from(fan[start_idx])] = vert;
            for j in 1..m {
                let idx = (start_idx + j) % m;
                let curr = fan[idx];
                if seams[crossed_edge[idx]] {
                    vert = out.vertex_to_left_most_corner.len();
                    out.vertex_to_left_most_corner.push(curr);
                }
                out.corner_to_vertex[usize::from(curr)] = vert;
            }
        }
    }

    outputs
}

/// Assigns a point id to every corner, given the vertex map computed for the
/// union of every attribute's seams. Points are the finest common refinement of
/// all attributes' seam sectors over each position fan (the decoder-side
/// equivalent of Google's `AssignPointsToCorners`): corners of one fan share a
/// point exactly when no attribute separates them by a seam.
pub(crate) fn assign_points(union_fan_vertices: &FanVertices) -> VecCornerIdx<PointIdx> {
    union_fan_vertices
        .corner_to_vertex
        .iter()
        .map(|&v| PointIdx::from(v))
        .collect::<Vec<_>>()
        .into()
}
