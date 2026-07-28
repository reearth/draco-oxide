//! Edgebreaker traversal decoding: the standard variant (CR-light symbol bit
//! stream) and the valence variant (per-valence-context rANS symbol streams),
//! plus the shared rabs-coded start-face interior flags and attribute seam bits,
//! and the topology-split event stream.
//!
//! The two variants implement the [`TraversalDecoder`] trait, so `reconstruct`
//! is generic over the traversal and monomorphizes the per-symbol dispatch away.

use crate::entropy::rans::RabsDecoder;
use crate::reader::RevReader;
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::Symbol;
use draco_oxide_core::codec::connectivity::edgebreaker::{MAX_VALENCE, MIN_VALENCE};
use draco_oxide_core::types::CornerIdx;
use draco_oxide_core::utils::bit_coder::leb128_read;

/// Builds a rabs decoder over a self-contained `[prob_zero | leb128 len | bytes]`
/// sub-stream read from `reader`.
fn start_rabs<'a>(reader: &mut Reader<'a>) -> Result<RabsDecoder<'a>, Err> {
    let prob_zero = reader.read_u8()?;
    let len = leb128_read(reader)? as usize;
    let rev = RevReader::new(reader.read_bytes(len)?);
    RabsDecoder::new(rev, prob_zero)
}

/// LSB-first bit reader over a fixed byte buffer
struct BitSource<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitSource<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> u32 {
        let bit = (self.bytes[self.byte_pos] >> self.bit_pos) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        bit as u32
    }

    fn read_bits(&mut self, n: u8) -> u32 {
        let mut value = 0;
        for i in 0..n {
            value |= self.read_bit() << i;
        }
        value
    }
}

/// A topology-split event, in encoder-symbol-id terms (as stored on the wire).
pub struct TopologySplit {
    pub source_symbol_id: usize,
    pub split_symbol_id: usize,
    /// True for `RIGHT_FACE_EDGE`, false for `LEFT_FACE_EDGE`.
    pub source_edge_right: bool,
}

/// Decodes the topology-split event stream: leb128 count, delta-coded id pairs, then
/// one LSB-first edge bit per split (byte-padded).
pub fn decode_topology_splits(reader: &mut Reader<'_>) -> Result<Vec<TopologySplit>, Err> {
    let num_splits = leb128_read(reader)? as usize;
    let mut splits = Vec::with_capacity(num_splits);

    let mut last_source = 0usize;
    for _ in 0..num_splits {
        let source_symbol_id = leb128_read(reader)? as usize + last_source;
        let delta = leb128_read(reader)? as usize;
        if delta > source_symbol_id {
            return Err(Err::MalformedConnectivity("split id delta out of range"));
        }
        let split_symbol_id = source_symbol_id - delta;
        last_source = source_symbol_id;
        splits.push(TopologySplit {
            source_symbol_id,
            split_symbol_id,
            source_edge_right: false,
        });
    }

    // One edge bit per split, LSB-first, byte-padded (no size prefix).
    if num_splits > 0 {
        let edge_bytes = reader.read_bytes(num_splits.div_ceil(8))?;
        let mut bits = BitSource::new(edge_bytes);
        for split in &mut splits {
            split.source_edge_right = bits.read_bit() & 1 == 1;
        }
    }

    Ok(splits)
}

/// The rabs-coded sub-streams shared by both traversal variants: one start-face
/// interior-flag stream and one attribute-seam stream per attribute.
struct SharedStreams<'a> {
    start_face: RabsDecoder<'a>,
    seams: Vec<RabsDecoder<'a>>,
}

impl<'a> SharedStreams<'a> {
    /// Reads the start-face rabs stream followed by one seam rabs stream per
    /// attribute, in the order the encoder wrote them.
    fn read(reader: &mut Reader<'a>, num_attribute_data: usize) -> Result<Self, Err> {
        let start_face = start_rabs(reader)?;
        let mut seams = Vec::with_capacity(num_attribute_data);
        for _ in 0..num_attribute_data {
            seams.push(start_rabs(reader)?);
        }
        Ok(Self { start_face, seams })
    }

