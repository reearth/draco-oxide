//! Decoder-side per-attribute corner table.
//!
//! Mirrors `core/corner_table/attribute_corner_table.rs` but operates on
//! the decoded universal `DecoderCornerTable` plus a stream of
//! per-attribute seam bits. Used to give attribute-decode loops the
//! right `vertex_idx`/`opposite`/`left_most_corner` queries when
//! attributes (typically UVs and sometimes normals) split a vertex
//! across a seam edge.

use draco_oxide_core::corner_table::GenericCornerTable;
use draco_oxide_core::types::{CornerIdx, FaceIdx, PointIdx, VertexIdx};
use crate::decode::connectivity::corner_table::{DecoderCornerTable, NO_CORNER};

/// A seam-aware corner table derived from the universal corner table
/// + per-attribute seam bits decoded from the connectivity bitstream.
///
/// `corner_to_vertex` is keyed by corner idx and yields *attribute*
/// vertex IDs (not universal). Because attribute seams split a single
/// universal vertex into multiple attribute vertices, `num_vertices()`
/// can be larger than the underlying `DecoderCornerTable::num_vertices()`.
#[derive(Clone)]
pub(crate) struct DecoderAttributeCornerTable {
    pub(crate) corner_to_vertex: Vec<usize>,
    pub(crate) is_edge_on_seam: Vec<bool>,
    pub(crate) left_most_corners: Vec<usize>,
    pub(crate) num_vertices: usize,
    /// Copy of the universal CT's opposite[] so `opposite()` can answer
    /// without needing a borrow back to the universal CT. Seam edges
    /// are returned as None; non-seam edges return the universal
    /// opposite.
    pub(crate) opposite_universal: Vec<usize>,
}

impl DecoderAttributeCornerTable {
    /// Like `build_with_offsets`, but resolves the corner→vertex map itself
    /// (slower; only used by a unit test on positions-only meshes).
    #[allow(dead_code)]
    pub(crate) fn build(ct: &DecoderCornerTable, seam_bits: &[bool]) -> Self {
        let num_corners = ct.num_corners();
        let mut corner_vertex = vec![0usize; num_corners];
        let mut num_universal_vertices = 0usize;
        for (c, slot) in corner_vertex.iter_mut().enumerate() {
            let v = usize::from(ct.vertex_idx(CornerIdx::from(c)));
            *slot = v;
            num_universal_vertices = num_universal_vertices.max(v + 1);
        }
        Self::build_with_offsets(ct, seam_bits, &corner_vertex, num_universal_vertices)
    }

