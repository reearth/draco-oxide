use crate::{core::shared::DataValue, prelude::{ByteReader, Vector}, shared::attribute::Portable};
use super::DeportabilizationImpl;

pub(crate) struct DequantizationRectangleArray<Data> {
    global_metadata_min: Data,
    quantization_size: Data,
    range: Data,
}

impl<Data> DequantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    pub(crate) fn new<R>(reader: &mut R) -> Self 
        where R: ByteReader
    {
        let global_metadata_min = Data::read_from(reader).unwrap(); // TODO: handle error properly
        let global_metadata_max = Data::read_from(reader).unwrap(); // TODO: handle error properly
        let unit_cube_size = Data::Component::read_from(reader).unwrap(); // TODO: handle error properly;
        
        // compute the range. This will be multiplied by 1.0001 to avoid the boundary value to overflow.
        let range = (global_metadata_max - global_metadata_min) * Data::Component::from_f64(1.0001);
        // compute the quantization size
        let mut quantization_size = range / unit_cube_size;
        for i in 0..Data::NUM_COMPONENTS {
            // Safety: Obvious.
            unsafe { 
                *quantization_size.get_unchecked_mut(i) = Data::Component::from_f64(
                    quantization_size.get_unchecked(i).to_f64().ceil()  + 1.0
                );
            };
        }

        Self  {
            global_metadata_min,
            quantization_size,
            range
        }
    }

    fn delinearize<R>(&self, reader: &mut R) -> Data 
        where R: ByteReader
    {
        Data::read_from(reader).unwrap() // TODO: handle error properly
    }
}

impl<Data> DeportabilizationImpl<Data> for DequantizationRectangleArray<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    fn deportabilize_next<R>(&self, reader: &mut R) -> Data 
        where R: ByteReader
    {
        let delinearized = self.delinearize(reader);
        let normalized = delinearized.elem_div(self.quantization_size);
        let diff = normalized.elem_mul(self.range);
        diff + self.global_metadata_min
    }
}


#[cfg(all(test, not(feature = "evaluation")))]
mod tests {
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

        let mut writer = Vec::new();
        Portabilization::new(data.clone(), cfg, &mut writer)
            .portabilize()
            .into_iter()
            .for_each(|x| x.for_each(|d| d.write_to(&mut writer)));

        let mut reader = writer.into_iter();
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