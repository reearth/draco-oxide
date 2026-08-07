//! rANS and rabs decoder state machines, plus the [`RansSymbolDecoder`] that parses
//! the frequency-table format written by the encoder's `RansSymbolEncoder`.
//!
//! These mirror Google Draco's `ans.h` decoders exactly (renormalize-before-decode,
//! `ans_read_init` state layout), which is required because our encoder emits the
//! same byte stream Google's encoder does. The renormalized bytes are consumed
//! back-to-front from the tail of the buffer via a [`RevReader`].

use crate::reader::RevReader;
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::entropy::{rans_symbol_table, RansSymbol};
use draco_oxide_core::utils::bit_coder::leb128_read;

/// Reads the final rANS/rabs state from the tail of the reversed buffer. The most
/// significant byte (written last, hence read first) carries a 2-bit size tag in
/// its top bits selecting a u6/u14/u22/u30 little-endian layout; `l_base` is added
/// back to undo the encoder's flush subtraction.
fn read_state_init(rev: &mut RevReader<'_>, l_base: usize) -> Result<usize, Err> {
    let msb = rev.read_u8_back()?;
    let tag = msb >> 6;
    let low6 = (msb & 0x3F) as usize;
    let state = match tag {
        0 => low6,
        1 => (low6 << 8) | rev.read_u8_back()? as usize,
        2 => {
            let b1 = rev.read_u8_back()? as usize;
            let b2 = rev.read_u8_back()? as usize;
            (low6 << 16) | (b1 << 8) | b2
        }
        _ => {
            let b1 = rev.read_u8_back()? as usize;
            let b2 = rev.read_u8_back()? as usize;
            let b3 = rev.read_u8_back()? as usize;
            (low6 << 24) | (b1 << 16) | (b2 << 8) | b3
        }
    };
    Ok(state + l_base)
}

/// Builds the slot-to-symbol lookup table: entry `r` is the symbol whose
/// cumulative range contains `r`. The input must come from
/// [`rans_symbol_table`], so the ranges tile `0..2^precision`.
/// `T` is the entry width; the alphabet's largest index must fit in it.
fn rans_slot_table<T: Copy + Default + TryFrom<usize>>(rans_symbols: &[RansSymbol]) -> Vec<T> {
    let total = rans_symbols
        .last()
        .map(|s| (s.freq_cumulative + s.freq_count) as usize)
        .unwrap_or(0);
    let mut slot_table = vec![T::default(); total];
    for (i, sym) in rans_symbols.iter().enumerate() {
        let start = sym.freq_cumulative as usize;
        let end = start + sym.freq_count as usize;
        let entry = T::try_from(i).unwrap_or_else(|_| unreachable!());
        slot_table[start..end].fill(entry);
    }
    slot_table
}

/// Slot-to-symbol lookup strategy. The table entry width follows the alphabet
/// size so the `2^precision`-entry table stays as small as possible; the
/// table is walked at a random slot per symbol, so its cache footprint is paid
/// on every decode.
enum SlotTable {
    U16(Vec<u16>),
    U32(Vec<u32>),
    BinarySearch,
}

/// Non-binary rANS decoder over a fixed symbol distribution. `precision` is the
/// log2 of the total frequency count and must match the value the encoder used.
///
/// The slot-to-symbol lookup is either a `2^precision`-entry table (O(1) per
/// symbol, but the build cost dominates short streams) or a binary search over the
/// cumulative frequencies; the caller picks via `use_lut`.
pub struct RansDecoder<'a> {
    rev: RevReader<'a>,
    state: usize,
    slot_table: SlotTable,
    rans_symbols: Vec<RansSymbol>,
    /// Renormalization strategy, fixed per stream: the branchless fold wins on
    /// high-entropy streams where the refill byte count is unpredictable, the
    /// byte loop on low-entropy streams where it is almost always zero.
    refill_branchless: bool,
    precision: u32,
    /// `4 << precision`, the renormalization threshold.
    l_base: usize,
}

