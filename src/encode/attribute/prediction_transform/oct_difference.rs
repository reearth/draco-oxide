use std::mem;

use crate::core::shared::{DataValue, NdVector, Vector};
use crate::encode::attribute::portabilization::{self, Portabilization};
use crate::encode::attribute::WritableFormat;
use super::geom::octahedral_transform;

use super::{FinalMetadata, PredictionTransformImpl};


pub struct OctahedronDifferenceTransform<Data> 
    where Data: Vector
{
    out: Vec<NdVector<2,f64>>,
    final_metadata: FinalMetadata<()>,
    portabilization: Portabilization<<Self as PredictionTransformImpl>::Correction>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> OctahedronDifferenceTransform<Data>
    where Data: Vector
{
    pub fn new(cfg: portabilization::Config) -> Self {
        Self {
            out: Vec::new(),
            final_metadata: FinalMetadata::Global(()),
            portabilization: Portabilization::new(cfg),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn get_corr(&self) -> &[NdVector<2,f64>] {
        &self.out
    }
}

impl<Data> PredictionTransformImpl for OctahedronDifferenceTransform<Data> 
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
        self.out.push( orig - pred );
    }

    fn squeeze_impl(&mut self) {
        self.final_metadata = FinalMetadata::Global(());
    }

    fn portabilize(&mut self) -> (WritableFormat, WritableFormat) {
        self.portabilization.portabilize(mem::take(&mut self.out))
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
        WritableFormat::from(self.final_metadata.clone()) // it's okay to clone because it's empty
    }
}