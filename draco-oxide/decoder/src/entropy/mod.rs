//! Entropy decoding: [`decode_symbols`] (DirectCoded now; Tagged/LengthCoded in
//! milestone B) over the rANS/rabs decoders in [`rans`], plus the [`unzigzag`]
//! inverse of core's `to_positive_i32`.

pub mod rans;

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
use rans::RansSymbolDecoder;

/// Decodes `num_values * num_components` symbols from `reader`. Mirrors Google's
/// `DecodeSymbols`: reads the encoding-method byte and dispatches. Only DirectCoded
/// is supported for now; `num_components` is used only by the Tagged path.
pub fn decode_symbols(
    reader: &mut Reader<'_>,
    num_values: usize,
    num_components: usize,
) -> Result<Vec<u64>, Err> {
    let method = SymbolEncodingMethod::read_from(reader)?;
    match method {
        SymbolEncodingMethod::DirectCoded => {
            decode_symbols_direct(reader, num_values * num_components)
        }
        SymbolEncodingMethod::LengthCoded => Err(Err::Unimplemented),
    }
}

/// Decodes `num_symbols` DirectCoded symbols. The leading bit-length byte selects
/// the rANS precision via the same mapping the encoder uses
/// (`clamp(3 * bit_length / 2, 12, 20)`).
fn decode_symbols_direct(reader: &mut Reader<'_>, num_symbols: usize) -> Result<Vec<u64>, Err> {
    let bit_length = reader.read_u8()?;
    match bit_length {
        1..=8 => decode_direct::<12>(reader, num_symbols),
        9 => decode_direct::<13>(reader, num_symbols),
        10 => decode_direct::<15>(reader, num_symbols),
        11 => decode_direct::<16>(reader, num_symbols),
        12 => decode_direct::<18>(reader, num_symbols),
        13 => decode_direct::<19>(reader, num_symbols),
        14..=18 => decode_direct::<20>(reader, num_symbols),
        _ => Err(Err::InvalidBitLength(bit_length)),
    }
}

fn decode_direct<const RANS_PRECISION: usize>(
    reader: &mut Reader<'_>,
    num_symbols: usize,
) -> Result<Vec<u64>, Err> {
    let mut decoder = RansSymbolDecoder::<'_, RANS_PRECISION>::new(reader, num_symbols)?;
    let mut out = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols {
        out.push(decoder.decode() as u64);
    }
    Ok(out)
}

/// Inverse of core's `to_positive_i32` zigzag mapping: even codes to non-negative,
/// odd codes to negative.
pub fn unzigzag(val: u32) -> i32 {
    ((val >> 1) as i32) ^ -((val & 1) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use draco_oxide_core::bit_coder::ByteWriter;
    use draco_oxide_core::codec::entropy::rans::RansSymbolEncoder;
    use draco_oxide_core::utils::to_positive_i32;

    #[test]
    fn unzigzag_inverts_to_positive_i32() {
        for n in -5000i32..=5000 {
            let code = to_positive_i32(n) as u32;
            assert_eq!(unzigzag(code), n);
        }
    }

    #[test]
    fn decode_symbols_direct_round_trip() {
        let symbols: Vec<u64> = vec![0, 1, 2, 1, 0, 3, 3, 2, 1, 0, 0, 1, 2, 3, 0, 2, 1, 3];

        // Reproduce the DirectCoded framing: method byte, bit-length byte (8 maps to
        // precision 12 in the decoder), then the `RansSymbolEncoder` payload.
        let mut buf: Vec<u8> = Vec::new();
        SymbolEncodingMethod::DirectCoded.write_to(&mut buf);
        buf.write_u8(8);
        let max = *symbols.iter().max().unwrap() as usize;
        let mut freq = vec![0usize; max + 1];
        for &s in &symbols {
            freq[s as usize] += 1;
        }
        let mut enc = RansSymbolEncoder::<'_, Vec<u8>, 5, 12>::new(&mut buf, freq, None).unwrap();
        for &s in symbols.iter().rev() {
            enc.write(s as usize).unwrap();
        }
        enc.flush().unwrap();

        // A trailing sentinel confirms `decode_symbols` consumes exactly the payload.
        buf.write_u8(0xAB);

        let mut reader = Reader::new(&buf);
        let decoded = decode_symbols(&mut reader, symbols.len(), 1).unwrap();
        assert_eq!(decoded, symbols);
        assert_eq!(reader.read_u8().unwrap(), 0xAB);
    }
}
