use std::vec::IntoIter;

use crate::core::shared::{DataValue, NdVector, Vector}; 
use crate::prelude::ByteWriter;
use super::{
    geom::*, 
    PredictionTransformImpl
};

pub struct OctahedronOrthogonalTransform<const N: usize> 
{
    out: Vec<NdVector<2,i32>>,
}

impl<const N: usize> OctahedronOrthogonalTransform<N> 
{
    pub fn new(_cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
        }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for OctahedronOrthogonalTransform<N> 
{
    fn map_with_tentative_metadata(&mut self, mut orig: NdVector<N, i32>, mut pred: NdVector<N, i32>) 
        where 
            NdVector<N, i32>: Vector<N, Component = i32>,
    {
        // Safety:
        // We made sure that the data is three dimensional.
        assert!(
            N==3,
        );

        unsafe {
            if *pred.get_unchecked(2) < 0 {
                *pred.get_unchecked_mut(2) *= -1;
                *orig.get_unchecked_mut(2) *= -1;
            }

            if *pred.get_unchecked(0) > 0 {
                if *pred.get_unchecked(1) > 0 {
                    // first quadrant. Rotate around z-axis by pi.
                    *pred.get_unchecked_mut(0) *= -1;
                    *pred.get_unchecked_mut(1) *= -1;
                    *orig.get_unchecked_mut(0) *= -1;
                    *orig.get_unchecked_mut(1) *= -1;
                } else {
                    // fourth quadrant. Rotate around z-axis by -pi/2.
                    let temp = *pred.get_unchecked(0);
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1);
                    *pred.get_unchecked_mut(1) = temp * -1;
                    *orig.get_unchecked_mut(0) = *orig.get_unchecked(1);
                    *orig.get_unchecked_mut(1) = temp * -1;
                }
            } else {
                if *pred.get_unchecked(1) > 0 {
                    // second quadrant. Rotate around z-axis by pi/2.
                    let temp = *pred.get_unchecked(0);
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1) * -1;
                    *pred.get_unchecked_mut(1) = temp;
                    *orig.get_unchecked_mut(0) = *orig.get_unchecked(1) * -1;
                    *orig.get_unchecked_mut(1) = temp;
                }
                // third quadrant will not be transformed.
            };
        }

        let orig = unsafe{ octahedral_transform(orig) };
        let pred = unsafe { octahedral_transform(pred) };
        // self.out.push( orig - pred );
    }

    fn squeeze<W>(self, _writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter
    {
        unimplemented!()
    }
}
