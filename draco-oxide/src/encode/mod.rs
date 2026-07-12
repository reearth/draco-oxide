pub(crate) mod attribute;
pub(crate) mod config_spec;
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

pub use attribute::{AttributeConfig, NormalEncoding, Quantization};
pub use connectivity::edgebreaker::Config as EdgebreakerConfig;
pub use connectivity::sequential::Config as SequentialConfig;
pub use connectivity::Config as ConnectivityConfig;

use config_spec::ConfigSpec;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(from = "ConfigSpec")]
pub struct Config {
    // Connectivity compression method and its config (edgebreaker or sequential).
    // Also the single source of truth for the header's encoder method.
    connectivity: connectivity::Config,
    // Per-attribute encoding configuration (see `attribute::Config`).
    attribute: attribute::Config,
    geometry_type: header::EncodedGeometryType,
    metadata: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            connectivity: connectivity::Config::default(),
            attribute: attribute::Config::default(),
            geometry_type: header::EncodedGeometryType::TrianglarMesh,
            metadata: false,
        }
    }
}

impl Config {
    /// Sets how normal attributes are compressed.
    ///
    /// [`NormalEncoding::PredictedOnly`] makes normal compression effectively
    /// zero-CPU: the encoder ignores the input normal values (using only their
    /// seam topology) and emits an all-zero correction stream, so the decoder
    /// reconstructs exactly the normals it predicts from the geometry.
    ///
    /// ```no_run
    /// # use draco_oxide::core::types::ConfigType;
    /// use draco_oxide::encode::{Config, NormalEncoding};
    /// let cfg = Config::default().with_normals(NormalEncoding::PredictedOnly);
    /// ```
    pub fn with_normals(mut self, enc: NormalEncoding) -> Self {
        self.attribute.set_normal_encoding(enc);
        self
    }

    /// Overrides the per-type encoding for `ty` (prediction scheme, transform,
    /// quantization, and — for normals — the normal encoding mode). Absent knobs
    /// fall back to the built-in default for that attribute type.
    pub fn with_attribute(
        mut self,
        ty: draco_oxide_core::attribute::AttributeType,
        cfg: AttributeConfig,
    ) -> Self {
        self.attribute.set(ty, cfg);
        self
    }

    /// Selects the connectivity compression method and its configuration.
    pub fn with_connectivity(mut self, cfg: ConnectivityConfig) -> Self {
        self.connectivity = cfg;
        self
    }

    /// Selects edgebreaker connectivity compression with the given config.
    pub fn with_edgebreaker(mut self, cfg: EdgebreakerConfig) -> Self {
        self.connectivity = ConnectivityConfig::Edgebreaker(cfg);
        self
    }

    /// Selects sequential connectivity compression with the given config.
    pub fn with_sequential(mut self, cfg: SequentialConfig) -> Self {
        self.connectivity = ConnectivityConfig::Sequential(cfg);
        self
    }

    /// Enables or disables metadata encoding.
    pub fn with_metadata(mut self, metadata: bool) -> Self {
        self.metadata = metadata;
        self
    }

    /// The current per-type attribute override for `ty` (empty default if none),
    /// for read-modify-write layering of overrides (e.g. a CLI flag patching a
    /// single knob on top of a file-loaded config).
    pub fn attribute_config(
        &self,
        ty: draco_oxide_core::attribute::AttributeType,
    ) -> AttributeConfig {
        self.attribute.get(ty)
    }

    /// The selected connectivity configuration.
    pub fn connectivity(&self) -> &ConnectivityConfig {
        &self.connectivity
    }

