use std::mem;
use crate::core::shared::{DataValue, NdVector, Vector}; 
use crate::encode::attribute::portabilization;
use crate::encode::attribute::{portabilization::Portabilization, WritableFormat};
use crate:: shared::attribute::Portable;
use super::{
    geom::*, 
    PredictionTransformImpl, 
    FinalMetadata
};

pub struct OctahedronOrthogonalTransform<Data> 
    where Data: Vector + Portable
{
    out: Vec<NdVector<2,f64>>,
    final_metadata: FinalMetadata<()>,
    portabilization: Portabilization<<Self as PredictionTransformImpl>::Correction>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> OctahedronOrthogonalTransform<Data> 
    where Data: Vector + Portable
{
    pub fn new(cfg: portabilization::Config) -> Self {
        Self {
            out: Vec::new(),
            final_metadata: FinalMetadata::Global(()),
            portabilization: Portabilization::new(cfg),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> PredictionTransformImpl for OctahedronOrthogonalTransform<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    const ID: usize = 3;

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

        unsafe {
            if *pred.get_unchecked(2) < Data::Component::zero() {
                let minus_one = Data::Component::from_f64(-1.0);
                *pred.get_unchecked_mut(2) *= minus_one;
                *orig.get_unchecked_mut(2) *= minus_one;
            }

            if *pred.get_unchecked(0) > Data::Component::zero() {
                if *pred.get_unchecked(1) > Data::Component::zero() {
                    // first quadrant. Rotate around z-axis by pi.
                    let minus_one = Data::Component::from_f64(-1.0);
                    *pred.get_unchecked_mut(0) *= minus_one;
                    *pred.get_unchecked_mut(1) *= minus_one;
                    *orig.get_unchecked_mut(0) *= minus_one;
                    *orig.get_unchecked_mut(1) *= minus_one;
                } else {
                    // fourth quadrant. Rotate around z-axis by -pi/2.
                    let temp = *pred.get_unchecked(0);
                    let one = Data::Component::one();
                    let minus_one = Data::Component::zero() - one;
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1);
                    *pred.get_unchecked_mut(1) = temp * minus_one;
                    *orig.get_unchecked_mut(0) = *orig.get_unchecked(1);
                    *orig.get_unchecked_mut(1) = temp * minus_one;
                }
            } else {
                if *pred.get_unchecked(1) > Data::Component::zero() {
                    // second quadrant. Rotate around z-axis by pi/2.
                    let temp = *pred.get_unchecked(0);
                    let one = Data::Component::one();
                    let minus_one = Data::Component::zero() - one;
                    *pred.get_unchecked_mut(0) = *pred.get_unchecked(1) * minus_one;
                    *pred.get_unchecked_mut(1) = temp;
                    *orig.get_unchecked_mut(0) = *orig.get_unchecked(1) * minus_one;
                    *orig.get_unchecked_mut(1) = temp;
                }
                // third quadrant will not be transformed.
            };
        }

        let orig = unsafe{ octahedral_transform(orig) };
        let pred = unsafe { octahedral_transform(pred) };
        self.out.push( orig - pred );
    }

    fn squeeze_impl(&mut self) {
        self.final_metadata = FinalMetadata::Global(()); 
    }

    fn portabilize(&mut self) -> (WritableFormat, WritableFormat) {
        self.portabilization.portabilize(self.out.clone())
    }

    fn portabilize_and_write_metadata<F>(&mut self, writer: &mut F) -> WritableFormat 
        where F: FnMut((u8, u64))
    {
        self.portabilization.portabilize_and_write_metadata(mem::take(&mut self.out), writer)
    }

    fn get_final_metadata(&self) -> &FinalMetadata<Self::Metadata> {
        &self.final_metadata
    }

    fn get_final_metadata_writable_form(&self) -> WritableFormat {
        self.final_metadata.clone().into()
    }
}
