pub(crate) mod header;
pub(crate) mod metadata;
pub(crate) mod connectivity;
pub(crate) mod attribute;
pub(crate) mod entropy;

use crate::{core::mesh::Mesh, prelude::ConfigType};
use thiserror::Error;

pub trait EncoderConfig {
    type Encoder;
    fn get_encoder(&self) -> Self::Encoder;
}

pub struct Config {
    connectivity_encoder_cfg: connectivity::Config,
    attribute_encoder_cfg: attribute::Config,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            connectivity_encoder_cfg: connectivity::Config::default(),
            attribute_encoder_cfg: attribute::Config::default(),
        }
    }
}

#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error")]
    AttributeError(#[from] attribute::Err),
    #[error("Connectivity encoding error")]
    ConnectivityError(#[from] connectivity::Err),
    #[error("Header encoding error")]
    HeaderError(#[from] header::Err),
    #[error("Metadata encoding error")]
    MetadataError(#[from] metadata::Err),
}


pub fn encode<F>(mut mesh: Mesh, writer: &mut F, cfg: Config) -> Result<(), Err> 
    where F: FnMut((u8, u64))
{
    // Encode header
    header::encode_header(writer)
        .map_err(|r| Err::HeaderError(r))?;

    // Encode metadata
    metadata::encode_metadata(&mesh, writer)
        .map_err(|r| Err::MetadataError(r))?;

    // Encode connectivity
    connectivity::encode_connectivity(&mut mesh, writer, cfg.connectivity_encoder_cfg)
        .map_err(|r| Err::ConnectivityError(r))?;

    // Encode attributes
    attribute::encode_attributes(&mut mesh, writer, cfg.attribute_encoder_cfg)
        .map_err(|r| Err::AttributeError(r))?;

    Ok(())
}