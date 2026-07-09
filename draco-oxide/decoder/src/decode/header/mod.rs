use draco_oxide_core::bit_coder::ReaderErr;
use crate::prelude::ByteReader;
use draco_oxide_core::codec::header::EncoderMethod;

#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Not a Draco file")]
    NotADracoFile,
    #[error("Not enough data: {0}")]
    NotEnoughData(#[from] ReaderErr),
}

pub(crate) struct Header {
    // Header version: read from the bitstream and exposed for callers
    // that want to gate on it. Currently no decode path branches on
    // version (Google 1.5.7 — the latest release as of 2026 — emits
    // 2.2 and we handle that uniformly).
    #[allow(dead_code)]
    pub version_major: u8,
    #[allow(dead_code)]
    pub version_minor: u8,
    #[allow(dead_code)]
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
    if !(0..5)
        .map(|_| reader.read_u8().unwrap() as char) // ToDo: remove unwrap, handle error properly
        .zip("DRACO".chars())
        .all(|(a, b)| a == b)
    {
        return Err(Err::NotADracoFile);
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

    Ok(Header {
        version_major,
        version_minor,
        encoder_type,
        encoding_method,
        contains_metadata,
    })
}
