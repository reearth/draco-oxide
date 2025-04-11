use crate::core::shared::{DataValue, NdVector, Vector};
use super::geom::{
    octahedral_inverse_transform, 
    octahedral_transform
};

use super::{FinalMetadata, PredictionTransform};


pub struct OctahedronDifferenceTransform<Data> {
    _out: Vec<NdVector<2,f64>>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> OctahedronDifferenceTransform<Data> 
    where Data: Vector
{
    pub fn new() -> Self {
        Self {
            _out: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> PredictionTransform for OctahedronDifferenceTransform<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    const ID: usize = 2;

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

        let pred_in_oct = unsafe {
            octahedral_transform(pred)
        };

        let orig = pred_in_oct + crr;

        // Safety:
        // We made sure that the data is three dimensional.
        unsafe {
            octahedral_inverse_transform(orig)
        }
    }

    fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>) {
        (
            FinalMetadata::Global(()), 
            std::mem::take(&mut self._out)
        )
    }
}