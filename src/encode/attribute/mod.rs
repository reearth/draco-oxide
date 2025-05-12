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

    // Write the number of attributes
    writer((16, non_conn_atts.len() as u64)); 
    
    for non_conn_att in non_conn_atts.into_iter() {
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
        }
    }

    Ok(())
}

pub(crate) struct WritableFormat {
    data: Vec<(u8, u64)>, // (size, data)
}

impl WritableFormat {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
        }
    }
    
    pub fn append(&mut self, other: &WritableFormat) {
        self.data.extend_from_slice(&other.data);
    }

    #[inline]
    pub fn push(&mut self, input: (u8, u64)) {
        self.data.push(input);
    }
    
    #[inline]
    pub fn from_vec(data: Vec<(u8, u64)>) -> Self {
        Self { data }
    }
    
    pub fn write<F>(self, writer: &mut F)
        where F: FnMut((u8, u64))
    {
        for (size, data) in self.data.into_iter() {
            writer((size, data));
        }
    }

    pub fn into_iter(self) -> IntoWritableFormatIter {
        IntoWritableFormatIter::new(self.data)
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


struct IntoWritableFormatIter {
    data: std::vec::IntoIter<(u8, u64)>,
}

impl IntoWritableFormatIter {
    fn new(data: Vec<(u8, u64)>) -> Self {
        Self {
            data: data.into_iter(),
        }
    }

    fn write_next<F>(&mut self, writer: &mut F)
        where F: FnMut((u8, u64))
    {
        writer(self.data.next().unwrap());
    }
}

impl Iterator for IntoWritableFormatIter {
    type Item = (u8, u64);

    fn next(&mut self) -> Option<Self::Item> {
        self.data.next()
    }
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