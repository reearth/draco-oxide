//! Corner-table reconstruction (spirale-reversi). Replays the symbol stream over an
//! active-corner stack (C/R/L/S/E), merging vertices on S, closing start-face fans,
//! and compacting isolated vertices, producing a `CornerTable` plus the
//! corner-to-vertex map. Ported from Google's `DecodeConnectivity` main loop.

use std::collections::HashMap;

use super::traversal::{TopologySplit, TraversalDecoder};
use crate::Err;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::Symbol;
use draco_oxide_core::types::{CornerIdx, VertexIdx};

/// The reconstructed connectivity: opposite/vertex corner maps and the derived data
/// the attribute stages need.
pub struct Reconstruction {
    /// Per-corner opposite corner (`CornerIdx::INVALID` on boundaries).
    pub opposite: Vec<CornerIdx>,
    /// Per-corner position vertex.
    pub corner_to_vertex: Vec<VertexIdx>,
    /// Left-most corner per position vertex (`CornerIdx::INVALID` for isolated
    /// vertices); a per-fan seed. For a hole (boundary) vertex it is the
    /// boundary-left-most corner; for an interior vertex it is an arbitrary
    /// incident corner.
    pub vertex_corners: Vec<CornerIdx>,
    /// Number of position vertices after isolated-vertex compaction.
    pub num_vertices: usize,
    /// Per-vertex boundary/hole flag (indexed by the pre-compaction vertex id).
    pub is_vert_hole: Vec<bool>,
    /// Seed corners of each component, in the order start faces were resolved.
    pub init_corners: Vec<CornerIdx>,
}

/// Mutable corner-table state used during reconstruction. The corner tables are
/// stored with a `CornerIdx::INVALID` sentinel rather than `Option<CornerIdx>`
/// so each entry is 4 bytes, halving the footprint the scattered per-symbol
/// accesses touch; the accessors still hand out `Option` at their boundary.
struct CornerTableBuilder {
    /// Per-corner opposite corner (`INVALID` on boundaries).
    opposite: Vec<CornerIdx>,
    corner_to_vertex: Vec<VertexIdx>,
    /// Left-most corner per vertex (`INVALID` when isolated).
    vertex_corners: Vec<CornerIdx>,
}

impl CornerTableBuilder {
    fn new(num_faces: usize) -> Self {
        let num_corners = num_faces * 3;
        Self {
            opposite: vec![CornerIdx::INVALID; num_corners],
            corner_to_vertex: vec![VertexIdx::INVALID; num_corners],
            vertex_corners: Vec::new(),
        }
    }

    fn opposite(&self, c: CornerIdx) -> Option<CornerIdx> {
        let o = self.opposite[usize::from(c)];
        (o != CornerIdx::INVALID).then_some(o)
    }

    fn set_opposite_corners(&mut self, a: CornerIdx, b: CornerIdx) {
        self.opposite[usize::from(a)] = b;
        self.opposite[usize::from(b)] = a;
    }

    fn vertex(&self, c: CornerIdx) -> VertexIdx {
        self.corner_to_vertex[usize::from(c)]
    }

    fn map_corner_to_vertex(&mut self, c: CornerIdx, v: VertexIdx) {
        self.corner_to_vertex[usize::from(c)] = v;
    }

    fn add_new_vertex(&mut self) -> VertexIdx {
        let v = self.vertex_corners.len();
        self.vertex_corners.push(CornerIdx::INVALID);
        VertexIdx::from(v)
    }

    fn num_vertices(&self) -> usize {
        self.vertex_corners.len()
    }

    fn left_most_corner(&self, v: VertexIdx) -> Option<CornerIdx> {
        let c = self.vertex_corners[usize::from(v)];
        (c != CornerIdx::INVALID).then_some(c)
    }

    fn set_left_most_corner(&mut self, v: VertexIdx, c: CornerIdx) {
        if v != VertexIdx::INVALID {
            self.vertex_corners[usize::from(v)] = c;
        }
    }

    fn make_vertex_isolated(&mut self, v: VertexIdx) {
        self.vertex_corners[usize::from(v)] = CornerIdx::INVALID;
    }

