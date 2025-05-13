use crate::core::shared::{DataValue, NdVector, Vector};
use crate::encode::attribute::WritableFormat;
use crate::shared::attribute::Portable;
use super::geom::octahedral_transform;

use super::PredictionTransformImpl;


pub struct OctahedronReflectionTransform<Data> 
    where Data: Vector + Portable
{
    out: Vec<NdVector<2, f64>>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> OctahedronReflectionTransform<Data> 
    where Data: Vector + Portable
{
    pub fn new(_cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> PredictionTransformImpl<Data> for OctahedronReflectionTransform<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    fn map_with_tentative_metadata(&mut self, mut orig: Data, mut pred: Data) {
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
        };

        let orig = unsafe{ octahedral_transform(orig) };
        let pred = unsafe { octahedral_transform(pred) };
        self.out.push( orig - pred );
    }

    fn squeeze<F>(&mut self, _writer: &mut F)
        where F: FnMut((u8, u64))
    {
        unimplemented!()
    }

    fn out<F>(self, _writer: &mut F) -> std::vec::IntoIter<WritableFormat>
        where F: FnMut((u8, u64))
    {
        unimplemented!()
    }
}