    /// Validates the configuration for internal consistency, rejecting
    /// combinations that would produce an undecodable or nonsensical stream (a
    /// texture predictor on a normal attribute, a coordinate max-error on an
    /// octahedral normal, an unimplemented traversal, out-of-range bits, …).
    /// Called automatically at the top of [`encode`].
    pub fn validate(&self) -> Result<(), ConfigError> {
        use draco_oxide_core::attribute::AttributeType;
        use draco_oxide_core::codec::connectivity::edgebreaker::EdgebreakerKind;

        // Connectivity: reject the unimplemented predictive edgebreaker traversal.
        if let ConnectivityConfig::Edgebreaker(eb) = &self.connectivity {
            if eb.traversal == EdgebreakerKind::Predictive {
                return Err(ConfigError::UnsupportedTraversal);
            }
        }

        for (&ty, over) in self.attribute.overrides() {
            if over.normal_encoding.is_some() && ty != AttributeType::Normal {
                return Err(ConfigError::NormalEncodingOnNonNormal(ty));
            }

            if let Some(scheme) = &over.prediction {
                if !allowed_schemes(ty).iter().any(|s| s == scheme) {
                    return Err(ConfigError::PredictionSchemeForType {
                        ty,
                        scheme: scheme.to_string(),
                    });
                }
            }

            if let Some(transform) = over.transform {
                if !allowed_transforms(ty).contains(&transform) {
                    return Err(ConfigError::TransformForType {
                        ty,
                        transform: format!("{transform:?}"),
                    });
                }
            }

            if let Some(quant) = over.quantization {
                // Octahedral (normal) resolution is angular, not coordinate; only
                // an explicit bit count is meaningful there.
                if ty == AttributeType::Normal && !matches!(quant, Quantization::Bits(_)) {
                    return Err(ConfigError::NonBitsQuantizationForNormal);
                }
                if let Quantization::Bits(n) = quant {
                    if !(1..=30).contains(&n) {
                        return Err(ConfigError::QuantizationBitsOutOfRange(n));
                    }
                }
            }
        }

        Ok(())
    }
}

/// The prediction schemes valid for a given attribute type.
fn allowed_schemes(
    ty: draco_oxide_core::attribute::AttributeType,
) -> Vec<draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType> {
    use draco_oxide_core::attribute::AttributeType::*;
    use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType as S;
    match ty {
        Position => vec![
            S::MeshParallelogramPrediction,
            S::MeshMultiParallelogramPrediction,
            S::DeltaPrediction,
            S::NoPrediction,
        ],
        Normal => vec![S::MeshNormalPrediction],
        TextureCoordinate => vec![
            S::MeshPredictionForTextureCoordinates,
            S::DerivativePrediction,
            S::DeltaPrediction,
            S::NoPrediction,
        ],
        // Color, Custom, and any other generic per-vertex attribute have no
        // mesh-geometry predictor.
        _ => vec![S::DeltaPrediction, S::NoPrediction],
    }
}

/// The prediction transforms valid for a given attribute type.
fn allowed_transforms(
    ty: draco_oxide_core::attribute::AttributeType,
) -> Vec<attribute::PredictionTransformType> {
    use attribute::PredictionTransformType as T;
    use draco_oxide_core::attribute::AttributeType::*;
    match ty {
        // Normals ride the octahedral transforms.
        Normal => vec![T::OctahedralOrthogonal, T::OctahedralReflection],
        _ => vec![T::Difference, T::WrappedDifference, T::NoTransform],
    }
}

/// Errors from [`Config::validate`].
#[remain::sorted]
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("normals accept only an explicit bit count (octahedral error is angular)")]
    NonBitsQuantizationForNormal,
    #[error("normal encoding was set on a non-normal attribute ({0:?})")]
    NormalEncodingOnNonNormal(draco_oxide_core::attribute::AttributeType),
    #[error("prediction scheme {scheme} is not valid for attribute type {ty:?}")]
    PredictionSchemeForType {
        ty: draco_oxide_core::attribute::AttributeType,
        scheme: String,
    },
    #[error("quantization bits {0} out of range (must be 1..=30)")]
    QuantizationBitsOutOfRange(u8),
    #[error("prediction transform {transform} is not valid for attribute type {ty:?}")]
    TransformForType {
        ty: draco_oxide_core::attribute::AttributeType,
        transform: String,
    },
    #[error("the selected edgebreaker traversal is not implemented")]
    UnsupportedTraversal,
}