    /// Build from a universal corner table + decoded seam bits.
    ///
    /// `seam_bits[i]` is the bit emitted for the i-th corner-pair in the
    /// encoder's reversed iteration order (= decoder face order). For
    /// each non-boundary corner whose opposite face hasn't been visited
    /// yet, a single bit is consumed; that bit becomes
    /// `is_edge_on_seam[corner]` (and is mirrored to the opposite
    /// corner). Boundary corners are always seams.
    ///
    /// `corner_vertex[c]` is the alias-resolved universal vertex of corner
    /// `c` — precomputed once by the caller and shared across every
    /// attribute table, so the hot seam-marking / ring-walk loops never
    /// re-walk an alias chain. `num_universal_vertices` sizes the
    /// per-vertex scratch arrays.
    pub(crate) fn build_with_offsets(
        ct: &DecoderCornerTable,
        seam_bits: &[bool],
        corner_vertex: &[usize],
        num_universal_vertices: usize,
    ) -> Self {
        let num_corners = ct.num_corners();
        let num_faces = ct.num_faces();

        let mut is_edge_on_seam = vec![false; num_corners];
        let mut bit_idx = 0usize;
        // Mirror Google's `DecodeAttributeConnectivitiesOnFace`: iterate faces
        // FORWARD, primary corner `3*f` (corners `[3f, 3f+1, 3f+2]`). A
        // boundary edge is always a seam (no bit). For an interior edge whose
        // opposite face comes LATER (`opp_face > f`), consume one seam bit (in
        // the same forward order the encoder wrote them); the edge whose
        // opposite face came earlier was already decided when that face was
        // processed.
        for f in 0..num_faces {
            let base = 3 * f;
            for c in base..base + 3 {
                let opp_raw = ct.opposite[c];
                if opp_raw == NO_CORNER {
                    is_edge_on_seam[c] = true;
                    continue;
                }
                if opp_raw / 3 > f {
                    let bit = seam_bits.get(bit_idx).copied().unwrap_or(false);
                    bit_idx += 1;
                    if bit {
                        is_edge_on_seam[c] = true;
                        is_edge_on_seam[opp_raw] = true;
                    }
                }
            }
        }

        // Mark vertices on seam edges.
        let mut is_vertex_on_seam = vec![false; num_universal_vertices];
        for c in 0..num_corners {
            if !is_edge_on_seam[c] {
                continue;
            }
            // The two endpoints of the edge opposite corner c are
            // next(c) and previous(c).
            let n_v = corner_vertex[usize::from(next_corner(CornerIdx::from(c)))];
            let p_v = corner_vertex[usize::from(previous_corner(CornerIdx::from(c)))];
            if n_v < is_vertex_on_seam.len() {
                is_vertex_on_seam[n_v] = true;
            }
            if p_v < is_vertex_on_seam.len() {
                is_vertex_on_seam[p_v] = true;
            }
        }

        // Reconstruct corner_to_vertex by walking each universal vertex's
        // 1-ring. Mirrors `AttributeCornerTable::recompute_vertices`.
        let mut corner_to_vertex = vec![usize::MAX; num_corners];
        let mut left_most_corners: Vec<usize> = Vec::new();
        let mut num_new_vertices = 0usize;
        // Non-seam vertices map 1:1 to a single attribute vertex. Walking their
        // whole 1-ring (the dominant recompute cost on large meshes) is wasted
        // work — we record the id here and assign all their corners in one flat
        // pass below. Only genuine seam vertices need the ring walk + split.
        let mut remap = vec![u32::MAX; num_universal_vertices];

        for v in 0..num_universal_vertices {
            // Use the universal corner table's left_most_corner[v]. For
            // merged-out (phantom) vertices this is NO_CORNER; we skip
            // them so we don't double-count.
            if v >= ct.left_most_corner.len() || ct.left_most_corner[v] == NO_CORNER {
                continue;
            }
            let c = CornerIdx::from(ct.left_most_corner[v]);
            // Sanity: the corner must actually point to v (after alias
            // resolution).
            if corner_vertex[usize::from(c)] != v {
                continue;
            }

            // Fast path: a vertex not on any seam is one attribute vertex; its
            // corners are filled by the flat pass after the loop.
            if !is_vertex_on_seam[v] {
                remap[v] = num_new_vertices as u32;
                left_most_corners.push(usize::from(c));
                num_new_vertices += 1;
                continue;
            }

            let mut first_vert_id = num_new_vertices;
            num_new_vertices += 1;

            // Swing left until either we hit a boundary or wrap (shouldn't
            // happen for a true seam vertex).
            let mut first_c = c;
            let mut maybe_curr_c = swing_left(ct, &is_edge_on_seam, first_c);
            while let Some(curr_c) = maybe_curr_c {
                first_c = curr_c;
                if curr_c == c {
                    break;
                }
                maybe_curr_c = swing_left(ct, &is_edge_on_seam, curr_c);
            }
            corner_to_vertex[usize::from(first_c)] = first_vert_id;
            left_most_corners.push(usize::from(first_c));

            // Swing right, splitting at attribute seams.
            let mut maybe_curr_c = swing_right_universal(ct, first_c);
            while let Some(curr_c) = maybe_curr_c {
                if curr_c == first_c {
                    break;
                }
                // If the corner OPPOSITE to next(curr_c) is across a
                // seam, this corner starts a new attribute vertex.
                let probe = next_corner(curr_c);
                if is_edge_on_seam[usize::from(probe)] {
                    first_vert_id = num_new_vertices;
                    num_new_vertices += 1;
                    left_most_corners.push(usize::from(curr_c));
                }
                corner_to_vertex[usize::from(curr_c)] = first_vert_id;
                maybe_curr_c = swing_right_universal(ct, curr_c);
            }
        }

        // Flat pass: assign every corner whose universal vertex is non-seam its
        // single attribute id (seam corners were set during the walk above).
        for (c, slot) in corner_to_vertex.iter_mut().enumerate() {
            let id = remap[corner_vertex[c]];
            if id != u32::MAX {
                *slot = id as usize;
            }
        }

        Self {
            corner_to_vertex,
            is_edge_on_seam,
            left_most_corners,
            num_vertices: num_new_vertices,
            opposite_universal: ct.opposite.clone(),
        }
    }
}

