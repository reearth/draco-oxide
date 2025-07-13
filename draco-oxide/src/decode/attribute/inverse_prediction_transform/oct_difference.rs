use super::InversePredictionTransformImpl;
use crate::decode::attribute::portabilization::{Deportabilization, DeportabilizationImpl};
use crate::encode::attribute::prediction_transform::geom::{
    octahedral_inverse_transform, 
    octahedral_transform
};
use crate::core::shared::{
    NdVector, Vector
};
use crate::prelude::ByteReader;

pub struct OctahedronDifferenceInverseTransform<Data> 
    where Data: Vector,
{
    _marker: std::marker::PhantomData<Data>,
    deportabilization: Deportabilization<<Self as InversePredictionTransformImpl>::Correction>,
}

impl<Data> InversePredictionTransformImpl for OctahedronDifferenceInverseTransform<Data> 
where
    Data: Vector,
{
    const ID: usize = 2;

    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = ();

    fn new<R>(reader: &mut R) -> Result<Self, super::Err>
        where R: ByteReader
    {
        let deportabilization = Deportabilization::new(reader)?;
        Ok(
            Self {
                _marker: std::marker::PhantomData,
                deportabilization,
            }
        )
    }

    fn inverse<R>(&self, pred: Self::Data, reader: &mut R) -> Self::Data 
        where R: ByteReader
    {
        let crr = self.deportabilization.deportabilize_next(reader);
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );

        let pred_in_oct = unsafe {
            octahedral_transform(pred)
        };

        let orig = pred_in_oct + crr;

        // Safety:
        // We made sure that the data is three dimensional.
        unsafe {
            octahedral_inverse_transform(orig)
        }
    }
}


// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::core::shared::NdVector;
//     use crate::encode::attribute::portabilization;
//     use crate::encode::attribute::prediction_transform::oct_difference::OctahedronDifferenceTransform;
//     use crate::encode::attribute::prediction_transform::PredictionTransformImpl;
//     use crate::core::shared::ConfigType;

//     #[test]
//     fn test_transform() {
//         let mut transform = OctahedronDifferenceTransform::<NdVector<3, f64>>::new(portabilization::Config::default());
//         let orig1 = NdVector::<3, f64>::from([1.0, 2.0, 3.0]).normalize();
//         let pred1 = NdVector::<3, f64>::from([1.0, 1.0, 1.0]).normalize();
//         let orig2 = NdVector::<3, f64>::from([4.0, 5.0, 6.0]).normalize();
//         let pred2 = NdVector::<3, f64>::from([5.0, 5.0, 5.0]).normalize();
        
//         transform.map_with_tentative_metadata(orig1.clone(), pred1.clone());
//         transform.map_with_tentative_metadata(orig2.clone(), pred2.clone());

//         transform.squeeze();
//         let mut inverse = OctahedronDifferenceInverseTransform::<NdVector<3, f64>>::new();
//         let recovered1 = inverse.inverse(pred1.clone(), transform.get_corr()[0]);
//         let recovered2 = inverse.inverse(pred2.clone(), transform.get_corr()[1]);
//         assert!((recovered1 - orig1).norm() < 0.000_000_1);
//         assert!((recovered2 - orig2).norm() < 0.000_000_1);
//     }
// }