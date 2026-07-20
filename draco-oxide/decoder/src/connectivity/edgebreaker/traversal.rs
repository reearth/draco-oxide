//! Edgebreaker traversal decoding: the standard variant (CR-light symbol bit
//! stream) and the valence variant (per-valence-context rANS symbol streams),
//! plus the shared rabs-coded start-face interior flags and attribute seam bits,
//! and the topology-split event stream.

use crate::entropy::rans::RabsDecoder;
use crate::Err;
use draco_oxide_core::bit_coder::ByteReader;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::Symbol;
use draco_oxide_core::codec::connectivity::edgebreaker::{MAX_VALENCE, MIN_VALENCE};
use draco_oxide_core::utils::bit_coder::leb128_read;

type Rev = <std::vec::IntoIter<u8> as ByteReader>::Rev;

/// Reads exactly `n` bytes from `reader`, keeping it positioned after them.
fn read_bytes<R: ByteReader>(reader: &mut R, n: usize) -> Result<Vec<u8>, Err> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(reader.read_u8()?);
    }
    Ok(out)
}

/// Builds a rabs decoder over a self-contained `[prob_zero | leb128 len | bytes]`
/// sub-stream read from `reader`.
fn start_rabs<R: ByteReader>(reader: &mut R) -> Result<RabsDecoder<Rev>, Err> {
    let prob_zero = reader.read_u8()?;
    let len = leb128_read(reader)? as usize;
    let bytes = read_bytes(reader, len)?;
    let mut iter = bytes.into_iter();
    let rev = iter.spown_reverse_reader_at(len)?;
    RabsDecoder::new(rev, prob_zero)
}

/// LSB-first bit reader over a fixed byte buffer, matching Google's
/// `DecoderBuffer::BitDecoder` (used for the symbol stream and split-edge bits).
struct BitSource {
    bytes: Vec<u8>,
    byte_pos: usize,
    bit_pos: u8,
}

impl BitSource {
    fn new(bytes: Vec<u8>) -> Self {
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
pub fn decode_topology_splits<R: ByteReader>(reader: &mut R) -> Result<Vec<TopologySplit>, Err> {
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
        let edge_bytes = read_bytes(reader, num_splits.div_ceil(8))?;
        let mut bits = BitSource::new(edge_bytes);
        for split in &mut splits {
            split.source_edge_right = bits.read_bit() & 1 == 1;
        }
    }

    Ok(splits)
}

/// The traversal variant on the wire, as selected by the leading traversal-type
/// byte of the edgebreaker section.
pub enum TraversalKind {
    Standard,
    Valence,
}

/// Symbol-stream state of the two traversal variants.
enum SymbolSource {
    /// The CR-light symbol bit stream.
    Standard {
        symbols: BitSource,
    },
    Valence(ValenceState),
}

/// State of the valence traversal: symbols are grouped by the entropy context
/// (the clamped valence of the active vertex), and the context of each symbol
/// is recovered by maintaining the valences of the partially reconstructed
/// mesh, mirroring the encoder in reverse.
struct ValenceState {
    /// Per-context symbol ids, consumed from the back.
    context_symbols: Vec<Vec<u64>>,
    /// Valence of the decoded portion of the mesh per reconstruction vertex.
    vertex_valences: Vec<usize>,
    last_symbol: Option<Symbol>,
    active_context: Option<usize>,
}

/// The traversal decoder: the variant-specific symbol source plus the rabs
/// decoders for start-face configuration and per-attribute seams.
pub struct TraversalDecoder {
    source: SymbolSource,
    start_face: RabsDecoder<Rev>,
    seams: Vec<RabsDecoder<Rev>>,
}

impl TraversalDecoder {
    /// Sets up the sub-streams in the order the encoder wrote them. Standard:
    /// symbol stream, start-face rabs stream, seam rabs streams. Valence:
    /// start-face rabs stream, seam rabs streams, then one rANS symbol stream
    /// per valence context. `max_num_vertices` bounds the valence table
    /// (encoded vertices plus split symbols); `num_faces` bounds each context's
    /// symbol count.
    pub fn start<R: ByteReader>(
        reader: &mut R,
        kind: TraversalKind,
        num_attribute_data: usize,
        max_num_vertices: usize,
        num_faces: usize,
    ) -> Result<Self, Err> {
        let symbols = match kind {
            TraversalKind::Standard => {
                let symbol_len = leb128_read(reader)? as usize;
                Some(BitSource::new(read_bytes(reader, symbol_len)?))
            }
            TraversalKind::Valence => None,
        };

        let start_face = start_rabs(reader)?;

        let mut seams = Vec::with_capacity(num_attribute_data);
        for _ in 0..num_attribute_data {
            seams.push(start_rabs(reader)?);
        }

        let source = match symbols {
            Some(symbols) => SymbolSource::Standard { symbols },
            None => {
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
                SymbolSource::Valence(ValenceState {
                    context_symbols,
                    vertex_valences: vec![0; max_num_vertices],
                    last_symbol: None,
                    active_context: None,
                })
            }
        };

        Ok(TraversalDecoder {
            source,
            start_face,
            seams,
        })
    }