    fn decode_start_face_config(&mut self) -> bool {
        self.start_face.decode_bit()
    }

    /// Decodes the per-attribute seam edges (port of Google's
    /// `DecodeAttributeConnectivitiesOnFace`, run over every face in order).
    /// Returns, per attribute, the `is_edge_on_seam` flag for each corner: a
    /// boundary edge is an automatic seam for every attribute and reads no
    /// bit; an interior edge decodes one seam bit per attribute, processed
    /// once from its lower-id face. Both corners of a seam edge are marked,
    /// matching `AddSeamEdge`. Consumes the seam streams: the common stream
    /// counts run monomorphized with the decoder states in locals, so the
    /// independent rabs chains overlap instead of round-tripping through
    /// memory at every bit.
    fn decode_attribute_seams(
        &mut self,
        opposite: &[CornerIdx],
        num_faces: usize,
    ) -> Vec<Vec<bool>> {
        let num_corners = num_faces * 3;
        let mut is_seam = vec![vec![false; num_corners]; self.seams.len()];
        let seams = std::mem::take(&mut self.seams);
        match seams.len() {
            0 => {}
            1 => {
                let decs: [RabsDecoder<'a>; 1] = seams.try_into().ok().expect("length checked");
                decode_seams_fixed(decs, opposite, num_faces, &mut is_seam);
            }
            2 => {
                let decs: [RabsDecoder<'a>; 2] = seams.try_into().ok().expect("length checked");
                decode_seams_fixed(decs, opposite, num_faces, &mut is_seam);
            }
            _ => decode_seams_general(seams, opposite, num_faces, &mut is_seam),
        }
        is_seam
    }
}

/// The seam-edge scan over `opposite`, monomorphized on the stream count so the
/// decoder states live in locals for the whole scan.
fn decode_seams_fixed<const N: usize>(
    mut decs: [RabsDecoder<'_>; N],
    opposite: &[CornerIdx],
    num_faces: usize,
    is_seam: &mut [Vec<bool>],
) {
    debug_assert!(opposite.len() == num_faces * 3);
    for f in 0..num_faces {
        let corner = CornerIdx::from(3 * f);
        for c in [corner, corner.next(), corner.previous()] {
            // SAFETY: c < num_faces * 3 == opposite.len(), and every seam vec
            // has that same length; a non-INVALID `opp` is itself a corner id
            // below num_faces * 3 (reconstruct's corner-bound contract).
            let opp = unsafe { *opposite.get_unchecked(usize::from(c)) };
            if opp == CornerIdx::INVALID {
                // Boundary edge: an automatic seam for every attribute, no bit.
                for seam in is_seam.iter_mut() {
                    unsafe { *seam.get_unchecked_mut(usize::from(c)) = true };
                }
                continue;
            }
            // Each shared edge is decoded once, from its lower-id face.
            if usize::from(opp.face_idx()) < f {
                continue;
            }
            for (dec, seam) in decs.iter_mut().zip(is_seam.iter_mut()) {
                if dec.decode_bit() {
                    unsafe {
                        *seam.get_unchecked_mut(usize::from(c)) = true;
                        *seam.get_unchecked_mut(usize::from(opp)) = true;
                    }
                }
            }
        }
    }
}

/// Fallback for stream counts without a monomorphized scan.
fn decode_seams_general(
    mut decs: Vec<RabsDecoder<'_>>,
    opposite: &[CornerIdx],
    num_faces: usize,
    is_seam: &mut [Vec<bool>],
) {
    debug_assert!(opposite.len() == num_faces * 3);
    for f in 0..num_faces {
        let corner = CornerIdx::from(3 * f);
        for c in [corner, corner.next(), corner.previous()] {
            // SAFETY: as in `decode_seams_fixed`.
            let opp = unsafe { *opposite.get_unchecked(usize::from(c)) };
            if opp == CornerIdx::INVALID {
                for seam in is_seam.iter_mut() {
                    unsafe { *seam.get_unchecked_mut(usize::from(c)) = true };
                }
                continue;
            }
            if usize::from(opp.face_idx()) < f {
                continue;
            }
            for (dec, seam) in decs.iter_mut().zip(is_seam.iter_mut()) {
                if dec.decode_bit() {
                    unsafe {
                        *seam.get_unchecked_mut(usize::from(c)) = true;
                        *seam.get_unchecked_mut(usize::from(opp)) = true;
                    }
                }
            }
        }
    }
}

/// One edgebreaker traversal variant, driving symbol decode plus the shared
/// start-face and attribute-seam streams. `reconstruct` is generic over this
/// trait, so the per-symbol variant dispatch is resolved at monomorphization
/// rather than per symbol.
pub trait TraversalDecoder {
    /// Decodes the next edgebreaker symbol.
    fn decode_symbol(&mut self) -> Result<Symbol, Err>;

    /// Hook called with the vertices of the active corner (tip, next, previous)
    /// after each symbol's face is built. Only the valence variant, which tracks
    /// vertex valences, overrides it; it is a no-op otherwise.
    fn new_active_corner_reached(&mut self, v_corner: usize, v_next: usize, v_prev: usize) {
        let _ = (v_corner, v_next, v_prev);
    }

    /// Hook for the S-symbol vertex merge, when `dest` absorbs `source`. Only the
    /// valence variant overrides it; it is a no-op otherwise.
    fn merge_vertices(&mut self, dest: usize, source: usize) {
        let _ = (dest, source);
    }

    /// Decodes one start-face configuration bit (true = interior face).
    fn decode_start_face_config(&mut self) -> bool;

    /// Decodes every attribute's seam edges from the per-attribute seam
    /// streams, consuming them. See [`SharedStreams::decode_attribute_seams`].
    fn decode_attribute_seams(
        &mut self,
        opposite: &[CornerIdx],
        num_faces: usize,
    ) -> Vec<Vec<bool>>;
}

/// Standard traversal: symbols come from a single CR-light bit stream.
pub struct StandardTraversalDecoder<'a> {
    symbols: BitSource<'a>,
    shared: SharedStreams<'a>,
}

impl<'a> StandardTraversalDecoder<'a> {
    /// Reads the symbol bit stream, then the shared start-face and seam streams,
    /// in the order the encoder wrote them.
    pub fn start(reader: &mut Reader<'a>, num_attribute_data: usize) -> Result<Self, Err> {
        let symbol_len = leb128_read(reader)? as usize;
        let symbols = BitSource::new(reader.read_bytes(symbol_len)?);
        let shared = SharedStreams::read(reader, num_attribute_data)?;
        Ok(Self { symbols, shared })
    }
}

impl TraversalDecoder for StandardTraversalDecoder<'_> {
    /// One CR-light code: one bit for `C`, otherwise a leading `1` plus two
    /// suffix bits forming the pattern `1 | (suffix << 1)`.
    fn decode_symbol(&mut self) -> Result<Symbol, Err> {
        if self.symbols.read_bit() == 0 {
            return Ok(Symbol::C);
        }
        let suffix = self.symbols.read_bits(2);
        Ok(match 1 | (suffix << 1) {
            1 => Symbol::S,
            3 => Symbol::L,
            5 => Symbol::R,
            _ => Symbol::E,
        })
    }

    fn decode_start_face_config(&mut self) -> bool {
        self.shared.decode_start_face_config()
    }

    fn decode_attribute_seams(
        &mut self,
        opposite: &[CornerIdx],
        num_faces: usize,
    ) -> Vec<Vec<bool>> {
        self.shared.decode_attribute_seams(opposite, num_faces)
    }
}

/// State of the valence traversal: symbols are grouped by the entropy context
/// (the clamped valence of the active vertex), and the context of each symbol is
/// recovered by maintaining the valences of the partially reconstructed mesh,
/// mirroring the encoder in reverse.
struct ValenceState {
    /// Per-context symbol ids, consumed from the back.
    context_symbols: Vec<Vec<u64>>,
    /// Valence of the decoded portion of the mesh per reconstruction vertex.
    vertex_valences: Vec<usize>,
    last_symbol: Option<Symbol>,
    active_context: Option<usize>,
}

/// Valence traversal: one rANS symbol stream per valence context.
pub struct ValenceTraversalDecoder<'a> {
    state: ValenceState,
    shared: SharedStreams<'a>,
}

