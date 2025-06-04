use std::vec::IntoIter;

use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::prelude::ByteWriter;
use crate::shared::attribute::Portable;

use super::Config;
use super::PortabilizationImpl;
use super::Resolution;
use crate::core::shared::Max;

#[cfg(feature = "evaluation")]
use crate::eval;

pub struct QuantizationRectangleArray<Data>
    where Data: Vector

{
    /// iterator over the attribute values.
    /// this is not 'Vec<_>' because we want nicely consume the data.
    att_vals: std::vec::IntoIter<Data>,

    /// the global metadata min.
    global_metadata_min: Data,

    /// 'global_metadata_max - global_metadata_min'
    /// This is precomputed to avoid recomputing it for each data.
    range: Data,

    /// the quantization size.
    /// Each component is of float type, though its value is an integer.
    quantization_size: Data,
}

impl<Data> QuantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    pub fn new<W>(att_vals: Vec<Data>, cfg: Config, writer: &mut W) -> Self 
        where W: ByteWriter
    {
        assert!(
            att_vals.len() > 1,
            "The number of attribute values must be greater than 1, but got {}",
            att_vals.len()
        );

        let mut global_metadata_min = att_vals[0];
        let mut global_metadata_max = att_vals[0];
        
        for att_val in &att_vals {
            for (i, &component) in (0..Data::NUM_COMPONENTS).map(|i| (i, unsafe{ att_val.get_unchecked(i)})) {
                unsafe {
                    if component < *global_metadata_min.get_unchecked(i) {
                        *global_metadata_min.get_unchecked_mut(i) = component;
                    } else if component > *global_metadata_max.get_unchecked(i) {
                        *global_metadata_max.get_unchecked_mut(i) = component;
                    }
                }
            }
        }

        // compute the range. This will be multiplied by 1.0001 to avoid the boundary value to overflow.
        let range = (global_metadata_max - global_metadata_min) * Data::Component::from_f64(1.0001);
        
        let unit_cube_size = match cfg.resolution {
            Resolution::UnitCubeSize(cube_size) => Data::Component::from_f64(cube_size),
            Resolution::DivisionSize(division_size) => {
                let mut min_diff = Data::Component::MAX_VALUE;
                for i in 0..Data::NUM_COMPONENTS {
                    // Safety: Obvious.
                    let diff_abs = unsafe { *range.get_unchecked(i) };
                    if diff_abs < min_diff && diff_abs > Data::Component::zero() {
                        min_diff = diff_abs;
                    }
                }
                min_diff / Data::Component::from_u64(division_size)
            }
        };

        // compute the quantization size
        let mut quantization_size = range / unit_cube_size;
        for i in 0..Data::NUM_COMPONENTS {
            // Safety: Obvious.
            unsafe { 
                *quantization_size.get_unchecked_mut(i) = Data::Component::from_f64(
                    quantization_size.get_unchecked(i).to_f64().ceil() + 1.0
                );
            };
        }


        // write metadata.
        // all the other information can be recovered on the decoder end.
        global_metadata_min.write_to(writer);
        global_metadata_max.write_to(writer);
        unit_cube_size.write_to(writer);

        #[cfg(feature = "evaluation")]
        {
            eval::write_json_pair("portabilization type", "QuantizationRectangleArray".into(), writer);
            eval::write_json_pair("global metadata min:", global_metadata_min.into(), writer);
            eval::write_json_pair("global metadata max:", global_metadata_max.into(), writer);
            eval::write_json_pair("global metadata range:", range.into(), writer);
            eval::write_json_pair("unit cube size:", unit_cube_size.into(), writer);
            eval::write_json_pair("quantization size:", quantization_size.into(), writer);
        }

        Self {
            global_metadata_min,
            range,
            quantization_size,
            att_vals: att_vals.into_iter(),
        }
    }
}

impl<Data> QuantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    fn linearize(&self, data: Data) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..Data::NUM_COMPONENTS {
            let component = *data.get(i);
            out.extend(component.to_bytes());
        }
        out
    }

    fn portabilize_next(&mut self) -> Vec<u8> {
        let att_val = self.att_vals.next().unwrap();
        let diff = att_val - self.global_metadata_min;
        let normalized = diff.elem_div(self.range);
        let val = normalized.elem_mul(self.quantization_size);

        self.linearize(val)
    }
}

impl<Data> PortabilizationImpl for QuantizationRectangleArray<Data> 
    where Data: Vector + Portable,
{
    fn portabilize(mut self) -> IntoIter<IntoIter<u8>> {
        let mut out = Vec::new();
        for _ in 0..self.att_vals.len() {
            out.push(self.portabilize_next().into_iter());
        }
        out.into_iter()
    }
}

 #[cfg(all(test, not(feature = "evaluation")))]
mod tests {
    use crate::{encode::attribute::portabilization::PortabilizationType, prelude::{FunctionalByteWriter, NdVector}};

    use super::*;
    // ToDo: Add tests
}