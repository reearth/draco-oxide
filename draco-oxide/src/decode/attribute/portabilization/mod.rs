use crate::{debug_expect, prelude::{ByteReader, Vector}, shared::attribute::Portable};

pub mod dequantization_rect_array;
pub mod to_bits;


#[enum_dispatch::enum_dispatch(DeportabilizationImpl<Data>)]
pub(crate) enum Deportabilization<Data> 
    where Data: Vector + Portable,
{
    DequantizationRectangleArray(dequantization_rect_array::DequantizationRectangleArray<Data>),
    ToBits(to_bits::ToBits<Data>),
}


impl<Data> Deportabilization<Data> 
    where Data: Vector + Portable,
{
    pub(crate) fn new<R>(reader: &mut R) -> Result<Self, Err> 
        where R: ByteReader
    {
        debug_expect!("Start of Portabilization Metadata", reader);
        let ty = DeportabilizationType::read_from(reader)
            .map_err(|id| Err::InvalidDeportabilizationId(id))?;
        let out = match ty {
            DeportabilizationType::DequantizationRectangleArray => {
                Deportabilization::DequantizationRectangleArray(dequantization_rect_array::DequantizationRectangleArray::new(reader))
            },
            DeportabilizationType::ToBits => {
                Deportabilization::ToBits(to_bits::ToBits::new(reader))
            }
        };
        debug_expect!("End of Portabilization Metadata", reader);
        Ok(out)
    }
}

#[enum_dispatch::enum_dispatch]
pub trait DeportabilizationImpl<Data> 
    where Data: Vector + Portable,
{
    /// Reads the portabilied data from the buffer and deportablize them.
    /// The outputs are (output data, metadata)
    fn deportabilize_next<R>(&self, reader: &mut R) -> Data
        where R: ByteReader;
}


#[remain::sorted]
#[derive(Clone, Copy)]
pub enum DeportabilizationType {
    DequantizationRectangleArray,
    ToBits,
}

impl DeportabilizationType {
    pub fn read_from<R>(reader: &mut R) -> Result<Self, usize> 
        where R: ByteReader
    {
        let id = reader.read_u8().unwrap() as usize; // ToDo: handle error properly.
        let out = match id {
            0 => DeportabilizationType::DequantizationRectangleArray,
            1 => DeportabilizationType::ToBits,
            _ => return Err(id as usize),
        };
        Ok(out)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Invalid deportabilization id: {0}")]
    InvalidDeportabilizationId(usize),
}