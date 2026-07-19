//! Metadata parsing. For now this only mirrors the encoder's stub (`u32 = 0`);
//! real metadata parsing arrives with Google interop in milestone B.

use crate::Err;
use draco_oxide_core::bit_coder::ByteReader;

/// Consumes the metadata section the encoder writes when the metadata flag is
/// set (currently a stub `u32`).
pub(crate) fn decode_metadata<R: ByteReader>(reader: &mut R) -> Result<(), Err> {
    reader.read_u32()?;
    Ok(())
}
