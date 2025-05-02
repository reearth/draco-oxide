use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;
use crate::shared::attribute::Portable;

use super::PortabilizationImpl;

pub struct QuantizationRectangleArray<Data> 
{
    global_metadata_min: Data,
    global_metadata_max: Data,
    quantization_size: Vec<usize>,
    unit_cube_size: f64,
}

impl<Data> QuantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    pub fn new(unit_cube_size: f64) -> Self {
        Self {
            global_metadata_min: Data::zero(), // random data
            global_metadata_max: Data::zero(), // random data
            unit_cube_size,
            quantization_size: Vec::new(),
        }
    }

    fn metadata_into_writable_format(&self) -> WritableFormat {
        let mut out = WritableFormat::new();
        out.append(&mut WritableFormat::from_vec(self.global_metadata_min.to_bits()));
        out.append(&mut WritableFormat::from_vec(self.global_metadata_max.to_bits()));
        for &x in &self.quantization_size {
            out.push((8, x as u64));
        }
        WritableFormat::from(out)
    }

    fn linearize(&self, data: Vec<Data>) -> WritableFormat {
        let size = self.quantization_size.iter().map(|&s|s as u8).product::<u8>();
        let out = data.into_iter()
            .map(|x| {
                let mut val = 0;
                for i in 0..Data::NUM_COMPONENTS {
                    let component = *x.get(i);
                    let max_component = self.quantization_size[i];
                    val += component.to_u64() * max_component as u64;
                }
                (size, val)
            })
            .collect::<Vec<_>>();

        WritableFormat::from_vec(out)
    }
}

impl<Data> PortabilizationImpl for QuantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
{
    type Data = Data;
    const PORTABILIZATION_ID: usize = 1;

    fn portabilize(&mut self, att_vals: Vec<Self::Data>) -> (WritableFormat, WritableFormat) {
        for att_val in &att_vals {
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
        
        let mut out = Vec::new();
        out.reserve(att_vals.len());
        for att_val in att_vals {
            let range = self.global_metadata_max - self.global_metadata_min;
            let diff = att_val - self.global_metadata_min;
            let normalized = diff.elem_div(range);
            let mut scale = Data::zero();
            for i in 0..Data::NUM_COMPONENTS {
                *scale.get_mut(i) = <Data as Vector>::Component::from_u64(self.quantization_size[i] as u64 - 1);
            }
            let val = normalized.elem_mul(scale);

            out.push(val);
        }

        (self.linearize(out), self.metadata_into_writable_format())
    }    
}