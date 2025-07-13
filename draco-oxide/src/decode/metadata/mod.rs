use crate::core::bit_coder::ReaderErr; 
use crate::prelude::ByteReader;
use crate::utils::bit_coder::leb128_read;


#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not enough data to decode metadata.")]
    NotEnoughData(#[from] ReaderErr),
}

pub struct Metadata {
    pub metadata: Vec<AttributeMetadata>,
    pub global_metadata: AttributeMetadata,
}

#[derive(Debug, Clone)]
pub struct SubMetadata {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

impl SubMetadata {
    pub fn read_from<W>(reader: &mut W) -> Result<Self, Err>
    where W: ByteReader,
    {
        let key_length = reader.read_u8()?;
        let mut key = vec![0; key_length as usize];
        for i in 0..key_length {
            key[i as usize] = reader.read_u8()?;
        }
        let value_length = reader.read_u8()?;
        let mut value = vec![0; value_length as usize];
        for i in 0..value_length {
            value[i as usize] = reader.read_u8()?;
        }
        Ok(SubMetadata { key, value })
    }
}

#[derive(Debug, Clone)]
pub struct AttributeMetadata {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub submetadata: Vec<SubMetadata>,
}

impl AttributeMetadata {
    pub fn read_from<W>(reader: &mut W) -> Result<Self, Err>
    where W: ByteReader,
    {
        let key_length = reader.read_u8()?;
        let mut key = vec![0; key_length as usize];
        for _ in 0..key_length {
            key.push(reader.read_u8()?);
        }
        let value_length = reader.read_u8()?;
        let mut value = vec![0; value_length as usize];
        for _ in 0..value_length {
            value.push(reader.read_u8()?);
        }

        // read sub_metadata
        let num_submetadata = leb128_read(reader)? as u32;
        let mut submetadata = Vec::with_capacity(num_submetadata as usize);
        for _ in 0..num_submetadata {
            submetadata.push(SubMetadata::read_from(reader)?);
        }
        Ok(AttributeMetadata {
            key,
            value,
            submetadata: submetadata,
        })
    }

    pub fn empty_metadta() -> Self {
        AttributeMetadata {
            key: Vec::new(),
            value: Vec::new(),
            submetadata: Vec::new(),
        }
    }
}

pub fn decode_metadata<W>(reader: &mut W) -> Result<Metadata, Err>
    where W: ByteReader,
{
    let num_metadata = reader.read_u32()?;
    let mut metadta_id = Vec::with_capacity(num_metadata as usize);
    let mut metadata = Vec::new();
    metadata.resize(num_metadata as usize, AttributeMetadata::empty_metadta()); 
    for _ in 0..num_metadata {
        metadta_id.push(leb128_read(reader)?);
        metadata[*metadta_id.last().unwrap() as usize] = AttributeMetadata::read_from(reader)?;
    }
    let global_metadta = AttributeMetadata::read_from(reader)?;

    let out = Metadata {
        metadata,
        global_metadata: global_metadta,
    };

    Ok(out)
}