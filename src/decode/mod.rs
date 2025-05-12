use crate::{debug_expect, prelude::ConfigType, Mesh};

mod header;
mod metadata;
mod connectivity;
mod attribute;

pub fn decode<F>(stream_in: &mut F, cfg: Config) -> Result<Mesh, Err> 
    where F: FnMut(u8) -> u64
{
    // Decode header
    header::decode_header(stream_in)
        .map_err(|r| Err::HeaderError(r))?;

    debug_expect!("Header done, now starting metadata.", stream_in);

    // Decode metadata
    let _metadata  = metadata::decode_metadata(stream_in)
        .map_err(|r| Err::MetadataError(r))?;

    debug_expect!("Metadata done, now starting connectivity.", stream_in);

    // Decode connectivity
    let connectivity_atts = connectivity::decode_connectivity_atts(stream_in)
        .map_err(|r| Err::ConnectivityError(r))?;

    debug_expect!("Connectivity done, now starting attributes.", stream_in);

    // Decode attributes
    let attributes = attribute::decode_attributes(stream_in, cfg.attribute_decoder_cfg, connectivity_atts)
        .map_err(|r| Err::AttributeError(r))?;

    debug_expect!("All done", stream_in);

    // Create mesh
    let mut mesh = Mesh::new();
    for att in attributes {
        mesh.add_attribute(att);
    }

    Ok(mesh)
}


#[derive(Debug, Clone)]
pub struct Config {
    attribute_decoder_cfg: attribute::Config,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            attribute_decoder_cfg: attribute::Config::default(),
        }
    }
}


#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Attribute encoding error")]
    AttributeError(#[from] attribute::Err),
    #[error("Connectivity encoding error")]
    ConnectivityError(#[from] connectivity::Err),
    #[error("Header encoding error")]
    HeaderError(#[from] header::Err),
    #[error("Metadata encoding error")]
    MetadataError(#[from] metadata::Err),
}

pub(crate) fn decode_string<F>(stream_in: &mut F) -> String
    where F: FnMut(u8)-> u64
{
    let mut bytes = Vec::new();
    let len = stream_in(64) as usize;
    for _ in 0..len {
        bytes.push(stream_in(8) as u8);
    }
    String::from_utf8(bytes).unwrap()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::core::buffer;
    use crate::encode::encode_string;
    
    #[test]
    fn test_encode_string() {
        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        encode_string("Hello World!😀", &mut writer);
        let data: buffer::Buffer = buff_writer.into();

        let mut buff_reader = data.into_reader();
        let mut reader = |size| buff_reader.next(size);
        let result = decode_string(&mut reader);
        
        assert_eq!(result, "Hello World!😀");
    }
}