//! Connectivity decoder.
//!
//! Mirrors `encode/connectivity/`. Currently only the Edgebreaker Standard
//! traversal is implemented (matching what the encoder hardcodes at
//! `encode/connectivity/edgebreaker.rs:509`).

pub(crate) mod attribute_corner_table;
pub(crate) mod corner_table;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use draco_oxide_core::types::VertexIdx;
use crate::decode::header::Header;
use crate::prelude::ByteReader;
use draco_oxide_core::codec::header::EncoderMethod;

pub(crate) use attribute_corner_table::DecoderAttributeCornerTable;
pub(crate) use corner_table::DecoderCornerTable;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Edgebreaker decode error: {0}")]
    Edgebreaker(#[from] edgebreaker::Err),
    #[error("Sequential decode error: {0}")]
    Sequential(#[from] sequential::Err),
}

/// Decoded mesh connectivity: triangle list addressing the position attribute,
/// the corner table that produced it (for the per-attribute decode pipeline),
/// and the corner-stack residue from the start-face-config replay (used as
/// seed corners for the attribute-side `Traverser`).
pub(crate) struct DecodedConnectivity {
    pub faces: Vec<[VertexIdx; 3]>,
    pub corner_table: DecoderCornerTable,
    /// Start corners for each connected component, in the order they were
    /// popped from the active stack during start-face-config replay.
    /// `Traverser::new` expects these as `corners_of_edgebreaker_traversal`.
    pub start_corners: Vec<draco_oxide_core::types::CornerIdx>,
    /// Number of unique attribute vertices the encoder reports for the
    /// position attribute (= `num_vertices` field in the bitstream). May be
    /// less than `corner_table.num_vertices()` because S-symbol merges
    /// remove vertices but `DecoderCornerTable` doesn't compact in-place.
    /// This is the authoritative count for sizing the per-attribute symbol
    /// stream.
    pub num_position_vertices: usize,
    /// Per-attribute corner tables. Index matches the `decoder_id`
    /// field on `AttributeMeta` (the encoder writes
    /// `(i as u8).wrapping_sub(1)` so 0xFF means "use position table"
    /// and other values are indices into this Vec).
    pub attribute_corner_tables: Vec<DecoderAttributeCornerTable>,
}

pub(crate) fn decode_connectivity<R: ByteReader>(
    reader: &mut R,
    header: &Header,
) -> Result<DecodedConnectivity, Err> {
    // Sequential meshes are decoded by `decode::decode_sequential_to_raw`
    // (they have no corner table); this entry point is edgebreaker-only.
    match header.encoding_method {
        EncoderMethod::Edgebreaker => edgebreaker::decode(reader).map_err(Into::into),
        EncoderMethod::Sequential => Err(Err::Sequential(sequential::Err::InvalidMethod(0xFF))),
    }
}
