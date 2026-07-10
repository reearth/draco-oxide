use crate::encode::connectivity::ConnectivityEncoder;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::connectivity::sequential::index_size_from_vertex_count;
use draco_oxide_core::codec::connectivity::sequential::Method;
use draco_oxide_core::debug_write;
use draco_oxide_core::types::ConfigType;
use draco_oxide_core::types::{CornerIdx, PointIdx};
use draco_oxide_core::utils::bit_coder::leb128_write;

pub(crate) struct Sequential {
    cfg: Config,
    num_points: usize,
    faces: Vec<[PointIdx; 3]>,
}

impl Sequential {
    pub fn new(faces: &[[PointIdx; 3]], config: Config, num_points: usize) -> Self {
        Self {
            cfg: config,
            num_points,
            faces: faces.to_vec(),
        }
    }

    fn encode_direct_indices<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        let index_size = match index_size_from_vertex_count(self.num_points) {
            Ok(index_size) => index_size as u8,
            Err(err) => return Err(Err::SharedError(err)),
        };
        debug_write!("Start of indices", writer);

        if index_size == 21 {
            // varint encoding
            for face in &self.faces {
                leb128_write(usize::from(face[0]) as u64, writer);
                leb128_write(usize::from(face[1]) as u64, writer);
                leb128_write(usize::from(face[2]) as u64, writer);
            }
        } else {
            // non-varint encoding
            match index_size {
                8 => {
                    for face in &self.faces {
                        writer.write_u8(usize::from(face[0]) as u8);
                        writer.write_u8(usize::from(face[1]) as u8);
                        writer.write_u8(usize::from(face[2]) as u8);
                    }
                }
                16 => {
                    for face in &self.faces {
                        writer.write_u16(usize::from(face[0]) as u16);
                        writer.write_u16(usize::from(face[1]) as u16);
                        writer.write_u16(usize::from(face[2]) as u16);
                    }
                }
                32 => {
                    for face in &self.faces {
                        writer.write_u32(usize::from(face[0]) as u32);
                        writer.write_u32(usize::from(face[1]) as u32);
                        writer.write_u32(usize::from(face[2]) as u32);
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }
}

impl ConnectivityEncoder for Sequential {
    type Err = Err;
    type Config = Config;

    fn encode_connectivity<W>(self, writer: &mut W) -> Result<Vec<CornerIdx>, Err>
    where
        W: ByteWriter,
    {
        writer.write_u64(self.faces.len() as u64);
        let encoder_method_id = self.cfg.encoder_method.get_id();
        writer.write_u8(encoder_method_id);
        self.encode_direct_indices(writer)?;

        // Sequential connectivity has no edgebreaker traversal to surface.
        Ok(Vec::new())
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
    SharedError(draco_oxide_core::codec::connectivity::sequential::Err),
}
