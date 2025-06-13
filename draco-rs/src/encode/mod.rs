pub(crate) mod header;
pub(crate) mod metadata;
pub(crate) mod connectivity;
pub(crate) mod attribute;
pub(crate) mod entropy;

use crate::core::mesh::Mesh;
use crate::{debug_write, shared};
use crate::core::shared::ConfigType;
use crate::core::bit_coder::ByteWriter;
use thiserror::Error;

#[cfg(feature = "evaluation")]
use crate::eval;

pub trait EncoderConfig {
    type Encoder;
    fn get_encoder(&self) -> Self::Encoder;
}

pub struct Config {
    connectivity_encoder_cfg: connectivity::Config,
    attribute_encoder_cfg: attribute::Config,
    geometry_type: header::EncodedGeometryType,
    encoder_method: shared::connectivity::Encoder,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            connectivity_encoder_cfg: connectivity::Config::default(),
            attribute_encoder_cfg: attribute::Config::default(),
            geometry_type: header::EncodedGeometryType::TrianglarMesh,
            encoder_method: shared::connectivity::Encoder::Edgebreaker,
        }
    }
}

#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute::Err),
    #[error("Connectivity encoding error: {0}")]
    ConnectivityError(#[from] connectivity::Err),
    #[error("Header encoding error: {0}")]
    HeaderError(#[from] header::Err),
    #[error("Metadata encoding error: {0}")]
    MetadataError(#[from] metadata::Err),
}


pub fn encode<W>(mesh: Mesh, writer: &mut W, cfg: Config) -> Result<(), Err> 
    where W: ByteWriter
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("compression info", writer);
    
    // Encode header
    header::encode_header(writer, &cfg)?;

    debug_write!("Header done, now starting metadata.", writer);

    // Encode metadata
    metadata::encode_metadata(&mesh, writer)?;

    debug_write!("Metadata done, now starting connectivity.", writer);

    // Destrut the mesh so that attributes and faces have the different lifetime. 
    let Mesh{attributes, faces, ..} = mesh;
    
    // Encode connectivity
    let conn_out = connectivity::encode_connectivity(&faces, &attributes, writer, &cfg)?;

    debug_write!("Connectivity done, now starting attributes.", writer);

    // Encode attributes
    attribute::encode_attributes(attributes, writer, conn_out, &cfg)?;

    debug_write!("All done", writer);

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    Ok(())
}
