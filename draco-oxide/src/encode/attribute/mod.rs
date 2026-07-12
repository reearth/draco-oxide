pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

use crate::encode::attribute::portabilization::PortabilizationType;
pub use crate::encode::attribute::portabilization::Quantization;
pub use crate::encode::attribute::prediction_transform::PredictionTransformType;
#[cfg(feature = "evaluation")]
use crate::eval;

use std::collections::HashMap;

use draco_oxide_core::attribute::{Attribute, AttributeType};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::connectivity::edgebreaker::TraversalType;
use draco_oxide_core::mesh::ds::AttributeDS;
use draco_oxide_core::types::{ConfigType, CornerIdx};

pub fn encode_attributes<W>(
    adss: Vec<AttributeDS>,
    // Corners of the edgebreaker traversal, produced by connectivity encoding and used to seed
    // each attribute's sequencing.
    corners_of_edgebreaker: Vec<CornerIdx>,
    writer: &mut W,
    cfg: &super::Config,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("attributes", writer);

    // Write the number of attribute encoders/decoders (In draco-oxide, this is the same as the number of attributes as
    // each attribute has its own encoder/decoder)
    writer.write_u8(adss.len() as u8);
    #[cfg(feature = "evaluation")]
    eval::write_json_pair("attributes count", adss.len().into(), writer);

    for (i, att) in adss.iter().enumerate() {
        if cfg.connectivity.encoder_method()
            == draco_oxide_core::codec::header::EncoderMethod::Edgebreaker
        {
            // encode decoder id
            writer.write_u8((i as u8).wrapping_sub(1));
            // encode attribute type
            att.att_data().get_domain().write_to(writer);
            // write traversal method for attribute encoding/decoding sequencer. We currently only support depth-first traversal.
            TraversalType::DepthFirst.write_to(writer);
        }
    }

    #[cfg(feature = "evaluation")]
    eval::array_scope_begin("attributes", writer);

    let mut port_atts: Vec<Attribute> = Vec::new();
    for att in &adss {
        // Write 1 to indicate that the encoder is for one attribute.
        writer.write_u8(1);

        att.att_data().get_attribute_type().write_to(writer);
        att.att_data().get_component_type().write_to(writer);
        writer.write_u8(att.att_data().get_num_components() as u8);
        writer.write_u8(0); // Normalized flag, currently not used.
        writer.write_u8(att.att_data().get_id().as_usize() as u8); // unique id

        // write the decoder type.
        PortabilizationType::default_for(att.att_data().get_attribute_type()).write_to(writer);
    }

    // `adss` is built one-per-attribute and in the same order as `atts`, so each attribute is
    // paired with its own attribute data structure here.
    for ads in adss {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("attribute", writer);

        let parents_ids = ads.att_data().get_parents();
        let parents = parents_ids
            .iter()
            .map(|id| port_atts.iter().find(|att| att.get_id() == *id).unwrap())
            .collect::<Vec<_>>();

        let ty = ads.att_data().get_attribute_type();
        let len = ads.att_data().len();
        let encoder = attribute_encoder::AttributeEncoder::new(
            ads,
            &parents,
            &corners_of_edgebreaker,
            writer,
            cfg.attribute.encoder_config_for(ty, len),
        );

        let port_att = encoder.encode::<true, false>()?;
        port_atts.push(port_att);

        #[cfg(feature = "evaluation")]
        eval::scope_end(writer);
    }

    #[cfg(feature = "evaluation")]
    {
        eval::array_scope_end(writer);
        eval::scope_end(writer);
    }

    Ok(())
}

/// Per-attribute encoding configuration, keyed by attribute type. Any type
/// without an explicit override falls back to the built-in `default_for(ty, len)`
/// behaviour, so `Config::default()` reproduces the previous hardcoded pipeline.
#[derive(Clone, Debug)]
pub struct Config {
    overrides: HashMap<AttributeType, AttributeConfig>,
}

/// Per-attribute-type encoding overrides. Every knob is optional; a `None` field
/// keeps the built-in default for that attribute type, so a bare
/// `AttributeConfig::default()` is a no-op. Invalid combinations (e.g. a texture
/// predictor on a normal attribute) are representable here and rejected by
/// [`Config::validate`](crate::encode::Config::validate).
#[derive(Clone, Debug, Default)]
pub struct AttributeConfig {
    /// Prediction scheme override.
    pub prediction: Option<PredictionSchemeType>,
    /// Prediction transform override.
    pub transform: Option<PredictionTransformType>,
    /// Quantization resolution override.
    pub quantization: Option<Quantization>,
    /// Normal-specific encoding mode override (only valid for `Normal`).
    pub normal_encoding: Option<NormalEncoding>,
}

/// How a normal attribute is compressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub enum NormalEncoding {
    /// Octahedrally quantize the input normals and encode the real octahedral
    /// corrections (the default, lossy-by-quantization path).
    #[default]
    Quantized,
    /// Zero-CPU: trust the decoder's geometry-derived prediction and emit an
    /// all-zero correction stream. The input normal values are ignored, and
    /// only their seams are used.
    PredictedOnly,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            overrides: HashMap::new(),
        }
    }
}

impl Config {
    /// Overrides how normal attributes are compressed.
    pub fn set_normal_encoding(&mut self, enc: NormalEncoding) {
        self.overrides
            .entry(AttributeType::Normal)
            .or_default()
            .normal_encoding = Some(enc);
    }

    /// Overrides the per-type encoding for `ty`, replacing any prior override.
    pub fn set(&mut self, ty: AttributeType, cfg: AttributeConfig) {
        self.overrides.insert(ty, cfg);
    }

    /// The current override for `ty` (a clone), or an empty default if none is
    /// set. Useful for read-modify-write layering (e.g. a CLI flag patching a
    /// single knob on top of a file-loaded config).
    pub fn get(&self, ty: AttributeType) -> AttributeConfig {
        self.overrides.get(&ty).cloned().unwrap_or_default()
    }

    /// The per-type overrides, for validation.
    pub(crate) fn overrides(&self) -> &HashMap<AttributeType, AttributeConfig> {
        &self.overrides
    }

    /// Resolves the per-attribute encoder config for an attribute of type `ty`
    /// with `len` values, honoring any override and otherwise falling back to the
    /// built-in default.
    fn encoder_config_for(&self, ty: AttributeType, len: usize) -> attribute_encoder::Config {
        let Some(over) = self.overrides.get(&ty) else {
            return attribute_encoder::Config::default_for(ty, len);
        };

        // Start from the zero-correction base for PredictedOnly normals, else the
        // regular per-type default; then patch in any explicit knobs.
        let mut base = if over.normal_encoding == Some(NormalEncoding::PredictedOnly) {
            attribute_encoder::Config::predicted_normals(len)
        } else {
            attribute_encoder::Config::default_for(ty, len)
        };

        if let Some(scheme) = &over.prediction {
            base.set_prediction_scheme(scheme.clone());
        }
        if let Some(transform) = over.transform {
            base.set_prediction_transform(transform);
        }
        if let Some(quant) = over.quantization {
            base.set_quantization(quant);
        }
        base
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute_encoder::Err),
}
