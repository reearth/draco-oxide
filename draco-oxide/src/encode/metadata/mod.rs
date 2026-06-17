use draco_oxide_core::bit_coder::ByteWriter;

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {}

pub fn encode_metadata<W>(_mesh: &draco_oxide_core::mesh::Mesh, writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    // Write Encoder
    writer.write_u32(0);

    // ToDo: Implement metadata encoding
    Ok(())
}
