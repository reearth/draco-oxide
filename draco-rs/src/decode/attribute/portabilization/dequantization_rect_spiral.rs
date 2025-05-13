use crate::{prelude::Vector, shared::attribute::Portable};

use super::DeportabilizationImpl;


pub(crate) struct DequantizationRectangleSpiral<Data> {
    pub(crate) unit_cube_size: f64,
    pub(crate) data: Vec<Data>,
}

impl<Data> DequantizationRectangleSpiral<Data> {
    pub(crate) fn new<F>(_stream_in: &mut F) -> Self 
        where F: FnMut(u8)->u64
    {
        Self {
            unit_cube_size: 1.0,
            data: Vec::new(),
        }
    }
}

impl<Data> DeportabilizationImpl<Data> for DequantizationRectangleSpiral<Data> 
    where Data: Vector + Portable,
{
    fn deportabilize_next<F>(&self, _stream_in: &mut F) -> Data 
        where F: FnMut(u8)->u64
    {
        // Implement the logic to portabilize the data
        unimplemented!()
    }
}