use crate::core::bit_coder::ByteWriter;
use crate::shared::entropy::{rans_build_tables, RansSymbol, DEFAULT_RABS_PRECISION, DEFAULT_RANS_PRECISION, L_RANS_BASE};
use crate::utils::bit_coder::leb128_write;

const SECOND_POW_6: usize = 1 << 6;
const SECOND_POW_14: usize = 1 << 14;
const SECOND_POW_22: usize = 1 << 22;
const SECOND_POW_30: usize = 1 << 30;

pub(crate) struct RansCoder<const RANS_PRECISION: usize = DEFAULT_RANS_PRECISION> {
    state: usize,
    writer: Vec<u8>,
    l_rans_base: usize,
    rans_symbols: Vec<RansSymbol>,
}


impl<const RANS_PRECISION: usize> RansCoder<RANS_PRECISION> {
    pub fn new(freq_counts: Vec<usize>, l_rans_base: Option<usize>) -> Result<Self, Err> {
        let l_rans_base = l_rans_base.unwrap_or((1<<RANS_PRECISION) << 2);

        let (_slot_table, rans_symbols) = rans_build_tables::<RANS_PRECISION>(&freq_counts)?;

        let writer: Vec<u8> = Vec::new();
        Ok( RansCoder {
            state: l_rans_base,
            writer,
            l_rans_base,
            rans_symbols,
        })
    }

    pub fn write(&mut self, idx: usize) -> Result<(), Err> {
        if idx >= self.rans_symbols.len() {
            return Err(Err::InvalidSymbolIndex);
        }
        
        let symbol = &self.rans_symbols[idx];
        let freq_count = symbol.freq_count;
        while self.state >= (self.l_rans_base>>RANS_PRECISION) * freq_count << 8 {
            self.writer.write_u8((self.state & 0xFF) as u8);
            self.state >>= 8;
        }
        self.state = ((self.state / freq_count) << RANS_PRECISION) + self.state % freq_count + symbol.freq_cumulative;
        Ok(())
    }

    pub fn flush(mut self) -> Result<Vec<u8>, Err> {
        self.state -= self.l_rans_base;
        match self.state {
            0..SECOND_POW_6 => {
                self.writer.write_u8((0x00 << 6) + (self.state as u8));
            },
            SECOND_POW_6..SECOND_POW_14 => {
                self.writer.write_u16((0x01 << 14) + (self.state as u16));
            },
            SECOND_POW_14..SECOND_POW_22 => {
                self.writer.write_u24((0x02 << 22) + (self.state as u32));
            },
            SECOND_POW_22..SECOND_POW_30 => {
                self.writer.write_u32((0x03 << 30) + (self.state as u32));
            },
            _ => {
                return Err(Err::StateTooLarge); // ToDo: Remove this error if possible.
            }
        };
        Ok(self.writer)
    }
}

pub(crate) struct RabsCoder<const RABS_PRECISION: usize = DEFAULT_RABS_PRECISION> {
    state: usize,
    freq_count_0: usize,
    writer: Vec<u8>,
    l_rabs_base: usize,
}

impl<const RABS_PRECISION: usize> RabsCoder<RABS_PRECISION> {
    pub fn new(freq_count_0: usize, l_rabs_base: Option<usize>) -> Self {
        let l_rabs_base = l_rabs_base.unwrap_or(L_RANS_BASE);
        let writer = Vec::new();
        RabsCoder {
            state: l_rabs_base,
            freq_count_0,
            writer,
            l_rabs_base,
        }
    }

    pub fn write(&mut self, value: u8) -> Result<(), Err> {
        let freq_count_1 = (1 << RABS_PRECISION) - self.freq_count_0;
        let freq_count = if value > 0 {
            freq_count_1
        } else {
            self.freq_count_0
        };
        if self.state >= (self.l_rabs_base >> RABS_PRECISION) * freq_count << 8 {
            self.writer.write_u8((self.state & 0xFF) as u8);
            self.state >>= 8;
        }
        let q = self.state / freq_count;
        let r = self.state % freq_count;
        self.state = (q << RABS_PRECISION) + r + (if value > 0 { 0 } else { freq_count_1 });
        Ok(())
    }

