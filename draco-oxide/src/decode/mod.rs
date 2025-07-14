use crate::{debug_expect, prelude::{ByteReader, ConfigType}, Mesh};

mod header;
mod metadata;
// mod connectivity;
// mod attribute;
mod entropy;

pub fn decode<W>(reader: &mut W, cfg: Config) -> Result<Mesh, Err> 
    where W: ByteReader
{
    // Decode header
    let header = header::decode_header(reader)
        .map_err(|r| Err::HeaderError(r))?;

    debug_expect!("Header done, now starting metadata.", reader);

    // Decode metadata
    if header.contains_metadata {
        let metadata  = metadata::decode_metadata(reader)
            .map_err(|r| Err::MetadataError(r))?;
    }

    debug_expect!("Metadata done, now starting connectivity.", reader);

    // Decode connectivity
    // let connectivity = connectivity::decode_connectivity_att(reader, header)
    //     .map_err(|r| Err::ConnectivityError(r))?;

    debug_expect!("Connectivity done, now starting attributes.", reader);

    // Decode attributes
    // let attributes = attribute::decode_attributes(reader, cfg.attribute_decoder_cfg, connectivity)
    //     .map_err(|r| Err::AttributeError(r))?;

    debug_expect!("All done", reader);

    // // Create mesh
    let mut mesh = Mesh::new();
    // for att in attributes {
    //     mesh.add_attribute(att);
    // }

    Ok(mesh)
}


#[derive(Debug, Clone)]
pub struct Config {
    // attribute_decoder_cfg: attribute::Config,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            // attribute_decoder_cfg: attribute::Config::default(),
        }
    }
}


#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    // #[error("Attribute encoding error")]
    // AttributeError(#[from] attribute::Err),
    // #[error("Connectivity encoding error")]
    // ConnectivityError(#[from] connectivity::Err),
    #[error("Header encoding error")]
    HeaderError(#[from] header::Err),
    #[error("Metadata encoding error")]
    MetadataError(#[from] metadata::Err),
}