    /// True for the valence variant, which needs the reconstruction hooks
    /// ([`Self::new_active_corner_reached`], [`Self::merge_vertices`]).
    pub fn is_valence(&self) -> bool {
        matches!(self.source, SymbolSource::Valence(_))
    }

    /// Decodes the next edgebreaker symbol. Standard: one CR-light code (one
    /// bit for `C`, otherwise a leading `1` plus two suffix bits forming the
    /// pattern `1 | (suffix << 1)`). Valence: the back of the active context's
    /// symbol list; the very first symbol has no context yet and is always `E`.
    pub fn decode_symbol(&mut self) -> Result<Symbol, Err> {
        match &mut self.source {
            SymbolSource::Standard { symbols } => {
                if symbols.read_bit() == 0 {
                    return Ok(Symbol::C);
                }
                let suffix = symbols.read_bits(2);
                Ok(match 1 | (suffix << 1) {
                    1 => Symbol::S,
                    3 => Symbol::L,
                    5 => Symbol::R,
                    _ => Symbol::E,
                })
            }
            SymbolSource::Valence(state) => {
                let symbol = match state.active_context {
                    None => Symbol::E,
                    Some(ctx) => {
                        let id =
                            state.context_symbols[ctx]
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
                            _ => {
                                return Err(Err::MalformedConnectivity("invalid valence symbol id"))
                            }
                        }
                    }
                };
                state.last_symbol = Some(symbol);
                Ok(symbol)
            }
        }
    }

    /// Valence hook, called with the vertices of the active corner (tip, next,
    /// previous) after each symbol's face is built: applies the decoded
    /// symbol's valence increments and selects the context for the next symbol
    /// from the valence of the next vertex.
    pub fn new_active_corner_reached(&mut self, v_corner: usize, v_next: usize, v_prev: usize) {
        let SymbolSource::Valence(state) = &mut self.source else {
            return;
        };
        let valences = &mut state.vertex_valences;
        match state.last_symbol {
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
        state.active_context = Some(clamped - MIN_VALENCE);
    }

    /// Valence hook for the S-symbol vertex merge: the merged vertex absorbs
    /// the source vertex's valence.
    pub fn merge_vertices(&mut self, dest: usize, source: usize) {
        if let SymbolSource::Valence(state) = &mut self.source {
            state.vertex_valences[dest] += state.vertex_valences[source];
        }
    }

    /// Decodes one start-face configuration bit (true = interior face).
    pub fn decode_start_face_config(&mut self) -> bool {
        self.start_face.decode_bit()
    }

    /// Decodes one attribute-seam bit for attribute `i`.
    pub fn decode_attribute_seam(&mut self, i: usize) -> bool {
        self.seams[i].decode_bit()
    }
}
