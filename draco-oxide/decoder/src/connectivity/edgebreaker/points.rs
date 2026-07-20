//! Point-id assignment and per-attribute vertex maps, derived from a single
//! walk over the position fans shared by every attribute.

use draco_oxide_core::mesh::ds::{CornerTable, GenericCornerTable};
use draco_oxide_core::types::{CornerIdx, PointIdx, VecCornerIdx, VertexIdx};

/// One attribute's sector decomposition of the position fans: per point (union
/// sector) its attribute vertex, and per vertex its left-most corner. Vertices
/// mirror the encoder's attribute vertex construction, so both sides agree on
/// which corners share an attribute value. Point-to-vertex is a function
/// because points are the finest common refinement of all attributes' sectors,
/// so every point lies inside exactly one sector of each attribute.
#[derive(Clone)]
pub(crate) struct FanVertices {
    pub point_to_vertex: Vec<VertexIdx>,
    pub vertex_to_left_most_corner: Vec<CornerIdx>,
}

impl FanVertices {
    fn new() -> Self {
        Self {
            point_to_vertex: Vec::new(),
            vertex_to_left_most_corner: Vec::new(),
        }
    }
}

/// The sector-left-most start index of one closed fan: the first seam reached
/// walking left from index 0, or index 1 when the fan carries no seam (the
/// right neighbor of the position-left-most corner, matching the encoder's
/// walk).
fn sector_start(is_seam: impl Fn(usize) -> bool, m: usize) -> usize {
    if m == 1 || is_seam(0) {
        return 0;
    }
    let mut j = m - 1;
    while j > 1 {
        if is_seam(j) {
            return j;
        }
        j -= 1;
    }
    1
}

/// Splits every position fan into points (the sectors of the union of all
/// seams, the decoder-side equivalent of Google's `AssignPointsToCorners`) and,
/// per attribute, into that attribute's seam-separated sectors. Returns the
/// corner-to-point map and, parallel to `seam_sets`, each attribute's
/// [`FanVertices`].
///
/// Each fan is walked once through the corner table (left-most search plus one
/// right sweep, buffering the corner sequence); the point assignment and every
/// attribute's vertex numbering are then derived from the buffer with plain
/// array reads, reproducing the per-attribute walk order exactly: sectors are
/// numbered from the sector-left-most corner rightward.
pub(crate) fn fan_vertices(
    pos_ct: &CornerTable,
    seam_sets: &[&[bool]],
    num_corners: usize,
) -> (VecCornerIdx<PointIdx>, Vec<FanVertices>) {
    let mut seamed_indices = seam_sets
        .iter()
        .enumerate()
        .filter(|(_, s)| s.iter().any(|&b| b))
        .map(|(i, _)| i);
    match (seamed_indices.next(), seamed_indices.next()) {
        (None, _) => {
            let (corner_to_point, fv) = fan_vertices_seamless(pos_ct, num_corners);
            (
                VecCornerIdx::from(corner_to_point),
                vec![fv; seam_sets.len()],
            )
        }
        (Some(k), None) => {
            let (corner_to_point, seamless, seamed) =
                fan_vertices_one_seamed(pos_ct, seam_sets[k], num_corners);
            let last_seamless = (0..seam_sets.len()).rev().find(|&i| i != k);
            let mut seamless = Some(seamless);
            let mut seamed = Some(seamed);
            let outputs = (0..seam_sets.len())
                .map(|i| {
                    if i == k {
                        seamed.take().unwrap()
                    } else if Some(i) == last_seamless {
                        seamless.take().unwrap()
                    } else {
                        seamless.clone().unwrap()
                    }
                })
                .collect();
            (VecCornerIdx::from(corner_to_point), outputs)
        }
        _ => fan_vertices_general(pos_ct, seam_sets, num_corners),
    }
}

/// Fast path when no attribute carries a seam: every fan is a single sector,
/// points coincide with position vertices, and all attributes share the same
/// vertex numbering.
fn fan_vertices_seamless(pos_ct: &CornerTable, num_corners: usize) -> (Vec<PointIdx>, FanVertices) {
    let mut corner_to_point = vec![PointIdx::INVALID; num_corners];
    let mut out = FanVertices::new();
    let mut visited = vec![false; num_corners];

    for start in 0..num_corners {
        if visited[start] {
            continue;
        }
        let start = CornerIdx::from(start);

        let mut pos_left_most = start;
        let mut closed = false;
        while let Some(l) = pos_ct.swing_left(pos_left_most) {
            if l == start {
                closed = true;
                break;
            }
            pos_left_most = l;
        }

        let pt = out.vertex_to_left_most_corner.len();
        corner_to_point[usize::from(pos_left_most)] = PointIdx::from(pt);
        visited[usize::from(pos_left_most)] = true;
        let mut prev = pos_left_most;
        while let Some(curr) = pos_ct.swing_right(prev) {
            if curr == pos_left_most {
                break;
            }
            corner_to_point[usize::from(curr)] = PointIdx::from(pt);
            visited[usize::from(curr)] = true;
            prev = curr;
        }

        // A closed seamless fan numbers its single sector from the right
        // neighbor of the position-left-most corner, matching the encoder.
        let left_most = if closed {
            pos_ct.swing_right(pos_left_most).unwrap_or(pos_left_most)
        } else {
            pos_left_most
        };
        out.vertex_to_left_most_corner.push(left_most);
        out.point_to_vertex.push(VertexIdx::from(pt));
    }

    (corner_to_point, out)
}

