use crate::core::shared::Vector;

pub mod rect_array;
pub mod rect_spiral;
pub mod spherical;

pub(crate) const PORTABILIZATION_ID_SLOT: usize = 0;

pub(crate) trait PortabilizationImpl {
    const PORTABILIZATION_ID: usize;
}

pub(crate) struct Quantized {
    size: usize,
    data: Vec<usize>,
}

impl Quantized {
    pub fn new(data: Vec<usize>, size: usize) -> Self {
        Self {
            size,
            data,
        }
    }
}


pub(crate) trait Linealizer<Data> 
    where Data: Vector,
{
    type Metadata;
    fn init(&mut self, metadata: Self::Metadata);
    fn linearize(&self, data: Vec<Data>) -> Vec<u64>;
    fn inverse_linialize(&self, data: &Vec<usize>) -> Vec<usize>;
}
