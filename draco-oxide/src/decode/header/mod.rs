use crate::prelude::ByteReader;
use crate::core::bit_coder::ReaderErr;
use crate::shared::header::EncoderMethod;


#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not a Draco file")]
    NotADracoFile,
    #[error("Not enough data: {0}")]
    NotEnoughData(#[from] ReaderErr),
}

pub(crate) struct Header {
    pub version_major: u8,
    pub version_minor: u8,
    pub encoder_type: u8,
    pub encoding_method: EncoderMethod,
    pub contains_metadata: bool,
}

const METADATA_FLAG_MASK: u16 = 32768;

pub fn decode_header<W>(reader: &mut W) -> Result<Header, Err>
where
    W: ByteReader,
{
    // Read the draco string
    if !(0..5).map(|_| reader.read_u8().unwrap() as char ) // ToDo: remove unwrap, handle error properly
            .zip("DRACO".chars())
            .all(|(a, b)| a == b)
    {
        return Err(Err::NotADracoFile)
    };

    // Read the version
    let version_major = reader.read_u8()?;
    let version_minor = reader.read_u8()?;

    // Readd the encoder type
    let encoder_type = reader.read_u8()?;

    // Read the encoding method
    let encoding_method = EncoderMethod::read_from(reader)?;

    let flags = reader.read_u16()?;

    let contains_metadata = flags & METADATA_FLAG_MASK != 0;

    Ok (
        Header {
            version_major,
            version_minor,
            encoder_type,
            encoding_method,
            contains_metadata,
        }
    )
}