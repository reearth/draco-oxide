use crate::encode::connectivity::ConnectivityEncoder;
use crate::encode::entropy::symbol_coding::encode_symbols;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::connectivity::sequential::index_size_from_vertex_count;
use draco_oxide_core::codec::connectivity::sequential::Method;
use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
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

    /// Writes the face indices verbatim, in the smallest width that holds the
    /// point space.
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
            for face in &self.faces {
                leb128_write(usize::from(face[0]) as u64, writer);
                leb128_write(usize::from(face[1]) as u64, writer);
                leb128_write(usize::from(face[2]) as u64, writer);
            }
        } else {
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

    /// Entropy-codes the face indices as deltas of the flattened index
    /// sequence, each delta stored as its magnitude with the sign in the low
    /// bit.
    fn encode_compressed_indices<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        debug_write!("Start of indices", writer);

        let mut symbols = Vec::with_capacity(self.faces.len() * 3);
        let mut last: i64 = 0;
        for face in &self.faces {
            for &p in face {
                let index = usize::from(p) as i64;
                let diff = index - last;
                symbols.push((diff.unsigned_abs() << 1) | (diff < 0) as u64);
                last = index;
            }
        }
        encode_symbols(symbols, 1, SymbolEncodingMethod::DirectCoded, writer)
            .map_err(Err::SymbolEncodingError)
    }
}

impl ConnectivityEncoder for Sequential {
    type Err = Err;
    type Config = Config;

    fn encode_connectivity<W>(self, writer: &mut W) -> Result<Vec<CornerIdx>, Err>
    where
        W: ByteWriter,
    {
        leb128_write(self.faces.len() as u64, writer);
        leb128_write(self.num_points as u64, writer);
        writer.write_u8(self.cfg.encoder_method.get_id());
        match self.cfg.encoder_method {
            Method::DirectIndices => self.encode_direct_indices(writer)?,
            Method::Compressed => self.encode_compressed_indices(writer)?,
        }

        // Sequential connectivity has no edgebreaker traversal to surface.
        Ok(Vec::new())
    }
}

/// Configuration for sequential connectivity encoding. Exported as
/// `SequentialConfig`.
#[derive(Clone, Debug)]
pub struct Config {
    /// How face indices are stored: verbatim, or entropy-coded as deltas.
    pub encoder_method: Method,
}

impl ConfigType for Config {
    fn default() -> Self {
        Config {
            encoder_method: Method::DirectIndices,
        }
    }
}

/// Errors from sequential connectivity encoding.
#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    /// The shared sequential connectivity codec reported an error.
    #[error("Invalid vertex count")]
    SharedError(draco_oxide_core::codec::connectivity::sequential::Err),
    /// Entropy coding of the face indices failed.
    #[error("Entropy Symbol Encoding Error: {0}")]
    SymbolEncodingError(crate::encode::entropy::symbol_coding::Err),
}
