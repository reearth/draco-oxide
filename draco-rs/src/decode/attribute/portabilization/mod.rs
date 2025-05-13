use crate::{debug_expect, prelude::Vector, shared::attribute::Portable};

pub mod dequantization_rect_array;
pub mod dequantization_rect_spiral;
pub mod dequantization_spherical;
pub mod to_bits;


#[enum_dispatch::enum_dispatch(DeportabilizationImpl<Data>)]
pub(crate) enum Deportabilization<Data> 
    where Data: Vector + Portable,
{
    DequantizationRectangleArray(dequantization_rect_array::DequantizationRectangleArray<Data>),
    DequantizationRectangleSpiral(dequantization_rect_spiral::DequantizationRectangleSpiral<Data>),
    DequantizationSpherical(dequantization_spherical::DequantizationSpherical<Data>),
    ToBits(to_bits::ToBits<Data>),
}


impl<Data> Deportabilization<Data> 
    where Data: Vector + Portable,
{
    pub(crate) fn new<F>(stream_in: &mut F) -> Result<Self, Err> 
        where F: FnMut(u8)->u64
    {
        debug_expect!("Start of Portabilization Metadata", stream_in);
        let ty = DeportabilizationType::from_id(stream_in)
            .map_err(|id| Err::InvalidDeportabilizationId(id))?;
        let out = match ty {
            DeportabilizationType::DequantizationRectangleArray => {
                Deportabilization::DequantizationRectangleArray(dequantization_rect_array::DequantizationRectangleArray::new(stream_in))
            },
            DeportabilizationType::DequantizationRectangleSpiral => {
                Deportabilization::DequantizationRectangleSpiral(dequantization_rect_spiral::DequantizationRectangleSpiral::new(stream_in))
            },
            DeportabilizationType::DequantizationSpherical => {
                Deportabilization::DequantizationSpherical(dequantization_spherical::DequantizationSpherical::new(stream_in))
            },
            DeportabilizationType::ToBits => {
                Deportabilization::ToBits(to_bits::ToBits::new(stream_in))
            }
        };
        debug_expect!("End of Portabilization Metadata", stream_in);
        Ok(out)
    }
}

#[enum_dispatch::enum_dispatch]
pub trait DeportabilizationImpl<Data> 
    where Data: Vector + Portable,
{
    /// Reads the portabilied data from the buffer and deportablize them.
    /// The outputs are (output data, metadata)
    fn deportabilize_next<F>(&self, stream_in: &mut F) -> Data
        where F: FnMut(u8)->u64;
}


#[remain::sorted]
#[derive(Clone, Copy)]
pub enum DeportabilizationType {
    DequantizationRectangleArray,
    DequantizationRectangleSpiral,
    DequantizationSpherical,
    ToBits,
}

impl DeportabilizationType {
    pub fn from_id<F>(stream_in: &mut F) -> Result<Self, usize> 
        where F: FnMut(u8)->u64
    {
        let id = stream_in(4) as usize;
        let out = match id {
            0 => DeportabilizationType::DequantizationRectangleArray,
            1 => DeportabilizationType::DequantizationRectangleSpiral,
            2 => DeportabilizationType::DequantizationSpherical,
            3 => DeportabilizationType::ToBits,
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