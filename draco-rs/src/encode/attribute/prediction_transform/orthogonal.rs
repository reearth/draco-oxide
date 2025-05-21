use std::marker::PhantomData;

use crate::core::shared::{Abs, DataValue, NdVector, Vector};
use crate::encode::attribute::WritableFormat;
use crate::shared::attribute::Portable;

use super::PredictionTransformImpl;

pub struct OrthogonalTransform<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    out: Vec<NdVector<2,f64>>,
    
    /// This metadata records whether the prediction uses 
    /// (1,0,0) or (0,1,0) as the reference vector.
    metadata: Vec<bool>,

    _marker: PhantomData<Data>,
}

impl<Data> OrthogonalTransform<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    pub fn new(_cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
            metadata: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<Data> PredictionTransformImpl<Data> for OrthogonalTransform<Data> 
    where
        Data: Vector + Portable,
        Data::Component: DataValue
{
    // ToDo: Add dynamic data check.

    fn map_with_tentative_metadata(&mut self, orig: Data, pred: Data) {
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

    fn out<F>(self, _writer: &mut F) -> std::vec::IntoIter<WritableFormat>where F:FnMut((u8,u64)) {
        unimplemented!()
    }

    fn squeeze<F>(&mut self, _writer: &mut F) 
        where F:FnMut((u8,u64)) 
    {
        unimplemented!()
    }
}