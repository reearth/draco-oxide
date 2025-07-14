pub mod connectivity;

pub mod attribute;

pub mod entropy;

pub mod header {
    use crate::{core::bit_coder::ReaderErr, prelude::{ByteReader, ByteWriter}};

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) enum EncoderMethod {
        Edgebreaker,
        #[allow(unused)]
        Sequential,
    }

    impl EncoderMethod {
        #[inline]
        #[allow(unused)]
        pub fn read_from<R>(reader: &mut R) -> Result<Self, ReaderErr> 
            where R: ByteReader
        {
            match reader.read_u8()? {
                0 => Ok(EncoderMethod::Sequential),
                1 => Ok(EncoderMethod::Edgebreaker),
                _ => panic!("Unknown encoder method ID"),
            }
        }

        #[inline]
        pub fn write_to<W>(self, writer: &mut W) 
            where W: ByteWriter
        {
            match self {
                EncoderMethod::Sequential => writer.write_u8(0),
                EncoderMethod::Edgebreaker => writer.write_u8(1),
            }
        }
    }
}