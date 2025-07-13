use crate::core::shared::{NdVector, Vector}; 
use crate::prelude::ByteWriter;
use super::{
    PredictionTransformImpl
};

pub struct OctahedronOrthogonalTransform<const N: usize> 
{
    out: Vec<NdVector<N,i32>>,
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
        // We made sure that the data is two dimensional.
        assert!(
            N==2,
        );

        // make sure that pred is in the upper hemisphere.
        let one = 255/2;
        *pred.get_mut(0) -= one;
        *pred.get_mut(1) -= one;
        *orig.get_mut(0) -= one;
        *orig.get_mut(1) -= one;
        if pred.get(0).abs() + pred.get(1).abs() > one {
            // we need to flip the z-axis. 
            // In the octahedron representation, this means that we need to flip inside out.
            let pred0 = *pred.get(0);
            let quadrant_sign = -(pred.get(0) * pred.get(1)).signum();
            *pred.get_mut(0) = quadrant_sign*pred.get(1) + pred.get(0).signum() * one;
            *pred.get_mut(1) = quadrant_sign*pred0 + pred.get(1).signum() * one;
            let orig0 = *orig.get(0);
            let quadrant_sign = -(orig.get(0) * orig.get(1)).signum();
            *orig.get_mut(0) = quadrant_sign*orig.get(1) + orig.get(0).signum() * one;
            *orig.get_mut(1) = quadrant_sign*orig0 + orig.get(1).signum() * one;
        }

        // Now rotate the sphere around the z-axis so that the x and y coordinates of pred are both negative.
        if pred != NdVector::<N, i32>::zero() {
            while *pred.get(0) >= 0 || *pred.get(1) > 0 {
                // rotate 90 degrees clockwise
                let tmp = *pred.get(0);
                *pred.get_mut(0) = -pred.get(1);
                *pred.get_mut(1) = tmp;

                let tmp = *orig.get(0);
                *orig.get_mut(0) = -orig.get(1);
                *orig.get_mut(1) = tmp;
            }
        }

        // Now we take the difference and make it positive.
        let mut corr = orig - pred;
        for i in 0..N {
            if *corr.get(i) < 0 {
                *corr.get_mut(i) += 255;
            }
        }
        self.out.push(corr);
    }

    fn squeeze<W>(self, writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter
    {
        // write the max quantized value.
        writer.write_u32(255);
        // write center of the octahedron.
        writer.write_u32(255/2);

        self.out
    }
}
