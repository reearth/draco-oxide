use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::utils::bit_coder::leb128_write;

/// Errors from metadata encoding. Currently uninhabited; metadata encoding
/// cannot fail.
#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {}

/// Writes a stub metadata section: no per-attribute metadata blocks, and a
/// geometry-level block with no entries and no sub-metadata. Mesh-carried
/// metadata is not encoded.
pub fn encode_metadata<W>(_mesh: &draco_oxide_core::mesh::Mesh, writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    write_stub(writer);
    Ok(())
}

/// Writes the stub metadata section for a point cloud.
pub fn encode_point_cloud_metadata<W>(
    _attributes: &[draco_oxide_core::attribute::Attribute],
    writer: &mut W,
) where
    W: ByteWriter,
{
    write_stub(writer);
}

fn write_stub<W>(writer: &mut W)
where
    W: ByteWriter,
{
    leb128_write(0, writer); // att_metadata_count
    leb128_write(0, writer); // num_entries
    leb128_write(0, writer); // num_sub_metadata
}
