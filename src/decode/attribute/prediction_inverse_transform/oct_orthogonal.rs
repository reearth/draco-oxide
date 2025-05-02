use super::PredictionInverseTransformImpl;
use crate::core::shared::{DataValue, NdVector, Vector};
use crate::encode::attribute::prediction_transform::geom::*;

pub(crate) struct OctrahedronOrthogonalInverseTransform<Data> {
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> PredictionInverseTransformImpl for OctrahedronOrthogonalInverseTransform<Data> 
where
    Data: Vector,
    Data::Component: DataValue
{
    const ID: usize = 3;

    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = ();

    fn init(&mut self, _metadata: Self::Metadata) {}

    fn inverse(&mut self, mut pred: Self::Data, crr: Self::Correction) -> Self::Data {
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );
        
        let mut reflected = false;
        let quadrant;
        unsafe {
            if *pred.get_unchecked(2) < Data::Component::zero() {
                reflected = true;
                let minus_one = Data::Component::from_f64(-1.0);
                *pred.get_unchecked_mut(2) *= minus_one;
            }

            if *pred.get_unchecked(0) > Data::Component::zero() {
                if *pred.get_unchecked(1) > Data::Component::zero() {
                    // first quadrant. Rotate around z-axis by pi.
                    quadrant = 1;
                    let minus_one = Data::Component::from_f64(-1.0);
                    *pred.get_unchecked_mut(0) *= minus_one;
                    *pred.get_unchecked_mut(1) *= minus_one;
                } else {
                    // fourth quadrant. Rotate around z-axis by -pi/2.
                    quadrant = 4;
                    let temp = *pred.get_unchecked(0);
                    let one = Data::Component::one();
                    let minus_one = Data::Component::zero() - one;
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1);
                    *pred.get_unchecked_mut(1) = temp * minus_one;
                }
            } else {
                if *pred.get_unchecked(1) > Data::Component::zero() {
                    // second quadrant. Rotate around z-axis by pi/2.
                    quadrant = 2;
                    let temp = *pred.get_unchecked(0);
                    let one = Data::Component::one();
                    let minus_one = Data::Component::zero() - one;
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1) * minus_one;
                    *pred.get_unchecked_mut(1) = temp;
                } else {
                    // third quadrant will not be transformed.
                    quadrant = 3;
                }
            };
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
        let mut orig: Data = unsafe {
            octahedral_inverse_transform(orig)
        };

        // pull back the transformation
        // Safety:
        // We made sure that the data is three dimensional.
        unsafe {
            if quadrant == 1 {
                let minus_one = Data::Component::from_f64(-1.0);
                *orig.get_unchecked_mut(0) *= minus_one;
                *orig.get_unchecked_mut(1) *= minus_one;
            } else if quadrant == 2 {
                // rotate around z-axis by -pi/2
                let temp = *orig.get_unchecked(0);
                let one = Data::Component::one();
                let minus_one = Data::Component::zero() - one;
                *orig.get_unchecked_mut(0) = *orig.get_unchecked(1) * minus_one;
                *orig.get_unchecked_mut(1) = temp;
            } else if quadrant == 4 {
                // rotate around z-axis by pi/2
                let temp = *orig.get_unchecked(0);
                let one = Data::Component::one();
                let minus_one = Data::Component::zero() - one;
                *orig.get_unchecked_mut(0) = *orig.get_unchecked(1);
                *orig.get_unchecked_mut(1) = temp * minus_one;
            }

            if reflected {
                let minus_one = Data::Component::from_f64(-1.0);
                *orig.get_unchecked_mut(2) *= minus_one;
            }
        }

        orig
    }
}