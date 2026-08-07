use crate::bit_coder::{ByteWriter, ReaderErr};

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
pub enum EdgebreakerKind {
    Standard,
    Predictive,
    Valence,
}

impl EdgebreakerKind {
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
