pub mod connectivity;

pub mod attribute;

pub mod entropy;

pub mod header {
    use crate::bit_coder::ByteWriter;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum EncoderMethod {
        Edgebreaker,
        Sequential,
    }

    impl EncoderMethod {
        #[inline]
        pub fn write_to<W>(self, writer: &mut W)
        where
            W: ByteWriter,
        {
            match self {
                EncoderMethod::Sequential => writer.write_u8(0),
                EncoderMethod::Edgebreaker => writer.write_u8(1),
            }
        }
    }
}
