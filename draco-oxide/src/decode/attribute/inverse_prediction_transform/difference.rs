use crate::core::shared::Vector; 
use crate::decode::attribute::portabilization::{
    Deportabilization, 
    DeportabilizationImpl
};
use crate::prelude::ByteReader;
use crate::shared::attribute::Portable;

use super::InversePredictionTransformImpl;


pub(crate) struct DifferenceInverseTransform<Data> 
    where Data: Vector + Portable
{
    metadata: Data,
    deportabilization: Deportabilization<<Self as InversePredictionTransformImpl>::Correction>,
}

impl<Data> InversePredictionTransformImpl for DifferenceInverseTransform<Data> 
    where Data: Vector + Portable
{
    type Data = Data;
    type Correction = Data;
    type Metadata = Data;

    const ID: usize = 1;

    fn new<R>(reader: &mut R) -> Result<Self, super::Err> 
        where R: ByteReader
    {
        let metadata = Data::read_from(reader).unwrap(); // TODO: handle error properly
        let deportabilization = Deportabilization::new(reader)?;
        Ok (
            Self {
                metadata,
                deportabilization,
             }
        )
    }

    fn inverse<R>(
        &self,
        pred: Self::Data,
        reader: &mut R,
    ) -> Self::Data 
        where R: ByteReader
    {
        let corr = self.deportabilization.deportabilize_next(reader);
        pred + corr + self.metadata
    }
}


// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::core::shared::NdVector;
//     use crate::encode::attribute::portabilization;
//     use crate::encode::attribute::prediction_transform::FinalMetadata;
//     use crate::encode::attribute::prediction_transform::{
//         self,
//         PredictionTransformImpl
//     };
//     use crate::decode::attribute::prediction_inverse_transform::InversePredictionTransformImpl;
//     use crate::core::shared::ConfigType;

//     #[test]
//     fn test_transform() {
//         let mut transform = prediction_transform::difference::Difference::<NdVector<3, f64>>::new(portabilization::Config::default());
//         let orig1 = NdVector::<3, f64>::from([1.0, 2.0, 3.0]);
//         let pred1 = NdVector::<3, f64>::from([1.0, 1.0, 1.0]);
//         let orig2 = NdVector::<3, f64>::from([4.0, 5.0, 6.0]);
//         let pred2 = NdVector::<3, f64>::from([5.0, 5.0, 5.0]);
        
//         transform.map_with_tentative_metadata(orig1.clone(), pred1.clone());
//         transform.map_with_tentative_metadata(orig2.clone(), pred2.clone());

//         transform.squeeze();
//         let final_metadata = match transform.get_final_metadata() {
//             FinalMetadata::Local(_) => panic!("Expected global metadata"),
//             FinalMetadata::Global(m) => m,
//         };
//         let metadata = NdVector::<3, f64>::from([-1.0, 0.0, 1.0]);
//         assert_eq!(final_metadata, &metadata);

//         let mut inverse = DifferenceInverseTransform::<NdVector<3, f64>>::new();
//         inverse.init(*final_metadata);
//         let recovered1 = inverse.inverse(pred1.clone(), transform.get_corr_as_slice()[0]);
//         let recovered2 = inverse.inverse(pred2.clone(), transform.get_corr_as_slice()[1]);
//         assert_eq!(recovered1, orig1);
//         assert_eq!(recovered2, orig2);
//     }
// }