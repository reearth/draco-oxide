use crate::core::{bit_coder::ReaderErr, mesh::metadata::Metadata}; 
use crate::prelude::ByteReader;


#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not enough data to decode metadata.")]
    NotEnoughData(#[from] ReaderErr),
}

pub fn decode_metadata<W>(_reader: &mut W) -> Result<Metadata, Err>
    where W: ByteReader,
{
    // Read Decoder
    _reader.read_u32()?;

    Ok(Metadata::new())
}