use crate::core::shared::{NdVector, Vector};
use crate::prelude::ByteWriter;

use super::PredictionTransformImpl;


pub struct OctahedronReflectionTransform<const N: usize> 
{
    out: Vec<NdVector<N, i32>>,
}

impl<const N: usize> OctahedronReflectionTransform<N> {
    pub fn new(_cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
        }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for OctahedronReflectionTransform<N> 
{
    fn map_with_tentative_metadata(&mut self, mut orig: NdVector<N,i32>, mut pred: NdVector<N,i32>)
        where NdVector<N, i32>: Vector<N, Component = i32>,
    {
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            N==2,
        );

        unsafe {
            if *pred.get_unchecked(2) < 0 {
                *pred.get_unchecked_mut(2) *= -1;
                *orig.get_unchecked_mut(2) *= -1;
            }
        };

        self.out.push( orig - pred );
    }

    fn squeeze<W>(self, _writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter
    {
        unimplemented!()
    }

}