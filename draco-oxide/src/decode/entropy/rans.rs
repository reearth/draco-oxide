use crate::core::bit_coder::{ReaderErr, ByteReader, ReverseByteReader};
use crate::shared::entropy::{
    rans_build_tables, RansSymbol, DEFAULT_RABS_PRECISION, DEFAULT_RANS_PRECISION, L_RANS_BASE
};
use crate::utils::bit_coder::leb128_read;

pub(crate) struct RansDecoder<R, const RANS_PRECISION: usize = DEFAULT_RANS_PRECISION> {
    state: usize,
    reader: R,
    slot_table: Vec<usize>,
    rans_syms: Vec<RansSymbol>,
    l_rans_base: usize,
}

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum Err {
    #[error("Not enough data to decode RANS symbol")]
    NotEnoughData(#[from] ReaderErr),
    #[error("Invalid frequency count")]
    InvalidFreqCount,
    #[error("General error in entropy coding")]
    SharedError(#[from] crate::shared::entropy::Err),
    #[error("RANS symbol decoder error: {0}")]
    RansSymbolDecoderError(&'static str),
}

impl<R, const RANS_PRECISION: usize> RansDecoder<R, RANS_PRECISION>
    where R: ReverseByteReader
{
    pub(crate) fn new<ForwardReader>(reader: &mut ForwardReader, offset: usize, freq_counts: Vec<usize>, l_rans_base: Option<usize>) -> Result<Self, Err> 
        where ForwardReader: ByteReader<Rev = R>
    {
        let l_rans_base = l_rans_base.unwrap_or((1<<RANS_PRECISION) << 2);
        let mut reverse_reader = reader.spown_reverse_reader_at(offset)?;
        let metadata = reverse_reader.read_u8_back()?;
        let flag = metadata >> 6;
        let mut state = match flag {
            0 => 0, 
            1 => reverse_reader.read_u8_back()? as usize & 0xFF,
            2 => reverse_reader.read_u16_back()? as usize & 0xFFFF,
            3 => reverse_reader.read_u24_back()? as usize & 0xFFFFFF,
            _ => unreachable!(), // No error handling needed here as the flag will always be in the range 0..=3
        };
        state |= ((metadata & 0x3F) as usize) << (flag<<3);
        state += l_rans_base;

        let (slot_table, rans_syms) = rans_build_tables::<RANS_PRECISION>(&freq_counts)?;

        Ok( RansDecoder {
            state,
            reader: reverse_reader,
            l_rans_base,
            slot_table,
            rans_syms,
        })
    }

    pub(crate) fn read(&mut self) -> Result<usize, Err> {
        while self.state < self.l_rans_base {
            self.state = self.state * 256 + self.reader.read_u8_back()? as usize;
        }
        let q = self.state / (1 << RANS_PRECISION);
        let r = self.state % (1 << RANS_PRECISION);
        let symbol_idx = self.slot_table[r];
        let symbol = &self.rans_syms[symbol_idx];
        self.state = q * symbol.freq_count + r - symbol.freq_cumulative;
        Ok(symbol_idx)
    }
}


pub(crate) struct RabsDecoder<R, const RANS_PRECISION: usize = DEFAULT_RABS_PRECISION> {
    state: usize,
    freq_count_0: usize,
    reverse_reader: R,
    l_rabs_base: usize,
}

impl<R, const RABS_PRECISION: usize> RabsDecoder<R, RABS_PRECISION>
    where R: ReverseByteReader
{
    pub(crate) fn new<ForwardReader>(reader: &mut ForwardReader, offset: usize, freq_count_0: usize, l_rabs_base: Option<usize>) -> Result<Self, Err> 
        where ForwardReader: ByteReader<Rev = R>
    {
        let l_rabs_base = l_rabs_base.unwrap_or(L_RANS_BASE);
        let mut reverse_reader = reader.spown_reverse_reader_at(offset)?;
        let metadata = reverse_reader.read_u8_back()?;
        let flag = metadata >> 6;
        let mut state = match flag {
            0 => 0, 
            1 => reverse_reader.read_u8_back()? as usize & 0xFF,
            2 => reverse_reader.read_u16_back()? as usize & 0xFFFF,
            3 => reverse_reader.read_u24_back()? as usize & 0xFFFFFF,
            _ => unreachable!(), // No error handling needed here as the flag will always be in the range 0..=3
        };
        state |= ((metadata & 0x3F) as usize) << (flag<<3);
        state += l_rabs_base;

        if freq_count_0 >= (1 << RABS_PRECISION) {
            return Err(Err::InvalidFreqCount);
        }

        Ok( RabsDecoder {
            state,
            freq_count_0,
            reverse_reader,
            l_rabs_base,
        })
    }

    pub(crate) fn read(&mut self) -> Result<usize, Err> {
        let freq_count_1 = (1<<RABS_PRECISION) - self.freq_count_0;
        
        if self.state < self.l_rabs_base {
            self.state = (self.state << 8) + self.reverse_reader.read_u8_back()? as usize;
        }

        let x = self.state;
        let q = x >> RABS_PRECISION;
        let r = x & ((1<<RABS_PRECISION)-1);
        let xn = q * freq_count_1;
        if r < freq_count_1 {
            self.state = xn + r; // q * freq_count_1 + r
            Ok(1)
        } else {
            self.state = x - xn - freq_count_1; // q * freq_count_0 + r - freq_count_1;
            Ok(0)
        }
    }
}

const fn compute_rans_precision(num_symbols_bit_length: usize) -> usize {
    let mut precision = 12;
    if num_symbols_bit_length > 0 {
        precision = (num_symbols_bit_length + 2) / 3;
    }
    if precision < 12 {
        12
    } else if precision > 20 {
        20
    } else {
        precision
    }
}

pub(crate) struct RansSymbolDecoder<R, const NUM_SYMBOLS_BIT_LENGTH: usize, const RANS_PRECISION: usize> 
    where R: ByteReader
{
    freq_counts: Vec<usize>,
    rans_decoder: RansDecoder<R::Rev, RANS_PRECISION>, 
}

impl<'reader, R, const NUM_SYMBOLS_BIT_LENGTH: usize, const RANS_PRECISION: usize> 
        RansSymbolDecoder<R, NUM_SYMBOLS_BIT_LENGTH, RANS_PRECISION>
    where R: ByteReader{
    pub fn new(reader: &mut R) -> Result<Self, Err>
        where R: ByteReader
    {
        let num_symbols = leb128_read(reader)? as usize;
        let mut freq_counts = vec![0; num_symbols];

        let mut i = 0;
        while i < num_symbols {
            let count = reader.read_u8()? as usize;
            
            let token = count & 3;
            if token == 3 {
                // Coming here means that the number of symbols with frequency count 0
                // shows up in the next 'count >> 2' elements.
                let offset = count >> 2;
                if i + offset >= num_symbols {
                    Err(Err::RansSymbolDecoderError("Invalid offset for frequency counts"))?;
                }
                for j in 0..=offset {
                    freq_counts[i+j] = 0;
                }
                i += offset;
            } else {
                let extra_bytes = token;
                let mut count = count >> 2;
                for j in 0..extra_bytes {
                    let eb = reader.read_u8()? as usize;
                    count |= (eb) << (8 * (j + 1) - 2);
                }
                freq_counts[i] = count;
            }
            i += 1;
        }

        let offset = leb128_read(reader)? as usize;

        let rans_decoder: RansDecoder<_, RANS_PRECISION> = RansDecoder::new(
            reader, 
            offset, 
            freq_counts.clone(), 
            None
        )?;

        Ok(Self {
            freq_counts,
            rans_decoder,
        })
    }

    pub fn decode_symbol(&mut self) -> Result<usize, Err> {
        self.rans_decoder.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::entropy::rans::{
        RansCoder,
        RabsCoder
    };

    #[test]
    fn test_rans_decoder() {
        let num_symbols = 43; // number of symbols
        let (data, freq_counts) = {
            let mut data = Vec::new();
            let mut freq_counts = vec![0; num_symbols];
            let mut x = 3;
            for _ in 0..1<<12 {
                x = (x+37)%num_symbols;
                data.push(x);
                freq_counts[x] += 1;
            }
            (data, freq_counts)
        };
        let mut encoder: RansCoder = RansCoder::new(freq_counts.clone(), None).unwrap();
        for &symbol in &data {
            encoder.write(symbol).unwrap();
        }
        let buffer = encoder.flush().unwrap();
        let len = buffer.len();
        let mut reader = buffer.into_iter();
        let mut decoder: RansDecoder<_> = RansDecoder::new(&mut reader, len, freq_counts, None).unwrap();
        
        for &symbol in data.iter().rev() {
            assert_eq!(decoder.read().unwrap(), symbol);
        }
        assert!(reader.next().is_none(), "Reader should be empty after decoding all symbols");
    }

    #[test]
    fn test_rabs_coder(){
        let num_zeros = 100;
        let data = {
            let mut data = vec![0_u8; 1<<DEFAULT_RABS_PRECISION];
            let mut sorted = data.clone();
            for i in num_zeros..sorted.len() {
                sorted[i] = 1;
            }
            let bijection = 67; // Some number coprime to 1<<DEFAULT_RABS_PRECISION to shuffle the data
            for i in 0..data.len() {
                let idx = (bijection * i) % data.len();
                data[idx] = sorted[i];
            }
            data
        };

        assert!(data.len() == 1<<DEFAULT_RABS_PRECISION);

        let mut encoder: RabsCoder = RabsCoder::new(num_zeros, None);
        for &bit in &data {
            encoder.write(bit).unwrap();
        }
        let buffer = encoder.flush().unwrap();
        let len = buffer.len();
        let mut reader = buffer.into_iter();
        let mut decoder: RabsDecoder<_> = RabsDecoder::new(&mut reader, len, num_zeros, None).unwrap();
        
        for &bit in data.iter().rev() {
            assert_eq!(decoder.read().unwrap(), bit as usize);
        }
        assert!(reader.next().is_none(), "Reader should be empty after decoding all symbols");

    }
}