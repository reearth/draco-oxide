//! Edgebreaker connectivity decoder.
//!
//! Reads the Standard-traversal Edgebreaker bitstream produced by
//! `encode/connectivity/edgebreaker.rs` and reconstructs the triangle list.
//!
//! Algorithm: Spirale Reversi — process symbols in the order they appear in
//! the bitstream (reverse of encoder traversal order), allocating one new
//! face per symbol and updating an incrementally-built corner table. Mirrors
//! the C/R/L/E/S handlers in Draco's
//! `mesh_edgebreaker_decoder_impl.cc::DecodeConnectivity`.

use std::collections::HashMap;

use draco_oxide_core::bit_coder::{BitReader, ReaderErr};
use draco_oxide_core::buffer::LsbFirst;
use draco_oxide_core::corner_table::GenericCornerTable;
use draco_oxide_core::types::{CornerIdx, VertexIdx};
use crate::decode::entropy::rans::{self as rans_dec, RabsDecoder};
use crate::prelude::ByteReader;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::{CrLight, Symbol, SymbolEncoder};
use draco_oxide_core::codec::connectivity::edgebreaker::{self, EdgebreakerKind, Orientation};
use draco_oxide_core::utils::bit_coder::leb128_read;

use super::corner_table::DecoderCornerTable;
use super::DecodedConnectivity;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Reader error: {0}")]
    Reader(#[from] ReaderErr),
    #[error("Shared edgebreaker error: {0}")]
    Shared(#[from] edgebreaker::Err),
    #[error("Unsupported Edgebreaker traversal id (only Standard is implemented)")]
    UnsupportedTraversal,
    #[error("Active corner stack empty when symbol expected one")]
    EmptyActiveStack,
    #[error(
        "Decoded face count {decoded} != expected {expected}; bitstream may be malformed or use \
         a feature not yet supported (holes, topology splits, multi-component)"
    )]
    FaceCountMismatch { decoded: usize, expected: usize },
    #[error("Symbol stream exhausted before all symbols decoded")]
    SymbolStreamExhausted,
    #[error("Topology splits not yet supported")]
    TopologySplitsTodo,
    #[error("Rans decoder error: {0}")]
    Rans(#[from] rans_dec::Err),
    #[error("Symbol coding error: {0}")]
    SymbolCoding(#[from] crate::decode::entropy::symbol_coding::Err),
    #[error("Invalid symbol id in Valence context: {0}")]
    InvalidValenceSymbolId(u32),
    #[error("Valence context array exhausted (mesh asks for more symbols than encoded)")]
    ValenceContextExhausted,
}

/// Topology split events read from the bitstream.
/// (See `encode/connectivity/edgebreaker.rs::encode_topology_splits`.)
#[allow(dead_code)] // fields consumed once topology-split handling lands.
#[derive(Debug, Clone)]
pub(crate) struct TopologySplit {
    pub merging_symbol_idx: usize,
    pub split_symbol_idx: usize,
    pub merging_edge_orientation: Orientation,
}

#[allow(dead_code)] // some fields consumed during reconstruction; keep field names stable.
pub(crate) struct EdgebreakerMeta {
    pub traversal: EdgebreakerKind,
    pub num_vertices: usize,
    pub num_faces: usize,
    pub num_attribute_data: u8,
    pub num_encoded_symbols: usize,
    pub num_split_symbols: usize,
    pub topology_splits: Vec<TopologySplit>,
}

pub(crate) fn decode<R: ByteReader>(reader: &mut R) -> Result<DecodedConnectivity, Err> {
    let meta = read_meta(reader)?;

    let seam_bufs: Vec<(u8, Vec<u8>)>;
    let symbol_source = match meta.traversal {
        EdgebreakerKind::Standard => {
            // Standard: symbols stored in a leading bit-stream + a u8
            // prob + start-face buf + per-attr seams.
            let symbols = read_symbols(reader, meta.num_encoded_symbols)?;
            let (start_face_prob_zero, start_face_buf) = read_buffer_with_prob(reader)?;
            seam_bufs = read_attribute_seams(reader, meta.num_attribute_data as usize)?;
            SymbolSource::Standard {
                symbols,
                start_face_prob_zero,
                start_face_buf,
            }
        }
        EdgebreakerKind::Valence => {
            // Valence: start-faces + seams come FIRST, then 6 per-context
            // symbol arrays. (Mirrors
            // `mesh_edgebreaker_traversal_valence_decoder.h::Start`.)
            let (start_face_prob_zero, start_face_buf) = read_buffer_with_prob(reader)?;
            seam_bufs = read_attribute_seams(reader, meta.num_attribute_data as usize)?;
            // 6 contexts (valence 2..=7).
            let mut contexts: Vec<Vec<u32>> = Vec::with_capacity(6);
            for _ in 0..6 {
                let n = leb128_read(reader)? as usize;
                let mut syms: Vec<u32> = Vec::with_capacity(n);
                if n > 0 {
                    let raw = crate::decode::entropy::symbol_coding::decode_symbols(n, 1, reader)?;
                    for v in raw {
                        syms.push(v as u32);
                    }
                }
                contexts.push(syms);
            }
            SymbolSource::Valence {
                contexts,
                start_face_prob_zero,
                start_face_buf,
            }
        }
        EdgebreakerKind::Predictive => return Err(Err::UnsupportedTraversal),
    };

    let (faces, corner_table, start_corners, primary_offsets) = replay_symbols(
        symbol_source,
        meta.num_encoded_symbols,
        meta.num_faces,
        meta.num_vertices,
        &meta.topology_splits,
    )?;

    // Decode the per-attribute seam bits and reconstruct an attribute
    // corner table for each attribute that has its own connectivity
    // (UV-/normal-seamed meshes). The encoder emits one bit per
    // non-boundary corner whose opposite face hadn't yet been visited
    // (in face-allocation order); we replay the same iteration to
    // associate bits with corners.
    let attribute_corner_tables =
        build_attribute_corner_tables(&corner_table, &seam_bufs, &primary_offsets)?;

    Ok(DecodedConnectivity {
        faces,
        corner_table,
        start_corners,
        num_position_vertices: meta.num_vertices,
        attribute_corner_tables,
    })
}

/// Decode each per-attribute seam buffer + build the corresponding
/// `DecoderAttributeCornerTable`.
///
/// `primary_offsets[face]` ∈ {0, 1, 2} is the offset within the face of
/// the encoder's "primary corner" (the `c` it pushed onto
/// `processed_connectivity_corners` for that symbol). The encoder
/// iterates `[c, next(c), prev(c)]` for seam-bit emission; we mirror
/// that order exactly so each bit lands on the right corner.
fn build_attribute_corner_tables(
    ct: &super::corner_table::DecoderCornerTable,
    seam_bufs: &[(u8, Vec<u8>)],
    primary_offsets: &[u8],
) -> Result<Vec<super::DecoderAttributeCornerTable>, Err> {
    use super::DecoderAttributeCornerTable;
    use draco_oxide_core::corner_table::GenericCornerTable;
    use draco_oxide_core::types::CornerIdx;

    let num_corners = ct.num_corners();
    let num_faces = num_corners / 3;

    // First pass: compute the count of seam bits the encoder emitted.
    let _ = primary_offsets; // no longer used for seam decode (Google uses offset 0)
    let bit_count = compute_seam_bit_count(ct, num_faces);

    // Resolve the universal corner→vertex map ONCE (alias chains walked a
    // single time) and share it across every attribute table — otherwise each
    // table re-walks every alias chain in its seam-marking and ring-walk loops.
    let mut corner_vertex = vec![0usize; num_corners];
    let mut num_universal_vertices = 0usize;
    for (c, slot) in corner_vertex.iter_mut().enumerate() {
        let v = usize::from(ct.vertex_idx(CornerIdx::from(c)));
        *slot = v;
        num_universal_vertices = num_universal_vertices.max(v + 1);
    }

    // Attributes that share the same seam edges (very common — e.g. normal +
    // tangent, or normal + UV on artist meshes) produce identical corner
    // tables. Building one is O(corners) with per-vertex ring walks and is the
    // single biggest connectivity cost on large multi-attribute meshes, so we
    // reuse the previous table when the decoded seam bits match (Google's
    // `hasSameSeams` → `adoptVertexRecompute`).
    let mut tables: Vec<DecoderAttributeCornerTable> = Vec::with_capacity(seam_bufs.len());
    let mut prev_bits: Option<Vec<bool>> = None;
    for (prob_zero, buf) in seam_bufs.iter() {
        let bits = decode_rabs_seam_bits(*prob_zero, buf, bit_count)?;
        if prev_bits.as_deref() == Some(bits.as_slice()) {
            let reused = tables.last().expect("prev_bits implies a prior table").clone();
            tables.push(reused);
        } else {
            tables.push(DecoderAttributeCornerTable::build_with_offsets(
                ct,
                &bits,
                &corner_vertex,
                num_universal_vertices,
            ));
            prev_bits = Some(bits);
        }
    }
    Ok(tables)
}

/// Counts how many attribute-seam bits the encoder emitted, mirroring
/// Google's `MeshEdgebreakerDecoderImpl::DecodeAttributeConnectivitiesOnFace`:
/// iterate faces in FORWARD order, primary corner `3*f` (corners
/// `[3f, 3f+1, 3f+2]`), and consume one bit per interior edge whose opposite
/// face comes LATER (`opp_face > f`). Boundary edges are seams but carry no
/// bit. (`primary_offsets` is irrelevant to seam decode — Google always uses
/// corner offset 0.)
fn compute_seam_bit_count(
    ct: &super::corner_table::DecoderCornerTable,
    num_faces: usize,
) -> usize {
    use super::corner_table::NO_CORNER;
    let mut count = 0usize;
    for f in 0..num_faces {
        let base = 3 * f;
        for c in base..base + 3 {
            let opp = ct.opposite[c];
            if opp == NO_CORNER {
                continue; // boundary: seam, but no bit
            }
            if opp / 3 > f {
                count += 1;
            }
        }
    }
    count
}

/// RABS-decode `count` bits from `buf` (zero-prob `prob_zero`), then
/// reverse them so they're in encoder-iteration (= decoder face) order.
fn decode_rabs_seam_bits(prob_zero: u8, buf: &[u8], count: usize) -> Result<Vec<bool>, Err> {
    if count == 0 || buf.is_empty() {
        return Ok(vec![false; count]);
    }
    let buf_len = buf.len();
    let mut reader: &[u8] = buf;
    let mut rabs: RabsDecoder<_> =
        RabsDecoder::new(&mut reader, buf_len, prob_zero as usize, None)?;
    let mut bits = Vec::with_capacity(count);
    for _ in 0..count {
        bits.push(rabs.read().unwrap_or(0) != 0);
    }
    // RABS is LIFO: encoder writes [out0, out1, ...] (reverse of
    // seams_data), decoder reads them back in reverse-of-write =
    // forward-seams_data = encoder push order. That's exactly the
    // order we'll consume below (face N-1 first, [c, next, prev]
    // within each), so no extra reverse needed.
    Ok(bits)
}

/// Source of Edgebreaker symbols during corner-table replay.
///
/// Standard stores all symbols in a single bit-stream; the replay loop
/// can iterate them in order. Valence stores per-context arrays; the
/// replay loop must compute `active_context` from the current vertex
/// valences and pop from the matching context array.
enum SymbolSource {
    Standard {
        symbols: Vec<Symbol>,
        start_face_prob_zero: u8,
        start_face_buf: Vec<u8>,
    },
    Valence {
        /// 6 context arrays indexed by `valence - MIN_VALENCE` (0..=5).
        /// Symbols are popped from the BACK of each array as they're
        /// consumed (matches Google's `context_counters_` decrement).
        contexts: Vec<Vec<u32>>,
        start_face_prob_zero: u8,
        start_face_buf: Vec<u8>,
    },
}

impl SymbolSource {
    fn start_face_buf(&self) -> (u8, &[u8]) {
        match self {
            SymbolSource::Standard {
                start_face_prob_zero,
                start_face_buf,
                ..
            }
            | SymbolSource::Valence {
                start_face_prob_zero,
                start_face_buf,
                ..
            } => (*start_face_prob_zero, start_face_buf),
        }
    }
}

const MIN_VALENCE: usize = 2;
const MAX_VALENCE: usize = 7;

/// Symbol id → topology Symbol mapping (Google's
/// `edge_breaker_symbol_to_topology_id` indexes by symbol-id-as-stored
/// in the Valence context arrays).
fn symbol_id_to_topology(id: u32) -> Option<Symbol> {
    // From `shared/connectivity/edgebreaker/symbol_encoder.rs::Symbol::get_id`:
    //   C=0, S=1, L=2, R=3, E=4
    match id {
        0 => Some(Symbol::C),
        1 => Some(Symbol::S),
        2 => Some(Symbol::L),
        3 => Some(Symbol::R),
        4 => Some(Symbol::E),
        _ => None,
    }
}

/// Reads metadata up to (but not including) the symbol stream.
pub(crate) fn read_meta<R: ByteReader>(reader: &mut R) -> Result<EdgebreakerMeta, Err> {
    let traversal = EdgebreakerKind::read_from(reader)?;
    let num_vertices = leb128_read(reader)? as usize;
    let num_faces = leb128_read(reader)? as usize;
    let num_attribute_data = reader.read_u8()?;
    let num_encoded_symbols = leb128_read(reader)? as usize;
    let num_split_symbols = leb128_read(reader)? as usize;
    let topology_splits = read_topology_splits(reader)?;

    Ok(EdgebreakerMeta {
        traversal,
        num_vertices,
        num_faces,
        num_attribute_data,
        num_encoded_symbols,
        num_split_symbols,
        topology_splits,
    })
}

fn read_topology_splits<R: ByteReader>(reader: &mut R) -> Result<Vec<TopologySplit>, Err> {
    let count = leb128_read(reader)? as usize;
    let mut splits: Vec<TopologySplit> = Vec::with_capacity(count);
    let mut last_idx = 0usize;
    for _ in 0..count {
        let merging = leb128_read(reader)? as usize + last_idx;
        let split = merging - leb128_read(reader)? as usize;
        splits.push(TopologySplit {
            merging_symbol_idx: merging,
            split_symbol_idx: split,
            merging_edge_orientation: Orientation::Right,
        });
        last_idx = merging;
    }
    if !splits.is_empty() {
        let mut bit_reader: BitReader<'_, _, LsbFirst> = BitReader::spown_from(reader).unwrap();
        for split in splits.iter_mut() {
            split.merging_edge_orientation = match bit_reader.read_bits(1)? {
                0 => Orientation::Left,
                _ => Orientation::Right,
            };
        }
    }
    Ok(splits)
}

/// Reads the symbol byte buffer and decodes `num_symbols` CrLight symbols.
fn read_symbols<R: ByteReader>(reader: &mut R, num_symbols: usize) -> Result<Vec<Symbol>, Err> {
    let byte_len = leb128_read(reader)? as usize;
    let buf = draco_oxide_core::utils::bit_coder::read_byte_buffer(reader, byte_len)?;
    let mut iter = buf.into_iter();
    let mut bit_reader: BitReader<'_, _, LsbFirst> =
        BitReader::spown_from(&mut iter).ok_or(Err::SymbolStreamExhausted)?;
    let mut out = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols {
        out.push(CrLight::decode_symbol(&mut bit_reader));
    }
    Ok(out)
}

/// Reads `prob_zero u8 + leb128 length + raw bytes`. Used for both the
/// start-face-config buffer and the per-attribute seam buffers.
fn read_buffer_with_prob<R: ByteReader>(reader: &mut R) -> Result<(u8, Vec<u8>), Err> {
    let prob_zero = reader.read_u8()?;
    let byte_len = leb128_read(reader)? as usize;
    let buf = draco_oxide_core::utils::bit_coder::read_byte_buffer(reader, byte_len)?;
    Ok((prob_zero, buf))
}

fn read_attribute_seams<R: ByteReader>(
    reader: &mut R,
    n: usize,
) -> Result<Vec<(u8, Vec<u8>)>, Err> {
    let mut all = Vec::with_capacity(n);
    for _ in 0..n {
        all.push(read_buffer_with_prob(reader)?);
    }
    Ok(all)
}

// ─────────────────────────────────────────────────────────────────────────
// Corner-table reconstruction (corner table struct lives in `corner_table.rs`).
// ─────────────────────────────────────────────────────────────────────────

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

/// Replays the Edgebreaker symbol stream to rebuild the face list.
///
/// Symbols arrive in reverse-encoder-traversal order; each symbol allocates
/// one new face and updates the active corner stack. After all symbols are
/// consumed, the residual `active_corner_stack` holds one corner per
/// connected-component start. For each, we read one bit from the start-face
/// `RabsDecoder` and (if interior) allocate one final face whose three edges
/// stitch back to existing geometry.
fn replay_symbols(
    source: SymbolSource,
    num_symbols: usize,
    expected_faces: usize,
    num_encoded_vertices: usize,
    topology_splits: &[TopologySplit],
) -> Result<
    (
        Vec<[VertexIdx; 3]>,
        DecoderCornerTable,
        Vec<CornerIdx>,
        Vec<u8>,
    ),
    Err,
> {
    let mut ct = DecoderCornerTable::with_capacity(expected_faces, expected_faces);
    let mut active_stack: Vec<CornerIdx> = Vec::new();
    let mut start_corners: Vec<CornerIdx> = Vec::new();
    let mut face_id = 0usize;
    // Per-face offset (0/1/2) of the encoder's "primary corner" within
    // the face's [base, base+1, base+2] allocation. Encoder iterates
    // [c, next(c), prev(c)] for seam-bit emission, so seam decoding
    // must use the same starting corner. Derived per symbol type:
    //   C: 0 (encoder's c == decoder's base = new vertex)
    //   S: 0 (encoder's c == decoder's base)
    //   R: 1 (encoder's c at push time == decoder's base+1)
    //   L: 2 (encoder's c at push time == decoder's base+2)
    //   E: 0 (strip start; encoder's c is the seed corner at base)
    let mut primary_offsets: Vec<u8> = Vec::with_capacity(expected_faces);

    // Build lookup: encoder_id of the L/R/E (merging) symbol →
    // (decoder_id of the corresponding S split, orientation).
    // See doc above `TopologySplit` for the encoder/decoder index dance.
    let mut splits_by_merging: HashMap<usize, Vec<(usize, Orientation)>> = HashMap::new();
    for split in topology_splits {
        let decoder_split_id =
            num_symbols
                .checked_sub(split.split_symbol_idx + 1)
                .ok_or(Err::FaceCountMismatch {
                    decoded: 0,
                    expected: 0,
                })?;
        splits_by_merging
            .entry(split.merging_symbol_idx)
            .or_default()
            .push((decoder_split_id, split.merging_edge_orientation));
    }
    let mut topology_split_active_corners: HashMap<usize, CornerIdx> = HashMap::new();

    // Valence-only state. For Standard, these are unused (active_context
    // stays None and vertex_valences is never read).
    //
    // `vertex_valences` is keyed by `usize::from(VertexIdx)`. We grow it
    // on demand as new vertices are added via E/R/L symbols.
    let mut vertex_valences: Vec<i32> = vec![0; num_encoded_vertices.max(1) * 2];
    let mut active_context: Option<usize> = None;
    // Per-context cursors into the Valence symbol arrays; pop from BACK.
    let mut valence_cursors: [usize; 6] = [0; 6];
    if let SymbolSource::Valence { contexts, .. } = &source {
        for (i, c) in contexts.iter().enumerate().take(6) {
            valence_cursors[i] = c.len();
        }
    }
    let (start_face_prob_zero, start_face_buf_owned) = source.start_face_buf();
    let start_face_buf: Vec<u8> = start_face_buf_owned.to_vec();

    let symbol_source = source;

    for decoder_sym_id in 0..num_symbols {
        // Pull next symbol based on source type.
        let symbol = match &symbol_source {
            SymbolSource::Standard { symbols, .. } => symbols[decoder_sym_id],
            SymbolSource::Valence { contexts, .. } => {
                if let Some(ctx) = active_context {
                    let cursor = valence_cursors[ctx];
                    if cursor == 0 {
                        return Err(Err::ValenceContextExhausted);
                    }
                    valence_cursors[ctx] = cursor - 1;
                    let raw = contexts[ctx][cursor - 1];
                    symbol_id_to_topology(raw).ok_or(Err::InvalidValenceSymbolId(raw))?
                } else {
                    // First Valence symbol must be E (no active context yet).
                    // See `MeshEdgebreakerTraversalValenceDecoder::DecodeSymbol`.
                    Symbol::E
                }
            }
        };

        let encoder_sym_id = num_symbols - 1 - decoder_sym_id;
        let base = CornerIdx::from(3 * face_id);
        face_id += 1;

        let primary_off: u8 = match symbol {
            Symbol::C | Symbol::S | Symbol::E => 0,
            Symbol::R => 1,
            Symbol::L => 2,
        };
        primary_offsets.push(primary_off);

        match symbol {
            Symbol::E => {
                // First face on a fresh strip: three brand-new vertices.
                let v0 = ct.add_new_vertex();
                let v1 = ct.add_new_vertex();
                let v2 = ct.add_new_vertex();
                ct.map_corner_to_vertex(base, v0);
                ct.map_corner_to_vertex(CornerIdx::from(usize::from(base) + 1), v1);
                ct.map_corner_to_vertex(CornerIdx::from(usize::from(base) + 2), v2);
                ct.set_left_most_corner(v0, base);
                ct.set_left_most_corner(v1, CornerIdx::from(usize::from(base) + 1));
                ct.set_left_most_corner(v2, CornerIdx::from(usize::from(base) + 2));
                active_stack.push(base);
            }
            Symbol::C => {
                let corner_a = *active_stack.last().ok_or(Err::EmptyActiveStack)?;
                let vertex_x = ct.vertex_idx(next_corner(corner_a));
                let corner_b = next_corner(ct.left_most_corner(vertex_x));

                ct.set_opposite(corner_a, CornerIdx::from(usize::from(base) + 1));
                ct.set_opposite(corner_b, CornerIdx::from(usize::from(base) + 2));

                ct.map_corner_to_vertex(base, vertex_x);
                ct.map_corner_to_vertex(
                    CornerIdx::from(usize::from(base) + 1),
                    ct.vertex_idx(next_corner(corner_b)),
                );
                ct.map_corner_to_vertex(
                    CornerIdx::from(usize::from(base) + 2),
                    ct.vertex_idx(previous_corner(corner_a)),
                );

                *active_stack.last_mut().unwrap() = base;
            }
            Symbol::R => {
                // R: opp_corner=base+2, corner_l=base+1, corner_r=base.
                let corner_a = *active_stack.last().ok_or(Err::EmptyActiveStack)?;
                let opp = CornerIdx::from(usize::from(base) + 2);
                ct.set_opposite(opp, corner_a);

                let new_v = ct.add_new_vertex();
                ct.map_corner_to_vertex(opp, new_v);
                ct.set_left_most_corner(new_v, opp);

                let vertex_r = ct.vertex_idx(previous_corner(corner_a));
                ct.map_corner_to_vertex(base, vertex_r);
                ct.set_left_most_corner(vertex_r, base);

                ct.map_corner_to_vertex(
                    CornerIdx::from(usize::from(base) + 1),
                    ct.vertex_idx(next_corner(corner_a)),
                );

                *active_stack.last_mut().unwrap() = base;
            }
            Symbol::L => {
                // L: opp_corner=base+1, corner_l=base, corner_r=base+2.
                let corner_a = *active_stack.last().ok_or(Err::EmptyActiveStack)?;
                let opp = CornerIdx::from(usize::from(base) + 1);
                ct.set_opposite(opp, corner_a);

                let new_v = ct.add_new_vertex();
                ct.map_corner_to_vertex(opp, new_v);
                ct.set_left_most_corner(new_v, opp);

                let vertex_r = ct.vertex_idx(previous_corner(corner_a));
                let corner_r = CornerIdx::from(usize::from(base) + 2);
                ct.map_corner_to_vertex(corner_r, vertex_r);
                ct.set_left_most_corner(vertex_r, corner_r);

                ct.map_corner_to_vertex(base, ct.vertex_idx(next_corner(corner_a)));

                *active_stack.last_mut().unwrap() = base;
            }
            Symbol::S => {
                // Pop top of stack as corner_b. If a topology split is
                // resolving here, push its stored corner so it becomes the
                // new top — this is what corner_a then reads.
                let corner_b = active_stack.pop().ok_or(Err::EmptyActiveStack)?;
                if let Some(stored) = topology_split_active_corners.remove(&decoder_sym_id) {
                    active_stack.push(stored);
                }
                let corner_a = *active_stack.last().ok_or(Err::EmptyActiveStack)?;

                ct.set_opposite(corner_a, CornerIdx::from(usize::from(base) + 2));
                ct.set_opposite(corner_b, CornerIdx::from(usize::from(base) + 1));

                let vertex_p = ct.vertex_idx(previous_corner(corner_a));
                ct.map_corner_to_vertex(base, vertex_p);
                ct.map_corner_to_vertex(
                    CornerIdx::from(usize::from(base) + 1),
                    ct.vertex_idx(next_corner(corner_a)),
                );

                let vert_b_prev = ct.vertex_idx(previous_corner(corner_b));
                let corner_2 = CornerIdx::from(usize::from(base) + 2);
                ct.map_corner_to_vertex(corner_2, vert_b_prev);
                ct.set_left_most_corner(vert_b_prev, corner_2);

                let corner_n = next_corner(corner_b);
                let vertex_n = ct.vertex_idx(corner_n);
                // Google's reference also propagates vertex_n's left_most_corner
                // to vertex_p before isolating vertex_n. Without this the
                // surviving vertex's left_most_corner can point at a corner
                // that's now mapped to vertex_p, but `is_on_boundary`/swing_left
                // queries via vertex_p use the unupdated value and miss edges.
                let vert_n_left_most = ct.left_most_corner(vertex_n);
                ct.set_left_most_corner(vertex_p, vert_n_left_most);
                ct.merge_vertex(vertex_n, vertex_p, corner_n);
                // Record vertex_n → vertex_p alias so subsequent symbol
                // handlers (and final face construction) resolve any
                // residual references to vertex_n via the chain.
                ct.record_alias(vertex_n, vertex_p);

                // Valence-only: traversal_decoder_.MergeVertices(vertex_p,
                // vertex_n) — fold vertex_n's valence into vertex_p so the
                // post-symbol context computation reflects the merged shape.
                if matches!(symbol_source, SymbolSource::Valence { .. }) {
                    let vp = usize::from(vertex_p);
                    let vn = usize::from(vertex_n);
                    let max_v = vp.max(vn);
                    if max_v >= vertex_valences.len() {
                        vertex_valences.resize(max_v * 2 + 8, 0);
                    }
                    vertex_valences[vp] += vertex_valences[vn];
                    vertex_valences[vn] = 0;
                }

                *active_stack.last_mut().unwrap() = base;
            }
        }

        // After R/L/E (the symbols that detect topology splits), check if
        // any of the recorded splits name `encoder_sym_id` as their
        // merging symbol. If so, derive the new active corner from the
        // current stack top + orientation and stash it under the
        // corresponding S symbol's decoder index.
        let check_topology_split = matches!(symbol, Symbol::R | Symbol::L | Symbol::E);
        if check_topology_split {
            if let Some(events) = splits_by_merging.get(&encoder_sym_id) {
                for &(decoder_split_id, orientation) in events {
                    let act_top = *active_stack.last().ok_or(Err::EmptyActiveStack)?;
                    let new_corner = match orientation {
                        Orientation::Right => next_corner(act_top),
                        Orientation::Left => previous_corner(act_top),
                    };
                    topology_split_active_corners.insert(decoder_split_id, new_corner);
                }
            }
        }

        // Valence-only post-symbol bookkeeping: update vertex_valences
        // around the new active corner, then compute active_context for
        // the NEXT symbol. Mirrors
        // `MeshEdgebreakerTraversalValenceDecoder::NewActiveCornerReached`.
        if matches!(symbol_source, SymbolSource::Valence { .. }) {
            // The "new active corner" is whatever the symbol made active
            // (last_mut().unwrap() = base in C/R/L/S; push(base) in E).
            let active_corner = *active_stack.last().ok_or(Err::EmptyActiveStack)?;
            let next_c = next_corner(active_corner);
            let prev_c = previous_corner(active_corner);
            let v_a = usize::from(ct.vertex_idx(active_corner));
            let v_n = usize::from(ct.vertex_idx(next_c));
            let v_p = usize::from(ct.vertex_idx(prev_c));
            let max_v = v_a.max(v_n).max(v_p);
            if max_v >= vertex_valences.len() {
                vertex_valences.resize(max_v * 2 + 8, 0);
            }
            match symbol {
                Symbol::C | Symbol::S => {
                    vertex_valences[v_n] += 1;
                    vertex_valences[v_p] += 1;
                }
                Symbol::R => {
                    vertex_valences[v_a] += 1;
                    vertex_valences[v_n] += 1;
                    vertex_valences[v_p] += 2;
                }
                Symbol::L => {
                    vertex_valences[v_a] += 1;
                    vertex_valences[v_n] += 2;
                    vertex_valences[v_p] += 1;
                }
                Symbol::E => {
                    vertex_valences[v_a] += 2;
                    vertex_valences[v_n] += 2;
                    vertex_valences[v_p] += 2;
                }
            }
            let active_valence = vertex_valences[v_n];
            let clamped = active_valence.clamp(MIN_VALENCE as i32, MAX_VALENCE as i32) as usize;
            active_context = Some(clamped - MIN_VALENCE);
        }
    }

    // ── Start-face configurations ────────────────────────────────────────
    // For each remaining corner on the active stack, decode one bit. If
    // interior, allocate a final face that closes off the component;
    // otherwise the start was on a boundary and no extra face is needed.
    if !active_stack.is_empty() {
        let buf_len = start_face_buf.len();
        let mut iter = start_face_buf.into_iter();
        let mut rabs: RabsDecoder<_> =
            RabsDecoder::new(&mut iter, buf_len, start_face_prob_zero as usize, None)?;

        while let Some(corner) = active_stack.pop() {
            let interior = rabs.read()? != 0;
            // Record the start corner for the per-attribute Traverser.
            start_corners.push(corner);
            if !interior {
                continue;
            }
            // Interior start face: three corners on existing faces are opposite
            // to the new face. Walk: a → vert_n → b → vert_x → c → vert_p.
            let new_corner = CornerIdx::from(3 * face_id);
            face_id += 1;
            // Sentinel: start-face faces are NOT in encoder's
            // processed_connectivity_corners, so encoder never emits
            // seam bits for them. Mark with u8::MAX so the seam loop
            // can skip them.
            primary_offsets.push(u8::MAX);

            let corner_a = corner;
            let vert_n = ct.vertex_idx(next_corner(corner_a));
            let corner_b = next_corner(ct.left_most_corner(vert_n));
            let vert_x = ct.vertex_idx(next_corner(corner_b));
            let corner_c = next_corner(ct.left_most_corner(vert_x));
            let vert_p = ct.vertex_idx(next_corner(corner_c));

            ct.set_opposite(new_corner, corner_a);
            ct.set_opposite(CornerIdx::from(usize::from(new_corner) + 1), corner_b);
            ct.set_opposite(CornerIdx::from(usize::from(new_corner) + 2), corner_c);

            ct.map_corner_to_vertex(new_corner, vert_x);
            ct.map_corner_to_vertex(CornerIdx::from(usize::from(new_corner) + 1), vert_p);
            ct.map_corner_to_vertex(CornerIdx::from(usize::from(new_corner) + 2), vert_n);
        }
    }

    if face_id != expected_faces {
        return Err(Err::FaceCountMismatch {
            decoded: face_id,
            expected: expected_faces,
        });
    }

    // Flatten the S-merge alias chains once now that replay is done,
    // so every subsequent `vertex_idx` / `point_idx` lookup is a
    // single array indirection instead of a chain walk. Chains can
    // only grow during replay, so the work doesn't need redoing.
    ct.compress_alias_chains();

    // Re-derive `left_most_corner[v]` to match the encoder's algorithm
    // (swing-left-most per vertex, iterated in face order). The values
    // set during symbol replay are correct for the replay's own
    // bookkeeping but don't match the input-mesh-driven values the
    // encoder uses for attribute corner table construction.
    ct.recompute_left_most_corners();

    let mut faces = Vec::with_capacity(face_id);
    for f in 0..face_id {
        let base = f * 3;
        faces.push([
            VertexIdx::from(ct.resolve_alias(ct.corner_to_vertex[base])),
            VertexIdx::from(ct.resolve_alias(ct.corner_to_vertex[base + 1])),
            VertexIdx::from(ct.resolve_alias(ct.corner_to_vertex[base + 2])),
        ]);
    }
    Ok((faces, ct, start_corners, primary_offsets))
}
