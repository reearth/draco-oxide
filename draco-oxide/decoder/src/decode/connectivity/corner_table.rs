//! Decoder-side corner table.
//!
//! Built incrementally by `decode/connectivity/edgebreaker.rs` and consumed
//! by `decode/attribute/` for the per-attribute prediction step. Implements
//! `GenericCornerTable` so the existing `shared::attribute::sequence::Traverser`
//! and `shared::attribute::prediction_scheme::*` can be reused on the decoder
//! side without modification.

use draco_oxide_core::corner_table::GenericCornerTable;
use draco_oxide_core::types::{CornerIdx, FaceIdx, PointIdx, VertexIdx};

/// Sentinel for "no opposite corner" (mesh boundary edge).
pub(crate) const NO_CORNER: usize = usize::MAX;

#[inline]
fn next_corner_inline(c: CornerIdx) -> CornerIdx {
    let i = usize::from(c);
    let face_base = (i / 3) * 3;
    CornerIdx::from(face_base + (i + 1 - face_base) % 3)
}

/// Layout (indexed by `usize::from(corner_idx)` / `usize::from(vertex_idx)`):
/// - `opposite[c]` — corner across the edge opposite to corner `c`; `NO_CORNER`
///   if the edge is on a boundary.
/// - `corner_to_vertex[c]` — vertex index at corner `c`.
/// - `left_most_corner[v]` — leftmost corner around vertex `v` used to walk
///   neighbours during C-symbol resolution and prediction lookups.
/// - `vertex_alias[v]` — when an S-symbol merges `vert_n` into `vert_p`, an
///   entry `vertex_alias[vert_n] = vert_p` is recorded. `vertex_idx(c)`
///   chases the alias chain so subsequent lookups yield the canonical
///   surviving vertex ID. The in-corner-table `merge_vertex` walk only
///   covers the swing_right 1-ring; corners assigned to `vert_n` AFTER
///   the S symbol processed (by later C/R/L) would otherwise leak the
///   merged-out ID into face data.
pub(crate) struct DecoderCornerTable {
    pub(crate) opposite: Vec<usize>,
    pub(crate) corner_to_vertex: Vec<usize>,
    pub(crate) left_most_corner: Vec<usize>,
    pub(crate) num_vertices: usize,
    pub(crate) vertex_alias: Vec<usize>,
}

impl DecoderCornerTable {
    pub(crate) fn with_capacity(num_faces: usize, num_vertices_hint: usize) -> Self {
        let num_corners = num_faces * 3;
        Self {
            opposite: vec![NO_CORNER; num_corners],
            corner_to_vertex: vec![NO_CORNER; num_corners],
            left_most_corner: Vec::with_capacity(num_vertices_hint),
            num_vertices: 0,
            vertex_alias: Vec::new(),
        }
    }

    /// Resolve a vertex ID through the S-merge alias chain to its
    /// surviving canonical ID. `usize::MAX` sentinel = no alias.
    ///
    /// Walks the chain, so callers on the per-corner hot path should
    /// invoke `compress_alias_chains` once after symbol replay
    /// finishes to flatten every entry to its terminal survivor — that
    /// reduces this from O(chain depth) to a single array indirection
    /// for every subsequent lookup.
    #[inline]
    pub(crate) fn resolve_alias(&self, mut v: usize) -> usize {
        while v < self.vertex_alias.len() && self.vertex_alias[v] != usize::MAX {
            v = self.vertex_alias[v];
        }
        v
    }

    /// Flatten every `vertex_alias` entry to the terminal survivor of
    /// its chain. After this runs, `resolve_alias` becomes a single
    /// indirection. Idempotent. Call once after symbol replay
    /// completes — chains can only grow during replay, so the work
    /// only needs doing once.
    pub(crate) fn compress_alias_chains(&mut self) {
        for i in 0..self.vertex_alias.len() {
            if self.vertex_alias[i] == usize::MAX {
                continue;
            }
            // Walk to the terminal survivor.
            let mut v = self.vertex_alias[i];
            while v < self.vertex_alias.len() && self.vertex_alias[v] != usize::MAX {
                v = self.vertex_alias[v];
            }
            self.vertex_alias[i] = v;
        }
    }

    /// Record `merged_out → survivor` for the post-replay alias walk.
    pub(crate) fn record_alias(&mut self, merged_out: VertexIdx, survivor: VertexIdx) {
        let m = usize::from(merged_out);
        if m >= self.vertex_alias.len() {
            self.vertex_alias.resize(m + 1, usize::MAX);
        }
        self.vertex_alias[m] = usize::from(survivor);
    }

    pub(crate) fn set_opposite(&mut self, a: CornerIdx, b: CornerIdx) {
        self.opposite[usize::from(a)] = usize::from(b);
        self.opposite[usize::from(b)] = usize::from(a);
    }

    pub(crate) fn map_corner_to_vertex(&mut self, c: CornerIdx, v: VertexIdx) {
        self.corner_to_vertex[usize::from(c)] = usize::from(v);
    }

    pub(crate) fn set_left_most_corner(&mut self, v: VertexIdx, c: CornerIdx) {
        self.left_most_corner[usize::from(v)] = usize::from(c);
    }

    pub(crate) fn add_new_vertex(&mut self) -> VertexIdx {
        let v = VertexIdx::from(self.num_vertices);
        self.num_vertices += 1;
        self.left_most_corner.push(NO_CORNER);
        v
    }

