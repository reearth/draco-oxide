//! Metadata parsing. For now this only mirrors the encoder's stub (`u32 = 0`);
//! real metadata parsing arrives with Google interop in milestone B.

use crate::Err;
use draco_oxide_core::bit_coder::Reader;

/// Consumes the metadata section the encoder writes when the metadata flag is
/// set (currently a stub `u32`).
pub(crate) fn decode_metadata(reader: &mut Reader<'_>) -> Result<(), Err> {
    reader.read_u32()?;
    Ok(())
}
