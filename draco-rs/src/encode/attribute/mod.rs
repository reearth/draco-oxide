pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

#[cfg(feature = "evaluation")]
use crate::eval;

use crate::prelude::{ByteWriter, ConfigType}; 
use crate::core::mesh::Mesh;

pub fn encode_attributes<W>(
    mesh: &Mesh,
    writer: &mut W,
    cfg: Config,
) -> Result<(), Err> 
    where W: ByteWriter
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("attributes", writer);

    let (_,non_conn_atts) = mesh.take_splitted_attributes();

    // Write the number of attributes
    writer.write_u16(non_conn_atts.len() as u16); 
    #[cfg(feature = "evaluation")]
    eval::write_json_pair("attributes count", non_conn_atts.len().into(), writer);

    #[cfg(feature = "evaluation")]
    eval::array_scope_begin("attributes", writer);
    
    for non_conn_att in non_conn_atts.into_iter() {
        #[cfg(feature = "evaluation")]
        eval::scope_begin("attribute", writer);

        let parents_ids = non_conn_att.get_parents();
        let parents = parents_ids.iter()
            .map(|&id| &mesh.get_attributes()[id.as_usize()])
            .collect::<Vec<_>>();

        let encoder = attribute_encoder::AttributeEncoder::new(
            non_conn_att,
            &parents,
            writer,
            attribute_encoder::Config::default(),
        );

        if cfg.merge_rans_coders {
            unimplemented!("Merging rANS coders is not implemented yet");
        } else {
            if let Err(err) = encoder.encode::<true>() {
                return Err(Err::AttributeError(err))
            }
        };

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
    merge_rans_coders: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            _cfgs: vec![attribute_encoder::Config::default()],
            merge_rans_coders: false,
        }
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error: {0}")]
    AttributeError(#[from] attribute_encoder::Err)
}