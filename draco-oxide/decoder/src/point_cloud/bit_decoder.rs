//! Bit decoders backing the kd-tree point decoder.

use crate::entropy::rans::RabsDecoder;
use crate::reader::RevReader;
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::utils::bit_coder::leb128_read;

/// A bit decoder over one sub-stream of the kd-tree payload. `decode_lsb32`
/// takes `nbits` in `1..=32`, most significant bit first.
pub(super) trait BitDecoder<'a>: Sized {
    fn start(reader: &mut Reader<'a>) -> Result<Self, Err>;
    fn decode_bit(&mut self) -> bool;
    fn decode_lsb32(&mut self, nbits: u32) -> Result<u32, Err>;
}

/// Uncompressed bits packed into u32 words, framed by a u32 byte size.
pub(super) struct DirectBitDecoder {
    words: Vec<u32>,
    pos: usize,
    used: u32,
}

impl<'a> BitDecoder<'a> for DirectBitDecoder {
    fn start(reader: &mut Reader<'a>) -> Result<Self, Err> {
        let size_in_bytes = reader.read_u32()? as usize;
        if size_in_bytes == 0 || size_in_bytes % 4 != 0 || size_in_bytes > reader.remaining() {
            return Err(Err::MalformedAttribute("invalid direct bit stream size"));
        }
        let bytes = reader.read_bytes(size_in_bytes)?;
        let words = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(Self {
            words,
            pos: 0,
            used: 0,
        })
    }

    fn decode_bit(&mut self) -> bool {
        if self.pos == self.words.len() {
            return false;
        }
        let bit = self.words[self.pos] & (1 << (31 - self.used)) != 0;
        self.used += 1;
        if self.used == 32 {
            self.pos += 1;
            self.used = 0;
        }
        bit
    }

    fn decode_lsb32(&mut self, nbits: u32) -> Result<u32, Err> {
        let remaining = 32 - self.used;
        if nbits <= remaining {
            if self.pos == self.words.len() {
                return Err(Err::MalformedAttribute("direct bit stream exhausted"));
            }
            let value = (self.words[self.pos] << self.used) >> (32 - nbits);
            self.used += nbits;
            if self.used == 32 {
                self.pos += 1;
                self.used = 0;
            }
            Ok(value)
        } else {
            if self.pos + 1 >= self.words.len() {
                return Err(Err::MalformedAttribute("direct bit stream exhausted"));
            }
            let value_l = self.words[self.pos] << self.used;
            let new_used = nbits - remaining;
            self.pos += 1;
            let value_r = self.words[self.pos] >> (32 - new_used);
            self.used = new_used;
            Ok((value_l >> (32 - new_used - remaining)) | value_r)
        }
    }
}

/// Binary rANS over a `[zero_prob][leb128 len][bytes]` sub-stream.
pub(super) struct RansBitDecoder<'a> {
    rabs: RabsDecoder<'a>,
}

impl<'a> BitDecoder<'a> for RansBitDecoder<'a> {
    fn start(reader: &mut Reader<'a>) -> Result<Self, Err> {
        let prob_zero = reader.read_u8()?;
        let len = leb128_read(reader)? as usize;
        if len > reader.remaining() {
            return Err(Err::MalformedAttribute("rans bit stream overruns input"));
        }
        let rev = RevReader::new(reader.read_bytes(len)?);
        Ok(Self {
            rabs: RabsDecoder::new(rev, prob_zero)?,
        })
    }

    fn decode_bit(&mut self) -> bool {
        self.rabs.decode_bit()
    }

    fn decode_lsb32(&mut self, nbits: u32) -> Result<u32, Err> {
        let mut value = 0;
        for _ in 0..nbits {
            value = (value << 1) | self.decode_bit() as u32;
        }
        Ok(value)
    }
}

/// 32 per-bit-position sub-coders plus one plain bit coder.
pub(super) struct FoldedBitDecoder<'a> {
    folded: Vec<RansBitDecoder<'a>>,
    bits: RansBitDecoder<'a>,
}

impl<'a> BitDecoder<'a> for FoldedBitDecoder<'a> {
    fn start(reader: &mut Reader<'a>) -> Result<Self, Err> {
        let mut folded = Vec::with_capacity(32);
        for _ in 0..32 {
            folded.push(RansBitDecoder::start(reader)?);
        }
        let bits = RansBitDecoder::start(reader)?;
        Ok(Self { folded, bits })
    }

    fn decode_bit(&mut self) -> bool {
        self.bits.decode_bit()
    }

    fn decode_lsb32(&mut self, nbits: u32) -> Result<u32, Err> {
        let mut value = 0;
        for i in 0..nbits as usize {
            value = (value << 1) | self.folded[i].decode_bit() as u32;
        }
        Ok(value)
    }
}
