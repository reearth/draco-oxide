use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;

use super::PortabilizationImpl;

pub struct QuantizationRectangleSpiral<Data>
    where Data: Vector,
{
    center: Data,
    radius: Data::Component,
}

impl<Data> QuantizationRectangleSpiral<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    pub fn new() -> Self {
        Self {
            center: Data::zero(), 
            radius: Data::Component::zero(),
        }
    }

    fn metadata_into_writable_format(&self) -> WritableFormat {
        unimplemented!()

        // let mut out = Vec::new();
        // out.reserve(Data::NUM_COMPONENTS * 2);
        // for i in 0..Data::NUM_COMPONENTS {
        //     out.push((64, self.center.get(i).to_bits() as usize));
        // }
        // out.push((64, self.radius.to_bits() as usize));
        // for i in 0..Data::NUM_COMPONENTS {
        //     out.push((32, self.quantization_size[i]));
        // }
        // WritableFormat::from(out)
    }

    fn linearize(&self, data: Vec<Data>, center: Data) -> WritableFormat {
        unimplemented!()
        // let size = self.quantization_size.iter().product::<usize>();
        // let out = data.into_iter()
        //     .map(|x| {
        //         let mut val = 0;
        //         for i in 0..Data::NUM_COMPONENTS {
        //             let component = *x.get(i);
        //             let max_component = self.quantization_size[i];
        //             val += component.to_u64() as usize * max_component;
        //         }
        //         (size, val)
        //     })
        //     .collect::<Vec<_>>();

        // WritableFormat::from(out)
    }
}

impl<Data> PortabilizationImpl for QuantizationRectangleSpiral<Data> 
    where 
        Data: Vector + Copy,
{
    type Data = Data;
    const PORTABILIZATION_ID: usize = 1;

    fn portabilize(&mut self, att_vals: Vec<Self::Data>) -> (WritableFormat, WritableFormat) {
        unimplemented!()
        // let mut global_metadata_min = Data::zero();
        // for (i, &component) in (0..Data::NUM_COMPONENTS).map(|i| (i, unsafe{ att_vals[0].get_unchecked(i)})) {
        //     unsafe{
        //         *global_metadata_min.get_unchecked_mut(i) = component;
        //     }
        // }
        // let mut global_metadata_max = global_metadata_min;
        // for att_val in &att_vals {
        //     for (i, &component) in (0..Data::NUM_COMPONENTS).map(|i| (i, unsafe{ att_val.get_unchecked(i)})) {
        //         unsafe{
        //             if component < *global_metadata_min.get_unchecked(i) {
        //                 *global_metadata_min.get_unchecked_mut(i) = component;
        //             } else if component > *global_metadata_max.get_unchecked(i) {
        //                 *global_metadata_max.get_unchecked_mut(i) = component;
        //             }
        //         }
        //     }
        // }
        
        // let mut out = Vec::new();
        // out.reserve(att_vals.len());
        // for att_val in att_vals {
        //     let range = global_metadata_max - global_metadata_min;
        //     let diff = att_val - global_metadata_min;
        //     let normalized = diff.elem_div(range);
        //     let mut scale = Data::zero();
        //     for i in 0..Data::NUM_COMPONENTS {
        //         *scale.get_mut(i) = <Data as Vector>::Component::from_u64(quantization_size[i] as u64 - 1);
        //     }
        //     let val = normalized.elem_mul(scale);

        //     out.push(val);
        // }

        // (self.linearize(out), self.metadata_into_writable_format())
    }
}