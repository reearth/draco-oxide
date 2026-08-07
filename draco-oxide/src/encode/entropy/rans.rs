//! rANS and rABS encoding: the [`RansCoder`] and [`RabsCoder`] state machines,
//! the [`RansSymbolEncoder`] that prefixes a payload with the frequency table
//! the decoder parses back, and [`encode_rabs_bit_stream`] for standalone bit
//! streams.

use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::entropy::{rans_symbol_table, RansSymbol, L_RANS_BASE};
use draco_oxide_core::safety_assert;
use draco_oxide_core::utils::bit_coder::leb128_write;

/// The rABS frequency precision; the decoder mirrors it.
const DEFAULT_RABS_PRECISION: usize = 8;

const SECOND_POW_6: usize = 1 << 6;
const SECOND_POW_14: usize = 1 << 14;
const SECOND_POW_22: usize = 1 << 22;
const SECOND_POW_30: usize = 1 << 30;

/// Multiplier for exact division by `f` via a 63-bit fixed-point reciprocal:
/// `x / f == (x * magic) >> 63` for every `x < 2^63 / f`. The rANS state at
/// division time is bounded by `(l_base >> precision) * f * 2^8`, far below
/// that limit for every precision and base this codec uses.
fn div_magic(f: usize) -> u64 {
    ((1u128 << 63) / f as u128) as u64 + 1
}

#[inline]
fn div_rem_by_magic(x: usize, f: usize, magic: u64) -> (usize, usize) {
    let q = ((x as u128 * magic as u128) >> 63) as usize;
    (q, x - q * f)
}

/// Appends the final coder state to `buffer`: `state - l_base` in a
/// u6/u14/u22/u30 field whose width the top two bits tag, the layout the
/// decoder's state reader expects.
#[allow(clippy::identity_op)]
fn write_state_tail(state: usize, l_base: usize, buffer: &mut Vec<u8>) -> Result<(), Err> {
    let state = state - l_base;
    match state {
        0..SECOND_POW_6 => {
            buffer.write_u8((0x00 << 6) + (state as u8));
        }
        SECOND_POW_6..SECOND_POW_14 => {
            buffer.write_u16((0x01 << 14) + (state as u16));
        }
        SECOND_POW_14..SECOND_POW_22 => {
            buffer.write_u24((0x02 << 22) + (state as u32));
        }
        SECOND_POW_22..SECOND_POW_30 => {
            buffer.write_u32((0x03 << 30) + (state as u32));
        }
        _ => {
            return Err(Err::StateTooLarge); // ToDo: Remove this error if possible.
        }
    };
    Ok(())
}

pub struct RabsCoder {
    state: usize,
    freq_count_0: usize,
    /// Reciprocals for dividing by the zero and one frequencies; 0 when that
    /// frequency is 0, in which case the matching bit must never be written.
    div_magics: [u64; 2],
    writer: Vec<u8>,
    l_rabs_base: usize,
}

impl RabsCoder {
    pub fn new(freq_count_0: usize, l_rabs_base: Option<usize>) -> Self {
        let l_rabs_base = l_rabs_base.unwrap_or(L_RANS_BASE);
        let freq_count_1 = (1 << DEFAULT_RABS_PRECISION) - freq_count_0;
        let magic = |f: usize| if f == 0 { 0 } else { div_magic(f) };
        let writer = Vec::new();
        RabsCoder {
            state: l_rabs_base,
            freq_count_0,
            div_magics: [magic(freq_count_0), magic(freq_count_1)],
            writer,
            l_rabs_base,
        }
    }

    pub fn write(&mut self, value: u8) -> Result<(), Err> {
        let freq_count_1 = (1 << DEFAULT_RABS_PRECISION) - self.freq_count_0;
        let (freq_count, magic) = if value > 0 {
            (freq_count_1, self.div_magics[1])
        } else {
            (self.freq_count_0, self.div_magics[0])
        };
        if self.state >= ((self.l_rabs_base >> DEFAULT_RABS_PRECISION) * freq_count) << 8 {
            self.writer.write_u8((self.state & 0xFF) as u8);
            self.state >>= 8;
        }
        let (q, r) = div_rem_by_magic(self.state, freq_count, magic);
        self.state = (q << DEFAULT_RABS_PRECISION) + r + (if value > 0 { 0 } else { freq_count_1 });
        Ok(())
    }

