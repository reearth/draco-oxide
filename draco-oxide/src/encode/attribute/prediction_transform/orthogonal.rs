use crate::core::shared::{NdVector, Vector};
use crate::prelude::ByteWriter;

use super::PredictionTransformImpl;

pub struct OrthogonalTransform<const N: usize> 
{
    #[allow(unused)]
    out: Vec<NdVector<2,i32>>,
    
    /// This metadata records whether the prediction uses 
    /// (1,0,0) or (0,1,0) as the reference vector.
    #[allow(unused)]
    metadata: Vec<bool>,
}

impl<const N: usize> OrthogonalTransform<N> 
{
    pub fn new(_cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
            metadata: Vec::new(),
        }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for OrthogonalTransform<N> {
    // ToDo: Add dynamic data check.

    fn map_with_tentative_metadata(&mut self, _orig: NdVector<N,i32>, _pred: NdVector<N,i32>) 
        where NdVector<N,i32>: Vector<N, Component = i32>,
    {
        unimplemented!();
        // let one = Data::Component::one();
        // let zero = Data::Component::zero();

        // // project 'r' to the plane defined by 'pred'
        // let pred_norm_squared = pred.dot(pred);
        // let ref_on_pred_perp = if unsafe{ pred.get_unchecked(1).abs() } > one/Data::Component::from_u64(10) {
        //     self.metadata.push(true);
        //     // Safety: 
        //     // dereferencing the constant-sized array by a constant index
        //     unsafe {
        //         let mut out = pred * (*pred.get_unchecked(0) / pred_norm_squared);
        //         *out.get_unchecked_mut(0) += one;
        //         out
        //     }
        // } else {
        //     self.metadata.push(false);
        //     // Safety: 
        //     // dereferencing the constant-sized array by a constant index
        //     unsafe {
        //         let mut out = pred * (*pred.get_unchecked(1) / pred_norm_squared);
        //         *out.get_unchecked_mut(1) += one;
        //         out
        //     }
        // };

        // let pred_norm_squared = pred_norm_squared.to_f64();

        // let pred_cross_orig = pred.cross(orig);
        // // 'ref_on_pred_perp' and pred_'cross_orig' are on the same plane defined by 'pred'
        // debug_assert!(pred_cross_orig.dot(pred).abs() < one/Data::Component::from_u64(1_000_000));
        // debug_assert!(ref_on_pred_perp.dot(pred).abs() < one/Data::Component::from_u64(1_000_000));

        // // get the angle between 'ref_on_pred_perp' and 'pred_cross_orig'
        // let ref_on_pred_perp_norm_squared = ref_on_pred_perp.dot(ref_on_pred_perp).to_f64();
        // let difference = ref_on_pred_perp-pred_cross_orig;
        // let difference_norm_squared = difference.dot(difference).to_f64();
        // let sign = if pred.dot(ref_on_pred_perp.cross(pred_cross_orig)) > zero { 1_f64 } else { -1_f64 };
        // let first_angle = sign * (1_f64+ref_on_pred_perp_norm_squared-difference_norm_squared/2_f64/ref_on_pred_perp_norm_squared.sqrt()).acos();


        // // get the angle between 'pred' and 'orig'
        // let orig_norm_squared = orig.dot(orig).to_f64();
        // let difference = pred - orig;
        // let difference_norm_squared = difference.dot(difference).to_f64();
        // let second_angle = (pred_norm_squared+orig_norm_squared-difference_norm_squared/(2_f64*pred_norm_squared.sqrt()*orig_norm_squared.sqrt())).acos();

        // self.out.push(NdVector::from(
        //     [first_angle, second_angle]
        // ));

    }

    fn squeeze<W>(self, _writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter 
    {
        unimplemented!()
    }
}