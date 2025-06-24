use crate::encode::entropy::rans::RansSymbolEncoder;
use crate::prelude::{BitWriter, ByteWriter};
use crate::shared::entropy::SymbolEncodingMethod;
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
    symbols: Vec<u64>, num_components: usize, config: SymbolEncodingMethod, writer: &mut W
) -> Result<(), Err> 
    where W: ByteWriter
{
    config.write_to(writer);
    // ToDo: Add the logic to dynamically determine the config
    match config {
        SymbolEncodingMethod::LengthCoded => {
            let mut bit_lengths = Vec::new();
            for i in 0..symbols.len()/num_components {
                let mut max_bit_length = 0;
                for j in 0..num_components {
                    let s = symbols[i * num_components + j];
                    let bit_length = (64-s.leading_zeros()) as usize;
                    if bit_length > max_bit_length {
                        max_bit_length = bit_length;
                    }
                }
                bit_lengths.push(max_bit_length as u8);
            }
            encode_symbols_length_coded(
                symbols, 
                num_components, 
                bit_lengths, 
                writer
            )
        },
        SymbolEncodingMethod::DirectCoded => {
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
///     symbols: the symbols to encode. For data with multiple components (e.g., 3D points are with 3 components), \
///        the symbols must be a vector of length `num_values * num_components` (e.g. a set of 100 3D points is\
///         represented as 300 symbols).
///     num_components: the number of components for each value (e.g., 3 for 3D points).
///     bit_lengths: the bit lengths of the symbols. It is a vector of 'symbols.len()/num_components' elements, and\
///         records the largest bit length of the 'num_components' components.
///     writer: byte writer
fn encode_symbols_length_coded<W>(
    symbols: Vec<u64>,
    num_components: usize,
    bit_lengths: Vec<u8>,
    writer: &mut W
) -> Result<(), Err> 
    where W: ByteWriter
{
    let mut freq_counts = Vec::new();

    for &bit_length in &bit_lengths {
        let bit_length = bit_length as usize;
        if freq_counts.len() <= bit_length {
            freq_counts.resize(bit_length + 1, 0);
        }
        freq_counts[bit_length] += 1;
    }

    let mut values = Vec::new();
    let mut encoder = RansSymbolEncoder::<'_,_,5, 12>::new(writer, freq_counts, None)?;
    for i in (0..symbols.len()/num_components).rev() {
        let bit_length = bit_lengths[i] as usize;
        encoder.write(bit_length as usize )?;
        
        // Values are always encoded in the normal order
        let j = symbols.len() - num_components - i * num_components;
        let value_bit_length = bit_lengths[j / num_components];
        for c in 0..num_components {
            values.push((value_bit_length, symbols[j + c]));
        }
    }
    encoder.flush()?;
    
    // Append the values to the end of the target buffer.
    let mut writer: BitWriter<_> = BitWriter::spown_from(writer);
    for val in values.into_iter() {
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
        1 => encode_symbols_direct_coded_precision_unwrapped::<W, 1, 12>(symbols, writer),
        2 => encode_symbols_direct_coded_precision_unwrapped::<W, 2, 12>(symbols, writer),
        3 => encode_symbols_direct_coded_precision_unwrapped::<W, 3, 12>(symbols, writer),
        4 => encode_symbols_direct_coded_precision_unwrapped::<W, 4, 12>(symbols, writer),
        5 => encode_symbols_direct_coded_precision_unwrapped::<W, 5, 12>(symbols, writer),
        6 => encode_symbols_direct_coded_precision_unwrapped::<W, 6, 12>(symbols, writer),
        7 => encode_symbols_direct_coded_precision_unwrapped::<W, 7, 12>(symbols, writer),
        8 => encode_symbols_direct_coded_precision_unwrapped::<W, 8, 12>(symbols, writer),
        9 => encode_symbols_direct_coded_precision_unwrapped::<W, 9, 13>(symbols, writer),
        10 => encode_symbols_direct_coded_precision_unwrapped::<W, 10, 15>(symbols, writer),
        11 => encode_symbols_direct_coded_precision_unwrapped::<W, 11, 16>(symbols, writer),
        12 => encode_symbols_direct_coded_precision_unwrapped::<W, 12, 18>(symbols, writer),
        13 => encode_symbols_direct_coded_precision_unwrapped::<W, 13, 19>(symbols, writer),
        14 => encode_symbols_direct_coded_precision_unwrapped::<W, 14, 20>(symbols, writer),
        15 => encode_symbols_direct_coded_precision_unwrapped::<W, 15, 20>(symbols, writer),
        16 => encode_symbols_direct_coded_precision_unwrapped::<W, 16, 20>(symbols, writer),
        17 => encode_symbols_direct_coded_precision_unwrapped::<W, 17, 20>(symbols, writer),
        18 => encode_symbols_direct_coded_precision_unwrapped::<W, 18, 20>(symbols, writer),
        _ => unreachable!("This should never happen, as the bit length is clamped to a minimum of 1 and a maximum of 18"),
    }
}

fn encode_symbols_direct_coded_precision_unwrapped<W, const NUM_SYMBOLS_BIT_LENGTH: usize, const RANS_PRECISION: usize>(
    symbols: Vec<u64>,
    writer: &mut W
) -> Result<(), Err>
    where W: ByteWriter,
{
    let mut freq_counts = Vec::with_capacity(symbols.len());
    let mut max_symbol = 0;
    for &s in symbols.iter() {
        if s >= max_symbol {
            max_symbol = s;
            freq_counts.resize((max_symbol + 1) as usize, 0);
        }
        freq_counts[s as usize] += 1;
    }
    
    let mut encoder = RansSymbolEncoder::<'_,_,NUM_SYMBOLS_BIT_LENGTH,RANS_PRECISION>::new(writer, freq_counts, None)?;

    for s in symbols.into_iter().rev() {
        encoder.write(s as usize)?;
    }
    encoder.flush()?;
    Ok(())
}