pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

use crate::encode::attribute::portabilization::PortabilizationType;
pub use crate::encode::attribute::portabilization::Quantization;
pub use crate::encode::attribute::prediction_transform::PredictionTransformType;
#[cfg(feature = "evaluation")]
use crate::eval;

use std::collections::HashMap;

use draco_oxide_core::attribute::{Attribute, AttributeDomain, AttributeType, ComponentDataType};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::attribute::sequence::PredictionDegreeTraverser;
use draco_oxide_core::codec::connectivity::edgebreaker::TraversalType;
use draco_oxide_core::codec::header::EncoderMethod;
use draco_oxide_core::mesh::ds::AttributeDS;
use draco_oxide_core::types::{ConfigType, CornerIdx};
use draco_oxide_core::utils::bit_coder::leb128_write;

use attribute_encoder::{SequenceSource, Sequencing};

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

    let result = match cfg.connectivity.encoder_method() {
        EncoderMethod::Edgebreaker => {
            encode_traversed_attributes(adss, corners_of_edgebreaker, writer, cfg)
        }
        EncoderMethod::Sequential => encode_linear_attributes(adss, writer, cfg),
    };

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);

    result
}

/// Encodes each attribute over a traversal of its own connectivity, one
/// attribute encoder per attribute.
fn encode_traversed_attributes<W>(
    adss: Vec<AttributeDS>,
    corners_of_edgebreaker: Vec<CornerIdx>,
    writer: &mut W,
    cfg: &super::Config,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    // Write the number of attribute encoders/decoders (In draco-oxide, this is the same as the number of attributes as
    // each attribute has its own encoder/decoder)
    writer.write_u8(adss.len() as u8);
    #[cfg(feature = "evaluation")]
    eval::write_json_pair("attributes count", adss.len().into(), writer);

    // The resolved traversal method per attribute. Prediction-degree traversal
    // is defined over the position connectivity only, so an attribute with
    // interior seams always walks depth-first.
    let traversals: Vec<TraversalType> = adss
        .iter()
        .map(|att| {
            if att.corner_table().has_interior_seams() {
                TraversalType::DepthFirst
            } else {
                cfg.attribute
                    .traversal_for(att.att_data().get_attribute_type())
            }
        })
        .collect();

    for (i, att) in adss.iter().enumerate() {
        // encode decoder id
        writer.write_u8((i as u8).wrapping_sub(1));
        // Element type: a corner attribute without interior seams shares the
        // position connectivity, so it is written as a vertex attribute,
        // matching Google's encoder.
        let domain = att.att_data().get_domain();
        let wire_domain =
            if domain == AttributeDomain::Corner && !att.corner_table().has_interior_seams() {
                AttributeDomain::Position
            } else {
                domain
            };
        wire_domain.write_to(writer);
        // write traversal method for attribute encoding/decoding sequencer.
        traversals[i].write_to(writer);
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
        PortabilizationType::default_for(
            att.att_data().get_attribute_type(),
            att.att_data().get_component_type(),
        )
        .write_to(writer);
    }

    // `adss` is built one-per-attribute and in the same order as `atts`, so each attribute is
    // paired with its own attribute data structure here.
    //
    // Attributes without interior seams share the position connectivity, so
    // attributes walking it with the same traversal method have identical
    // sequences. Mirroring the decoder, the first attribute of each method
    // records the walk and later attributes replay the recording borrowed;
    // an attribute with its own connectivity walks lazily inside its encoder.
    // Prediction-degree traversal has no lazy walk and is materialized up
    // front.
    let mut shared_sequences: Vec<(TraversalType, Vec<CornerIdx>)> = Vec::new();
    for (ads, traversal) in adss.into_iter().zip(traversals) {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("attribute", writer);

        let parents_ids = ads.att_data().get_parents();
        let parents = parents_ids
            .iter()
            .map(|id| port_atts.iter().find(|att| att.get_id() == *id).unwrap())
            .collect::<Vec<_>>();

        let sequence = if ads.corner_table().has_interior_seams() {
            SequenceSource::Own
        } else {
            match shared_sequences.iter().position(|(t, _)| *t == traversal) {
                Some(i) => SequenceSource::Shared(&shared_sequences[i].1),
                None => match traversal {
                    TraversalType::DepthFirst => {
                        shared_sequences.push((traversal, Vec::new()));
                        SequenceSource::Record(&mut shared_sequences.last_mut().unwrap().1)
                    }
                    TraversalType::PredictionDegree => {
                        let s =
                            PredictionDegreeTraverser::new(&ads, corners_of_edgebreaker.clone())
                                .compute_seqeunce();
                        shared_sequences.push((traversal, s));
                        SequenceSource::Shared(&shared_sequences.last().unwrap().1)
                    }
                },
            }
        };

        let ty = ads.att_data().get_attribute_type();
        let component_ty = ads.att_data().get_component_type();
        let len = ads.att_data().len();
        let encoder = attribute_encoder::AttributeEncoder::new(
            ads,
            &parents,
            &corners_of_edgebreaker,
            writer,
            cfg.attribute.encoder_config_for(ty, component_ty, len),
            Sequencing::Traversal,
            sequence,
        );

        // This encoder carries one attribute, so its portabilization metadata
        // belongs immediately after its payload.
        let (port_att, port_info) = encoder.encode::<true, false>()?;
        port_atts.push(port_att);
        for byte in port_info {
            writer.write_u8(byte);
        }

        #[cfg(feature = "evaluation")]
        eval::scope_end(writer);
    }

    #[cfg(feature = "evaluation")]
    eval::array_scope_end(writer);

    Ok(())
}

