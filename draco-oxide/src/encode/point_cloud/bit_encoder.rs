//! Bit encoders backing the kd-tree point encoder.

use crate::encode::entropy::rans::{encode_rabs_bit_stream, Err as EntropyErr};
use draco_oxide_core::bit_coder::ByteWriter;

/// A bit encoder over one sub-stream of the kd-tree payload. `encode_lsb32`
/// takes `nbits` in `1..=32`, most significant bit first.
pub(super) trait BitEncoder: Default {
    fn encode_bit(&mut self, bit: bool);
    fn encode_lsb32(&mut self, nbits: u32, value: u32);
    fn end<W: ByteWriter>(self, writer: &mut W) -> Result<(), EntropyErr>;
}

/// Uncompressed bits packed into u32 words, framed by a u32 byte size.
#[derive(Default)]
pub(super) struct DirectBitEncoder {
    words: Vec<u32>,
    local: u32,
    num_local: u32,
}

impl BitEncoder for DirectBitEncoder {
    fn encode_bit(&mut self, bit: bool) {
        if bit {
            self.local |= 1 << (31 - self.num_local);
        }
        self.num_local += 1;
        if self.num_local == 32 {
            self.words.push(self.local);
            self.local = 0;
            self.num_local = 0;
        }
    }

    fn encode_lsb32(&mut self, nbits: u32, value: u32) {
        let remaining = 32 - self.num_local;
        let aligned = if nbits == 32 {
            value
        } else {
            value << (32 - nbits)
        };
        if nbits <= remaining {
            self.local |= aligned >> self.num_local;
            self.num_local += nbits;
            if self.num_local == 32 {
                self.words.push(self.local);
                self.local = 0;
                self.num_local = 0;
            }
        } else {
            let value = aligned >> (32 - nbits);
            self.num_local = nbits - remaining;
            self.local |= value >> self.num_local;
            self.words.push(self.local);
            self.local = value << (32 - self.num_local);
        }
    }

    fn end<W: ByteWriter>(mut self, writer: &mut W) -> Result<(), EntropyErr> {
        // The trailing partial word is always emitted: the size stays a
        // multiple of four and the stream is never empty.
        self.words.push(self.local);
        writer.write_u32((self.words.len() * 4) as u32);
        for word in self.words {
            writer.write_u32(word);
        }
        Ok(())
    }
}

/// Binary rANS over a `[zero_prob][leb128 len][bytes]` sub-stream.
#[derive(Default)]
pub(super) struct RansBitEncoder {
    bits: Vec<bool>,
}

impl BitEncoder for RansBitEncoder {
    fn encode_bit(&mut self, bit: bool) {
        self.bits.push(bit);
    }

    fn encode_lsb32(&mut self, nbits: u32, value: u32) {
        for i in (0..nbits).rev() {
            self.bits.push(value & (1 << i) != 0);
        }
    }

    fn end<W: ByteWriter>(self, writer: &mut W) -> Result<(), EntropyErr> {
        encode_rabs_bit_stream(&self.bits, writer)
    }
}

/// 32 per-bit-position sub-coders plus one plain bit coder.
pub(super) struct FoldedBitEncoder {
    folded: Vec<RansBitEncoder>,
    bits: RansBitEncoder,
}

impl Default for FoldedBitEncoder {
    fn default() -> Self {
        Self {
            folded: (0..32).map(|_| RansBitEncoder::default()).collect(),
            bits: RansBitEncoder::default(),
        }
    }
}

impl BitEncoder for FoldedBitEncoder {
    fn encode_bit(&mut self, bit: bool) {
        self.bits.encode_bit(bit);
    }

    fn encode_lsb32(&mut self, nbits: u32, value: u32) {
        for i in 0..nbits {
            let bit = value & (1 << (nbits - 1 - i)) != 0;
            self.folded[i as usize].encode_bit(bit);
        }
    }

    fn end<W: ByteWriter>(self, writer: &mut W) -> Result<(), EntropyErr> {
        for sub in self.folded {
            sub.end(writer)?;
        }
        self.bits.end(writer)
    }
}
