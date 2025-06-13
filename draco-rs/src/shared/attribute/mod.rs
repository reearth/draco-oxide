use crate::core::bit_coder::ReaderErr;
use crate::prelude::{ByteReader, ByteWriter};

pub mod prediction_scheme;
pub mod portabilization;
pub mod sequence;

#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Invalid Attribute Kind: {0}")]
    AttributeKindError(u8),
    #[error("Reader Error: {0}")]
    ReaderError(#[from] ReaderErr),
}

pub trait Portable: Sized {
    fn to_bytes(self) -> Vec<u8>;
    fn write_to<W>(self, writer: &mut W) where W: ByteWriter;
    fn read_from<R>(reader: &mut R) -> Result<Self, ReaderErr>
        where R: ByteReader;
}


impl Portable for bool {
    fn to_bytes(self) -> Vec<u8> {
        vec![self as u8]
    }
    fn write_to<W>(self, writer: &mut W) where W: ByteWriter {
        writer.write_u8(self as u8);
    }
    fn read_from<R>(reader: &mut R) -> Result<Self, ReaderErr>
        where R: ByteReader
    {
        Ok(reader.read_u8()? != 0)
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeKind {
    MeshVertex,
    MeshCorner,
}

impl AttributeKind {
    pub(crate) fn read_from<R>(reader: &mut R) -> Result<Self, Err>
        where R: ByteReader
    {
        let id = reader.read_u8()?;
        match id {
            0 => Ok(Self::MeshVertex),
            1 => Ok(Self::MeshCorner),
            _ => Err(Err::AttributeKindError(id)),
        }
    }
}



#[cfg(test)]
mod tests {
    use crate::prelude::NdVector;
    use super::Portable;

    #[test]
    fn from_bits_f32() {
        let data = NdVector::from([1_f32, -1.0, 1.0]);
        let mut buff_writer = Vec::new();
        data.write_to(&mut buff_writer);
        let mut buff_reader = buff_writer.into_iter();
        let dequant_data: NdVector<3,f32> = NdVector::read_from(&mut buff_reader).unwrap();
        assert_eq!(data, dequant_data);
    }

    #[test]
    fn from_bits_f64() {
        let data = NdVector::from([1_f64, -1.0, 1.0]);
        let mut buff_writer = Vec::new();
        data.write_to(&mut buff_writer);
        let mut buff_reader = buff_writer.into_iter();
        let dequant_data: NdVector<3,f64> = NdVector::read_from(&mut buff_reader).unwrap();
        assert_eq!(data, dequant_data);
    }
}