/// Encodes every attribute over the point space in index order, in a single
/// attribute encoder. Matches Google's sequential encoder, which has no corner
/// table to sequence or predict over.
fn encode_linear_attributes<W>(
    adss: Vec<AttributeDS>,
    writer: &mut W,
    cfg: &super::Config,
) -> Result<(), Err>
where
    W: ByteWriter,
{
    // One attribute encoder carries every attribute.
    writer.write_u8(1);
    #[cfg(feature = "evaluation")]
    eval::write_json_pair("attributes count", adss.len().into(), writer);

    leb128_write(adss.len() as u64, writer);
    for ads in &adss {
        let att = ads.att_data();
        att.get_attribute_type().write_to(writer);
        att.get_component_type().write_to(writer);
        writer.write_u8(att.get_num_components() as u8);
        writer.write_u8(0); // Normalized flag, currently not used.
        leb128_write(att.get_id().as_usize() as u64, writer);
    }
    for ads in &adss {
        PortabilizationType::default_for(
            ads.att_data().get_attribute_type(),
            ads.att_data().get_component_type(),
        )
        .write_to(writer);
    }

    #[cfg(feature = "evaluation")]
    eval::array_scope_begin("attributes", writer);

    let num_points = adss[0].global_ds().num_points();
    let mut port_infos = Vec::with_capacity(adss.len());
    for ads in adss {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("attribute", writer);

        let ty = ads.att_data().get_attribute_type();
        let component_ty = ads.att_data().get_component_type();
        let len = ads.att_data().len();
        let encoder = attribute_encoder::AttributeEncoder::new(
            ads,
            &[],
            &[],
            writer,
            cfg.attribute
                .encoder_config_for(ty, component_ty, len)
                .for_sequential(),
            Sequencing::Linear { num_points },
            attribute_encoder::SequenceSource::Own,
        );
        port_infos.push(encoder.encode::<true, false>()?.1);

        #[cfg(feature = "evaluation")]
        eval::scope_end(writer);
    }

    // The encoder emits every payload before the first portabilization block.
    for byte in port_infos.into_iter().flatten() {
        writer.write_u8(byte);
    }

    #[cfg(feature = "evaluation")]
    eval::array_scope_end(writer);

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
    /// Traversal method override. Applies only under edgebreaker connectivity,
    /// and only to attributes without interior seams; everything else walks
    /// depth-first.
    pub traversal: Option<TraversalType>,
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

    /// Resolves the traversal method for an attribute of type `ty`.
    fn traversal_for(&self, ty: AttributeType) -> TraversalType {
        self.overrides
            .get(&ty)
            .and_then(|o| o.traversal)
            .unwrap_or(TraversalType::DepthFirst)
    }

    /// Resolves the per-attribute encoder config for an attribute of type `ty`
    /// with `len` values, honoring any override and otherwise falling back to the
    /// built-in default.
    fn encoder_config_for(
        &self,
        ty: AttributeType,
        component_ty: ComponentDataType,
        len: usize,
    ) -> attribute_encoder::Config {
        let Some(over) = self.overrides.get(&ty) else {
            return attribute_encoder::Config::default_for(ty, component_ty, len);
        };

        // Start from the zero-correction base for PredictedOnly normals, else the
        // regular per-type default; then patch in any explicit knobs.
        let mut base = if over.normal_encoding == Some(NormalEncoding::PredictedOnly) {
            attribute_encoder::Config::predicted_normals(len)
        } else {
            attribute_encoder::Config::default_for(ty, component_ty, len)
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
