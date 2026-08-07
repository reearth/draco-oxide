pub(crate) mod attribute;
pub(crate) mod config_spec;
pub(crate) mod connectivity;
pub(crate) mod ds;
pub(crate) mod entropy;
pub(crate) mod header;
pub(crate) mod metadata;
/// Point-cloud encoding: the kd-tree method and its configuration.
pub mod point_cloud;

use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::debug_write;
use draco_oxide_core::mesh::Mesh;
use draco_oxide_core::point_cloud::PointCloud;
use draco_oxide_core::types::ConfigType;
use thiserror::Error;

/// Per-attribute encoding configuration and its option types.
pub use attribute::{AttributeConfig, NormalEncoding, Quantization};
/// Configuration for edgebreaker connectivity encoding.
pub use connectivity::edgebreaker::Config as EdgebreakerConfig;
/// Configuration for sequential connectivity encoding.
pub use connectivity::sequential::Config as SequentialConfig;
/// Selection of the connectivity encoding method and its configuration.
pub use connectivity::Config as ConnectivityConfig;
/// Configuration for point-cloud encoding.
pub use point_cloud::Config as PointCloudConfig;

use config_spec::ConfigSpec;

/// The encoder configuration: connectivity method, per-attribute encoding
/// options, geometry type, and metadata toggle. Built with the `with_*`
/// builder methods, or deserialized from TOML.
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
    /// quantization, and the normal encoding mode for normals). Absent knobs
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
    /// Called automatically by every encode entry point.
    pub fn validate(&self) -> Result<(), ConfigError> {
        use draco_oxide_core::attribute::AttributeType;
        use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
        use draco_oxide_core::codec::connectivity::edgebreaker::EdgebreakerKind;

        // Connectivity: reject the unimplemented predictive edgebreaker traversal.
        if let ConnectivityConfig::Edgebreaker(eb) = &self.connectivity {
            if eb.traversal == EdgebreakerKind::Predictive {
                return Err(ConfigError::UnsupportedTraversal);
            }
        }
        let sequential = matches!(self.connectivity, ConnectivityConfig::Sequential(_));

        for (&ty, over) in self.attribute.overrides() {
            // A sequential stream carries no connectivity, so nothing can
            // predict from the mesh: the built-in mesh defaults degrade to
            // delta, but an explicit request for one cannot be honored, and
            // trusting a geometry-derived normal prediction is meaningless.
            if sequential {
                if let Some(scheme) = &over.prediction {
                    if !matches!(
                        scheme,
                        PredictionSchemeType::DeltaPrediction | PredictionSchemeType::NoPrediction
                    ) {
                        return Err(ConfigError::MeshPredictionUnderSequential(format!(
                            "{scheme:?}"
                        )));
                    }
                }
                if over.normal_encoding == Some(NormalEncoding::PredictedOnly) {
                    return Err(ConfigError::PredictedNormalsUnderSequential);
                }
                if over.traversal
                    == Some(draco_oxide_core::codec::connectivity::edgebreaker::TraversalType::PredictionDegree)
                {
                    return Err(ConfigError::PredictionDegreeUnderSequential);
                }
            }

            if over.normal_encoding.is_some() && ty != AttributeType::Normal {
                return Err(ConfigError::NormalEncodingOnNonNormal(ty));
            }

            if let Some(scheme) = &over.prediction {
                if !allowed_schemes(ty).iter().any(|s| s == scheme) {
                    return Err(ConfigError::PredictionSchemeForType {
                        ty,
                        scheme: format!("{scheme:?}"),
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
                // The wire frames NoPrediction without any transform, so a
                // transform override cannot be honored alongside it.
                if over.prediction == Some(PredictionSchemeType::NoPrediction)
                    && transform != attribute::PredictionTransformType::NoTransform
                {
                    return Err(ConfigError::TransformWithNoPrediction);
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
            S::MeshConstrainedMultiParallelogramPrediction,
            S::DeltaPrediction,
            S::NoPrediction,
        ],
        Normal => vec![S::MeshNormalPrediction],
        TextureCoordinate => vec![
            S::MeshParallelogramPrediction,
            S::MeshConstrainedMultiParallelogramPrediction,
            S::MeshPredictionForTextureCoordinates,
            S::DeltaPrediction,
            S::NoPrediction,
        ],
        // Color, Custom, and any other generic per-vertex attribute have no
        // geometry-derived predictor, but the parallelogram family predicts
        // any value carried over the mesh connectivity.
        _ => vec![
            S::MeshConstrainedMultiParallelogramPrediction,
            S::DeltaPrediction,
            S::NoPrediction,
        ],
    }
}

/// The prediction transforms valid for a given attribute type.
fn allowed_transforms(
    ty: draco_oxide_core::attribute::AttributeType,
) -> Vec<attribute::PredictionTransformType> {
    use attribute::PredictionTransformType as T;
    use draco_oxide_core::attribute::AttributeType::*;
    match ty {
        // Normals ride the octahedral transform.
        Normal => vec![T::OctahedralOrthogonal],
        _ => vec![T::Difference, T::WrappedDifference, T::NoTransform],
    }
}

/// Errors from [`Config::validate`].
#[remain::sorted]
#[derive(Error, Debug)]
pub enum ConfigError {
    /// A mesh-based prediction scheme was requested under sequential
    /// connectivity encoding, which carries no connectivity to predict from.
    #[error("prediction scheme {0} needs mesh connectivity, which sequential encoding omits")]
    MeshPredictionUnderSequential(String),
    /// A quantization mode other than an explicit bit count was set on a
    /// normal attribute.
    #[error("normals accept only an explicit bit count (octahedral error is angular)")]
    NonBitsQuantizationForNormal,
    /// A normal encoding mode was set on an attribute that is not a normal.
    #[error("normal encoding was set on a non-normal attribute ({0:?})")]
    NormalEncodingOnNonNormal(draco_oxide_core::attribute::AttributeType),
    /// Geometry-predicted normals were requested under sequential connectivity
    /// encoding, which carries no connectivity to predict from.
    #[error("geometry-predicted normals need mesh connectivity, which sequential encoding omits")]
    PredictedNormalsUnderSequential,
    /// Prediction-degree traversal was requested under sequential connectivity
    /// encoding, which carries no connectivity to traverse.
    #[error(
        "prediction-degree traversal needs mesh connectivity, which sequential encoding omits"
    )]
    PredictionDegreeUnderSequential,
    /// The requested prediction scheme is not valid for the attribute type.
    #[error("prediction scheme {scheme} is not valid for attribute type {ty:?}")]
    PredictionSchemeForType {
        ty: draco_oxide_core::attribute::AttributeType,
        scheme: String,
    },
    /// The requested quantization bit count is outside the supported range.
    #[error("quantization bits {0} out of range (must be 1..=30)")]
    QuantizationBitsOutOfRange(u8),
    /// The requested prediction transform is not valid for the attribute type.
    #[error("prediction transform {transform} is not valid for attribute type {ty:?}")]
    TransformForType {
        ty: draco_oxide_core::attribute::AttributeType,
        transform: String,
    },
    /// A prediction transform other than NoTransform was combined with
    /// NoPrediction, which carries no transform on the wire.
    #[error("NoPrediction carries no transform on the wire; only NoTransform can accompany it")]
    TransformWithNoPrediction,
    /// The selected edgebreaker traversal is not implemented.
    #[error("the selected edgebreaker traversal is not implemented")]
    UnsupportedTraversal,
}

/// Errors returned by the encode entry points.
#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    /// Attribute encoding failed.
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute::Err),
    /// The configuration failed validation.
    #[error("Invalid encoder configuration: {0}")]
    ConfigError(#[from] ConfigError),
    /// Connectivity encoding failed.
    #[error("Connectivity encoding error: {0}")]
    ConnectivityError(#[from] connectivity::Err),
    /// Header encoding failed.
    #[error("Header encoding error: {0}")]
    HeaderError(#[from] header::Err),
    /// Metadata encoding failed.
    #[error("Metadata encoding error: {0}")]
    MetadataError(#[from] metadata::Err),
    /// Point-cloud encoding failed.
    #[error("Point cloud encoding error: {0}")]
    PointCloudError(#[from] point_cloud::Err),
    /// The input mesh has no faces. Encode it with
    /// [`Encoder::encode_point_cloud`] instead.
    #[error("the mesh has no faces; encode it as a point cloud instead")]
    PointCloudInput,
}

/// The mesh encoder. A single instance is meant to be reused across encodes
/// so it can share resources between runs.
#[derive(Default)]
pub struct Encoder {}

impl Encoder {
    /// Creates a new encoder.
    pub fn new() -> Self {
        Self {}
    }

    /// Encodes the input mesh into a provided byte stream using the provided configuration.
    pub fn encode_mesh<W>(&mut self, mesh: Mesh, writer: &mut W, cfg: Config) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        encode_impl(mesh, writer, cfg)
    }

    /// Encodes the input point cloud into a provided byte stream using the
    /// provided configuration.
    pub fn encode_point_cloud<W>(
        &mut self,
        pc: PointCloud,
        writer: &mut W,
        cfg: PointCloudConfig,
    ) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        point_cloud::encode_impl(pc, writer, cfg)?;
        Ok(())
    }
}

/// Encodes the input mesh into a provided byte stream using the provided
/// configuration, with a freshly constructed [`Encoder`].
pub fn encode_mesh<W>(mesh: Mesh, writer: &mut W, cfg: Config) -> Result<(), Err>
where
    W: ByteWriter,
{
    Encoder::new().encode_mesh(mesh, writer, cfg)
}

/// Encodes the input point cloud into a provided byte stream using the provided
/// configuration, with a freshly constructed [`Encoder`].
pub fn encode_point_cloud<W>(
    pc: PointCloud,
    writer: &mut W,
    cfg: PointCloudConfig,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    Encoder::new().encode_point_cloud(pc, writer, cfg)
}

fn encode_impl<W>(mesh: Mesh, writer: &mut W, cfg: Config) -> Result<(), Err>
where
    W: ByteWriter,
{
    // Reject inconsistent configs before writing anything.
    cfg.validate()?;

    // A faceless input has no connectivity to encode; it belongs on the
    // point-cloud entry point.
    if mesh.faces.is_empty() {
        return Err(Err::PointCloudInput);
    }

    // Encode header
    header::encode_header(writer, &cfg)?;

    debug_write!("Header done, now starting metadata.", writer);

    // Encode metadata
    if cfg.metadata {
        metadata::encode_metadata(&mesh, writer)?;
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

    let (ds, pos_corner_table) = ds::build_global_ds(faces, &mut attributes);
    let mut adss = ds::build_attribute_ds(&ds, &pos_corner_table, attributes);

    // Encode connectivity
    let corners_of_edgebreaker = connectivity::encode_connectivity(&mut adss, writer, &cfg)?;
    debug_write!("Connectivity done, now starting attributes.", writer);

    // Encode attributes
    attribute::encode_attributes(adss, corners_of_edgebreaker, writer, &cfg)?;

    debug_write!("All done", writer);

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
    fn faceless_mesh_is_rejected_as_point_cloud() {
        use draco_oxide_core::attribute::{Attribute, AttributeDomain, AttributeType};
        use draco_oxide_core::types::NdVector;
        let mut mesh = Mesh::new();
        mesh.attributes = vec![Attribute::new::<NdVector<3, f32>, 3>(
            vec![[0.0, 0.0, 0.0].into(), [1.0, 0.0, 0.0].into()],
            AttributeType::Position,
            AttributeDomain::Position,
            Vec::new(),
        )];
        let mut out = Vec::new();
        assert!(matches!(
            encode_mesh(mesh, &mut out, <Config as ConfigType>::default()),
            Err(Err::PointCloudInput)
        ));
        assert!(out.is_empty(), "nothing must be written before the check");
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
