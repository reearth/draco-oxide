//! Edgebreaker traversal decoding: the standard and valence variants, the
//! shared start-face and seam streams, and the topology-split stream.

use crate::entropy::rans::RabsDecoder;
use crate::reader::RevReader;
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::Symbol;
use draco_oxide_core::codec::connectivity::edgebreaker::{MAX_VALENCE, MIN_VALENCE};
use draco_oxide_core::types::{CornerIdx, VertexIdx};
use draco_oxide_core::utils::bit_coder::leb128_read;

/// A rabs decoder over a `[prob_zero | leb128 len | bytes]` sub-stream.
fn start_rabs<'a>(reader: &mut Reader<'a>) -> Result<RabsDecoder<'a>, Err> {
    let prob_zero = reader.read_u8()?;
    let len = leb128_read(reader)? as usize;
    let rev = RevReader::new(reader.read_bytes(len)?);
    RabsDecoder::new(rev, prob_zero)
}

/// LSB-first bit reader.
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

/// A topology-split event in encoder-symbol-id terms.
pub struct TopologySplit {
    pub source_symbol_id: usize,
    pub split_symbol_id: usize,
    pub source_edge_right: bool,
}

/// Decodes the topology-split stream: leb128 count, delta-coded id pairs,
/// one edge bit per split.
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

    if num_splits > 0 {
        let edge_bytes = reader.read_bytes(num_splits.div_ceil(8))?;
        let mut bits = BitSource::new(edge_bytes);
        for split in &mut splits {
            split.source_edge_right = bits.read_bit() & 1 == 1;
        }
    }

    Ok(splits)
}

/// Per-stream seam statistics gathered during the seam scan.
pub struct SeamStats {
    /// Whether any interior edge is a seam.
    pub has_interior: bool,
    /// Marked corners; each is the left seam of one sector-start corner.
    pub starts: usize,
    /// Position vertices with at least one sector start in their fan.
    pub fans_with_starts: usize,
}

/// The rabs streams shared by both traversal variants.
struct SharedStreams<'a> {
    start_face: RabsDecoder<'a>,
    seams: Vec<RabsDecoder<'a>>,
}

/// The seam-marking state of the scan: packed per-corner flags plus the
/// running statistics.
struct SeamAcc {
    seam_bits: Vec<u8>,
    starts: Vec<usize>,
    fans_with_starts: Vec<usize>,
    marks: Vec<usize>,
    has_start: Vec<Vec<bool>>,
}

impl SeamAcc {
    fn new(num_streams: usize, num_corners: usize, num_vertices: usize) -> Self {
        Self {
            seam_bits: vec![0; num_corners],
            starts: vec![0; num_streams],
            fans_with_starts: vec![0; num_streams],
            marks: vec![0; num_streams],
            has_start: vec![vec![false; num_vertices]; num_streams],
        }
    }

    /// Counts the sector start of a seam mark at corner `c` for stream `i`.
    ///
    /// # Safety
    /// `c` must be a corner of the table and `c2v[c.previous()]` must be below
    /// the vertex count this accumulator was created with.
    #[inline]
    unsafe fn count_start(&mut self, i: usize, c: CornerIdx, c2v: &[VertexIdx]) {
        self.starts[i] += 1;
        self.marks[i] += 1;
        let v = usize::from(*c2v.get_unchecked(usize::from(c.previous())));
        let has = self.has_start[i].get_unchecked_mut(v);
        if !*has {
            *has = true;
            self.fans_with_starts[i] += 1;
        }
    }

    /// Sets stream `i`'s seam bit at `c` and counts its sector start.
    ///
    /// # Safety
    /// As for [`Self::count_start`].
    #[inline]
    unsafe fn mark(&mut self, i: usize, c: CornerIdx, c2v: &[VertexIdx]) {
        *self.seam_bits.get_unchecked_mut(usize::from(c)) |= 1 << i;
        self.count_start(i, c, c2v);
    }

    fn finish(self, num_boundary_corners: usize) -> (Vec<u8>, Vec<SeamStats>) {
        let stats = self
            .marks
            .iter()
            .zip(self.starts.iter().zip(&self.fans_with_starts))
            .map(|(&marks, (&starts, &fans_with_starts))| SeamStats {
                has_interior: marks > num_boundary_corners,
                starts,
                fans_with_starts,
            })
            .collect();
        (self.seam_bits, stats)
    }
}

impl<'a> SharedStreams<'a> {
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

