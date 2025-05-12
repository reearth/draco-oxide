use crate::core::shared::{DataValue, NdVector, Vector};
use crate::encode::attribute::portabilization::{Portabilization, PortabilizationImpl};
use crate::encode::attribute::WritableFormat;
use super::geom::octahedral_transform;

use super::{FinalMetadata, PredictionTransformImpl};


pub struct OctahedronDifferenceTransform<Data> 
    where Data: Vector
{
    cfg: super::portabilization::Config,
    final_metadata: FinalMetadata<()>,
    out: Vec<NdVector<2,f64>>,
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> OctahedronDifferenceTransform<Data>
    where Data: Vector
{
    pub fn new(cfg: super::Config) -> Self {
        Self {
            cfg: cfg.portabilization,
            out: Vec::new(),
            final_metadata: FinalMetadata::Global(()),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn get_corr(&self) -> &[NdVector<2,f64>] {
        &self.out
    }
}

impl<Data> PredictionTransformImpl<Data> for OctahedronDifferenceTransform<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    fn map_with_tentative_metadata(&mut self, orig: Data, pred: Data) {
        // Safety:
        // We made sure that the data is three dimensional.
        debug_assert!(
            Data::NUM_COMPONENTS == 3,
        );

        let orig = unsafe{ octahedral_transform(orig) };
        let pred = unsafe { octahedral_transform(pred) };
        self.out.push( orig - pred );
    }

    fn squeeze<F>(&mut self, _writer: &mut F)
        where F: FnMut((u8, u64))
    {
        self.final_metadata = FinalMetadata::Global(());
    }

    fn out<F>(self, writer: &mut F) -> std::vec::IntoIter<WritableFormat>
        where F: FnMut((u8, u64))
    {
        Portabilization::new(
            self.out,
            self.cfg,
            writer
        ).portabilize()
    }
}