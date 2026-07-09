use draco_oxide_core::bit_coder::ReaderErr;
use crate::prelude::ByteReader;
use draco_oxide_core::utils::bit_coder::leb128_read;

#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not enough data to decode metadata.")]
    NotEnoughData(#[from] ReaderErr),
}

/// Decoded metadata block. Fields are read into memory but not yet
/// surfaced via the public Mesh API; they're kept here so a future
/// "expose metadata to consumers" change is non-breaking.
#[allow(dead_code)]
pub struct Metadata {
    pub metadata: Vec<AttributeMetadata>,
    pub global_metadata: AttributeMetadata,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SubMetadata {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

impl SubMetadata {
    pub fn read_from<W>(reader: &mut W) -> Result<Self, Err>
    where
        W: ByteReader,
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

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AttributeMetadata {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub submetadata: Vec<SubMetadata>,
}

impl AttributeMetadata {
    pub fn read_from<W>(reader: &mut W) -> Result<Self, Err>
    where
        W: ByteReader,
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

        // read sub_metadata
        let num_submetadata = leb128_read(reader)? as u32;
        let mut submetadata = Vec::with_capacity(num_submetadata as usize);
        for _ in 0..num_submetadata {
            submetadata.push(SubMetadata::read_from(reader)?);
        }
        Ok(AttributeMetadata {
            key,
            value,
            submetadata,
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
where
    W: ByteReader,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_metadata_read_lengths_match_declared() {
        // 1-byte key length + 3 key bytes, 1-byte value length + 4
        // value bytes, leb128 0 = no submetadata.
        let bytes: Vec<u8> = vec![3, b'k', b'e', b'y', 4, b'd', b'a', b't', b'a', 0];
        let mut reader = bytes.into_iter();
        let meta = AttributeMetadata::read_from(&mut reader).unwrap();
        assert_eq!(meta.key, vec![b'k', b'e', b'y']);
        assert_eq!(meta.value, vec![b'd', b'a', b't', b'a']);
        assert_eq!(meta.submetadata.len(), 0);
    }

    #[test]
    fn submetadata_read_lengths_match_declared() {
        let bytes: Vec<u8> = vec![2, 0xAA, 0xBB, 3, 0x01, 0x02, 0x03];
        let mut reader = bytes.into_iter();
        let sub = SubMetadata::read_from(&mut reader).unwrap();
        assert_eq!(sub.key, vec![0xAA, 0xBB]);
        assert_eq!(sub.value, vec![0x01, 0x02, 0x03]);
    }
}
