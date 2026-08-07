use crate::encode::entropy::rans;
use crate::encode::entropy::rans::RansSymbolEncoder;
use draco_oxide_core::bit_coder::BitWriter;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
use draco_oxide_core::types::{NdVector, Vector};

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    #[error("RANS encoding error")]
    RansEncodingError(#[from] rans::Err),
    #[error("Invalid inputs for encode_tagged_symbol(): It must be true that symbol.len()==num_values*num_components, but got symbol.len()={0}, num_values={1}, num_components={2}")]
    InvalidInputs(usize, usize, usize),
    #[error("Invalid bit length: {0}")]
    InvalidBitLength(usize),
}

pub fn encode_symbols<W>(
    symbols: Vec<u64>,
    num_components: usize,
    config: SymbolEncodingMethod,
    writer: &mut W,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    config.write_to(writer);
    // ToDo: Add the logic to dynamically determine the config
    match config {
        SymbolEncodingMethod::LengthCoded => {
            let mut bit_lengths = Vec::with_capacity(symbols.len() / num_components);
            for i in 0..symbols.len() / num_components {
                let mut max_bit_length = 0;
                for j in 0..num_components {
                    let s = symbols[i * num_components + j];
                    let bit_length = (64 - s.leading_zeros()) as usize;
                    if bit_length > max_bit_length {
                        max_bit_length = bit_length;
                    }
                }
                bit_lengths.push(max_bit_length as u8);
            }
            encode_symbols_length_coded(symbols, num_components, bit_lengths, writer)
        }
        SymbolEncodingMethod::DirectCoded => encode_symbols_direct_coded(symbols, writer),
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
    writer: &mut W,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    let mut freq_counts = Vec::new();

    for &bit_length in &bit_lengths {
        let bit_length = bit_length as usize;
        if freq_counts.len() <= bit_length {
            freq_counts.resize(bit_length + 1, 0);
        }
        freq_counts[bit_length] += 1;
    }

    let mut encoder = RansSymbolEncoder::new(writer, freq_counts, None, 12)?;
    for i in (0..symbols.len() / num_components).rev() {
        encoder.write(bit_lengths[i] as usize)?;
    }
    encoder.flush()?;

    // Values are always encoded in the normal order, appended to the end of
    // the target buffer.
    let mut writer: BitWriter<_> = BitWriter::spown_from(writer);
    for i in 0..symbols.len() / num_components {
        let value_bit_length = bit_lengths[i];
        for c in 0..num_components {
            writer.write_bits((value_bit_length, symbols[i * num_components + c]));
        }
    }
    Ok(())
}

/// Encodes symbols with the raw rANS scheme. The leading bit-length byte is
/// the bit width of the number of distinct symbol values, matching Google's
/// encoder; the decoder derives the rANS precision from it.
fn encode_symbols_direct_coded<W>(symbols: Vec<u64>, writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    encode_direct_coded_streams(
        symbols.iter().map(|&s| s as usize),
        symbols.iter().rev().map(|&s| s as usize),
        writer,
    )
}

/// Encodes per-value correction vectors with the raw rANS scheme, reading the
/// components in place. The stream is identical to flattening the components
/// into `u64` symbols and calling [`encode_symbols`] with `DirectCoded`, but
/// no flat symbol array is materialized.
pub fn encode_vector_symbols<W, const N: usize>(
    values: &[NdVector<N, i32>],
    writer: &mut W,
) -> Result<(), Err>
where
    W: ByteWriter,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    SymbolEncodingMethod::DirectCoded.write_to(writer);
    encode_direct_coded_streams(
        values
            .iter()
            .flat_map(|v| (0..N).map(move |i| *v.get(i) as usize)),
        values
            .iter()
            .rev()
            .flat_map(|v| (0..N).rev().map(move |i| *v.get(i) as usize)),
        writer,
    )
}

/// The raw rANS scheme over two reads of the same symbol stream: `forward`
/// yields every symbol once for the frequency table, and `reversed` must
/// yield exactly the same symbols in reverse for the rANS feed.
fn encode_direct_coded_streams<W>(
    forward: impl Iterator<Item = usize>,
    reversed: impl Iterator<Item = usize>,
    writer: &mut W,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    let mut freq_counts: Vec<usize> = Vec::new();
    let mut max_symbol = 0;
    for s in forward {
        if s >= max_symbol {
            max_symbol = s;
            freq_counts.resize(max_symbol + 1, 0);
        }
        freq_counts[s] += 1;
    }
    let num_unique_symbols = freq_counts.iter().filter(|&&c| c > 0).count();

    let bit_length = (usize::BITS - num_unique_symbols.leading_zeros()) as usize;
    let bit_length = bit_length.clamp(1, 18);
    writer.write_u8(bit_length as u8);
    // The same bit-length-to-precision mapping the decoder derives
    // (`clamp(3 * bit_length / 2, 12, 20)`).
    let precision = match bit_length {
        1..=8 => 12,
        9 => 13,
        10 => 15,
        11 => 16,
        12 => 18,
        13 => 19,
        _ => 20,
    };
    let mut encoder = RansSymbolEncoder::new(writer, freq_counts, None, precision)?;
    for s in reversed {
        encoder.write(s)?;
    }
    encoder.flush()?;
    Ok(())
}