#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute::Err),
    #[error("Invalid encoder configuration: {0}")]
    ConfigError(#[from] ConfigError),
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
    // Reject inconsistent configs before writing anything.
    cfg.validate()?;

    #[cfg(feature = "evaluation")]
    eval::scope_begin("compression info", writer);

    // Encode header
    header::encode_header(writer, &cfg)?;

    debug_write!("Header done, now starting metadata.", writer);

    // Encode metadata
    if cfg.metadata {
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

    if !attributes
        .iter()
        .any(|att| att.get_attribute_type() == draco_oxide_core::attribute::AttributeType::Position)
    {
        return Err(Err::ConnectivityError(
            connectivity::Err::PositionAttributeTypeError,
        ));
    }

    let (pos_ds, pos_corner_table) = ds::build_global_ds(faces, &mut attributes);
    let mut adss = ds::build_attribute_ds(&pos_ds, &pos_corner_table, attributes);

    // Encode connectivity
    let corners_of_edgebreaker = connectivity::encode_connectivity(&mut adss, writer, &cfg)?;
    debug_write!("Connectivity done, now starting attributes.", writer);

    // Encode attributes
    attribute::encode_attributes(adss, corners_of_edgebreaker, writer, &cfg)?;

    debug_write!("All done", writer);

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    Ok(())
}

#[cfg(test)]
mod config_tests {
    use super::*;
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
    use draco_oxide_core::codec::connectivity::edgebreaker::EdgebreakerKind;

    #[test]
    fn default_config_is_valid() {
        assert!(<Config as ConfigType>::default().validate().is_ok());
    }

    #[test]
    fn position_quantization_override_validates() {
        let cfg = Config::default().with_attribute(
            AttributeType::Position,
            AttributeConfig {
                quantization: Some(Quantization::Bits(14)),
                ..Default::default()
            },
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn texture_predictor_on_normal_is_rejected() {
        let cfg = Config::default().with_attribute(
            AttributeType::Normal,
            AttributeConfig {
                prediction: Some(PredictionSchemeType::MeshPredictionForTextureCoordinates),
                ..Default::default()
            },
        );
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::PredictionSchemeForType { .. })
        ));
    }

    #[test]
    fn max_error_quantization_on_normal_is_rejected() {
        let cfg = Config::default().with_attribute(
            AttributeType::Normal,
            AttributeConfig {
                quantization: Some(Quantization::MaxError(0.01)),
                ..Default::default()
            },
        );
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::NonBitsQuantizationForNormal)
        ));
    }

    #[test]
    fn normal_encoding_on_position_is_rejected() {
        let cfg = Config::default().with_attribute(
            AttributeType::Position,
            AttributeConfig {
                normal_encoding: Some(NormalEncoding::PredictedOnly),
                ..Default::default()
            },
        );
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::NormalEncodingOnNonNormal(
                AttributeType::Position
            ))
        ));
    }

    #[test]
    fn out_of_range_bits_is_rejected() {
        let cfg = Config::default().with_attribute(
            AttributeType::Position,
            AttributeConfig {
                quantization: Some(Quantization::Bits(40)),
                ..Default::default()
            },
        );
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::QuantizationBitsOutOfRange(40))
        ));
    }

    #[test]
    fn predictive_edgebreaker_is_rejected() {
        let cfg = Config::default().with_edgebreaker(EdgebreakerConfig {
            traversal: EdgebreakerKind::Predictive,
            use_single_connectivity: false,
        });
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::UnsupportedTraversal)
        ));
    }

    #[test]
    fn sequential_selects_sequential_encoder_method() {
        use draco_oxide_core::codec::header::EncoderMethod;
        let cfg = Config::default().with_sequential(SequentialConfig::default());
        assert_eq!(cfg.connectivity.encoder_method(), EncoderMethod::Sequential);
    }
}
