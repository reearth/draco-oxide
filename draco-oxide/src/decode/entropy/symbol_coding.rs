use crate::core::bit_coder::ReaderErr;
use crate::decode::entropy::rans::RansSymbolDecoder;
use crate::prelude::{BitReader, ByteReader};
use crate::shared::entropy::SymbolEncodingMethod;
use super::rans;

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum Err {
    #[error("RANS decoding error: {0}")]
    RansDecodingError(#[from] rans::Err),
    #[error("Invalid bit length: {0}")]
    InvalidBitLength(usize),
    #[error("Not enough data to decode RANS symbol: {0}")]
    NotEnoughData(#[from] ReaderErr),
    #[error("General error in entropy coding: {0}")]
    SharedError(#[from] crate::shared::entropy::Err),
}


/// Reads and decodes symbols from the given reader.
/// Arguments:
/// - `num_symbols`: The number of symbols to decode.
/// - `num_components`: The number of components for each symbol.
/// - `reader`: The byte reader to read the encoded symbols from.
/// Returns a vector of decoded symbols or an error if decoding fails.
#[allow(unused)] // TODO: Remove this when decoder is fully implemented
pub fn decode_symbols<R>(
    num_symbols: usize, num_components: usize, reader: &mut R
) -> Result<Vec<u64>, Err> 
    where R: ByteReader 
{
    let method = SymbolEncodingMethod::read_from(reader)?;

    match method {
        SymbolEncodingMethod::LengthCoded => 
            decode_symbols_length_coded(num_symbols, num_components, reader),
        SymbolEncodingMethod::DirectCoded => 
            decode_symbols_direcd_coded(num_symbols, reader)
    }
}



#[allow(unused)] // TODO: Remove this when decoder is fully implemented
pub fn decode_symbols_length_coded<R>(
    num_symbols: usize, num_components: usize, reader: &mut R
) -> Result<Vec<u64>, Err> 
    where R: ByteReader 
{
    let mut out = Vec::with_capacity(num_symbols * num_components);
    // Decode the encoded data.
    let mut length_coded_decoder: RansSymbolDecoder<R, 5, 12> = RansSymbolDecoder::new(reader)?;

    let mut bit_reader: BitReader<'_, R> = BitReader::spown_from(reader).unwrap(); // ToDo: Handle error
    for _ in (0..num_symbols/num_components).map(|e| e * num_components) {
        // Decode the length
        let len = length_coded_decoder.decode_symbol()?;
        // Decode the symbol.
        if len == 0 {
            // If the length is 0, we can skip decoding this symbol.
            for _ in 0..num_components {
                out.push(0);
            }
            continue;
        }
        for _ in 0..num_components {
            let val = bit_reader.read_bits(len as u8)?;
            out.push(val);
        }
    }
    
    Ok(out)
}

#[allow(unused)] // TODO: Remove this when decoder is fully implemented
pub fn decode_symbols_direcd_coded<R>(
    num_symbols: usize, reader: &mut R
) -> Result<Vec<u64>, Err> 
    where R: ByteReader 
{
  let max_bit_length = reader.read_u8()?;

  match max_bit_length {
    1 => decode_symbols_direcd_coded_precision_unwrapped::<R, 1,12>(num_symbols, reader),
    2 => decode_symbols_direcd_coded_precision_unwrapped::<R, 2, 12>(num_symbols, reader),
    3 => decode_symbols_direcd_coded_precision_unwrapped::<R, 3, 12>(num_symbols, reader),
    4 => decode_symbols_direcd_coded_precision_unwrapped::<R, 4, 12>(num_symbols, reader),
    5 => decode_symbols_direcd_coded_precision_unwrapped::<R, 5, 12>(num_symbols, reader),
    6 => decode_symbols_direcd_coded_precision_unwrapped::<R, 6, 12>(num_symbols, reader),
    7 => decode_symbols_direcd_coded_precision_unwrapped::<R, 7, 12>(num_symbols, reader),
    8 => decode_symbols_direcd_coded_precision_unwrapped::<R, 8, 12>(num_symbols, reader),
    9 => decode_symbols_direcd_coded_precision_unwrapped::<R, 9, 13>(num_symbols, reader),
    10 => decode_symbols_direcd_coded_precision_unwrapped::<R, 10, 15>(num_symbols, reader),
    11 => decode_symbols_direcd_coded_precision_unwrapped::<R, 11, 16>(num_symbols, reader),
    12 => decode_symbols_direcd_coded_precision_unwrapped::<R, 12, 18>(num_symbols, reader),
    13 => decode_symbols_direcd_coded_precision_unwrapped::<R, 13, 19>(num_symbols, reader),
    14 => decode_symbols_direcd_coded_precision_unwrapped::<R, 14, 20>(num_symbols, reader),
    15 => decode_symbols_direcd_coded_precision_unwrapped::<R, 15, 20>(num_symbols, reader),
    16 => decode_symbols_direcd_coded_precision_unwrapped::<R, 16, 20>(num_symbols, reader),
    17 => decode_symbols_direcd_coded_precision_unwrapped::<R, 17, 20>(num_symbols, reader),
    18 => decode_symbols_direcd_coded_precision_unwrapped::<R, 18, 20>(num_symbols, reader),
    _ => return Err(Err::InvalidBitLength(max_bit_length as usize)),
  }
}

pub fn decode_symbols_direcd_coded_precision_unwrapped<R, const NUM_SYMBOLS_BIT_LENGTH: usize, const RANS_PRECISION: usize>(
    num_symbols: usize, reader: &mut R
) -> Result<Vec<u64>, Err> 
    where R: ByteReader,
{
    let mut decoder: RansSymbolDecoder<R, NUM_SYMBOLS_BIT_LENGTH, RANS_PRECISION> = RansSymbolDecoder::new(reader)?;
    let mut out = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols {
        out.push( decoder.decode_symbol()? as u64 );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::entropy::*;

    #[test]
    fn test_encode_decode_symbols() -> Result<(), Err> {
        let len = 100;
        let symbols = (0..len).map(|x| (x*x*x)%23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(), 
            1, 
            SymbolEncodingMethod::LengthCoded, 
            &mut buffer
        ).unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(
            len as usize, 
            1, 
            &mut reader
        )?;
        assert_eq!(reader.next(), None, "Reader should be empty after decoding all symbols");
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }

        #[test]
    fn test_encode_decode_symbols_multi_components() -> Result<(), Err> {
        let len = 300;
        let symbols = (0..len).map(|x| (x*x*x)%23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(), 
            3, 
            SymbolEncodingMethod::LengthCoded, 
            &mut buffer
        ).unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(
            len as usize, 
            3,
            &mut reader
        )?;
        assert_eq!(reader.next(), None, "Reader should be empty after decoding all symbols");
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }

    #[test]
    fn test_encode_decode_symbols_direct_coded() -> Result<(), Err> {
        let len = 100;
        let symbols = (0..len).map(|x| (x*x*x)%23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(), 
            1, 
            SymbolEncodingMethod::DirectCoded, 
            &mut buffer
        ).unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(
            len as usize, 
            1, 
            &mut reader
        )?;
        assert_eq!(reader.next(), None, "Reader should be empty after decoding all symbols");
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }

    #[test]
    fn test_encode_decode_symbols_direct_coded_multi_components() -> Result<(), Err> {
        let len = 300;
        let symbols = (0..len).map(|x| (x*x*x)%23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(), 
            3, 
            SymbolEncodingMethod::DirectCoded, 
            &mut buffer
        ).unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(
            len as usize, 
            3,
            &mut reader
        )?;
        assert_eq!(reader.next(), None, "Reader should be empty after decoding all symbols");
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }
}