use crate::bit_coder::{ByteWriter, Reader, ReaderErr};

pub mod prediction;
pub mod symbol_encoder;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopologySplit {
    pub merging_symbol_idx: usize,
    pub split_symbol_idx: usize,
    pub merging_edge_orientation: Orientation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // This enum is not used yet, as we only support the default configuration.
pub enum EdgebreakerKind {
    Standard,
    Predictive,
    Valence,
}

impl EdgebreakerKind {
    #[allow(unused)] // TODO: Remove this function when the decoder is complete
    pub fn read_from(reader: &mut Reader<'_>) -> Result<Self, Err> {
        let traversal_type = reader.read_u8()?;
        match traversal_type {
            0 => Ok(Self::Standard),
            1 => Ok(Self::Predictive),
            2 => Ok(Self::Valence),
            _ => Err(Err::InvalidTraversalType(traversal_type)),
        }
    }

    pub fn write_to<W>(self, writer: &mut W)
    where
        W: ByteWriter,
    {
        let traversal_type = match self {
            Self::Standard => 0,
            Self::Predictive => 1,
            Self::Valence => 2,
        };
        writer.write_u8(traversal_type);
    }
}

pub const MAX_VALENCE: usize = 7;
pub const MIN_VALENCE: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraversalType {
    DepthFirst,
    PredictionDegree,
}

impl TraversalType {
    #[allow(unused)] // TODO: Remove this function when the decoder is complete
    pub fn read_from(reader: &mut Reader<'_>) -> Result<Self, Err> {
        let traversal_type = reader.read_u8()?;
        match traversal_type {
            0 => Ok(Self::DepthFirst),
            1 => Ok(Self::PredictionDegree),
            _ => Err(Err::InvalidTraversalType(traversal_type)),
        }
    }

    pub fn write_to<W>(self, writer: &mut W)
    where
        W: ByteWriter,
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
pub enum SymbolRansEncodingConfig {
    LengthCoded,
    DirectCoded,
}

impl SymbolRansEncodingConfig {
    #[allow(unused)] // This function is not used yet, as we only support the default configuration.
    pub fn read_from(reader: &mut Reader<'_>) -> Result<Self, Err> {
        let config = reader.read_u8()?;
        match config {
            0 => Ok(Self::LengthCoded),
            1 => Ok(Self::DirectCoded),
            _ => Err(Err::InvalidTraversalType(config)),
        }
    }

    #[allow(unused)] // TODO: Remove this.
    pub fn write_to<W>(self, writer: &mut W)
    where
        W: ByteWriter,
    {
        let config = match self {
            Self::LengthCoded => 0,
            Self::DirectCoded => 1,
        };
        writer.write_u8(config);
    }
}