impl<'a> RansDecoder<'a> {
    /// Initializes the decoder from the reversed buffer and a symbol distribution
    /// (as produced by [`rans_symbol_table`]).
    pub fn new(
        mut rev: RevReader<'a>,
        rans_symbols: Vec<RansSymbol>,
        use_lut: bool,
        refill_branchless: bool,
        precision: usize,
    ) -> Result<Self, Err> {
        let l_base = 4usize << precision;
        let state = read_state_init(&mut rev, l_base)?;
        let slot_table = if !use_lut {
            SlotTable::BinarySearch
        } else if rans_symbols.len() <= 1 << 16 {
            SlotTable::U16(rans_slot_table(&rans_symbols))
        } else {
            SlotTable::U32(rans_slot_table(&rans_symbols))
        };
        Ok(RansDecoder {
            rev,
            state,
            slot_table,
            rans_symbols,
            refill_branchless,
            precision: precision as u32,
            l_base,
        })
    }

    /// Decodes the next symbol index.
    #[inline(always)]
    pub fn read(&mut self) -> usize {
        if self.refill_branchless {
            self.state = self.rev.rans_refill(self.state, self.l_base);
        } else {
            while self.state < self.l_base {
                match self.rev.read_u8_back() {
                    Ok(byte) => self.state = (self.state << 8) | byte as usize,
                    Err(_) => break,
                }
            }
        }
        let quo = self.state >> self.precision;
        let rem = self.state & ((self.l_base >> 2) - 1);
        let symbol = match &self.slot_table {
            SlotTable::U16(table) => table[rem] as usize,
            SlotTable::U32(table) => table[rem] as usize,
            // The last symbol whose cumulative start is <= rem. Ties from
            // zero-frequency symbols resolve to the last of the group, which is
            // the one whose range actually contains rem.
            SlotTable::BinarySearch => {
                self.rans_symbols
                    .partition_point(|s| s.freq_cumulative as usize <= rem)
                    - 1
            }
        };
        let sym = &self.rans_symbols[symbol];
        self.state = quo * sym.freq_count as usize + rem - sym.freq_cumulative as usize;
        symbol
    }
}

/// Binary rANS (rabs) decoder. `prob_zero` is the frequency of the zero bit out of
/// `2^8`, matching the encoder's `RabsCoder`.
pub struct RabsDecoder<'a> {
    rev: RevReader<'a>,
    state: usize,
    prob_zero: usize,
}

impl<'a> RabsDecoder<'a> {
    const RABS_PRECISION_VALUE: usize = 1 << 8;
    const L_RABS_BASE: usize = draco_oxide_core::codec::entropy::L_RANS_BASE;

    /// Initializes the decoder from the reversed buffer.
    pub fn new(mut rev: RevReader<'a>, prob_zero: u8) -> Result<Self, Err> {
        let state = read_state_init(&mut rev, Self::L_RABS_BASE)?;
        Ok(RabsDecoder {
            rev,
            state,
            prob_zero: prob_zero as usize,
        })
    }

    /// Decodes the next bit.
    pub fn decode_bit(&mut self) -> bool {
        if self.state < Self::L_RABS_BASE {
            if let Ok(byte) = self.rev.read_u8_back() {
                self.state = (self.state << 8) | byte as usize;
            }
        }
        let freq_count_1 = Self::RABS_PRECISION_VALUE - self.prob_zero;
        let quo = self.state / Self::RABS_PRECISION_VALUE;
        let rem = self.state % Self::RABS_PRECISION_VALUE;
        let bit = rem < freq_count_1;
        self.state = if bit {
            quo * freq_count_1 + rem
        } else {
            quo * self.prob_zero + rem - freq_count_1
        };
        bit
    }
}

/// Decodes a sequence of symbols coded with the encoder's `RansSymbolEncoder`:
/// parses the leb128 alphabet size and per-symbol frequency table (with zero-run
/// flags), rebuilds the decode tables, and drives a [`RansDecoder`] over the
/// length-prefixed rANS payload.
pub struct RansSymbolDecoder<'a> {
    decoder: RansDecoder<'a>,
}