/// Fast path when exactly one attribute carries seams: the union of all seams
/// equals that attribute's seams, so points coincide with its vertices, and
/// every seamless attribute keeps one vertex per fan.
fn fan_vertices_one_seamed(
    pos_ct: &CornerTable,
    seams: &[bool],
    num_corners: usize,
) -> (Vec<PointIdx>, FanVertices, FanVertices) {
    let mut corner_to_point = vec![PointIdx::INVALID; num_corners];
    let mut seamless = FanVertices::new();
    let mut seamed = FanVertices::new();
    let mut num_points = 0usize;

    let mut visited = vec![false; num_corners];
    let mut fan: Vec<CornerIdx> = Vec::new();

    for start in 0..num_corners {
        if visited[start] {
            continue;
        }
        let start = CornerIdx::from(start);

        let mut pos_left_most = start;
        let mut closed = false;
        while let Some(l) = pos_ct.swing_left(pos_left_most) {
            if l == start {
                closed = true;
                break;
            }
            pos_left_most = l;
        }

        fan.clear();
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
        let m = fan.len();

        let s = if closed {
            sector_start(|j| seams[usize::from(fan[j].next())], m)
        } else {
            0
        };
        let fan_vert = VertexIdx::from(seamless.vertex_to_left_most_corner.len());
        let seamless_start = if closed && m > 1 { 1 } else { 0 };
        seamless
            .vertex_to_left_most_corner
            .push(fan[seamless_start]);

        // Fused pass: point ids equal the seamed attribute's vertex ids.
        for jj in 0..m {
            let idx = (s + jj) % m;
            if jj == 0 || seams[usize::from(fan[idx].next())] {
                seamed.vertex_to_left_most_corner.push(fan[idx]);
                seamed.point_to_vertex.push(VertexIdx::from(num_points));
                seamless.point_to_vertex.push(fan_vert);
                num_points += 1;
            }
            corner_to_point[usize::from(fan[idx])] = PointIdx::from(num_points - 1);
        }
    }

    (corner_to_point, seamless, seamed)
}

fn fan_vertices_general(
    pos_ct: &CornerTable,
    seam_sets: &[&[bool]],
    num_corners: usize,
) -> (VecCornerIdx<PointIdx>, Vec<FanVertices>) {
    let num_outputs = seam_sets.len();
    let mut outputs: Vec<FanVertices> = (0..num_outputs).map(|_| FanVertices::new()).collect();
    let mut corner_to_point = vec![PointIdx::INVALID; num_corners];
    let mut num_points = 0usize;

    let mut visited = vec![false; num_corners];
    // Scratch reused across fans: the fan's corners in right-sweep order from
    // the position-left-most corner; per corner the edge index consulted for a
    // seam crossing (the edge swung across to reach it); whether that edge is a
    // seam of any attribute (a point boundary); the fan-local point ids; and
    // each output's start index.
    let mut fan: Vec<CornerIdx> = Vec::new();
    let mut crossed_edge: Vec<usize> = Vec::new();
    let mut is_point_boundary: Vec<bool> = Vec::new();
    let mut point_of: Vec<usize> = Vec::new();
    let mut starts: Vec<usize> = vec![0; num_outputs];

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
        let m = fan.len();
        crossed_edge.clear();
        crossed_edge.extend(fan.iter().map(|&c| usize::from(c.next())));
        is_point_boundary.clear();
        is_point_boundary.extend(crossed_edge.iter().map(|&e| seam_sets.iter().any(|s| s[e])));

        // On an open fan every sector numbering starts at the
        // position-left-most corner itself.
        let union_start = if !closed {
            starts.iter_mut().for_each(|s| *s = 0);
            0
        } else {
            for (k, s) in starts.iter_mut().enumerate() {
                *s = sector_start(|j| seam_sets[k][crossed_edge[j]], m);
            }
            sector_start(|j| is_point_boundary[j], m)
        };

        // Union pass: assign point ids and the corner-to-point map.
        point_of.clear();
        point_of.resize(m, 0);
        let mut cur_pt = num_points;
        num_points += 1;
        for jj in 0..m {
            let idx = (union_start + jj) % m;
            if jj > 0 && is_point_boundary[idx] {
                cur_pt = num_points;
                num_points += 1;
            }
            point_of[idx] = cur_pt;
            corner_to_point[usize::from(fan[idx])] = PointIdx::from(cur_pt);
        }
        for out in outputs.iter_mut() {
            out.point_to_vertex.resize(num_points, VertexIdx::INVALID);
        }

        // Attribute passes: number this attribute's sectors from its own start
        // and record the vertex once per point (point boundaries subsume every
        // attribute's seams, so the vertex is constant between them).
        for (k, out) in outputs.iter_mut().enumerate() {
            let seams = seam_sets[k];
            let s = starts[k];
            let mut vert = VertexIdx::from(out.vertex_to_left_most_corner.len());
            out.vertex_to_left_most_corner.push(fan[s]);
            out.point_to_vertex[point_of[s]] = vert;
            for jj in 1..m {
                let idx = (s + jj) % m;
                if seams[crossed_edge[idx]] {
                    vert = VertexIdx::from(out.vertex_to_left_most_corner.len());
                    out.vertex_to_left_most_corner.push(fan[idx]);
                }
                if is_point_boundary[idx] {
                    out.point_to_vertex[point_of[idx]] = vert;
                }
            }
        }
    }

    (VecCornerIdx::from(corner_to_point), outputs)
}
