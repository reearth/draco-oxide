use crate::bit_coder::{ByteWriter, Reader, ReaderErr};

pub mod shannon;

pub const L_RANS_BASE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolEncodingMethod {
    LengthCoded,
    DirectCoded,
}

impl SymbolEncodingMethod {
    pub fn read_from(reader: &mut Reader<'_>) -> Result<Self, Err> {
        let method = reader.read_u8()?;
        match method {
            0 => Ok(SymbolEncodingMethod::LengthCoded),
            1 => Ok(SymbolEncodingMethod::DirectCoded),
            _ => Err(Err::InvalidSymbolEncodingMethod),
        }
    }
    pub fn write_to<W>(&self, writer: &mut W)
    where
        W: ByteWriter,
    {
        match self {
            SymbolEncodingMethod::LengthCoded => writer.write_u8(0),
            SymbolEncodingMethod::DirectCoded => writer.write_u8(1),
        }
    }
}

/// One alphabet entry of a rANS distribution. The fields fit in `u32` because
/// frequencies sum to `2^precision` and the precision never exceeds 20.
pub struct RansSymbol {
    pub freq_count: u32,
    pub freq_cumulative: u32,
}

/// Builds the cumulative-frequency table over the alphabet. The frequencies
/// must sum to exactly `2^precision`.
pub fn rans_symbol_table(freq_counts: &[usize], precision: usize) -> Result<Vec<RansSymbol>, Err> {
    let mut rans_syms = Vec::with_capacity(freq_counts.len());

    let mut freq_cumulative: usize = 0;
    for freq_count in freq_counts {
        // The casts are lossless for every table that passes the final sum
        // check, since all partial sums are then bounded by 2^precision.
        rans_syms.push(RansSymbol {
            freq_count: *freq_count as u32,
            freq_cumulative: freq_cumulative as u32,
        });
        freq_cumulative = freq_cumulative
            .checked_add(*freq_count)
            .ok_or(Err::InvalidFreqCount)?;
    }

    if freq_cumulative != 1 << precision {
        return Err(Err::FrequencyCountNotCompatibleWithRansPrecision(
            freq_cumulative,
            1 << precision,
        ));
    }

    Ok(rans_syms)
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    #[error(
        "Frequency count not compatible with RANS precision: freq_count=={0}!={1}==rans_precision"
    )]
    FrequencyCountNotCompatibleWithRansPrecision(usize, usize),
    #[error("Invalid frequency count")]
    InvalidFreqCount,
    #[error("Invalid symbol encoding method")]
    InvalidSymbolEncodingMethod,
    #[error("Reader error")]
    ReaderError(#[from] ReaderErr),
}