impl<'a> RansSymbolDecoder<'a> {
    /// Parses the frequency table and rANS payload from `reader`, leaving `reader`
    /// positioned immediately after the payload. `num_symbols_to_decode` sizes the
    /// lookup strategy: long streams amortize the `2^precision`-entry slot
    /// table, short ones decode faster with a binary search per symbol.
    pub fn new(
        reader: &mut Reader<'a>,
        num_symbols_to_decode: usize,
        precision: usize,
    ) -> Result<Self, Err> {
        let num_symbols = leb128_read(reader)? as usize;
        let mut freq_counts = vec![0usize; num_symbols];

        let mut i = 0;
        while i < num_symbols {
            let prob_data = reader.read_u8()?;
            let token = prob_data & 3;
            if token == 3 {
                // Zero-run: this symbol plus the next `offset` symbols have zero
                // frequency. They are already zero in `freq_counts`.
                let offset = (prob_data >> 2) as usize;
                i += offset;
            } else {
                let num_extra_bytes = token as usize;
                let mut freq = (prob_data >> 2) as usize;
                for b in 0..num_extra_bytes {
                    let extra = reader.read_u8()? as usize;
                    freq |= extra << (8 * (b + 1) - 2);
                }
                freq_counts[i] = freq;
            }
            i += 1;
        }

        let rans_symbols = rans_symbol_table(&freq_counts, precision)?;
        let use_lut = num_symbols_to_decode >= (1 << precision) >> 6;

        let payload_len = leb128_read(reader)? as usize;
        // Branchless refill pays off once the refill byte count is unpredictable,
        // which the stream's average bytes per symbol proxies well.
        let refill_branchless = payload_len * 5 >= num_symbols_to_decode * 2;
        let rev = RevReader::new(reader.read_bytes(payload_len)?);
        let decoder = RansDecoder::new(rev, rans_symbols, use_lut, refill_branchless, precision)?;
        Ok(RansSymbolDecoder { decoder })
    }

    /// Decodes the next symbol.
    #[inline]
    pub fn decode(&mut self) -> usize {
        self.decoder.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rans_decoder_matches_rans_coder() {
        // `symbols` coded at precision 12 against `freq`, which already sums to
        // 2^12 so the encoder's normalization is a no-op and `rans_symbol_table`
        // sees the same frequencies. Both lookup strategies and both refill
        // modes must decode it identically.
        let freq = vec![1000usize, 2000, 1096];
        assert_eq!(freq.iter().sum::<usize>(), 1 << 12);
        let symbols = vec![0usize, 1, 1, 2, 0, 1, 2, 2, 1, 0, 2, 1, 0, 0, 1];
        let buffer = [200u8, 58, 180, 36, 66];

        for use_lut in [true, false] {
            for refill_branchless in [true, false] {
                let rans_symbols = rans_symbol_table(&freq, 12).unwrap();
                let rev = RevReader::new(&buffer);
                let mut dec =
                    RansDecoder::new(rev, rans_symbols, use_lut, refill_branchless, 12).unwrap();

                let decoded: Vec<usize> = (0..symbols.len()).map(|_| dec.read()).collect();
                assert_eq!(decoded, symbols);
            }
        }
    }

    #[test]
    fn rabs_decoder_round_trip() {
        // `bits` coded by the encoder's `RabsCoder` at the zero-probability its
        // bit balance yields.
        let bits = vec![
            true, false, true, true, false, false, false, true, false, true, true, true, false,
            true, false, false, true, true,
        ];
        let zero_prob = 114u8;
        let buffer = [42u8, 138, 100, 103];

        let rev = RevReader::new(&buffer);
        let mut dec = RabsDecoder::new(rev, zero_prob).unwrap();
        let decoded: Vec<bool> = (0..bits.len()).map(|_| dec.decode_bit()).collect();
        assert_eq!(decoded, bits);
    }
}