    pub fn flush(mut self) -> Result<Vec<u8>, Err> {
        write_state_tail(self.state, self.l_rabs_base, &mut self.writer)?;
        Ok(self.writer)
    }
}

/// Encodes `bits` as one rABS bit stream: a `zero_prob` byte, the leb128
/// length of the coded buffer, then the buffer.
pub fn encode_rabs_bit_stream<W>(bits: &[bool], writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    let freq_count_0 = bits.iter().filter(|&&o| !o).count();
    let zero_prob = if bits.is_empty() {
        1
    } else {
        (((freq_count_0 as f32 / bits.len() as f32) * 256.0 + 0.5) as u16).clamp(1, 255) as u8
    };
    let mut rabs_coder: RabsCoder = RabsCoder::new(zero_prob as usize, None);
    writer.write_u8(zero_prob);
    // rABS decodes last-written-first, so reverse order here is stream order.
    for &b in bits.iter().rev() {
        rabs_coder.write(if b { 1 } else { 0 })?;
    }
    let buffer = rabs_coder.flush()?;
    leb128_write(buffer.len() as u64, writer);
    for byte in buffer {
        writer.write_u8(byte);
    }
    Ok(())
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    #[error("Cannot build a rANS table over an empty alphabet")]
    EmptyAlphabet,
    #[error("Invalid symbol index")]
    InvalidSymbolIndex,
    #[error("General error in entropy coding")]
    SharedError(#[from] draco_oxide_core::codec::entropy::Err),
    #[error("State too large for RANS coder")]
    StateTooLarge,
    #[error("Too many zero frequency counts in RANS coder")]
    TooManyZeroFreqCounts,
}

pub struct RansCoder {
    state: usize,
    writer: Vec<u8>,
    l_rans_base: usize,
    rans_symbols: Vec<RansSymbol>,
    /// Per-symbol reciprocal for the frequency division; 0 for zero-frequency
    /// symbols, which must never be written.
    div_magics: Vec<u64>,
    precision: usize,
}

impl RansCoder {
    pub fn new(
        freq_counts: Vec<usize>,
        l_rans_base: Option<usize>,
        precision: usize,
    ) -> Result<Self, Err> {
        let l_rans_base = l_rans_base.unwrap_or((1 << precision) << 2);

        let rans_symbols = rans_symbol_table(&freq_counts, precision)?;
        let div_magics = rans_symbols
            .iter()
            .map(|s| {
                if s.freq_count == 0 {
                    0
                } else {
                    div_magic(s.freq_count as usize)
                }
            })
            .collect();

        let writer: Vec<u8> = Vec::new();
        Ok(RansCoder {
            state: l_rans_base,
            writer,
            l_rans_base,
            rans_symbols,
            div_magics,
            precision,
        })
    }

    pub fn write(&mut self, idx: usize) -> Result<(), Err> {
        if idx >= self.rans_symbols.len() {
            return Err(Err::InvalidSymbolIndex);
        }

        let symbol = &self.rans_symbols[idx];
        let freq_count = symbol.freq_count as usize;
        while self.state >= ((self.l_rans_base >> self.precision) * freq_count) << 8 {
            self.writer.write_u8((self.state & 0xFF) as u8);
            self.state >>= 8;
        }
        let (q, r) = div_rem_by_magic(self.state, freq_count, self.div_magics[idx]);
        self.state = (q << self.precision) + r + symbol.freq_cumulative as usize;
        Ok(())
    }

    pub fn flush(mut self) -> Result<Vec<u8>, Err> {
        write_state_tail(self.state, self.l_rans_base, &mut self.writer)?;
        Ok(self.writer)
    }
}

pub struct RansSymbolEncoder<'writer, W> {
    rans_coder: RansCoder,
    num_symbols: usize,
    writer: &'writer mut W,
}