    /// Re-derive `left_most_corner[v]` for every vertex using the encoder's
    /// `compute_left_most_corners` algorithm (see
    /// `core/corner_table/mod.rs:358`). The values set during Edgebreaker
    /// symbol replay are correct for the replay's own bookkeeping but
    /// don't match what the encoder picks (encoder iterates the input mesh
    /// in face order; we iterate decoder face_id order which is encoder
    /// SYMBOL order, a different permutation). Without this re-derivation,
    /// the per-attribute corner table's `recompute_vertices` walk picks
    /// different starting fans → different attribute-vertex labels →
    /// wrong neighbor lookups during UV prediction.
    ///
    /// Run AFTER all symbol replay + start-face configs complete, so the
    /// `opposite[]` array is fully populated. Mirrors the encoder algorithm
    /// 1:1 (alias-resolved vertex_idx, swing_left walk per vertex, swing_right
    /// fallback at boundary).
    pub(crate) fn recompute_left_most_corners(&mut self) {
        let num_corners = self.corner_to_vertex.len();
        for slot in self.left_most_corner.iter_mut() {
            *slot = NO_CORNER;
        }
        let mut visited_vertices = vec![false; self.num_vertices];
        let mut visited_corners = vec![false; num_corners];

        for c_raw in 0..num_corners {
            if visited_corners[c_raw] {
                continue;
            }
            let c = CornerIdx::from(c_raw);
            let v = usize::from(<Self as GenericCornerTable>::vertex_idx(self, c));
            if v >= self.num_vertices || visited_vertices[v] {
                // Either phantom (alias-merged-out) or non-manifold:
                // the alias chain may collapse multiple corners to the
                // same vertex without sharing a real 1-ring. Mark this
                // corner visited but skip the swing — it'll be reached
                // via swing-right from another corner of v's true
                // neighborhood.
                visited_corners[c_raw] = true;
                continue;
            }
            visited_vertices[v] = true;
            visited_corners[c_raw] = true;
            self.left_most_corner[v] = c_raw;

            // Swing left as far as possible. Inline to avoid using
            // GenericCornerTable swing_left which goes through opposite
            // (alias-aware) — we want the raw universal swing here.
            let mut maybe_act_c = self.swing_left_raw(c);
            while let Some(act_c) = maybe_act_c {
                if act_c == c {
                    break;
                }
                visited_corners[usize::from(act_c)] = true;
                self.left_most_corner[v] = usize::from(act_c);
                maybe_act_c = self.swing_left_raw(act_c);
            }

            // Boundary: swing right from c to mark the rest of the 1-ring.
            if maybe_act_c.is_none() {
                let mut maybe_act_c = Some(c);
                while let Some(act_c) = maybe_act_c {
                    visited_corners[usize::from(act_c)] = true;
                    let nxt = next_corner_inline(act_c);
                    let opp = self.opposite[usize::from(nxt)];
                    if opp == NO_CORNER {
                        break;
                    }
                    let next_act = next_corner_inline(CornerIdx::from(opp));
                    if next_act == c {
                        break;
                    }
                    maybe_act_c = Some(next_act);
                }
            }
        }
    }

    fn swing_left_raw(&self, c: CornerIdx) -> Option<CornerIdx> {
        let nxt = next_corner_inline(c);
        let opp = self.opposite[usize::from(nxt)];
        if opp == NO_CORNER {
            None
        } else {
            Some(next_corner_inline(CornerIdx::from(opp)))
        }
    }

    /// Walks around `vert_n` and remaps every corner reaching it to point at
    /// `vert_p` instead. Used by the S symbol when two strips merge.
    pub(crate) fn merge_vertex(
        &mut self,
        vert_n: VertexIdx,
        vert_p: VertexIdx,
        start_corner: CornerIdx,
    ) {
        let mut maybe_c = Some(start_corner);
        let first = start_corner;
        let mut iters = 0;
        let max_iters = self.corner_to_vertex.len() + 1;
        while let Some(c) = maybe_c {
            self.corner_to_vertex[usize::from(c)] = usize::from(vert_p);
            iters += 1;
            if iters > max_iters {
                break;
            }
            let nxt = <Self as GenericCornerTable>::next(self, c);
            maybe_c = self
                .opposite(nxt)
                .map(|opp| <Self as GenericCornerTable>::next(self, opp));
            if let Some(n) = maybe_c {
                if n == first {
                    break;
                }
            }
        }
        self.left_most_corner[usize::from(vert_n)] = NO_CORNER;
    }
}

impl GenericCornerTable for DecoderCornerTable {
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

    /// For position attributes with no seams, point id == vertex id.
    /// Per-attribute corner tables handle seams via their own
    /// `corner_to_vertex` map.
    fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        PointIdx::from(self.resolve_alias(self.corner_to_vertex[usize::from(corner)]))
    }

    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        VertexIdx::from(self.resolve_alias(self.corner_to_vertex[usize::from(corner)]))
    }

    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        let v = self.opposite[usize::from(corner)];
        if v == NO_CORNER {
            None
        } else {
            Some(CornerIdx::from(v))
        }
    }

    fn previous(&self, corner: CornerIdx) -> CornerIdx {
        let i = usize::from(corner);
        let face_base = (i / 3) * 3;
        CornerIdx::from(face_base + (i + 2 - face_base) % 3)
    }

    fn next(&self, corner: CornerIdx) -> CornerIdx {
        let i = usize::from(corner);
        let face_base = (i / 3) * 3;
        CornerIdx::from(face_base + (i + 1 - face_base) % 3)
    }

    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        CornerIdx::from(self.left_most_corner[usize::from(vertex)])
    }
}
