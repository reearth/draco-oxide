use super::InversePredictionTransformImpl;
use crate::core::shared::{DataValue, NdVector, Vector};
use crate::decode::attribute::portabilization::Deportabilization;
use crate::encode::attribute::prediction_transform::geom::*;
use crate::shared::attribute::Portable;

pub(crate) struct OctahedronOrthogonalInverseTransform<Data> 
    where Data: Vector + Portable,
{
    deportabilization: Deportabilization<<Self as InversePredictionTransformImpl>::Correction>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> InversePredictionTransformImpl for OctahedronOrthogonalInverseTransform<Data> 
where
    Data: Vector + Portable,
    Data::Component: DataValue
{
    const ID: usize = 3;

    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = ();

    fn new<F>(stream_in: &mut F) -> Result<Self, super::Err>
        where F: FnMut(u8) -> u64 
    {
        let deportabilization = Deportabilization::new(stream_in)?;
        Ok(
            Self {
                _marker: std::marker::PhantomData,
                deportabilization,
            }
        )
    }

    fn inverse<F>(&self, mut pred: Self::Data, stream_in: &mut F) -> Self::Data 
        where F: FnMut(u8) -> u64,
    {
        unimplemented!();

        // // Safety:
        // // We made sure that the data is three dimensional.
        // debug_assert!(
        //     Data::NUM_COMPONENTS == 3,
        // );
        
        // let mut reflected = false;
        // let quadrant;
        // unsafe {
        //     if *pred.get_unchecked(2) < Data::Component::zero() {
        //         reflected = true;
        //         let minus_one = Data::Component::from_f64(-1.0);
        //         *pred.get_unchecked_mut(2) *= minus_one;
        //     }

        //     if *pred.get_unchecked(0) > Data::Component::zero() {
        //         if *pred.get_unchecked(1) > Data::Component::zero() {
        //             // first quadrant. Rotate around z-axis by pi.
        //             quadrant = 1;
        //             let minus_one = Data::Component::from_f64(-1.0);
        //             *pred.get_unchecked_mut(0) *= minus_one;
        //             *pred.get_unchecked_mut(1) *= minus_one;
        //         } else {
        //             // fourth quadrant. Rotate around z-axis by -pi/2.
        //             quadrant = 4;
        //             let temp = *pred.get_unchecked(0);
        //             let one = Data::Component::one();
        //             let minus_one = Data::Component::zero() - one;
        //             *pred.get_unchecked_mut(0) = *pred.get_unchecked(1);
        //             *pred.get_unchecked_mut(1) = temp * minus_one;
        //         }
        //     } else {
        //         if *pred.get_unchecked(1) > Data::Component::zero() {
        //             // second quadrant. Rotate around z-axis by pi/2.
        //             quadrant = 2;
        //             let temp = *pred.get_unchecked(0);
        //             let one = Data::Component::one();
        //             let minus_one = Data::Component::zero() - one;
        //             *pred.get_unchecked_mut(0) = *pred.get_unchecked(1) * minus_one;
        //             *pred.get_unchecked_mut(1) = temp;
        //         } else {
        //             // third quadrant will not be transformed.
        //             quadrant = 3;
        //         }
        //     };
        // }

        // let pred_in_oct = unsafe {
        //     octahedral_transform(pred)
        // };

        // let orig = pred_in_oct + crr;
        // unsafe{
        //     if *pred.get_unchecked(2) < Data::Component::zero() {
        //         let minus_one = Data::Component::from_f64(-1.0);
        //         *pred.get_unchecked_mut(2) *= minus_one;
        //     }
        // }

        // // Safety:
        // // We made sure that the data is three dimensional.
        // let mut orig: Data = unsafe {
        //     octahedral_inverse_transform(orig)
        // };

        // // pull back the transformation
        // // Safety:
        // // We made sure that the data is three dimensional.
        // unsafe {
        //     if quadrant == 1 {
        //         let minus_one = Data::Component::from_f64(-1.0);
        //         *orig.get_unchecked_mut(0) *= minus_one;
        //         *orig.get_unchecked_mut(1) *= minus_one;
        //     } else if quadrant == 2 {
        //         // rotate around z-axis by -pi/2
        //         let temp = *orig.get_unchecked(0);
        //         let one = Data::Component::one();
        //         let minus_one = Data::Component::zero() - one;
        //         *orig.get_unchecked_mut(0) = *orig.get_unchecked(1) * minus_one;
        //         *orig.get_unchecked_mut(1) = temp;
        //     } else if quadrant == 4 {
        //         // rotate around z-axis by pi/2
        //         let temp = *orig.get_unchecked(0);
        //         let one = Data::Component::one();
        //         let minus_one = Data::Component::zero() - one;
        //         *orig.get_unchecked_mut(0) = *orig.get_unchecked(1);
        //         *orig.get_unchecked_mut(1) = temp * minus_one;
        //     }

        //     if reflected {
        //         let minus_one = Data::Component::from_f64(-1.0);
        //         *orig.get_unchecked_mut(2) *= minus_one;
        //     }
        // }

        // orig
    }
}