    /// Decodes the seam edges over the final corner table, faces in id order: a
    /// boundary edge is an automatic seam with no bit; an interior edge decodes
    /// one bit per stream, once from its lower-id face, marking both corners.
    fn decode_attribute_seams(
        &mut self,
        opposite: &[CornerIdx],
        num_faces: usize,
        c2v: &[VertexIdx],
        num_vertices: usize,
    ) -> (Vec<u8>, Vec<SeamStats>) {
        let num_corners = num_faces * 3;
        let mut acc = SeamAcc::new(self.seams.len(), num_corners, num_vertices);
        let seams = std::mem::take(&mut self.seams);
        let num_boundary = match seams.len() {
            0 => 0,
            1 => {
                let decs: [RabsDecoder<'a>; 1] = seams.try_into().ok().expect("length checked");
                decode_seams_fixed(decs, opposite, num_faces, c2v, &mut acc)
            }
            2 => {
                let decs: [RabsDecoder<'a>; 2] = seams.try_into().ok().expect("length checked");
                decode_seams_fixed(decs, opposite, num_faces, c2v, &mut acc)
            }
            _ => decode_seams_general(seams, opposite, num_faces, c2v, &mut acc),
        };
        acc.finish(num_boundary)
    }
}

/// The seam scan monomorphized on the stream count, decoder states in locals.
/// Returns the boundary corner count.
fn decode_seams_fixed<const N: usize>(
    mut decs: [RabsDecoder<'_>; N],
    opposite: &[CornerIdx],
    num_faces: usize,
    c2v: &[VertexIdx],
    acc: &mut SeamAcc,
) -> usize {
    debug_assert!(opposite.len() == num_faces * 3);
    let mut num_boundary = 0usize;
    for f in 0..num_faces {
        let corner = CornerIdx::from(3 * f);
        for c in [corner, corner.next(), corner.previous()] {
            // SAFETY: c < num_faces * 3 == opposite.len(), and every seam vec
            // has that same length; a non-INVALID `opp` is itself a corner id
            // below num_faces * 3 (reconstruct's corner-bound contract), and
            // c2v holds vertex ids below the reconstruction's vertex count.
            let opp = unsafe { *opposite.get_unchecked(usize::from(c)) };
            if opp == CornerIdx::INVALID {
                num_boundary += 1;
                // SAFETY: c < num_faces * 3 == seam_bits.len().
                unsafe {
                    *acc.seam_bits.get_unchecked_mut(usize::from(c)) = (1u16 << N) as u8 - 1;
                }
                for i in 0..N {
                    unsafe { acc.count_start(i, c, c2v) };
                }
                continue;
            }
            if usize::from(opp.face_idx()) < f {
                continue;
            }
            for (i, dec) in decs.iter_mut().enumerate() {
                if dec.decode_bit() {
                    unsafe {
                        acc.mark(i, c, c2v);
                        acc.mark(i, opp, c2v);
                    }
                }
            }
        }
    }
    num_boundary
}

/// Fallback scan for stream counts without a monomorphization.
fn decode_seams_general(
    mut decs: Vec<RabsDecoder<'_>>,
    opposite: &[CornerIdx],
    num_faces: usize,
    c2v: &[VertexIdx],
    acc: &mut SeamAcc,
) -> usize {
    debug_assert!(opposite.len() == num_faces * 3);
    let mut num_boundary = 0usize;
    for f in 0..num_faces {
        let corner = CornerIdx::from(3 * f);
        for c in [corner, corner.next(), corner.previous()] {
            // SAFETY: as in `decode_seams_fixed`.
            let opp = unsafe { *opposite.get_unchecked(usize::from(c)) };
            if opp == CornerIdx::INVALID {
                num_boundary += 1;
                // SAFETY: c < num_faces * 3 == seam_bits.len().
                unsafe {
                    *acc.seam_bits.get_unchecked_mut(usize::from(c)) =
                        ((1u16 << decs.len()) - 1) as u8;
                }
                for i in 0..decs.len() {
                    unsafe { acc.count_start(i, c, c2v) };
                }
                continue;
            }
            if usize::from(opp.face_idx()) < f {
                continue;
            }
            for (i, dec) in decs.iter_mut().enumerate() {
                if dec.decode_bit() {
                    unsafe {
                        acc.mark(i, c, c2v);
                        acc.mark(i, opp, c2v);
                    }
                }
            }
        }
    }
    num_boundary
}

/// One edgebreaker traversal variant.
pub trait TraversalDecoder {
    /// Decodes the next edgebreaker symbol.
    fn decode_symbol(&mut self) -> Result<Symbol, Err>;

    /// Hook after each symbol's face; only the valence variant overrides it.
    fn new_active_corner_reached(&mut self, v_corner: usize, v_next: usize, v_prev: usize) {
        let _ = (v_corner, v_next, v_prev);
    }

    /// Hook for the S-symbol vertex merge; only the valence variant overrides it.
    fn merge_vertices(&mut self, dest: usize, source: usize) {
        let _ = (dest, source);
    }

    /// Decodes one start-face configuration bit.
    fn decode_start_face_config(&mut self) -> bool;

    fn decode_attribute_seams(
        &mut self,
        opposite: &[CornerIdx],
        num_faces: usize,
        c2v: &[VertexIdx],
        num_vertices: usize,
    ) -> (Vec<u8>, Vec<SeamStats>);
}

/// Standard traversal: symbols from a single CR-light bit stream.
pub struct StandardTraversalDecoder<'a> {
    symbols: BitSource<'a>,
    shared: SharedStreams<'a>,
}

impl<'a> StandardTraversalDecoder<'a> {
    pub fn start(reader: &mut Reader<'a>, num_attribute_data: usize) -> Result<Self, Err> {
        let symbol_len = leb128_read(reader)? as usize;
        let symbols = BitSource::new(reader.read_bytes(symbol_len)?);
        let shared = SharedStreams::read(reader, num_attribute_data)?;
        Ok(Self { symbols, shared })
    }
}

impl TraversalDecoder for StandardTraversalDecoder<'_> {
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
        c2v: &[VertexIdx],
        num_vertices: usize,
    ) -> (Vec<u8>, Vec<SeamStats>) {
        self.shared
            .decode_attribute_seams(opposite, num_faces, c2v, num_vertices)
    }
}

/// Valence traversal state: per-context symbol lists consumed from the back,
/// with valences maintained to recover each symbol's context.
struct ValenceState {
    context_symbols: Vec<Vec<u64>>,
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
        c2v: &[VertexIdx],
        num_vertices: usize,
    ) -> (Vec<u8>, Vec<SeamStats>) {
        self.shared
            .decode_attribute_seams(opposite, num_faces, c2v, num_vertices)
    }
}
