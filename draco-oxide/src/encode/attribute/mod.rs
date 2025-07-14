pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

use crate::encode::attribute::portabilization::PortabilizationType;
use crate::encode::connectivity::ConnectivityEncoderOutput;
#[cfg(feature = "evaluation")]
use crate::eval;

use crate::prelude::{Attribute, ByteWriter, ConfigType};
use crate::shared::connectivity::edgebreaker::TraversalType;

pub fn encode_attributes<W>(
    atts: Vec<Attribute>,
    writer: &mut W,
    conn_out: ConnectivityEncoderOutput<'_>,
    cfg: &super::Config,
) -> Result<(), Err> 
    where W: ByteWriter
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("attributes", writer);

    // Write the number of attribute encoders/decoders (In draco-oxide, this is the same as the number of attributes as 
    // each attribute has its own encoder/decoder)
    writer.write_u8(atts.len() as u8);
    #[cfg(feature = "evaluation")]
    eval::write_json_pair("attributes count", atts.len().into(), writer);

    for (i, att) in atts.iter().enumerate() {
        if cfg.encoder_method == crate::shared::header::EncoderMethod::Edgebreaker {
            // encode decoder id
            writer.write_u8((i as u8).wrapping_sub(1));
            // encode attribute type
            att.get_domain().write_to(writer);
            // write traversal method for attribute encoding/decoding sequencer. We currently only support depth-first traversal.
            TraversalType::DepthFirst.write_to(writer);
        }
    }

    #[cfg(feature = "evaluation")]
    eval::array_scope_begin("attributes", writer);

    let mut port_atts: Vec<Attribute> = Vec::new();
    for att in &atts {
        // Write 1 to indicate that the encoder is for one attribute.
        writer.write_u8(1);

        att.get_attribute_type().write_to(writer);
        att.get_component_type().write_to(writer);
        writer.write_u8(att.get_num_components() as u8);
        writer.write_u8(0); // Normalized flag, currently not used.
        writer.write_u8(att.get_id().as_usize() as u8); // unique id, just write 0 as we have one encoder per attribute.

        // write the decoder type.
        PortabilizationType::default_for(att.get_attribute_type()).write_to(writer);
    }
    
    for (i, att) in atts.into_iter().enumerate() {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("attribute", writer);

        let parents_ids = att.get_parents();
        let parents = parents_ids.iter()
            .map(|id| port_atts.iter().find(|att| att.get_id() == *id).unwrap())
            .collect::<Vec<_>>();

        let ty = att.get_attribute_type();
        let len = att.len();
        let encoder = attribute_encoder::AttributeEncoder::new(
            att,
            i,
            &parents,
            &conn_out,
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
    #[allow(unused)] // This field is unused in the current implementation, as we only support the default attribute encoder configuration.
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
    AttributeError(#[from] attribute_encoder::Err)
}