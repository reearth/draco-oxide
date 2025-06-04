use crate::core::bit_coder::ByteWriter;

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
}

pub enum EncodedGeometryType {
    PointCloud,
    TrianglarMesh,
}

impl EncodedGeometryType {
    pub fn get_id(&self) -> u8 {
        match self {
            EncodedGeometryType::PointCloud => 0,
            EncodedGeometryType::TrianglarMesh => 1,
        }
    }
}

pub fn encode_header<W>(writer: &mut W, cfg: &super::Config) -> Result<(), Err>
where
    W: ByteWriter,
{
    // Write the draco string
    "DRACO".as_bytes().iter().for_each(|&b| {
        writer.write_u8(b);
    });

    // Write the version
    writer.write_u8(1);
    writer.write_u8(5);

    // Write encoder type
    let id = cfg.geometry_type.get_id();
    writer.write_u8(id);

    // Write the encoding method
    let encoder_method = cfg.encoder_method.get_id() as u8;
    writer.write_u8(encoder_method);

    Ok(())
}