pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

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
        if cfg.encoder_method == crate::shared::connectivity::Encoder::Edgebreaker {
            // encode decoder id
            writer.write_u8(i as u8);
            // encode attribute type
            att.get_domain().write_to(writer);
            // write traversal method for attribute encoding/decoding sequencer. We currently only support depth-first traversal.
            TraversalType::DepthFirst.write_to(writer);
        }
    }

    #[cfg(feature = "evaluation")]
    eval::array_scope_begin("attributes", writer);
    
    for att in &atts {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("attribute", writer);

        let parents_ids = att.get_parents();
        let parents = parents_ids.iter()
            .map(|id| atts.iter().find(|att| att.get_id() == *id).unwrap())
            .collect::<Vec<_>>();

        let encoder = attribute_encoder::AttributeEncoder::new(
            &att,
            &parents,
            &conn_out,
            writer,
            attribute_encoder::Config::default(),
        );

        encoder.encode::<true>()?;

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


pub struct Config {
    _cfgs: Vec<attribute_encoder::Config>,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            _cfgs: vec![attribute_encoder::Config::default()],
        }
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute_encoder::Err)
}