//! Entropy decoding: [`decode_symbols`] over the rANS/rabs decoders in
//! [`rans`], plus the [`unzigzag`] inverse of core's `to_positive_i32`.
//!
//! Two symbol streams exist on the wire. A DirectCoded stream rANS-codes the
//! values themselves. A LengthCoded stream rANS-codes one bit width per value
//! group and packs the values behind it as raw bit fields of that width; the
//! encoder picks whichever it estimates to be smaller.

pub mod rans;

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
use rans::RansSymbolDecoder;

/// The rANS precision of a LengthCoded stream's tag alphabet. The tags span
/// 1..=32, so the encoder's `clamp(3 * 5 / 2, 12, 20)` fixes this at 12 and no
/// bit-length byte is written for it.
const TAG_PRECISION: usize = 12;

/// The widest bit width a tag may name.
const MAX_TAG_BIT_LENGTH: u8 = 32;

/// Decodes `num_values * num_components` symbols from `reader`. Mirrors Google's
/// `DecodeSymbols`: reads the encoding-method byte and dispatches. `num_components`
/// sizes the value groups a LengthCoded stream shares a bit width across.
pub fn decode_symbols(
    reader: &mut Reader<'_>,
    num_values: usize,
    num_components: usize,
) -> Result<Vec<u64>, Err> {
    let num_symbols = num_values * num_components;
    let method = SymbolEncodingMethod::read_from(reader)?;
    match method {
        SymbolEncodingMethod::DirectCoded => decode_symbols_direct(reader, num_symbols),
        SymbolEncodingMethod::LengthCoded => {
            let mut decoder = start_tagged_decoder(reader, num_symbols, num_components)?;
            Ok((0..num_symbols).map(|_| decoder.decode() as u64).collect())
        }
    }
}

/// The rANS precision for a DirectCoded stream's bit-length byte, the same
/// mapping the encoder uses (`clamp(3 * bit_length / 2, 12, 20)`).
fn precision_for_bit_length(bit_length: u8) -> Result<usize, Err> {
    match bit_length {
        1..=8 => Ok(12),
        9 => Ok(13),
        10 => Ok(15),
        11 => Ok(16),
        12 => Ok(18),
        13 => Ok(19),
        14..=18 => Ok(20),
        _ => Err(Err::InvalidBitLength(bit_length)),
    }
}

/// Decodes `num_symbols` DirectCoded symbols.
fn decode_symbols_direct(reader: &mut Reader<'_>, num_symbols: usize) -> Result<Vec<u64>, Err> {
    let bit_length = reader.read_u8()?;
    let precision = precision_for_bit_length(bit_length)?;
    let mut decoder = RansSymbolDecoder::new(reader, num_symbols, precision)?;
    let mut out = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols {
        out.push(decoder.decode() as u64);
    }
    Ok(out)
}

/// An LSB-first cursor over a borrowed bit field, matching Google's
/// `DecoderBuffer::BitDecoder`: bit `i` of a value comes from stream bit
/// `pos + i`, and bits are numbered from the least significant within a byte.
/// Reads past the end yield zero bits, so decoding a truncated stream produces
/// garbage rather than panicking.
struct BitCursor<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitCursor { data, bit_pos: 0 }
    }

    /// Reads the next `size` bits, `size <= 32`.
    #[inline]
    fn read_bits(&mut self, size: u8) -> u32 {
        if size == 0 {
            return 0;
        }
        let byte = self.bit_pos >> 3;
        let shift = (self.bit_pos & 7) as u32;
        // `shift + size` never exceeds 39 bits, so eight bytes always cover the
        // field; only the tail of the buffer needs the byte-wise gather.
        let window = match self.data.get(byte..byte + 8) {
            Some(w) => u64::from_le_bytes(w.try_into().unwrap()),
            None => {
                let mut w = 0u64;
                for i in 0..8 {
                    w |= (self.data.get(byte + i).copied().unwrap_or(0) as u64) << (8 * i);
                }
                w
            }
        };
        self.bit_pos += size as usize;
        ((window >> shift) & ((1u64 << size) - 1)) as u32
    }
}

/// A live LengthCoded symbol decoder. The tags are decoded up front (their sum
/// is what locates the end of the value field, which carries no length prefix
/// of its own); the values themselves stay in the buffer and are read on demand.
pub struct TaggedSymbolDecoder<'a> {
    /// Bit width of each value group, in group order.
    tags: Vec<u8>,
    values: BitCursor<'a>,
    num_components: u8,
    next_tag: usize,
    /// Components still to emit from the current group.
    remaining: u8,
    /// Bit width of the current group.
    width: u8,
}

