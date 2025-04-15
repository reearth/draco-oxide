use std::marker::PhantomData;
use std::mem;

use crate::core::shared::{Abs, DataValue, NdVector, Vector};

use super::{FinalMetadata, PredictionTransform};
use super::geom::*;

pub struct OrthogonalTransform<Data> {
    out: Vec<NdVector<2,f64>>,
    
    /// This metadata records whether the prediction uses 
    /// (1,0,0) or (0,1,0) as the reference vector.
    metadata: Vec<bool>,
    
    _marker: PhantomData<Data>,
}

impl<Data> OrthogonalTransform<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            metadata: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<Data> PredictionTransform for OrthogonalTransform<Data> 
    where Data: Vector,
    Data::Component: DataValue
{
    const ID: usize = 5;

    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = bool;

    fn map(_orig: Self::Data, _pred: Self::Data, _metadata: Self::Metadata) -> Self::Correction {
        unimplemented!()
    }

    // ToDo: Add dynamic data check.

    fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data) {
        let one = Data::Component::one();
        let zero = Data::Component::zero();

        // project 'r' to the plane defined by 'pred'
        let pred_norm_squared = pred.dot(pred);
        let ref_on_pred_perp = if unsafe{ pred.get_unchecked(1).abs() } > one/Data::Component::from_u64(10) {
            self.metadata.push(true);
            // Safety: 
            // dereferencing the constant-sized array by a constant index
            unsafe {
                let mut out = pred * (*pred.get_unchecked(0) / pred_norm_squared);
                *out.get_unchecked_mut(0) += one;
                out
            }
        } else {
            self.metadata.push(false);
            // Safety: 
            // dereferencing the constant-sized array by a constant index
            unsafe {
                let mut out = pred * (*pred.get_unchecked(1) / pred_norm_squared);
                *out.get_unchecked_mut(1) += one;
                out
            }
        };

        let pred_norm_squared = pred_norm_squared.to_f64();

        let pred_cross_orig = pred.cross(orig);
        // 'ref_on_pred_perp' and pred_'cross_orig' are on the same plane defined by 'pred'
        debug_assert!(pred_cross_orig.dot(pred).abs() < one/Data::Component::from_u64(1_000_000));
        debug_assert!(ref_on_pred_perp.dot(pred).abs() < one/Data::Component::from_u64(1_000_000));

        // get the angle between 'ref_on_pred_perp' and 'pred_cross_orig'
        let ref_on_pred_perp_norm_squared = ref_on_pred_perp.dot(ref_on_pred_perp).to_f64();
        let difference = ref_on_pred_perp-pred_cross_orig;
        let difference_norm_squared = difference.dot(difference).to_f64();
        let sign = if pred.dot(ref_on_pred_perp.cross(pred_cross_orig)) > zero { 1_f64 } else { -1_f64 };
        let first_angle = sign * (1_f64+ref_on_pred_perp_norm_squared-difference_norm_squared/2_f64/ref_on_pred_perp_norm_squared.sqrt()).acos();


        // get the angle between 'pred' and 'orig'
        let orig_norm_squared = orig.dot(orig).to_f64();
        let difference = pred - orig;
        let difference_norm_squared = difference.dot(difference).to_f64();
        let second_angle = (pred_norm_squared+orig_norm_squared-difference_norm_squared/(2_f64*pred_norm_squared.sqrt()*orig_norm_squared.sqrt())).acos();

        self.out.push(NdVector::from(
            [first_angle, second_angle]
        ));

    }

    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction, metadata: Self::Metadata) -> Self::Data {
        let one = Data::Component::one();
        let pred_norm_squared = pred.dot(pred);
        let ref_on_pred_perp = if metadata {
            // Safety: 
            // dereferencing the constant-sized array by a constant index
            unsafe {
                let mut out = pred * (*pred.get_unchecked(0) / pred_norm_squared);
                *out.get_unchecked_mut(0) += one;
                out
            }
        } else {
            // Safety: 
            // dereferencing the constant-sized array by a constant index
            unsafe {
                let mut out = pred * (*pred.get_unchecked(1) / pred_norm_squared);
                *out.get_unchecked_mut(1) += one;
                out
            }
        };

        let mut pred_cross_orig = Data::zero();
        let rotation = rotation_matrix_from(pred, unsafe{ *crr.get_unchecked(0) });
        unsafe {
            *pred_cross_orig.get_unchecked_mut(0) = rotation.get_unchecked(0).dot(ref_on_pred_perp);
            *pred_cross_orig.get_unchecked_mut(1) = rotation.get_unchecked(1).dot(ref_on_pred_perp);
            *pred_cross_orig.get_unchecked_mut(2) = rotation.get_unchecked(2).dot(ref_on_pred_perp);
        };

        // now recover the original vector by rotating 'pred' on the plane defined by 'pred_cross_orig'
        let rotation = rotation_matrix_from(pred_cross_orig, unsafe{ *crr.get_unchecked(1) });
        let mut orig = Data::zero();
        unsafe {
            *orig.get_unchecked_mut(0) = rotation.get_unchecked(0).dot(pred);
            *orig.get_unchecked_mut(1) = rotation.get_unchecked(1).dot(pred);
            *orig.get_unchecked_mut(2) = rotation.get_unchecked(2).dot(pred);
        };
        
        orig
    }

    fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>) {
        if self.metadata.iter().all(|&v|v) {
            (
                FinalMetadata::Global(self.metadata.pop().unwrap()),
                mem::take(&mut self.out)
            )
        } else {
            (
                FinalMetadata::Local(std::mem::take(&mut self.metadata)),
                mem::take(&mut self.out)
            )
        }
    }
}