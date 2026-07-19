//! Standard edgebreaker traversal: CR-light symbol stream, rabs-coded start-face
//! interior flags, and rabs-coded attribute seam bits. Also the topology-split
//! event stream. The valence variant is deferred to milestone B.

use crate::entropy::rans::RabsDecoder;
use crate::Err;
use draco_oxide_core::bit_coder::ByteReader;
use draco_oxide_core::codec::connectivity::edgebreaker::symbol_encoder::Symbol;
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

/// The standard traversal decoder: the CR-light symbol bit stream plus the rabs
/// decoders for start-face configuration and per-attribute seams.
pub struct TraversalDecoder {
    symbols: BitSource,
    start_face: RabsDecoder<Rev>,
    seams: Vec<RabsDecoder<Rev>>,
}

impl TraversalDecoder {
    /// Sets up all three sub-streams in the order the encoder wrote them: symbol
    /// stream, start-face rabs stream, then one seam rabs stream per attribute.
    pub fn start<R: ByteReader>(reader: &mut R, num_attribute_data: usize) -> Result<Self, Err> {
        let symbol_len = leb128_read(reader)? as usize;
        let symbols = BitSource::new(read_bytes(reader, symbol_len)?);

        let start_face = start_rabs(reader)?;

        let mut seams = Vec::with_capacity(num_attribute_data);
        for _ in 0..num_attribute_data {
            seams.push(start_rabs(reader)?);
        }

        Ok(TraversalDecoder {
            symbols,
            start_face,
            seams,
        })
    }

    /// Decodes the next CR-light symbol: one bit for `C`, otherwise a leading `1`
    /// plus two suffix bits forming the pattern `1 | (suffix << 1)`.
    pub fn decode_symbol(&mut self) -> Symbol {
        if self.symbols.read_bit() == 0 {
            return Symbol::C;
        }
        let suffix = self.symbols.read_bits(2);
        match 1 | (suffix << 1) {
            1 => Symbol::S,
            3 => Symbol::L,
            5 => Symbol::R,
            _ => Symbol::E,
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
