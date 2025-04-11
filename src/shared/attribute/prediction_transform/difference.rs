use crate::core::shared::Vector;

use super::{FinalMetadata, PredictionTransform};


pub struct Difference<Data> {
    out: Vec<Data>,
    metadata: Data,
}

impl<Data> Difference<Data> 
    where Data: Vector
{
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            metadata: Data::zero(),
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
        self.out.push(corr);
        // update metadata
        for i in 0..Data::NUM_COMPONENTS {
            unsafe{
                if self.metadata.get_unchecked(i) > corr.get_unchecked(i) {
                    *self.metadata.get_unchecked_mut(i) = *corr.get_unchecked(i);
                }
            }
        }
    }

    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction, metadata: Self::Metadata) -> Self::Data {
        pred + crr + metadata
    }

    fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>) {
        self.out.iter_mut()
            .for_each(|v|
                *v -= self.metadata
            );
        (
            FinalMetadata::Global(self.metadata), 
            std::mem::take(&mut self.out)
        )
    }
}