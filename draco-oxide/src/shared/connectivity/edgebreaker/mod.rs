use crate::{core::{bit_coder::ReaderErr}, prelude::{ByteReader, ByteWriter}};

pub mod symbol_encoder;
pub mod prediction;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TopologySplit {
    pub merging_symbol_idx: usize,
    pub split_symbol_idx: usize,
    pub merging_edge_orientation: Orientation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Orientation {
    Left,
    Right,
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // This enum is not used yet, as we only support the default configuration.
pub(crate) enum EdgebreakerKind {
    Standard,
    Predictive,
    Valence,
}

impl EdgebreakerKind {
    #[allow(unused)] // TODO: Remove this function when the decoder is complete
    pub(crate) fn read_from<R>(reader: &mut R) -> Result<Self, Err> 
        where R: ByteReader
    {
        let traversal_type = reader.read_u8()?;
        match traversal_type {
            0 => Ok(Self::Standard),
            1 => Ok(Self::Predictive),
            2 => Ok(Self::Valence),
            _ => Err(Err::InvalidTraversalType(traversal_type)),
        }
    }


    pub(crate) fn write_to<W>(self, writer: &mut W) 
        where W: ByteWriter
    {
        let traversal_type = match self {
            Self::Standard => 0,
            Self::Predictive => 1,
            Self::Valence => 2,
        };
        writer.write_u8(traversal_type);
    }
}


pub(crate) const MAX_VALENCE: usize = 7;
pub(crate) const MIN_VALENCE: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TraversalType {
    DepthFirst,
    #[allow(dead_code)] // This variant is not used yet. We might not implement this and may simply remove it in the future.
    PredictionDegree,
}

impl TraversalType {
    #[allow(unused)] // TODO: Remove this function when the decoder is complete
    pub(crate) fn read_from<R>(reader: &mut R) -> Result<Self, Err> 
        where R: ByteReader
    {
        let traversal_type = reader.read_u8()?;
        match traversal_type {
            0 => Ok(Self::DepthFirst),
            1 => Ok(Self::PredictionDegree),
            _ => Err(Err::InvalidTraversalType(traversal_type)),
        }
    }

    pub(crate) fn write_to<W>(self, writer: &mut W) 
        where W: ByteWriter
    {
        let traversal_type = match self {
            Self::DepthFirst => 0,
            Self::PredictionDegree => 1,
        };
        writer.write_u8(traversal_type);
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Err {
    #[error("Invalid traversal type: {0}")]
    InvalidTraversalType(u8),
    #[error("Reader error")]
    ReaderError(#[from] ReaderErr),
}


#[allow(unused)] // This enum is not used yet, as we only support the default configuration.
pub(crate) enum SymbolRansEncodingConfig {
    LengthCoded,
    DirectCoded,
}

impl SymbolRansEncodingConfig {
    #[allow(unused)] // This function is not used yet, as we only support the default configuration.
    pub(crate) fn read_from<R>(reader: &mut R) -> Result<Self, Err> 
        where R: ByteReader
    {
        let config = reader.read_u8()?;
        match config {
            0 => Ok(Self::LengthCoded),
            1 => Ok(Self::DirectCoded),
            _ => Err(Err::InvalidTraversalType(config)),
        }
    }

    #[allow(unused)] // TODO: Remove this.
    pub(crate) fn write_to<W>(self, writer: &mut W) 
        where W: ByteWriter
    {
        let config = match self {
            Self::LengthCoded => 0,
            Self::DirectCoded => 1,
        };
        writer.write_u8(config);
    }
}
