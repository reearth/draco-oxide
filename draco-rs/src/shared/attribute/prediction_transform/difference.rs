use crate::core::shared::Vector;

use super::{FinalMetadata, PredictionTransform};
use crate::core::shared::Max;


pub struct Difference<Data> {
    _out: Vec<Data>,
    _metadata: Data,
}

impl<Data> Difference<Data> 
    where Data: Vector
{
    pub fn new() -> Self {
        let mut _metadata = Data::zero();
        for i in 0..Data::NUM_COMPONENTS {
            // Safety:
            // iterating over a constant-sized array
            unsafe{
                *_metadata.get_unchecked_mut(i) = Data::Component::MAX_VALUE;
            }
        }
        Self {
            _out: Vec::new(),
            _metadata,
        }
    }
}

impl<Data> PredictionTransform for Difference<Data> 
    where Data: Vector
{
    const ID: usize = 1;

    type Data = Data;
    type Correction = Data;
    type Metadata = Data;

    fn map(orig: Self::Data, pred: Self::Data, metadata: Self::Metadata) -> Self::Correction {
        orig - pred - metadata
    }

    fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data) {
        let corr = orig - pred;
        self._out.push(corr);
        // update metadata
        for i in 0..Data::NUM_COMPONENTS {
            unsafe{
                if self._metadata.get_unchecked(i) > corr.get_unchecked(i) {
                    *self._metadata.get_unchecked_mut(i) = *corr.get_unchecked(i);
                }
            }
        }
    }

    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction, metadata: Self::Metadata) -> Self::Data {
        pred + crr + metadata
    }

    fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>) {
        self._out.iter_mut()
            .for_each(|v|
                *v -= self._metadata
            );
        (
            FinalMetadata::Global(self._metadata), 
            std::mem::take(&mut self._out)
        )
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::shared::NdVector;

    #[test]
    fn test_transform() {
        let mut transform = Difference::<NdVector<3, f64>>::new();
        let orig1 = NdVector::<3, f64>::from([1.0, 2.0, 3.0]);
        let pred1 = NdVector::<3, f64>::from([1.0, 1.0, 1.0]);
        let orig2 = NdVector::<3, f64>::from([4.0, 5.0, 6.0]);
        let pred2 = NdVector::<3, f64>::from([5.0, 5.0, 5.0]);
        
        transform.map_with_tentative_metadata(orig1.clone(), pred1.clone());
        transform.map_with_tentative_metadata(orig2.clone(), pred2.clone());

        let (final_metadata, corrections) = transform.squeeze();
        let final_metadata = match final_metadata {
            FinalMetadata::Local(_) => panic!("Expected global metadata"),
            FinalMetadata::Global(m) => m,
        };
        let metadata = NdVector::<3, f64>::from([-1.0, 0.0, 1.0]);
        assert_eq!(final_metadata, metadata);
        let recovered1 = transform.inverse(pred1.clone(), corrections[0], final_metadata);
        let recovered2 = transform.inverse(pred2.clone(), corrections[1], final_metadata);
        assert_eq!(recovered1, orig1);
        assert_eq!(recovered2, orig2);
    }
}