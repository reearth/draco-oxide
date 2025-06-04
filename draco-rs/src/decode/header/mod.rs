use crate::prelude::ByteReader;
use crate::core::bit_coder::ReaderErr;


#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not a Draco file")]
    NotADracoFile,
    #[error("Not enough data: {0}")]
    NotEnoughData(#[from] ReaderErr),
}

pub(crate) struct GlobalConfig {
    pub version_major: u8,
    pub version_minor: u8,
    pub encoder_type: u8,
    pub encoding_method: u8,
}

pub fn decode_header<W>(reader: &mut W) -> Result<GlobalConfig, Err>
where
    W: ByteReader,
{
    // Read the draco string
    if !(0..5).map(|_| reader.read_u8().unwrap() as char )
            .zip("DRACO".chars())
            .all(|(a, b)| a == b)
    {
        return Err(Err::NotADracoFile)
    };

    // Read the version
    let version_major = reader.read_u8().unwrap();
    let version_minor = reader.read_u8().unwrap();

    // Readd the encoder type
    let encoder_type = reader.read_u8().unwrap();

    // Read the encoding method
    let encoding_method = reader.read_u8().unwrap();

    Ok (
        GlobalConfig {
            version_major,
            version_minor,
            encoder_type,
            encoding_method,
        }
    )
}