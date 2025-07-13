use crate::{core::shared::DataValue, prelude::{ByteReader, Vector}, shared::attribute::Portable};
use super::DeportabilizationImpl;

pub(crate) struct ToBits<Data> {
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> ToBits<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    pub(crate) fn new<R>(_reader: &mut R) -> Self 
        where R: ByteReader
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
    fn deportabilize_next<R>(&self, reader: &mut R) -> Data 
        where R: ByteReader
    {
        Data::read_from(reader).unwrap() // TODO: handle error properly
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