    fn swing_left(&self, c: CornerIdx) -> Option<CornerIdx> {
        self.opposite(c.next()).map(CornerIdx::next)
    }

    fn swing_right(&self, c: CornerIdx) -> Option<CornerIdx> {
        self.opposite(c.previous()).map(CornerIdx::previous)
    }
}

/// Consumes matching topology-split events for `encoder_symbol_id` (back-to-front).
/// Returns `Some(Ok((edge_right, split_symbol_id)))` on a match, `Some(Err(..))` on
/// a corruption sentinel, or `None` when no event matches.
fn take_topology_split(
    splits: &mut Vec<TopologySplit>,
    encoder_symbol_id: usize,
) -> Option<Result<(bool, usize), Err>> {
    let back = splits.last()?;
    if back.source_symbol_id > encoder_symbol_id {
        return Some(Err(Err::MalformedConnectivity(
            "topology split id out of order",
        )));
    }
    if back.source_symbol_id != encoder_symbol_id {
        return None;
    }
    let s = splits.pop().unwrap();
    Some(Ok((s.source_edge_right, s.split_symbol_id)))
}

/// Runs the spirale-reversi reconstruction over a concrete traversal variant.
pub fn reconstruct<T: TraversalDecoder>(
    traversal: &mut T,
    num_symbols: usize,
    num_encoded_vertices: usize,
    num_split_symbols: usize,
    num_faces: usize,
    num_attribute_data: usize,
    mut splits: Vec<TopologySplit>,
) -> Result<Reconstruction, Err> {
    let mut ct = CornerTableBuilder::new(num_faces);

    let max_num_vertices = num_encoded_vertices + num_split_symbols;
    let mut is_vert_hole = vec![true; max_num_vertices];

    let mut active_corner_stack: Vec<CornerIdx> = Vec::new();
    let mut topology_split_active_corners: HashMap<usize, CornerIdx> = HashMap::new();
    let mut invalid_vertices: Vec<VertexIdx> = Vec::new();
    let remove_invalid_vertices = num_attribute_data == 0;

    let mut num_faces_built = 0usize;

    let malformed = || Err::MalformedConnectivity("invalid edgebreaker symbol stream");

    for symbol_id in 0..num_symbols {
        let face = num_faces_built;
        num_faces_built += 1;
        let c0 = CornerIdx::from(3 * face);
        let c1 = CornerIdx::from(3 * face + 1);
        let c2 = CornerIdx::from(3 * face + 2);

        let symbol = traversal.decode_symbol()?;
        let mut check_topology_split = false;

        let top = active_corner_stack.last().copied();
        let (v_next_a, v_prev_a, opp_a) = match top {
            Some(ca) => (ct.vertex(ca.next()), ct.vertex(ca.previous()), ct.opposite(ca)),
            None => (VertexIdx::INVALID, VertexIdx::INVALID, None),
        };

        let corner_b = if usize::from(v_next_a) < ct.num_vertices() {
            ct.left_most_corner(v_next_a).map(CornerIdx::next)
        } else {
            None
        };
        let corner_b_op = corner_b.map(|c| ct.opposite(c));
        let vert_b_next = corner_b.map(|c| ct.vertex(c.next()));

        match symbol {
            Symbol::C => {
                let corner_a = top.ok_or_else(malformed)?;
                let vertex_x = v_next_a;
                let corner_b = corner_b.ok_or_else(malformed)?;
                let corner_b_op = corner_b_op.unwrap();
                if corner_a == corner_b {
                    return Err(malformed());
                }
                if opp_a.is_some() || corner_b_op.is_some() {
                    return Err(malformed());
                }
                ct.set_opposite_corners(corner_a, c1);
                ct.set_opposite_corners(corner_b, c2);
                let vert_a_prev = v_prev_a;
                let vert_b_next = vert_b_next.unwrap();
                if vertex_x == vert_a_prev || vertex_x == vert_b_next {
                    return Err(malformed());
                }
                ct.map_corner_to_vertex(c0, vertex_x);
                ct.map_corner_to_vertex(c1, vert_b_next);
                ct.map_corner_to_vertex(c2, vert_a_prev);
                ct.set_left_most_corner(vert_a_prev, c2);
                is_vert_hole[usize::from(vertex_x)] = false;
                *active_corner_stack.last_mut().unwrap() = c0;
            }
            Symbol::R | Symbol::L => {
                let corner_a = top.ok_or_else(malformed)?;
                if opp_a.is_some() {
                    return Err(malformed());
                }
                let (opp_corner, corner_l, corner_r) = if symbol == Symbol::R {
                    (c2, c1, c0)
                } else {
                    (c1, c0, c2)
                };
                ct.set_opposite_corners(opp_corner, corner_a);
                let new_vert = ct.add_new_vertex();
                if ct.num_vertices() > max_num_vertices {
                    return Err(malformed());
                }
                ct.map_corner_to_vertex(opp_corner, new_vert);
                ct.set_left_most_corner(new_vert, opp_corner);
                let vertex_r = v_prev_a;
                ct.map_corner_to_vertex(corner_r, vertex_r);
                ct.set_left_most_corner(vertex_r, corner_r);
                let vertex_l = v_next_a;
                ct.map_corner_to_vertex(corner_l, vertex_l);
                *active_corner_stack.last_mut().unwrap() = c0;
                check_topology_split = true;
            }
            Symbol::S => {
                let corner_b = active_corner_stack.pop().ok_or_else(malformed)?;
                if let Some(&c) = topology_split_active_corners.get(&symbol_id) {
                    active_corner_stack.push(c);
                }
                let corner_a = *active_corner_stack.last().ok_or_else(malformed)?;
                if corner_a == corner_b {
                    return Err(malformed());
                }
                if ct.opposite(corner_a).is_some() || ct.opposite(corner_b).is_some() {
                    return Err(malformed());
                }
                ct.set_opposite_corners(corner_a, c2);
                ct.set_opposite_corners(corner_b, c1);
                let vertex_p = ct.vertex(corner_a.previous());
                ct.map_corner_to_vertex(c0, vertex_p);
                ct.map_corner_to_vertex(c1, ct.vertex(corner_a.next()));
                let vert_b_prev = ct.vertex(corner_b.previous());
                ct.map_corner_to_vertex(c2, vert_b_prev);
                ct.set_left_most_corner(vert_b_prev, c2);
                let corner_n = corner_b.next();
                let vertex_n = ct.vertex(corner_n);
                traversal.merge_vertices(usize::from(vertex_p), usize::from(vertex_n));
                let vertex_n_corner = ct.left_most_corner(vertex_n).ok_or_else(malformed)?;
                ct.set_left_most_corner(vertex_p, vertex_n_corner);
                let first_corner = corner_n;
                let mut cn = Some(corner_n);
                while let Some(c) = cn {
                    ct.map_corner_to_vertex(c, vertex_p);
                    cn = ct.swing_left(c);
                    if cn == Some(first_corner) {
                        return Err(malformed());
                    }
                }
                ct.make_vertex_isolated(vertex_n);
                if remove_invalid_vertices {
                    invalid_vertices.push(vertex_n);
                }
                *active_corner_stack.last_mut().unwrap() = c0;
            }
            Symbol::E => {
                let first_vert = ct.add_new_vertex();
                let vert1 = ct.add_new_vertex();
                let vert2 = ct.add_new_vertex();
                if ct.num_vertices() > max_num_vertices {
                    return Err(malformed());
                }
                ct.map_corner_to_vertex(c0, first_vert);
                ct.map_corner_to_vertex(c1, vert1);
                ct.map_corner_to_vertex(c2, vert2);
                ct.set_left_most_corner(first_vert, c0);
                ct.set_left_most_corner(vert1, c1);
                ct.set_left_most_corner(vert2, c2);
                active_corner_stack.push(c0);
                check_topology_split = true;
            }
        }

        let active_top = *active_corner_stack.last().ok_or_else(malformed)?;
        traversal.new_active_corner_reached(
            usize::from(ct.vertex(active_top)),
            usize::from(ct.vertex(active_top.next())),
            usize::from(ct.vertex(active_top.previous())),
        );

        if check_topology_split {
            let encoder_symbol_id = num_symbols - symbol_id - 1;
            while let Some(event) = take_topology_split(&mut splits, encoder_symbol_id) {
                let (edge_right, split_symbol_id) = event?;
                let act_top = *active_corner_stack.last().ok_or_else(malformed)?;
                let new_active = if edge_right {
                    act_top.next()
                } else {
                    act_top.previous()
                };
                let decoder_split_symbol_id = num_symbols - split_symbol_id - 1;
                topology_split_active_corners.insert(decoder_split_symbol_id, new_active);
            }
        }
    }

    // Resolve start faces: drain the active-corner stack, one config bit each.
    let mut init_corners: Vec<CornerIdx> = Vec::new();
    while let Some(corner) = active_corner_stack.pop() {
        let interior = traversal.decode_start_face_config();
        if interior {
            if num_faces_built >= num_faces {
                return Err(malformed());
            }
            let corner_a = corner;
            let vert_n = ct.vertex(corner_a.next());
            let corner_b = ct.left_most_corner(vert_n).ok_or_else(malformed)?.next();
            let vert_x = ct.vertex(corner_b.next());
            let corner_c = ct.left_most_corner(vert_x).ok_or_else(malformed)?.next();
            if corner == corner_b || corner == corner_c || corner_b == corner_c {
                return Err(malformed());
            }
            if ct.opposite(corner).is_some()
                || ct.opposite(corner_b).is_some()
                || ct.opposite(corner_c).is_some()
            {
                return Err(malformed());
            }
            let vert_p = ct.vertex(corner_c.next());
            let face = num_faces_built;
            num_faces_built += 1;
            let nc0 = CornerIdx::from(3 * face);
            let nc1 = CornerIdx::from(3 * face + 1);
            let nc2 = CornerIdx::from(3 * face + 2);
            ct.set_opposite_corners(nc0, corner);
            ct.set_opposite_corners(nc1, corner_b);
            ct.set_opposite_corners(nc2, corner_c);
            ct.map_corner_to_vertex(nc0, vert_x);
            ct.map_corner_to_vertex(nc1, vert_p);
            ct.map_corner_to_vertex(nc2, vert_n);
            for nc in [nc0, nc1, nc2] {
                is_vert_hole[usize::from(ct.vertex(nc))] = false;
            }
            init_corners.push(nc0);
        } else {
            // Boundary start faces add no geometry, only a seed corner.
            init_corners.push(corner);
        }
    }

    if num_faces_built != num_faces {
        return Err(malformed());
    }

    // Compact isolated vertices when there is no attribute data (Google's fast path).
    let mut num_vertices = ct.num_vertices();
    if remove_invalid_vertices {
        for invalid_vert in invalid_vertices {
            let mut src_vert = num_vertices - 1;
            while ct.left_most_corner(VertexIdx::from(src_vert)).is_none() {
                num_vertices -= 1;
                src_vert = num_vertices - 1;
            }
            if src_vert < usize::from(invalid_vert) {
                continue;
            }
            let src = VertexIdx::from(src_vert);
            for c in corners_around(&ct, src) {
                ct.map_corner_to_vertex(c, invalid_vert);
            }
            let lmc = ct.left_most_corner(src).unwrap(); // the loop above stopped at a non-isolated vertex
            ct.set_left_most_corner(invalid_vert, lmc);
            ct.make_vertex_isolated(src);
            is_vert_hole[usize::from(invalid_vert)] = is_vert_hole[src_vert];
            is_vert_hole[src_vert] = false;
            num_vertices -= 1;
        }
    }

    Ok(Reconstruction {
        opposite: ct.opposite,
        corner_to_vertex: ct.corner_to_vertex,
        vertex_corners: ct.vertex_corners,
        num_vertices,
        is_vert_hole,
        init_corners,
    })
}

/// Corners incident to `v`, walked clockwise from the left-most corner.
fn corners_around(ct: &CornerTableBuilder, v: VertexIdx) -> Vec<CornerIdx> {
    let mut out = Vec::new();
    let Some(start) = ct.left_most_corner(v) else {
        return out;
    };
    let mut c = Some(start);
    while let Some(cur) = c {
        out.push(cur);
        c = ct.swing_right(cur);
        if c == Some(start) {
            break;
        }
    }
    out
}