impl TaggedSymbolDecoder<'_> {
    /// Decodes the next symbol.
    #[inline]
    pub fn decode(&mut self) -> usize {
        if self.remaining == 0 {
            self.width = self.tags.get(self.next_tag).copied().unwrap_or(0);
            self.next_tag += 1;
            self.remaining = self.num_components;
        }
        self.remaining -= 1;
        self.values.read_bits(self.width) as usize
    }
}

/// A live symbol decoder over either stream kind, for consumers that pop
/// symbols one at a time instead of batch-decoding. Symbols come out in the
/// same order [`decode_symbols`] returns them. Callers match on the variant
/// rather than popping through the enum, so the per-symbol pop stays
/// monomorphic in the hot walk.
pub enum AnySymbolDecoder<'a> {
    /// Decoder over a DirectCoded stream.
    Direct(RansSymbolDecoder<'a>),
    /// Decoder over a LengthCoded stream.
    Tagged(TaggedSymbolDecoder<'a>),
}

/// Parses a LengthCoded stream: the tag alphabet and its rANS payload, then the
/// bit field holding the values. Leaves `reader` immediately after the field.
fn start_tagged_decoder<'a>(
    reader: &mut Reader<'a>,
    num_symbols: usize,
    num_components: usize,
) -> Result<TaggedSymbolDecoder<'a>, Err> {
    let num_components = num_components.max(1);
    if num_components > u8::MAX as usize {
        return Err(Err::MalformedAttribute("component count exceeds 255"));
    }
    let num_tags = num_symbols.div_ceil(num_components);

    let mut tag_decoder = RansSymbolDecoder::new(reader, num_tags, TAG_PRECISION)?;
    let mut tags = Vec::with_capacity(num_tags);
    let mut total_bits = 0usize;
    for _ in 0..num_tags {
        let tag = tag_decoder.decode();
        if tag > MAX_TAG_BIT_LENGTH as usize {
            return Err(Err::InvalidBitLength(tag as u8));
        }
        tags.push(tag as u8);
        total_bits += tag * num_components;
    }

    // The value field is appended raw, so its length is only known once every
    // tag is in hand; it is padded to a whole number of bytes.
    let values = reader.read_bytes(total_bits.div_ceil(8))?;

    Ok(TaggedSymbolDecoder {
        tags,
        values: BitCursor::new(values),
        num_components: num_components as u8,
        next_tag: 0,
        remaining: 0,
        width: 0,
    })
}

/// Parses a symbol stream's framing and frequency table, returning a live
/// decoder positioned before the first symbol. The reader is left immediately
/// after the stream's payload, so parsing may continue past it while the
/// returned decoder is drained later.
pub fn start_symbol_decoder<'a>(
    reader: &mut Reader<'a>,
    num_symbols: usize,
    num_components: usize,
) -> Result<AnySymbolDecoder<'a>, Err> {
    match SymbolEncodingMethod::read_from(reader)? {
        SymbolEncodingMethod::DirectCoded => {
            let bit_length = reader.read_u8()?;
            let precision = precision_for_bit_length(bit_length)?;
            Ok(AnySymbolDecoder::Direct(RansSymbolDecoder::new(
                reader,
                num_symbols,
                precision,
            )?))
        }
        SymbolEncodingMethod::LengthCoded => Ok(AnySymbolDecoder::Tagged(start_tagged_decoder(
            reader,
            num_symbols,
            num_components,
        )?)),
    }
}

/// Inverse of core's `to_positive_i32` zigzag mapping: even codes to non-negative,
/// odd codes to negative.
pub fn unzigzag(val: u32) -> i32 {
    ((val >> 1) as i32) ^ -((val & 1) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use draco_oxide_core::utils::to_positive_i32;

    #[test]
    fn unzigzag_inverts_to_positive_i32() {
        for n in -5000i32..=5000 {
            let code = to_positive_i32(n) as u32;
            assert_eq!(unzigzag(code), n);
        }
    }

    #[test]
    fn bit_cursor_reads_lsb_first_across_bytes() {
        // Bit `i` of a field comes from stream bit `pos + i`, numbering bits
        // within a byte from the least significant. The 6-bit field spans the
        // byte boundary: five bits from the first byte, one from the second.
        let data = [0b1011_0101u8, 0b0100_1110];
        let mut cursor = BitCursor::new(&data);
        assert_eq!(cursor.read_bits(3), 0b101);
        assert_eq!(cursor.read_bits(6), 0b01_0110);
        assert_eq!(cursor.read_bits(7), 0b010_0111);
        // Reads past the end yield zeros rather than panicking.
        assert_eq!(cursor.read_bits(8), 0);
    }
}