impl<'a> ValenceTraversalDecoder<'a> {
    /// Reads the shared start-face and seam streams, then one rANS symbol stream
    /// per valence context. `max_num_vertices` bounds the valence table (encoded
    /// vertices plus split symbols); `num_faces` bounds each context's symbol
    /// count.
    pub fn start(
        reader: &mut Reader<'a>,
        num_attribute_data: usize,
        max_num_vertices: usize,
        num_faces: usize,
    ) -> Result<Self, Err> {
        let shared = SharedStreams::read(reader, num_attribute_data)?;

        let num_contexts = MAX_VALENCE - MIN_VALENCE + 1;
        let mut context_symbols = Vec::with_capacity(num_contexts);
        for _ in 0..num_contexts {
            let num_symbols = leb128_read(reader)? as usize;
            if num_symbols > num_faces {
                return Err(Err::MalformedConnectivity(
                    "valence context symbol count exceeds the face count",
                ));
            }
            context_symbols.push(if num_symbols > 0 {
                crate::entropy::decode_symbols(reader, num_symbols, 1)?
            } else {
                Vec::new()
            });
        }

        Ok(Self {
            state: ValenceState {
                context_symbols,
                vertex_valences: vec![0; max_num_vertices],
                last_symbol: None,
                active_context: None,
            },
            shared,
        })
    }
}