impl<'writer, W> RansSymbolEncoder<'writer, W>
where
    W: ByteWriter,
{
    /// Creates a new RANS symbol encoder with the given frequency counts and optional base for the RANS coder.
    /// If the `l_rans_base` is `None`, it defaults to `L_RANS_BASE`.
    /// # Arguments
    /// * `writer` - A mutable reference to the byte writer.
    /// * `freq_counts` - A vector of frequency counts for each symbol. This need not be normalized to match `precision`.
    /// * `l_rans_base` - An optional base for the RANS coder.
    /// * `precision` - The rANS precision (log2 of the normalized total frequency).
    pub fn new(
        writer: &'writer mut W,
        freq_counts: Vec<usize>,
        l_rans_base: Option<usize>,
        precision: usize,
    ) -> Result<Self, Err> {
        let total_freq = freq_counts.iter().sum::<usize>() as f64;

        let num_symbols = freq_counts
            .iter()
            .enumerate()
            .rev()
            .find(|(_, &c)| c > 0)
            .ok_or(Err::EmptyAlphabet)?
            .0
            + 1;
        safety_assert!((num_symbols..freq_counts.len()).all(|i| freq_counts[i] == 0));

        let mut distribution = Vec::with_capacity(num_symbols);
        let rans_precision = 1 << precision;
        let mut total_rans_prob = 0;
        for freq in freq_counts.iter().take(num_symbols).copied() {
            let prob = freq as f64 / total_freq;

            let mut new_freq = (prob * rans_precision as f64 + 0.5) as usize;
            if new_freq == 0 && freq > 0 {
                new_freq = 1;
            }
            distribution.push(new_freq);
            total_rans_prob += new_freq;
        }

        if total_rans_prob != rans_precision {
            let mut sorted_probabilities = Vec::with_capacity(num_symbols);
            for i in 0..num_symbols {
                sorted_probabilities.push(i);
            }
            sorted_probabilities.sort_by_key(|&i| distribution[i]);
            if total_rans_prob < rans_precision {
                distribution[*sorted_probabilities.last().unwrap()] +=
                    rans_precision - total_rans_prob;
            } else {
                // ToDo: Do better descrete normalization.
                let mut err = total_rans_prob - rans_precision;
                let mut i = distribution.len() - 1;
                while err > 0 {
                    if distribution[sorted_probabilities[i]] > 1 {
                        distribution[sorted_probabilities[i]] -= 1;
                        err -= 1;
                    }
                    if i == 0 {
                        // Wrap around if we still have error to distribute
                        i = distribution.len() - 1;
                    } else {
                        i -= 1;
                    }
                }
            }
        }

        safety_assert!(distribution.iter().sum::<usize>() == rans_precision);

        // encode distribution
        leb128_write(num_symbols as u64, writer);
        let mut i = 0;
        while i < num_symbols {
            let freq = distribution[i];
            if freq == 0 {
                // when we find a symbol with zero frequency, we encode the flag (1-bit) and the
                // 6-bit offset to the next symbol with non-zero frequency.
                let mut offset = 0;
                while offset < (1 << 6) && i + offset + 1 < num_symbols {
                    let next_prob = distribution[i + offset + 1];
                    if next_prob > 0 {
                        i += offset;
                        break;
                    }
                    offset += 1;
                }
                writer.write_u8(((offset as u8) << 2) | 3);
            } else {
                let mut num_extra_bytes = 0;
                if freq >= (1 << 6) {
                    num_extra_bytes += 1;
                    if freq >= (1 << 14) {
                        num_extra_bytes += 1;
                        if freq >= (1 << 22) {
                            // This never occurs as we made rans_precision less than 2^20
                            unreachable!("RANS precision too high, prob: {}", freq);
                        }
                    }
                }
                writer.write_u8(((freq << 2) | (num_extra_bytes & 3)) as u8);
                for b in 0..num_extra_bytes {
                    writer.write_u8((freq >> (8 * (b + 1) - 2)) as u8);
                }
            }
            i += 1;
        }

        // return encoder
        let out: RansSymbolEncoder<'_, W> = RansSymbolEncoder {
            rans_coder: RansCoder::new(distribution, l_rans_base, precision)?,
            num_symbols,
            writer,
        };
        Ok(out)
    }

    pub fn write(&mut self, idx: usize) -> Result<(), Err> {
        if idx >= self.num_symbols {
            return Err(Err::InvalidSymbolIndex);
        }
        self.rans_coder.write(idx)
    }

    pub fn flush(self) -> Result<(), Err> {
        let buffer = self.rans_coder.flush()?;
        leb128_write(buffer.len() as u64, self.writer);
        for byte in buffer {
            self.writer.write_u8(byte);
        }
        Ok(())
    }
}
