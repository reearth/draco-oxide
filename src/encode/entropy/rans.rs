use std::marker::PhantomData;

use crate::shared::entropy::Symbol;

pub struct RansCoder<Writer, S> 
where
    Writer: FnMut(usize),
    S: Symbol,
{
    writer: Writer,
    freq_count: Vec<usize>,
    sum: usize,
    mask: usize,
    partial_sum: Vec<usize>,
    curr_buff: usize,
    range_param: usize,
    _marker: PhantomData<S>,
}

impl<Writer, S> RansCoder<Writer, S>
    where 
        Writer: FnMut(usize),
        S: Symbol,
{
    pub fn new(writer: Writer, raw_freq_count: Vec<usize>, range_param: Option<usize>) -> Self {
        let (freq_count, sum, mask) = Self::preprocess_freq_count(raw_freq_count);
        let partial_sum = freq_count.iter().scan(0, |state, &x| {
                let old = *state;
                *state += x;
                Some(old)
            }).collect::<Vec<_>>();
        let range_param = range_param.unwrap_or(1 << 8);
        Self {
            writer, 
            freq_count,
            sum,
            mask,
            partial_sum,
            curr_buff: 0,
            range_param,
            _marker: PhantomData,
        }
    }
    
    fn preprocess_freq_count(mut raw_freq_count: Vec<usize>) -> (Vec<usize>, usize,  usize) {
        let total = raw_freq_count.iter().sum::<usize>();

        // Normalize the frequency counts to be a power of two
        let next_power_of_two = total.next_power_of_two();
        let scale_factor = next_power_of_two as f64 / total as f64;

        for freq in &mut raw_freq_count {
            *freq = (*freq as f64 * scale_factor).round() as usize;
        }

        // Adjust the total to ensure it matches the next power of two
        let adjusted_total: usize = raw_freq_count.iter().sum();
        if adjusted_total != next_power_of_two {
            let diff = next_power_of_two as isize - adjusted_total as isize;
            if let Some(first_non_zero) = raw_freq_count.iter_mut().find(|&&mut x| x > 0) {
            *first_non_zero = (*first_non_zero as isize + diff) as usize;
            }
        }

        debug_assert!(total.is_power_of_two(), "Total frequency count must be a power of two");
        (raw_freq_count, total, next_power_of_two)
    }

    pub fn encode(&mut self, symbol: impl Symbol) {
        let index = symbol.get_id();
        let freq = self.freq_count[index];
        let start = self.partial_sum[index];

        // check the ranges for the parameter 'range_param'
        while self.curr_buff >= self.range_param {
            (self.writer)(self.curr_buff & 1);
            self.curr_buff >>= 1;
        }
        
        // Do rANS encoding
        // ToDo: Change to run the euclidean algorithm only once.
        let rem = self.curr_buff%freq;
        self.curr_buff /= freq; 
        self.curr_buff <<= self.mask;
        self.curr_buff += start + rem;
    }

    pub fn encode_metadata(&self) {
        // Encode the metadata (frequency counts) for the symbols
        let mut metadata = vec![0; self.freq_count.len()];
        for (i, &freq) in self.freq_count.iter().enumerate() {
            metadata[i] = freq;
        }
        metadata
    }
}