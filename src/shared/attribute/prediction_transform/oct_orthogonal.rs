use crate::core::shared::{DataValue, NdVector, Vector};
use super::geom::*;

use super::{FinalMetadata, PredictionTransform};

pub struct OctahedronOrthogonalTransform<Data> {
    _out: Vec<NdVector<2,f64>>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> OctahedronOrthogonalTransform<Data> 
    where Data: Vector
{
    pub fn new() -> Self {
        Self {
            _out: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> PredictionTransform for OctahedronOrthogonalTransform<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    const ID: usize = 3;

    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = ();

    fn map(_orig: Self::Data, _pred: Self::Data, _: Self::Metadata) -> Self::Correction {
        unimplemented!()
    }

    fn map_with_tentative_metadata(&mut self, mut orig: Self::Data, mut pred: Self::Data) {
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );

        unsafe {
            if *pred.get_unchecked(2) < Data::Component::zero() {
                let minus_one = Data::Component::from_f64(-1.0);
                *pred.get_unchecked_mut(2) *= minus_one;
                *orig.get_unchecked_mut(2) *= minus_one;
            }

            if *pred.get_unchecked(0) > Data::Component::zero() {
                if *pred.get_unchecked(1) > Data::Component::zero() {
                    // first quadrant. Rotate around z-axis by pi.
                    let minus_one = Data::Component::from_f64(-1.0);
                    *pred.get_unchecked_mut(0) *= minus_one;
                    *pred.get_unchecked_mut(1) *= minus_one;
                    *orig.get_unchecked_mut(0) *= minus_one;
                    *orig.get_unchecked_mut(1) *= minus_one;
                } else {
                    // fourth quadrant. Rotate around z-axis by -pi/2.
                    let temp = *pred.get_unchecked(0);
                    let one = Data::Component::one();
                    let minus_one = Data::Component::zero() - one;
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1);
                    *pred.get_unchecked_mut(1) = temp * minus_one;
                    *orig.get_unchecked_mut(0) = *orig.get_unchecked(1);
                    *orig.get_unchecked_mut(1) = temp * minus_one;
                }
            } else {
                if *pred.get_unchecked(1) > Data::Component::zero() {
                    // second quadrant. Rotate around z-axis by pi/2.
                    let temp = *pred.get_unchecked(0);
                    let one = Data::Component::one();
                    let minus_one = Data::Component::zero() - one;
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1) * minus_one;
                    *pred.get_unchecked_mut(1) = temp;
                    *orig.get_unchecked_mut(0) = *orig.get_unchecked(1) * minus_one;
                    *orig.get_unchecked_mut(1) = temp;
                }
                // third quadrant will not be transformed.
            };
        }

        let orig = unsafe{ octahedral_transform(orig) };
        let pred = unsafe { octahedral_transform(pred) };
        self._out.push( orig - pred );
    }

    fn inverse(&mut self, mut pred: Self::Data, crr: Self::Correction, _: Self::Metadata) -> Self::Data {
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );
        
        let mut reflected = false;
        let mut quadrant = 0;
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

    fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>) {
        (
            FinalMetadata::Global(()), 
            std::mem::take(&mut self._out)
        )
    }
}
