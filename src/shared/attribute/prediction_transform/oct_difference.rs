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

    fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data) {
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );

        let orig = unsafe{ octahedral_transform(orig) };
        let pred = unsafe { octahedral_transform(pred) };
        self._out.push( orig - pred );
    }

    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction, _: Self::Metadata) -> Self::Data {
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::shared::NdVector;

    #[test]
    fn test_transform() {
        let mut transform = OctahedronDifferenceTransform::<NdVector<3, f64>>::new();
        let orig1 = NdVector::<3, f64>::from([1.0, 2.0, 3.0]).normalize();
        let pred1 = NdVector::<3, f64>::from([1.0, 1.0, 1.0]).normalize();
        let orig2 = NdVector::<3, f64>::from([4.0, 5.0, 6.0]).normalize();
        let pred2 = NdVector::<3, f64>::from([5.0, 5.0, 5.0]).normalize();
        
        transform.map_with_tentative_metadata(orig1.clone(), pred1.clone());
        transform.map_with_tentative_metadata(orig2.clone(), pred2.clone());

        let (_, corrections) = transform.squeeze();
        let recovered1 = transform.inverse(pred1.clone(), corrections[0], ());
        let recovered2 = transform.inverse(pred2.clone(), corrections[1], ());
        assert!((recovered1 - orig1).norm() < 0.000_000_1);
        assert!((recovered2 - orig2).norm() < 0.000_000_1);
    }
}