    pub fn flush(mut self) -> Result<Vec<u8>, Err> {
        self.state -= self.l_rabs_base;
        match self.state {
            0..SECOND_POW_6 => {
                self.writer.write_u8((0x00 << 6) + (self.state as u8));
            },
            SECOND_POW_6..SECOND_POW_14 => {
                self.writer.write_u16((0x01 << 14) + (self.state as u16));
            },
            SECOND_POW_14..SECOND_POW_22 => {
                self.writer.write_u24((0x02 << 22) + (self.state as u32));
            },
            SECOND_POW_22..SECOND_POW_30 => {
                self.writer.write_u32((0x03 << 30) + (self.state as u32));
            },
            _ => {
                return Err(Err::StateTooLarge); // ToDo: Remove this error if possible.
            }
        };
        Ok(self.writer)
    }
}


pub(crate) struct RansSymbolEncoder<'writer, W, const NUM_SYMBOLS_BIT_LENGTH: usize, const RANS_PRECISION: usize> {
    rans_coder: RansCoder<RANS_PRECISION>,
    num_symbols: usize,
    writer: &'writer mut W,
}

impl<'writer, W, const NUM_SYMBOLS_BIT_LENGTH: usize, const RANS_PRECISION: usize> RansSymbolEncoder<'writer, W, NUM_SYMBOLS_BIT_LENGTH, RANS_PRECISION> 
    where W: ByteWriter
{
    /// Creates a new RANS symbol encoder with the given frequency counts and optional base for the RANS coder.
    /// If the `l_rans_base` is `None`, it defaults to `L_RANS_BASE`.
    /// # Arguments
    /// * `writer` - A mutable reference to the byte writer.
    /// * `freq_counts` - A vector of frequency counts for each symbol. This need not be normalized to match RANS_PRECISION.
    /// * `l_rans_base` - An optional base for the RANS coder. 
    pub fn new(writer: &'writer mut W, freq_counts: Vec<usize>, l_rans_base: Option<usize>) -> Result<Self, Err> {
        let total_freq = freq_counts.iter().sum::<usize>() as f64;

        let num_symbols = freq_counts.iter().enumerate()
            .rev()
            .find(|(_, &c)| c > 0)
            .unwrap()
            .0 + 1;
        debug_assert!((num_symbols..freq_counts.len()).all(|i| freq_counts[i] == 0));

        let mut distribution = Vec::with_capacity(num_symbols);
        let rans_precision = 1<<RANS_PRECISION;
        let mut total_rans_prob = 0;
        for i in 0..num_symbols {
            let freq = freq_counts[i];

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
                distribution[*sorted_probabilities.last().unwrap()] += rans_precision - total_rans_prob;
            } else {
                // ToDo: Do better descrete normalization.
                let mut err = total_rans_prob - rans_precision;
                let mut i = distribution.len() - 1;
                while err > 0 {
                    distribution[sorted_probabilities[i]] -= 1;
                    i-=1;   
                    err -= 1;
                }
            }
        }

        debug_assert!(distribution.iter().sum::<usize>() == rans_precision);

        // encode distribution
        leb128_write(num_symbols as u64, writer);
        let mut i = 0;
        while i < num_symbols {
            let freq = distribution[i];
            if freq == 0 {
                // when we find a symbol with zero frequency, we encode the flag (1-bit) and the 
                // 6-bit offset to the next symbol with non-zero frequency.
                let mut offset = 0;
                while offset < (1 << 6) {
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
                    num_extra_bytes+=1;
                    if freq >= (1 << 14) {
                        num_extra_bytes+=1;
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
        let out: RansSymbolEncoder<'_, W, NUM_SYMBOLS_BIT_LENGTH, RANS_PRECISION> = RansSymbolEncoder {
            rans_coder: RansCoder::<RANS_PRECISION>::new(distribution, l_rans_base)?,
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


#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    #[error("Invalid symbol index")]
    InvalidSymbolIndex,
    #[error("General error in entropy coding")]
    SharedError(#[from] crate::shared::entropy::Err),
    #[error("State too large for RANS coder")]
    StateTooLarge,
    #[error("Too many zero frequency counts in RANS coder")]
    TooManyZeroFreqCounts,
}