use crate::core::shared::VertexIdx;
use crate::core::shared::ConfigType;
use crate::debug_write;
use crate::encode::connectivity::ConnectivityEncoder;
use crate::prelude::ByteWriter;
use crate::shared::connectivity::sequential::index_size_from_vertex_count;
use crate::shared::connectivity::sequential::Method;
use crate::utils::bit_coder::leb128_write;

pub(crate) struct Sequential {
    cfg: Config,
    num_points: usize,
}

impl Sequential {
    pub fn new(config: Config, num_points: usize) -> Self {
        Self {
            cfg: config,
            num_points
        }
    }

    fn encode_direct_indices<W>(
        &self,
        faces: &[[VertexIdx; 3]],
        writer: &mut W
    ) -> Result<(), Err> 
        where  W: ByteWriter,
    {
        let index_size = match index_size_from_vertex_count(self.num_points) {
            Ok(index_size) => index_size as u8,
            Err(err) => return Err(Err::SharedError(err)),
        };
        debug_write!("Start of indices", writer);
        
        if index_size == 21 {
            // varint encoding
            for face in faces {
                leb128_write(face[0] as u64, writer);
                leb128_write(face[1] as u64, writer);
                leb128_write(face[2] as u64, writer);
            }
        } else {
            // non-varint encoding
            match index_size {
                8 => for face in faces {
                    writer.write_u8(face[0] as u8);
                    writer.write_u8(face[1] as u8);
                    writer.write_u8(face[2] as u8);
                }
                16 => for face in faces {
                    writer.write_u16(face[0] as u16);
                    writer.write_u16(face[1] as u16);
                    writer.write_u16(face[2] as u16);
                },
                32 => for face in faces {
                    writer.write_u32(face[0] as u32);
                    writer.write_u32(face[1] as u32);
                    writer.write_u32(face[2] as u32);
                },
                _ => unreachable!()
            }
        }
        Ok(())
    }
}

impl ConnectivityEncoder for Sequential {
    type Err = Err;
    type Config = Config;
    type Output = ();

    fn encode_connectivity<W>(
        self, 
        faces: &[[VertexIdx; 3]],
        writer: &mut W
    ) -> Result<(), Err> 
        where  W: ByteWriter,
    {
        writer.write_u64(faces.len() as u64);
        let encoder_method_id = self.cfg.encoder_method.get_id();
        writer.write_u8(encoder_method_id);
        self.encode_direct_indices(faces, writer)?;

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub encoder_method: Method,
}

impl ConfigType for Config {
    fn default() -> Self {
        Config {
            encoder_method: Method::DirectIndices,
        }
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Invalid vertex count")]
    SharedError(crate::shared::connectivity::sequential::Err),
}

