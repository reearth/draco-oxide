use std::mem;

use crate::core::buffer::writer::Writer;
use crate::core::buffer::MsbFirst;
use crate::core::shared::DataValue;
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
    pub fn new(quantization_bits: u8) -> Self {
        Self {
            global_metadata_min: Data::zero(), // random data
            global_metadata_max: Data::zero(), // random data
            quantization_bits
        }
    }
}

impl<Data> Portabilization for Quantization<Data> 
    where 
        Data: Vector + Copy,
{
    type Data = Data;
    fn skim(&mut self, att_vals: &[Self::Data]) {
        for att_val in att_vals {
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
    }
    fn portabilize_and_encode(&mut self, att_val: Self::Data, buffer: &mut Writer<MsbFirst>)  {
        let range = self.global_metadata_max - self.global_metadata_min;
        let diff = att_val - self.global_metadata_min;
        let normalized = diff.elem_div(range);
        let scale = <Data as Vector>::Component::from_u64((1 << self.quantization_bits) as u64)
             - <Data as Vector>::Component::one();
        let val = normalized * scale;


        let size = mem::size_of::<Data::Component>() << 3;
        for i in 0..Data::NUM_COMPONENTS {
            unsafe {
                buffer.next((size, val.get_unchecked(i).to_u64() as usize));
            }
        }
    }
}