#[inline]
fn next_corner(c: CornerIdx) -> CornerIdx {
    let i = usize::from(c);
    let face_base = (i / 3) * 3;
    CornerIdx::from(face_base + (i + 1 - face_base) % 3)
}

#[inline]
fn previous_corner(c: CornerIdx) -> CornerIdx {
    let i = usize::from(c);
    let face_base = (i / 3) * 3;
    CornerIdx::from(face_base + (i + 2 - face_base) % 3)
}

/// Swing right on the UNIVERSAL corner table (ignoring attribute seams).
/// `previous(c) → opposite(prev) → previous(opp)`.
fn swing_right_universal(ct: &DecoderCornerTable, c: CornerIdx) -> Option<CornerIdx> {
    let prev = previous_corner(c);
    let opp_raw = ct.opposite[usize::from(prev)];
    if opp_raw == NO_CORNER {
        return None;
    }
    Some(previous_corner(CornerIdx::from(opp_raw)))
}

/// Swing left on the ATTRIBUTE corner table — stops at seam edges.
fn swing_left(
    ct: &DecoderCornerTable,
    is_edge_on_seam: &[bool],
    c: CornerIdx,
) -> Option<CornerIdx> {
    let nxt = next_corner(c);
    if is_edge_on_seam[usize::from(nxt)] {
        return None;
    }
    let opp_raw = ct.opposite[usize::from(nxt)];
    if opp_raw == NO_CORNER {
        return None;
    }
    Some(next_corner(CornerIdx::from(opp_raw)))
}

impl GenericCornerTable for DecoderAttributeCornerTable {
    fn face_idx_containing(&self, corner: CornerIdx) -> FaceIdx {
        FaceIdx::from(usize::from(corner) / 3)
    }

    fn num_faces(&self) -> usize {
        self.corner_to_vertex.len() / 3
    }

    fn num_corners(&self) -> usize {
        self.corner_to_vertex.len()
    }

    fn num_vertices(&self) -> usize {
        self.num_vertices
    }

    fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        PointIdx::from(self.corner_to_vertex[usize::from(corner)])
    }

    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        VertexIdx::from(self.corner_to_vertex[usize::from(corner)])
    }

    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        if self.is_edge_on_seam[usize::from(corner)] {
            return None;
        }
        let opp = self.opposite_universal[usize::from(corner)];
        if opp == NO_CORNER {
            None
        } else {
            Some(CornerIdx::from(opp))
        }
    }

    fn previous(&self, corner: CornerIdx) -> CornerIdx {
        previous_corner(corner)
    }

    fn next(&self, corner: CornerIdx) -> CornerIdx {
        next_corner(corner)
    }

    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        CornerIdx::from(self.left_most_corners[usize::from(vertex)])
    }
}
