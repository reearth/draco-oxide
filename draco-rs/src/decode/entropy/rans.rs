use crate::core::bit_coder::{ReaderErr, ByteReader, ReverseByteReader};
use crate::prelude::BitReader;
use crate::shared::entropy::{
    rans_build_tables, RansSymbol, SymbolEncodingMethod, DEFAULT_RABS_PRECISION, DEFAULT_RANS_PRECISION, L_RANS_BASE
};
use crate::utils::bit_coder::leb128_read;

pub(crate) fn decode_symbols<R: ByteReader>(reader: &mut R, num_symbols: usize, num_components: usize) -> Result<Vec<usize>, Err> {
    let method = SymbolEncodingMethod::read_from(reader)?;
    match method {
        SymbolEncodingMethod::Tagged => {
            let num_symbol_kinds = leb128_read(reader)? as usize;
            let freq_counts = read_freq_counts(reader, num_symbol_kinds)?;
            let buff_size = leb128_read(reader)? as usize;
            let mut decoder: RansDecoder<_> = RansDecoder::new(reader, buff_size, freq_counts, None)?;
            // read the tagged symbols
            let mut symbols = Vec::new();
            let mut bit_reader: BitReader<_> = BitReader::spown_from(reader).unwrap();
            for _ in 0..num_symbols {
                let size = decoder.read()?;
                for _ in 0..num_components {
                    symbols.push(bit_reader.read_bits(size as u8)? as usize);
                }
            }
            Ok(symbols)
        },
        SymbolEncodingMethod::RawSymbols => {
            let max_bit_length = reader.read_u8()? as usize;
            let num_symbols = leb128_read(reader)? as usize;
            let rans_precision_bits = ((3*max_bit_length)/2).clamp(12, 20);
            let l_rans_base = (1<<rans_precision_bits) * 4;
            let freq_counts = read_freq_counts(reader, num_symbols)?;
            let buff_size = leb128_read(reader)? as usize;
            match rans_precision_bits {
                12 => read_symbols_raw::<12, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                13 => read_symbols_raw::<13, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                14 => read_symbols_raw::<14, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                15 => read_symbols_raw::<15, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                16 => read_symbols_raw::<16, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                17 => read_symbols_raw::<17, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                18 => read_symbols_raw::<18, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                19 => read_symbols_raw::<19, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                20 => read_symbols_raw::<20, _>(reader, buff_size, freq_counts, l_rans_base, num_symbols),
                _ => unreachable!() // it is fail safe to make this unreachable, as the rans_precision_bits is always in the range 12..=20
            }
        },
    }
}

fn read_freq_counts<R>(reader: &mut R, num_symbols: usize) -> Result<Vec<usize>, Err> 
    where R: ByteReader
{
    let mut i=0;
    let mut freq_counts = vec![0; num_symbols];
    while i<num_symbols {
        let byte = reader.read_u8()? as usize;
        let metadata = byte & 3;
        if metadata == 3 {
            i += byte >> 2;
        } else {
            let mut freq_count = byte >> 2;
            for j in 0..metadata {
                freq_count |= (reader.read_u8().unwrap() as usize) << (8*(j+1)-2);
            }
            freq_counts[i] = freq_count;
            i += 1;
        }
    }

    Ok(freq_counts)
}

fn read_symbols_raw<const PRECISION_BITS: usize, R>(reader: &mut R, buff_size: usize, freq_counts: Vec<usize>, l_rans_base: usize, num_symbols: usize) -> Result<Vec<usize>, Err>
    where R: ByteReader
{
    let mut decoder: RansDecoder<_> = RansDecoder::new(reader, buff_size, freq_counts, Some(l_rans_base))?;
    let mut symbols = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols {
        symbols.push(decoder.read()?);
    }
    Ok(symbols)
}

pub(crate) struct RansDecoder<R, const RANS_PRECISION: usize = DEFAULT_RANS_PRECISION> {
    state: usize,
    reader: R,
    slot_table: Vec<usize>,
    rans_syms: Vec<RansSymbol>,
    l_rans_base: usize,
}

#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not enough data to decode RANS symbol")]
    NotEnoughData(#[from] ReaderErr),
    #[error("Invalid frequency count")]
    InvalidFreqCount,
    #[error("General error in entropy coding")]
    SharedError(#[from] crate::shared::entropy::Err),
}

impl<R, const RANS_PRECISION: usize> RansDecoder<R, RANS_PRECISION>
    where R: ReverseByteReader
{
    pub(crate) fn new<ForwardReader>(reader: &mut ForwardReader, offset: usize, freq_counts: Vec<usize>, l_rans_base: Option<usize>) -> Result<Self, Err> 
        where ForwardReader: ByteReader<Rev = R>
    {
        let l_rans_base = l_rans_base.unwrap_or(L_RANS_BASE);
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
        println!("RANS state: {}", state);
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
        println!("RABS state: {}", self.state);
        let freq_count_1 = (1<<RABS_PRECISION) - self.freq_count_0;
        
        if self.state < self.l_rabs_base {
            self.state = (self.state << 3) + self.reverse_reader.read_u8_back()? as usize;
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
        let mut buffer = Vec::new();
        let mut encoder: RansCoder<_> = RansCoder::new(&mut buffer, freq_counts.clone(), None).unwrap();
        for &symbol in &data {
            encoder.write(symbol).unwrap();
        }
        encoder.flush().unwrap();
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

        let mut buffer = Vec::new();
        let mut encoder: RabsCoder<_> = RabsCoder::new(&mut buffer, num_zeros, None);
        for &bit in &data {
            encoder.write(bit).unwrap();
        }
        encoder.flush().unwrap();
        let len = buffer.len();
        println!("Buffer length: {}", len);
        let mut reader = buffer.into_iter();
        let mut decoder: RabsDecoder<_> = RabsDecoder::new(&mut reader, len, num_zeros, None).unwrap();
        
        for &bit in data.iter().rev() {
            assert_eq!(decoder.read().unwrap(), bit as usize);
            println!("Decoded bit: {}", bit);
        }
        assert!(reader.next().is_none(), "Reader should be empty after decoding all symbols");

    }
}