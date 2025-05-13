use super::InversePredictionTransformImpl;
use crate::core::shared::{
    DataValue,
    NdVector,
    Vector,
};
use crate::decode::attribute::portabilization::{Deportabilization, DeportabilizationImpl};
use crate::encode::attribute::prediction_transform::geom::*;
use crate::shared::attribute::Portable;

pub(crate) struct OrthogonalInverseTransform<Data> 
    where Data: Vector,
{
    metadata: bool,
    deportabilization: Deportabilization<<Self as InversePredictionTransformImpl>::Correction>,
    _phantom: std::marker::PhantomData<Data>,
}

impl<Data> InversePredictionTransformImpl for OrthogonalInverseTransform<Data> 
    where 
    Data: Vector,
    Data::Component: DataValue,
{
    type Data = Data;
    type Correction = NdVector<2,f64>;
    type Metadata = bool;

    const ID: usize = 5;

    fn new<F>(stream_in: &mut F) -> Result<Self, super::Err> 
        where F: FnMut(u8) -> u64 
    {
        let deportabilization = Deportabilization::new(stream_in)?;
        let metadata = <bool as Portable>::read_from_bits(stream_in);
        
        Ok(
            Self {
                metadata,
                deportabilization,
                _phantom: std::marker::PhantomData,
            }
        )
    }

    fn inverse<F>(&self, pred: Self::Data, stream_in: &mut F) -> Self::Data 
        where F: FnMut(u8)->u64
    {
        let crr = self.deportabilization.deportabilize_next(stream_in);
        
        let one = Data::Component::one();
        let pred_norm_squared = pred.dot(pred);
        let ref_on_pred_perp = if self.metadata {
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
}

