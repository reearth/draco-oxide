use std::mem;
use crate::{core::shared::DataValue, encode::attribute::WritableFormat, prelude::Vector, shared::attribute::Portable};
use super::DeportabilizationImpl;

pub(crate) struct ToBits<Data> {
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> ToBits<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    pub(crate) fn new<F>(_stream_in: &mut F) -> Self 
        where F: FnMut(u8)->u64
    {
        // there is no metadata to read.
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> DeportabilizationImpl<Data> for ToBits<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    fn deportabilize_next<F>(&self, stream_in: &mut F) -> Data 
        where F: FnMut(u8) -> u64
    {
        Data::read_from_bits(stream_in)
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
    fn test() {
        let data = vec![
            NdVector::from([1_f32, -1.0, 1.0]),
            NdVector::from([0.7, 0.8, 0.9]),
            NdVector::from([0.0, 0.5, 0.0]),
            NdVector::from([0.5, 1.0, 0.0]),
        ];

        let cfg = Config {
            type_: PortabilizationType::ToBits,
            resolution: Resolution::DivisionSize(1), // does not matter
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