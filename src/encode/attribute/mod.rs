pub(crate) mod attribute_encoder;
pub(crate) mod portabilization;
pub(crate) mod prediction_transform;

use crate::{prelude::ConfigType, shared::attribute::Portable, Mesh};

pub fn encode_attributes<F>(
    mesh: &Mesh,
    writer: &mut F,
    cfg: Config,
) -> Result<(), Err> 
    where F: FnMut((u8, u64))
{
    let (_,non_conn_atts) = mesh.take_splitted_attributes();
    
    for (non_conn_att, att_cfg) in non_conn_atts.into_iter().zip(cfg.cfgs.into_iter()) {
        let parents_ids = non_conn_att.get_parents();
        let parents = parents_ids.iter()
            .map(|&id| &mesh.get_attributes()[id.as_usize()])
            .collect::<Vec<_>>();
        let encoder = attribute_encoder::AttributeEncoder::new(
            non_conn_att,
            &parents,
            writer,
            att_cfg,
        );

        if cfg.merge_rans_coders {
            unimplemented!("Merging rANS coders is not implemented yet");
        } else {
            if let Err(err) = encoder.encode::<false>() {
                return Err(Err::AttributeError(err))
            }
        }
    }

    Ok(())
}

struct WritableFormat {
    data: Vec<(u8, u64)>, // (size, data)
}

impl WritableFormat {
    fn new() -> Self {
        Self {
            data: Vec::new(),
        }
    }
    
    fn append(&mut self, other: &WritableFormat) {
        self.data.extend_from_slice(&other.data);
    }

    #[inline]
    fn push(&mut self, input: (u8, u64)) {
        self.data.push(input);
    }
    
    fn from_vec(data: Vec<(u8, u64)>) -> Self {
        Self { data }
    }
    
    fn write<F>(&mut self, writer: &mut F)
        where F: FnMut((u8, u64))
    {
        for (size, data) in self.data.iter() {
            writer((*size, *data));
        }
    }
}

impl<T> From<T> for WritableFormat 
    where T: Portable
{
    fn from(data: T) -> Self {
        WritableFormat::from_vec(
            data.to_bits()
        )
    }
}

impl From<()> for WritableFormat {
    fn from(_: ()) -> Self {
        WritableFormat::new()
    }
} 

impl From<bool> for WritableFormat {
    fn from(input: bool) -> Self {
        let data = if input { 1 } else { 0 };
        WritableFormat::from_vec(vec![(1, data)])
    }
} 


pub struct Config {
    cfgs: Vec<attribute_encoder::Config>,
    merge_rans_coders: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            cfgs: vec![attribute_encoder::Config::default()],
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