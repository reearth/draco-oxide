use crate::core::shared::Vector;

use super::PredictionInverseTransformImpl;


pub(crate) struct DifferenceInverseTransform<Data> {
    metadata: Data,
}

impl<Data> DifferenceInverseTransform<Data> 
where Data: Vector
{
    /// Create a new instance of the `Difference` struct
    pub fn new() -> Self {
        Self {
            metadata: Data::zero(),
        }
    }
}

impl<Data> PredictionInverseTransformImpl for DifferenceInverseTransform<Data> 
    where Data: Vector
{
    type Data = Data;
    type Correction = Data;
    type Metadata = Data;

    const ID: usize = 1;

    fn init(&mut self, metadata: Self::Metadata) {
        // Initialize the metadata
        self.metadata = metadata;
    }

    /// Inverse the prediction using the difference method
    /// `pred + crr + metadata`
    ///
    /// # Arguments
    /// * `pred` - The predicted value
    /// * `crr` - The correction value
    /// * `metadata` - The metadata value
    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction) -> Self::Data {
        pred + crr + self.metadata
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
//     use crate::decode::attribute::prediction_inverse_transform::PredictionInverseTransformImpl;
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