use std::ops;

use crate::core::shared::Cast;
use crate::core::shared::DataValue;
use crate::core::shared::ElementWiseDiv;
use crate::core::shared::Float;
use crate::core::shared::NdVector;
use crate::core::shared::Vector;

use super::Portabilization;

pub struct Quantization<Data> 
{
    global_metadata_min: Data,
    global_metadata_max: Data,
    quantization_bits: u8,
}

impl<Data> Quantization<Data> 
    where Data: Vector+Copy
{
    pub fn new(quantization_bits: u8, sample_data: Data) -> Self {
        Self {
            global_metadata_min: sample_data,
            global_metadata_max: sample_data,
            quantization_bits
        }
    }
}

impl<Data> Portabilization for Quantization<Data> 
    where 
        Data: Vector + Cast + Copy,
        <Data as Cast>::Output: Vector<Component = u64>,
        Data: ops::Add<Output = Data> + ops::Sub<Output = Data> + ops::Mul<Data::Component, Output = Data> + ElementWiseDiv<Output = Data>,
        Data::Component: Float + DataValue
{
    type Data = Data;
    type Output = <Data as Cast>::Output;
    fn skim(&mut self, att_val: &Self::Data) {
        for (i, &component) in (0..Data::NUM_COMPONENTS).map(|i| (i, unsafe{ att_val.get_unchecked(i)})) {
            unsafe{
                if component < *self.global_metadata_min.get_unchecked(i) {
                    *self.global_metadata_min.get_unchecked_mut(i) = component;
                } else if component > *self.global_metadata_max.get_unchecked(i) {
                    *self.global_metadata_max.get_unchecked_mut(i) = component;
                }
            }
        }
    }
    fn portabilize(&mut self, att_val: Self::Data) -> Self::Output {
        let range = self.global_metadata_max - self.global_metadata_min;
        let scale = Data::Component::from_u64((1 << self.quantization_bits) as u64) - Data::Component::one();
        let normalized = (att_val - self.global_metadata_min).elem_div(range);

        (normalized * scale).cast()
    }
}