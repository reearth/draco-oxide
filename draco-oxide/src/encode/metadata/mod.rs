use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::utils::bit_coder::leb128_write;

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {}

/// Writes an empty geometry metadata section: no per-attribute metadata
/// blocks, and a geometry-level block with no entries and no sub-metadata.
///
/// TODO: encode actual metadata (per-attribute blocks, name/value entries,
/// nested sub-metadata) once the mesh carries any.
pub fn encode_metadata<W>(_mesh: &draco_oxide_core::mesh::Mesh, writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    leb128_write(0, writer); // att_metadata_count
    leb128_write(0, writer); // num_entries
    leb128_write(0, writer); // num_sub_metadata
    Ok(())
}
