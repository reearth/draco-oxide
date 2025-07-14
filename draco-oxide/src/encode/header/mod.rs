use crate::{core::bit_coder::ByteWriter, shared::header::EncoderMethod};

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
}

#[derive(Clone, Debug)]
pub enum EncodedGeometryType {
    #[allow(unused)]
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

const METADATA_FLAG_MASK: u16 = 32768;

pub fn encode_header<W>(writer: &mut W, cfg: &super::Config) -> Result<(), Err>
where
    W: ByteWriter,
{
    // Write the draco string
    "DRACO".as_bytes().iter().for_each(|&b| {
        writer.write_u8(b);
    });

    // Write the version
    writer.write_u8(2);
    writer.write_u8(2);

    // Write encoder type
    let id = cfg.geometry_type.get_id();
    writer.write_u8(id);

    // Write the encoding method
    // Currently, we only support the edgebreaker method
    EncoderMethod::Edgebreaker.write_to(writer);

    // Write the connectivity encoder config
    if cfg.metdata {
        writer.write_u16(METADATA_FLAG_MASK);
    } else {
        writer.write_u16(0);
    }

    Ok(())
}