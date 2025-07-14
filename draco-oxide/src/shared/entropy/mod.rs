use crate::{core::bit_coder::ReaderErr, prelude::{ByteReader, ByteWriter}};

pub(crate) const L_RANS_BASE: usize = 4096;
pub(crate) const DEFAULT_RANS_PRECISION: usize = 12;
pub(crate) const DEFAULT_RABS_PRECISION: usize = 8;

pub trait Symbol {
    fn cardinality() -> usize;
    fn size(&self) -> usize;
    fn get_id(&self) -> usize;
    fn from_id(id: usize) -> Self;
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolEncodingMethod {
    #[allow(unused)]
    LengthCoded,
    DirectCoded,
}

impl SymbolEncodingMethod {
    #[allow(unused)]
    pub fn read_from<R>(reader: &mut R) -> Result<Self, Err>
        where R: ByteReader
    {
        let method = reader.read_u8()?;
        match method {
            0 => Ok(SymbolEncodingMethod::LengthCoded),
            1 => Ok(SymbolEncodingMethod::DirectCoded),
            _ => Err(Err::InvalidSymbolEncodingMethod),
        }
    }
    pub fn write_to<W>(&self, writer: &mut W) 
        where W: ByteWriter
    {
        match self {
            SymbolEncodingMethod::LengthCoded => writer.write_u8(0),
            SymbolEncodingMethod::DirectCoded => writer.write_u8(1),
        }
    }
}

pub(crate) struct RansSymbol {
    pub freq_count: usize,
    pub freq_cumulative: usize,
}

pub(crate) fn rans_build_tables<const RANS_PRECISION: usize>(freq_counts: &[usize]) -> Result<(Vec<usize>, Vec<RansSymbol>), Err> {
    let mut slot_table = Vec::with_capacity(1<<RANS_PRECISION);
    let mut rans_syms = Vec::with_capacity(freq_counts.len());

    let mut freq_cumulative = 0;
    for (i, freq_count) in freq_counts.iter().enumerate() {
        let symbol = RansSymbol {
            freq_count: *freq_count,
            freq_cumulative,
        };
        rans_syms.push(symbol);
        let tmp = freq_cumulative;
        freq_cumulative = freq_cumulative.checked_add(*freq_count).ok_or(Err::InvalidFreqCount)?; // cumulative frequency count is not inclusive, so this operation is done after creating the symbol
        for _ in tmp..freq_cumulative {
            slot_table.push(i);
        }
    }

    if freq_cumulative != 1 << RANS_PRECISION {
        return Err(Err::FrequencyCountNotCompatibleWithRansPrecision(freq_cumulative, 1 << RANS_PRECISION));
    }

    Ok((slot_table, rans_syms))
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Err {
    #[error("Frequency count not compatible with RANS precision: freq_count=={0}!={1}==rans_precision")]
    FrequencyCountNotCompatibleWithRansPrecision(usize, usize),
    #[error("Invalid frequency count")]
    InvalidFreqCount,
    #[error("Invalid symbol encoding method")]
    InvalidSymbolEncodingMethod,
    #[error("Reader error")]
    ReaderError(#[from] ReaderErr),
}
    