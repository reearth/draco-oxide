use std::mem;
use crate::{core::shared::DataValue, prelude::Vector, shared::attribute::Portable};
use super::DeportabilizationImpl;

pub(crate) struct DequantizationRectangleArray<Data> {
    bit_size: u8,
    encoded_with_single_u64: bool,
    global_metadata_min: Data,
    quantization_size: Data,
    range: Data,
}

impl<Data> DequantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    pub(crate) fn new<F>(stream_in: &mut F) -> Self 
        where F: FnMut(u8)->u64
    {
        let global_metadata_min = Data::read_from_bits(stream_in);
        let global_metadata_max = Data::read_from_bits(stream_in);
        let unit_cube_size = DataValue::from_bits(stream_in((mem::size_of::<Data::Component>()<<3) as u8));
        
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

        Self  {
            bit_size,
            encoded_with_single_u64,
            global_metadata_min,
            quantization_size,
            range
        }
    }

    fn delinearize<F>(&self, stream_in: &mut F) -> Data 
        where F: FnMut(u8) -> u64
    {
        if self.encoded_with_single_u64 {
            let mut data = stream_in(self.bit_size);
            let mut out = Data::zero();
            for i in (1..Data::NUM_COMPONENTS).rev() {
                let value = data % self.quantization_size.get(i).to_u64();
                // Safety: Obvious.
                unsafe {
                    data /= self.quantization_size.get_unchecked(i).to_u64();
                    *out.get_unchecked_mut(i) = Data::Component::from_u64(value);
                }
            }
            unsafe {
                *out.get_unchecked_mut(0) = Data::Component::from_u64(data);
            }
            out
        } else {
            let mut out = Data::zero();
            let size = (mem::size_of::<Data::Component>()<<3) as u8;
            for i in 0..Data::NUM_COMPONENTS {
                // Safety: Obvious.
                unsafe {
                    *out.get_unchecked_mut(i) = Data::Component::from_u64(stream_in(size));
                }
            }
            out
        }
    }
}

impl<Data> DeportabilizationImpl<Data> for DequantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    fn deportabilize_next<F>(&self, stream_in: &mut F) -> Data 
        where F: FnMut(u8) -> u64
    {
        let delinearized = self.delinearize(stream_in);
        let normalized = delinearized.elem_div(self.quantization_size);
        let diff = normalized.elem_mul(self.range);
        diff + self.global_metadata_min
    }
}


#[cfg(test)]
mod tests {
    use crate::core::buffer;
    use crate::core::shared::NdVector;
    use crate::decode::attribute::portabilization::Deportabilization;
    use crate::encode::attribute::portabilization::{Portabilization, PortabilizationImpl, PortabilizationType, Resolution}; 
    use crate::encode::attribute::portabilization::Config;
    use super::*;
    
    #[test]
    fn test_dequantization_rectangle_array() {
        let data = vec![
            NdVector::from([1_f32, -1.0, 1.0]),
            NdVector::from([0.7, 0.8, 0.9]),
            NdVector::from([0.0, 0.5, 0.0]),
            NdVector::from([0.5, 1.0, 0.0]),
        ];

        let cfg = Config {
            type_: PortabilizationType::QuantizationRectangleArray,
            resolution: Resolution::DivisionSize(1000),
        };

        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        Portabilization::new(data.clone(), cfg, &mut writer)
            .portabilize()
            .into_iter()
            .for_each(|x| x.write(&mut writer));

        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |size| buff_reader.next(size);
        let dequant = Deportabilization::new(&mut reader).unwrap();
        for i in 0..data.len() {
            let dequant_data: NdVector<3,f32> = dequant.deportabilize_next(&mut reader);
            let err = (dequant_data-data[i]).norm();
            assert!(
                err < 1e-2,
                "Err too large ({err}). Dequantization failed: expected {:?}, got {:?}",
                data[i], dequant_data
            );
        }
    }
}