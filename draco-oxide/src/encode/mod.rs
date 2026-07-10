pub(crate) mod attribute;
pub(crate) mod connectivity;
pub(crate) mod ds;
pub(crate) mod entropy;
pub(crate) mod header;
pub(crate) mod metadata;

use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::debug_write;
use draco_oxide_core::mesh::Mesh;
use draco_oxide_core::types::ConfigType;
use thiserror::Error;

#[cfg(feature = "evaluation")]
use crate::eval;

pub trait EncoderConfig {
    type Encoder;
    fn get_encoder(&self) -> Self::Encoder;
}

#[derive(Debug, Clone)]
pub struct Config {
    #[allow(unused)]
    // This field is unused in the current implementation, as we only support edgebreaker.
    connectivity_encoder_cfg: connectivity::Config,
    #[allow(unused)]
    // This field is unused in the current implementation, as we only suport the default attribute encoder configuration.
    attribute_encoder_cfg: attribute::Config,
    geometry_type: header::EncodedGeometryType,
    encoder_method: draco_oxide_core::codec::header::EncoderMethod,
    metdata: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            connectivity_encoder_cfg: connectivity::Config::default(),
            attribute_encoder_cfg: attribute::Config::default(),
            geometry_type: header::EncodedGeometryType::TrianglarMesh,
            encoder_method: draco_oxide_core::codec::header::EncoderMethod::Edgebreaker,
            metdata: false,
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

/// Encodes the input mesh into a provided byte stream using the provided configuration.
pub fn encode<W>(mesh: Mesh, writer: &mut W, cfg: Config) -> Result<(), Err>
where
    W: ByteWriter,
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("compression info", writer);

    // Encode header
    header::encode_header(writer, &cfg)?;

    debug_write!("Header done, now starting metadata.", writer);

    // Encode metadata
    if cfg.metdata {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("metadata", writer);
        metadata::encode_metadata(&mesh, writer)?;
        #[cfg(feature = "evaluation")]
        eval::scope_end(writer);
    }

    debug_write!("Metadata done, now starting connectivity.", writer);

    // Destruct the mesh so that attributes and faces have the different lifetime.
    let Mesh {
        mut attributes,
        faces,
        ..
    } = mesh;

    let pos_att = attributes
        .iter()
        .find(|att| {
            att.get_attribute_type() == draco_oxide_core::attribute::AttributeType::Position
        })
        .ok_or(Err::ConnectivityError(
            connectivity::Err::PositionAttributeTypeError,
        ))?;

    let (pos_ds, pos_corner_table) = ds::build_global_ds(faces, pos_att);
    let mut adss = ds::build_attribute_ds(&pos_ds, &pos_corner_table, attributes);

    // Encode connectivity. This returns the corners of the edgebreaker traversal, which the
    // attribute encoder needs to seed its per-attribute sequencing.
    let corners_of_edgebreaker = connectivity::encode_connectivity(&mut adss, writer, &cfg)?;
    debug_write!("Connectivity done, now starting attributes.", writer);

    // Encode attributes
    attribute::encode_attributes(adss, corners_of_edgebreaker, writer, &cfg)?;

    debug_write!("All done", writer);

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    Ok(())
}