impl TraversalDecoder for ValenceTraversalDecoder<'_> {
    /// The back of the active context's symbol list; the very first symbol has no
    /// context yet and is always `E`.
    fn decode_symbol(&mut self) -> Result<Symbol, Err> {
        let state = &mut self.state;
        let symbol = match state.active_context {
            None => Symbol::E,
            Some(ctx) => {
                let id = state.context_symbols[ctx]
                    .pop()
                    .ok_or(Err::MalformedConnectivity(
                        "valence context ran out of symbols",
                    ))?;
                match id {
                    0 => Symbol::C,
                    1 => Symbol::S,
                    2 => Symbol::L,
                    3 => Symbol::R,
                    4 => Symbol::E,
                    _ => return Err(Err::MalformedConnectivity("invalid valence symbol id")),
                }
            }
        };
        state.last_symbol = Some(symbol);
        Ok(symbol)
    }

    /// Applies the decoded symbol's valence increments and selects the context
    /// for the next symbol from the valence of the next (active) vertex.
    #[inline]
    fn new_active_corner_reached(&mut self, v_corner: usize, v_next: usize, v_prev: usize) {
        let valences = &mut self.state.vertex_valences;
        match self.state.last_symbol {
            Some(Symbol::C) | Some(Symbol::S) => {
                valences[v_next] += 1;
                valences[v_prev] += 1;
            }
            Some(Symbol::R) => {
                valences[v_corner] += 1;
                valences[v_next] += 1;
                valences[v_prev] += 2;
            }
            Some(Symbol::L) => {
                valences[v_corner] += 1;
                valences[v_next] += 2;
                valences[v_prev] += 1;
            }
            Some(Symbol::E) => {
                valences[v_corner] += 2;
                valences[v_next] += 2;
                valences[v_prev] += 2;
            }
            None => {}
        }
        let active_valence = valences[v_next];
        let clamped = active_valence.clamp(MIN_VALENCE, MAX_VALENCE);
        self.state.active_context = Some(clamped - MIN_VALENCE);
    }

    /// The merged vertex absorbs the source vertex's valence.
    fn merge_vertices(&mut self, dest: usize, source: usize) {
        self.state.vertex_valences[dest] += self.state.vertex_valences[source];
    }

    fn decode_start_face_config(&mut self) -> bool {
        self.shared.decode_start_face_config()
    }

    fn decode_attribute_seams(
        &mut self,
        opposite: &[CornerIdx],
        num_faces: usize,
    ) -> Vec<Vec<bool>> {
        self.shared.decode_attribute_seams(opposite, num_faces)
    }
}
