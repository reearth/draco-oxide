//! TOML/serde-facing view of [`Config`](super::Config).
//!
//! Deserializing a `Config` routes through [`ConfigSpec`], a flat, all-optional
//! spec that goes through the public builder methods, keeping the real config's
//! private fields and internal representation off the deserialization surface.
//! Core codec enums are mirrored by small local `*Name` enums here so that
//! `draco-oxide-core` stays free of a serde dependency. Grow this alongside the
//! builder methods.
//!
//! ```toml
//! metadata = false
//! connectivity = "Edgebreaker"
//!
//! [edgebreaker]
//! traversal = "Valence"
//!
//! [attributes.Position]
//! prediction   = "MeshParallelogramPrediction"
//! quantization = { bits = 14 }        # or { max_error = 0.001 }
//!
//! [attributes.Normal]
//! encoding     = "PredictedOnly"
//! quantization = { bits = 8 }
//! ```

use std::collections::HashMap;

use serde::Deserialize;

use draco_oxide_core::attribute::AttributeType;
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::connectivity::edgebreaker::EdgebreakerKind;
use draco_oxide_core::types::ConfigType;

use super::attribute::{AttributeConfig, NormalEncoding, PredictionTransformType, Quantization};
use super::{Config, EdgebreakerConfig, SequentialConfig};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ConfigSpec {
    /// Back-compat shortcut equivalent to `[attributes.Normal] encoding = …`.
    normal: NormalEncoding,
    metadata: bool,
    connectivity: ConnectivityName,
    edgebreaker: EdgebreakerSpec,
    attributes: HashMap<AttributeName, AttributeConfigSpec>,
}

impl From<ConfigSpec> for Config {
    fn from(spec: ConfigSpec) -> Self {
        let mut cfg = <Config as ConfigType>::default()
            .with_metadata(spec.metadata)
            // Applied first so an explicit `[attributes.Normal]` below wins.
            .with_normals(spec.normal);

        cfg = match spec.connectivity {
            ConnectivityName::Edgebreaker => cfg.with_edgebreaker(EdgebreakerConfig {
                traversal: spec.edgebreaker.traversal.into(),
                use_single_connectivity: spec.edgebreaker.use_single_connectivity,
            }),
            ConnectivityName::Sequential => cfg.with_sequential(SequentialConfig::default()),
        };

        for (name, aspec) in spec.attributes {
            cfg = cfg.with_attribute(name.into(), aspec.into());
        }

        cfg
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
enum ConnectivityName {
    #[default]
    Edgebreaker,
    Sequential,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EdgebreakerSpec {
    traversal: TraversalName,
    use_single_connectivity: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
enum TraversalName {
    Standard,
    Predictive,
    #[default]
    Valence,
}

impl From<TraversalName> for EdgebreakerKind {
    fn from(t: TraversalName) -> Self {
        match t {
            TraversalName::Standard => EdgebreakerKind::Standard,
            TraversalName::Predictive => EdgebreakerKind::Predictive,
            TraversalName::Valence => EdgebreakerKind::Valence,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
enum AttributeName {
    Position,
    Normal,
    Color,
    TextureCoordinate,
    Custom,
    Tangent,
    Material,
    Joint,
    Weight,
}

impl From<AttributeName> for AttributeType {
    fn from(n: AttributeName) -> Self {
        match n {
            AttributeName::Position => AttributeType::Position,
            AttributeName::Normal => AttributeType::Normal,
            AttributeName::Color => AttributeType::Color,
            AttributeName::TextureCoordinate => AttributeType::TextureCoordinate,
            AttributeName::Custom => AttributeType::Custom,
            AttributeName::Tangent => AttributeType::Tangent,
            AttributeName::Material => AttributeType::Material,
            AttributeName::Joint => AttributeType::Joint,
            AttributeName::Weight => AttributeType::Weight,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AttributeConfigSpec {
    prediction: Option<SchemeName>,
    transform: Option<TransformName>,
    quantization: Option<QuantizationSpec>,
    encoding: Option<NormalEncoding>,
}

impl From<AttributeConfigSpec> for AttributeConfig {
    fn from(s: AttributeConfigSpec) -> Self {
        AttributeConfig {
            prediction: s.prediction.map(Into::into),
            transform: s.transform.map(Into::into),
            quantization: s.quantization.map(Into::into),
            normal_encoding: s.encoding,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum SchemeName {
    DeltaPrediction,
    DerivativePrediction,
    MeshMultiParallelogramPrediction,
    MeshParallelogramPrediction,
    MeshNormalPrediction,
    MeshPredictionForTextureCoordinates,
    NoPrediction,
}

impl From<SchemeName> for PredictionSchemeType {
    fn from(s: SchemeName) -> Self {
        match s {
            SchemeName::DeltaPrediction => PredictionSchemeType::DeltaPrediction,
            SchemeName::DerivativePrediction => PredictionSchemeType::DerivativePrediction,
            SchemeName::MeshMultiParallelogramPrediction => {
                PredictionSchemeType::MeshMultiParallelogramPrediction
            }
            SchemeName::MeshParallelogramPrediction => {
                PredictionSchemeType::MeshParallelogramPrediction
            }
            SchemeName::MeshNormalPrediction => PredictionSchemeType::MeshNormalPrediction,
            SchemeName::MeshPredictionForTextureCoordinates => {
                PredictionSchemeType::MeshPredictionForTextureCoordinates
            }
            SchemeName::NoPrediction => PredictionSchemeType::NoPrediction,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum TransformName {
    NoTransform,
    Difference,
    WrappedDifference,
    OctahedralOrthogonal,
    OctahedralReflection,
    Orthogonal,
}

impl From<TransformName> for PredictionTransformType {
    fn from(t: TransformName) -> Self {
        match t {
            TransformName::NoTransform => PredictionTransformType::NoTransform,
            TransformName::Difference => PredictionTransformType::Difference,
            TransformName::WrappedDifference => PredictionTransformType::WrappedDifference,
            TransformName::OctahedralOrthogonal => PredictionTransformType::OctahedralOrthogonal,
            TransformName::OctahedralReflection => PredictionTransformType::OctahedralReflection,
            TransformName::Orthogonal => PredictionTransformType::Orthogonal,
        }
    }
}

/// TOML form of [`Quantization`]: `{ bits = N }` or `{ max_error = E }`. `bits`
/// takes precedence if both are present; an empty table falls back to the
/// attribute's default resolution.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct QuantizationSpec {
    bits: Option<u8>,
    max_error: Option<f32>,
}

impl From<QuantizationSpec> for Quantization {
    fn from(q: QuantizationSpec) -> Self {
        match (q.bits, q.max_error) {
            (Some(bits), _) => Quantization::Bits(bits),
            (None, Some(max_error)) => Quantization::MaxError(max_error),
            (None, None) => Quantization::default(),
        }
    }
}
