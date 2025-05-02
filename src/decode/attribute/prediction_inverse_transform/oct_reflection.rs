use super::PredictionInverseTransformImpl;
use crate::{core::shared::{DataValue, NdVector, Vector}, encode::attribute::prediction_transform::geom::*};

pub(crate) struct OctahedronReflectionInverseTransform<Data> {
    metadata: Data,
}

impl <Data> OctahedronReflectionInverseTransform<Data> 
where 
    Data: Vector,
{
    pub fn new() -> Self {
        Self {
            metadata: Data::zero(),
        }
    }
}

impl<Data> PredictionInverseTransformImpl for OctahedronReflectionInverseTransform<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = Data;

    const ID: usize = 4;

    fn init(&mut self, metadata: Self::Metadata) {
        self.metadata = metadata;
    }

    fn inverse(&mut self, mut pred: Self::Data, crr: Self::Correction) -> Self::Data {
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