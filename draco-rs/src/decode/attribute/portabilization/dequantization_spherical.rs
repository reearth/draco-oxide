use crate::{prelude::Vector, shared::attribute::Portable};

use super::DeportabilizationImpl;

pub(crate) struct DequantizationSpherical<Data> {
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> DequantizationSpherical<Data> 
{
    pub fn new<F>(_stream_in: &mut F) -> Self 
        where F: FnMut(u8)->u64
    {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> DeportabilizationImpl<Data> for DequantizationSpherical<Data> 
    where Data: Vector + Portable,
{
    fn deportabilize_next<F>(&self, _stream_in: &mut F) -> Data 
        where F: FnMut(u8) -> u64
    {
        // Implement the logic to portabilize the data
        unimplemented!()
    }
}