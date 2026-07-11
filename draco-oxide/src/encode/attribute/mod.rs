pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

use crate::encode::attribute::portabilization::PortabilizationType;
#[cfg(feature = "evaluation")]
use crate::eval;

use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::bit_coder::ByteWriter;
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
        if cfg.encoder_method == draco_oxide_core::codec::header::EncoderMethod::Edgebreaker {
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
            attribute_encoder::Config::default_for(ty, len),
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

#[derive(Clone, Debug)]
pub struct Config {
    #[allow(unused)]
    // This field is unused in the current implementation, as we only support the default attribute encoder configuration.
    cfgs: Vec<attribute_encoder::Config>,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            cfgs: vec![attribute_encoder::Config::default()],
        }
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute_encoder::Err),
}
