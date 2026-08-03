//! Shannon entropy estimation over unsigned symbol streams, used by
//! encoder-side prediction selection to compare candidate configurations by
//! their approximate coded size.

/// A snapshot of the tracked stream's entropy state.
#[derive(Clone, Copy, Debug, Default)]
pub struct EntropyData {
    /// `sum_over_symbols(frequency * log2(frequency))`; the entropy of the
    /// stream is `log2(num_values) - entropy_norm / num_values`.
    pub entropy_norm: f64,
    pub num_values: u64,
    pub max_symbol: u32,
    pub num_unique_symbols: u64,
}

/// Incrementally tracks the Shannon entropy of a symbol stream, supporting
/// speculative extension: [`peek`](Self::peek) evaluates the state as if the
/// given symbols were appended, [`push`](Self::push) appends them for real.
#[derive(Clone, Debug, Default)]
pub struct ShannonEntropyTracker {
    frequencies: Vec<u64>,
    data: EntropyData,
}

impl ShannonEntropyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The entropy state with `symbols` appended, without updating the stream.
    pub fn peek(&mut self, symbols: &[u32]) -> EntropyData {
        self.update(symbols, false)
    }

    /// Appends `symbols` to the stream and returns the updated state.
    pub fn push(&mut self, symbols: &[u32]) -> EntropyData {
        self.update(symbols, true)
    }

    fn update(&mut self, symbols: &[u32], push_changes: bool) -> EntropyData {
        let mut ret = self.data;
        ret.num_values += symbols.len() as u64;
        for &symbol in symbols {
            let s = symbol as usize;
            if self.frequencies.len() <= s {
                self.frequencies.resize(s + 1, 0);
            }
            let frequency = &mut self.frequencies[s];
            let old_symbol_entropy_norm = if *frequency > 1 {
                *frequency as f64 * (*frequency as f64).log2()
            } else {
                if *frequency == 0 {
                    ret.num_unique_symbols += 1;
                    if symbol > ret.max_symbol {
                        ret.max_symbol = symbol;
                    }
                }
                0.0
            };
            *frequency += 1;
            let new_symbol_entropy_norm = *frequency as f64 * (*frequency as f64).log2();
            ret.entropy_norm += new_symbol_entropy_norm - old_symbol_entropy_norm;
        }
        if push_changes {
            self.data = ret;
        } else {
            for &symbol in symbols {
                self.frequencies[symbol as usize] -= 1;
            }
        }
        ret
    }

    /// Bits needed to code the stream's values at its entropy.
    pub fn data_bits(data: &EntropyData) -> i64 {
        if data.num_values < 2 {
            return 0;
        }
        let n = data.num_values as f64;
        (n * n.log2() - data.entropy_norm).ceil() as i64
    }

    /// Approximate bits needed to store the rANS frequency table of the stream.
    pub fn rans_table_bits(data: &EntropyData) -> i64 {
        let max_value = data.max_symbol as i64 + 1;
        let num_unique = data.num_unique_symbols as i64;
        let table_zero_frequency_bits = 8 * (num_unique + (max_value - num_unique) / 64);
        8 * num_unique + table_zero_frequency_bits
    }
}

/// The entropy in bits per value of a binary stream with `num_true_values` set
/// bits out of `num_values`.
pub fn binary_entropy(num_values: u64, num_true_values: u64) -> f64 {
    if num_values == 0 || num_true_values == 0 || num_values == num_true_values {
        return 0.0;
    }
    let true_freq = num_true_values as f64 / num_values as f64;
    let false_freq = 1.0 - true_freq;
    -(true_freq * true_freq.log2() + false_freq * false_freq.log2())
}

/// Maps a signed value to the unsigned symbol space (the zigzag mapping).
pub fn signed_to_symbol(val: i32) -> u32 {
    ((val << 1) ^ (val >> 31)) as u32
}
