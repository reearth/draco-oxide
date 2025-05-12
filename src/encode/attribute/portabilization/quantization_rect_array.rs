use std::mem;

use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;
use crate::shared::attribute::Portable;

use super::Config;
use super::PortabilizationImpl;
use super::Resolution;
use crate::core::shared::Max;

pub struct QuantizationRectangleArray<Data>
    where Data: Vector

{
    /// whether or not a single data can be encoded with a single u64. If not, then
    /// 'writer(data)' will be called 'Data::NUM_COMPONENTS' times.
    encoded_with_single_u64: bool,

    /// the bit size of the data, used only if 'encoded_with_single_u64' is true.
    bit_size: u8,

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
    pub fn new<F>(att_vals: Vec<Data>, cfg: Config, writer: &mut F) -> Self 
        where F:FnMut((u8, u64)) 
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

        let unit_cube_size = match cfg.resolution {
            Resolution::UnitCubeSize(cube_size) => Data::Component::from_f64(cube_size),
            Resolution::DivisionSize(division_size) => {
                let diff = global_metadata_max - global_metadata_min;
                let mut min_diff = Data::Component::MAX_VALUE;
                for i in 0..Data::NUM_COMPONENTS {
                    // Safety: Obvious.
                    let diff_abs = unsafe { *diff.get_unchecked(i) };
                    if diff_abs < min_diff {
                        min_diff = diff_abs;
                    }
                }
                min_diff / Data::Component::from_u64(division_size)
            }
        };

        // compute the range. This will be multiplied by 1.0001 to avoid the boundary value to overflow.
        let range = (global_metadata_max - global_metadata_min) * Data::Component::from_f64(1.0001);
        // compute the quantization size
        let mut quantization_size = range / unit_cube_size;
        for i in 0..Data::NUM_COMPONENTS {
            // Safety: Obvious.
            unsafe { 
                *quantization_size.get_unchecked_mut(i) = Data::Component::from_f64(
                    quantization_size.get_unchecked(i).to_f64().ceil()
                );
            };
        }


        // whether or not a single data can be encoded with a single u64. If not, then 
        // 'writer(data)' will be called 'Data::NUM_COMPONENTS' times.
        let mut size: u64 = 1;
        let encoded_with_single_u64 = {
            let mut out = true;
            for i in 0..Data::NUM_COMPONENTS {
                // Safety: Obvious.
                let new_size = size.checked_mul(unsafe { *quantization_size.get_unchecked(i) }.to_u64());
                size = if let Some(size) = new_size {
                    size 
                } else {
                    out = false;
                    break;
                };
            }
            out
        };
        let bit_size = (0..)
            .find(|&x| (1 << x) > size)
            .unwrap();

        // write metadata.
        // all the other information can be recovered on the decoder end.
        WritableFormat::from_vec(global_metadata_min.to_bits()).write(writer);
        WritableFormat::from_vec(global_metadata_max.to_bits()).write(writer);
        writer(unit_cube_size.to_bits());



        Self {
            encoded_with_single_u64,
            bit_size,
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
    fn linearize(&self, data: Data) -> WritableFormat {
        if self.encoded_with_single_u64 {
            // Safety: Obvious. (NUM_COMPONENTS > 0)
            let mut val = unsafe{ *data.get_unchecked(0) }.to_u64();
            for i in 1..Data::NUM_COMPONENTS {
                // Safety: Obvious.
                let component = unsafe{ *data.get_unchecked(i) }.to_u64();
                debug_assert!(
                    component < self.quantization_size.get(i).to_u64(),
                    "component {} is out of range {}",  
                    component,
                    self.quantization_size.get(i).to_u64()
                );
                // Safety: Obvious.
                let offset = unsafe{ self.quantization_size.get_unchecked(i) }.to_u64();
                // the multiplication will never overflow because we checked it before.
                // see the computation of 'encoded_with_single_u64'.
                val *= offset;
                val += component;
            }

            WritableFormat::from_vec(vec![(self.bit_size, val)])
        } else {
            let mut out = Vec::new();
            let size = (mem::size_of::<Data::Component>()<<3) as u8;
            for i in 0..Data::NUM_COMPONENTS {
                let component = *data.get(i);
                out.push((size, component.to_u64()));
            }
            WritableFormat::from_vec(out)
        }
    }

    fn portabilize_next(&mut self) -> WritableFormat {
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
    fn portabilize(mut self) -> std::vec::IntoIter<WritableFormat> {
        let mut out = Vec::new();
        for _ in 0..self.att_vals.len() {
            out.push(self.portabilize_next());
        }
        out.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{encode::attribute::portabilization::PortabilizationType, prelude::NdVector};

    use super::*;

    #[test]
    fn portabilize_all() {
        let data = vec![
            NdVector::from([1_f32, -1.0, 1.0]),
            NdVector::from([0.0, 0.5, 0.0]),
            NdVector::from([0.7, 0.8, 0.9]),
            NdVector::from([0.5, 1.0, 0.0]),
        ];

        let cfg = Config {
            type_: PortabilizationType::QuantizationRectangleArray,
            resolution: Resolution::DivisionSize(10),
        };

        let mut metadata = vec![
            // min
            <f32 as DataValue>::to_bits(0_f32),
            <f32 as DataValue>::to_bits(-1_f32),
            <f32 as DataValue>::to_bits(0_f32),

            // max
            <f32 as DataValue>::to_bits(1_f32),
            <f32 as DataValue>::to_bits(1_f32),
            <f32 as DataValue>::to_bits(1_f32),

            // division size
            <f32 as DataValue>::to_bits(0.1_f32),

            // data
            (12, 10*11*21 +  0*11 + 10),
            (12,  0*11*21 + 15*11 +  0),
            (12,  7*11*21 + 18*11 +  9),
            (12,  5*11*21 + 20*11 +  0),

        ].into_iter();

        let mut writer = |input| {
            assert_eq!(input, metadata.next().unwrap());
        };

        QuantizationRectangleArray::new(data, cfg, &mut writer)
            .portabilize().into_iter().for_each(|w|
                w.write(&mut writer)
            );
    }

    // ToDo: Test the case where the data is too big to fit in a single u64.
}