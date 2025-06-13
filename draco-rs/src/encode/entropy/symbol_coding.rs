use crate::encode::entropy::rans::{RansCoder, RansSymbolEncoder};
use crate::prelude::{BitWriter, ByteWriter};
use crate::shared::connectivity::edgebreaker::SymbolRansEncodingConfig;
use super::rans;

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    #[error("RANS encoding error")]
    RansEncodingError(#[from] rans::Err),
    #[error("Invalid inputs for encode_tagged_symbol(): It must be true that symbol.len()==num_values*num_components, but got symbol.len()={0}, num_values={1}, num_components={2}")]
    InvalidInputs(usize, usize, usize),
    #[error("Invalid bit length: {0}")]
    InvalidBitLength(usize),
}

const MAX_TAG_SYMBOL_BIT_LENGTH: usize = 32;

pub fn encode_symbols<W>(
    symbols: Vec<u64>, num_values: usize, num_components: usize, config: SymbolRansEncodingConfig, writer: &mut W
) -> Result<(), Err> 
    where W: ByteWriter
{
    // ToDo: Add the logic to dynamically determine the config
    match config {
        SymbolRansEncodingConfig::LengthCoded => {
            let mut max_bit_length = 0;
            let mut bit_lengths = Vec::new();
            for &s in &symbols {
                let bit_length = 64-s.leading_zeros() as usize;
                if bit_length > MAX_TAG_SYMBOL_BIT_LENGTH {
                    return Err(Err::InvalidBitLength(bit_length));
                }
                if bit_length > max_bit_length {
                    max_bit_length = bit_length;
                    bit_lengths.resize(max_bit_length as usize+1, 0);
                }
                bit_lengths[bit_length as usize]+=1;
            }
            encode_symbols_length_coded(
                symbols, 
                num_values,
                num_components, 
                bit_lengths, 
                writer
            )
        },
        SymbolRansEncodingConfig::DirectCoded => {
            let num_symbols = symbols.iter().filter(|&&x| x>0).count();

            encode_symbols_direct_coded(
                symbols,
                num_symbols,
                writer
            )
        }
    }
}


/// Encodes symbols using the rANS coder as the tag encoder, that is, the symbols are encoded as bits, and the 
/// bit lengths are encoded by the rANS coder.
///     symbols: the symbols to encode. It must be true that 'symbols.len() == num_values * num_components'
///     num_values: the number of values to encode.
///     num_components: the number of components of the symbols
///     bit_lengths: the bit lengths of the symbols.
///     writer: byte writer
fn encode_symbols_length_coded<W>(
    symbols: Vec<u64>,
    num_values: usize,
    num_components: usize,
    bit_lengths: Vec<u8>,
    writer: &mut W
) -> Result<(), Err> 
    where W: ByteWriter
{
    if symbols.len() != num_values * num_components {
        return Err(Err::InvalidInputs(symbols.len(), num_values, num_components))
    }
    let mut freq_counts = Vec::new();

    for &bit_length in &bit_lengths {
        let bit_length = bit_length as usize;
        if freq_counts.len() <= bit_length {
            freq_counts.resize(bit_length + 1, 0);
        }
        freq_counts[bit_length] += 1;
    }

    let mut values = Vec::new();
    let mut encoder = RansCoder::<'_,_,5>::new(writer, freq_counts, Some(MAX_TAG_SYMBOL_BIT_LENGTH))?;
    for i in (0..num_values).rev() {
        let bit_length = bit_lengths[i];
        encoder.write(bit_length as usize );

        // Values are always encoded in the normal order
        let j = symbols.len() - num_components - i * num_components;
        let value_bit_length = bit_lengths[j / num_components];
        for c in 0..num_components {
            values.push((value_bit_length, symbols[j + c]));
        }
    }
    encoder.flush();

    // Append the values to the end of the target buffer.
    let mut writer: BitWriter<_> = BitWriter::spown_from(writer);
    for val in values {
        writer.write_bits(val);
    }
    Ok(())
}


fn encode_symbols_direct_coded<W>(
    symbols: Vec<u64>,
    num_unique_symbols: usize,
    writer: &mut W
) 
    -> Result<(), Err>
where
    W: ByteWriter,
{
    let bit_length = (64-num_unique_symbols.leading_zeros() as usize + 1).clamp(1, 18);
    writer.write_u8(bit_length as u8);
    match bit_length {
        1 => encode_symbols_direct_coded_precision_unwrapped::<W, 1>(symbols, writer),
        2 => encode_symbols_direct_coded_precision_unwrapped::<W, 2>(symbols, writer),
        3 => encode_symbols_direct_coded_precision_unwrapped::<W, 3>(symbols, writer),
        4 => encode_symbols_direct_coded_precision_unwrapped::<W, 4>(symbols, writer),
        5 => encode_symbols_direct_coded_precision_unwrapped::<W, 5>(symbols, writer),
        6 => encode_symbols_direct_coded_precision_unwrapped::<W, 6>(symbols, writer),
        7 => encode_symbols_direct_coded_precision_unwrapped::<W, 7>(symbols, writer),
        8 => encode_symbols_direct_coded_precision_unwrapped::<W, 8>(symbols, writer),
        9 => encode_symbols_direct_coded_precision_unwrapped::<W, 9>(symbols, writer),
        10 => encode_symbols_direct_coded_precision_unwrapped::<W, 10>(symbols, writer),
        11 => encode_symbols_direct_coded_precision_unwrapped::<W, 11>(symbols, writer),
        12 => encode_symbols_direct_coded_precision_unwrapped::<W, 12>(symbols, writer),
        13 => encode_symbols_direct_coded_precision_unwrapped::<W, 13>(symbols, writer),
        14 => encode_symbols_direct_coded_precision_unwrapped::<W, 14>(symbols, writer),
        15 => encode_symbols_direct_coded_precision_unwrapped::<W, 15>(symbols, writer),
        16 => encode_symbols_direct_coded_precision_unwrapped::<W, 16>(symbols, writer),
        17 => encode_symbols_direct_coded_precision_unwrapped::<W, 17>(symbols, writer),
        18 => encode_symbols_direct_coded_precision_unwrapped::<W, 18>(symbols, writer),
        _ => unreachable!("This should never happen, as the bit length is clamped to a minimum of 1 and a maximum of 18"),
    }
}

fn encode_symbols_direct_coded_precision_unwrapped<W, const RANS_PRECISION: usize>(
    symbols: Vec<u64>,
    writer: &mut W
) -> Result<(), Err>
where
    W: ByteWriter,
{
    let mut freq_counts = Vec::with_capacity(symbols.len());
    let mut max_symbol = 0;
    for &s in symbols.iter() {
        if s > max_symbol {
            max_symbol = s;
            freq_counts.resize((max_symbol + 1) as usize, 0);
        }
        freq_counts[s as usize] += 1;
    }
    
    let mut encoder = RansSymbolEncoder::<'_,_,RANS_PRECISION>::new(writer, freq_counts, None)?;

    for s in symbols.into_iter().rev() {
        encoder.write(s as usize)?;
    }
    encoder.flush()?;
    Ok(())
}