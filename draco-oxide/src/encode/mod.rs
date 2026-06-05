pub(crate) mod attribute;
pub(crate) mod connectivity;
pub(crate) mod entropy;
pub(crate) mod header;
pub(crate) mod metadata;

use std::collections::HashMap;

use crate::core::bit_coder::ByteWriter;
use crate::core::mesh::Mesh;
use crate::core::shared::ConfigType;
use crate::encode::attribute::portabilization::ExplicitQuantization;
use crate::prelude::AttributeType;
use crate::{debug_write, shared};
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
    encoder_method: shared::header::EncoderMethod,
    metdata: bool,
    /// Per-attribute caller-supplied explicit quantization. Populated by
    /// [`Config::set_attribute_explicit_quantization`]. See the method docs
    /// for semantics.
    pub(crate) explicit_quantization: HashMap<AttributeType, ExplicitQuantization>,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            connectivity_encoder_cfg: connectivity::Config::default(),
            attribute_encoder_cfg: attribute::Config::default(),
            geometry_type: header::EncodedGeometryType::TrianglarMesh,
            encoder_method: shared::header::EncoderMethod::Edgebreaker,
            metdata: false,
            explicit_quantization: HashMap::new(),
        }
    }
}

impl Config {
    /// Sets caller-supplied explicit quantization parameters for the given
    /// attribute type. Mirrors upstream Draco C++'s
    /// `Encoder::SetAttributeExplicitQuantization`
    /// (see `src/draco/compression/encode.cc:71-79` in `google/draco`).
    ///
    /// When set, the encoder skips its per-mesh min/max scan for attributes
    /// of this type and uses the supplied `(origin, range, quantization_bits)`
    /// directly. Two encodes of the same input vertex under the same
    /// parameters produce bit-identical bitstream bytes for that vertex —
    /// the property tiled-output emitters need for cross-tile vertex
    /// determinism.
    ///
    /// `range` is a single scalar applied to all components (cube anchored
    /// at `origin` with edge length `range`, not an axis-aligned box).
    /// Input attribute values must lie in `[origin[i], origin[i] + range]`
    /// for every component i. `num_dims` must equal `origin.len()` and the
    /// number of components on the matching attribute at encode time.
    ///
    /// # Panics
    ///
    /// Panics if `origin.len() != num_dims`.
    pub fn set_attribute_explicit_quantization(
        &mut self,
        att_type: AttributeType,
        quantization_bits: i32,
        num_dims: i32,
        origin: &[f32],
        range: f32,
    ) -> &mut Self {
        assert_eq!(
            origin.len() as i32,
            num_dims,
            "origin.len() ({}) must equal num_dims ({})",
            origin.len(),
            num_dims,
        );
        self.explicit_quantization.insert(
            att_type,
            ExplicitQuantization {
                origin: origin.to_vec(),
                range,
                quantization_bits: quantization_bits as u8,
            },
        );
        self
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

    // Encode connectivity
    let conn_out = connectivity::encode_connectivity(&faces, &mut attributes, writer, &cfg)?;
    debug_write!("Connectivity done, now starting attributes.", writer);

    // Encode attributes
    attribute::encode_attributes(attributes, writer, conn_out, &cfg)?;

    debug_write!("All done", writer);

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    Ok(())
}
