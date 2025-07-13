use std::result::Result;

use super::InversePredictionTransformImpl;
use crate::core::shared::{DataValue, NdVector, Vector}; 
use crate::decode::attribute::portabilization::{Deportabilization, DeportabilizationImpl};
use crate::encode::attribute::prediction_transform::geom::*;
use crate::prelude::ByteReader;
use crate::shared::attribute::Portable;

pub(crate) struct OctahedronReflectionInverseTransform<Data> 
    where Data: Vector + Portable,
{
    metadata: Data,
    deportabilization: Deportabilization<<Self as InversePredictionTransformImpl>::Correction>,
}

impl<Data> InversePredictionTransformImpl for OctahedronReflectionInverseTransform<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = Data;

    const ID: usize = 4;

    fn new<R>(reader: &mut R) -> Result<Self, crate::decode::attribute::inverse_prediction_transform::Err> 
        where R: ByteReader
    {
        let deportabilization = Deportabilization::new(reader)?;
        let metadata = Data::read_from(reader)?;
        Ok(
            Self {
                metadata,
                deportabilization,
            }
        )
    }

    fn inverse<R>(&self, mut pred: Self::Data, reader: &mut R) -> Self::Data 
        where R: ByteReader
    {
        let crr = self.deportabilization.deportabilize_next(reader);
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );

        let pred_lies_in_upper_half = unsafe {
            *pred.get_unchecked(2) > Data::Component::zero()
        };

        if pred_lies_in_upper_half {
            let minus_one = Data::Component::from_f64(-1.0);
            unsafe{ *pred.get_unchecked_mut(2) *= minus_one; }
        }

        let pred_in_oct = unsafe {
            octahedral_transform(pred)
        };

        let orig = pred_in_oct + crr;
        unsafe{
            if *pred.get_unchecked(2) < Data::Component::zero() {
                let minus_one = Data::Component::from_f64(-1.0);
                *pred.get_unchecked_mut(2) *= minus_one;
            }
        }

        // Safety:
        // We made sure that the data is three dimensional.
        unsafe {
            octahedral_inverse_transform(orig)
        }
    }
}