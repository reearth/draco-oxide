use std::vec::IntoIter;

use crate::core::shared::{DataValue, NdVector, Vector};
use crate::encode::attribute::portabilization::{Portabilization, PortabilizationImpl};
use crate::prelude::ByteWriter;
use super::geom::octahedral_transform;

use super::PredictionTransformImpl;


pub struct OctahedronDifferenceTransform<Data> 
    where Data: Vector
{
    cfg: super::portabilization::Config,
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
            _marker: std::marker::PhantomData,
        }
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

    fn squeeze<W>(&mut self, _writer: &mut W)
    {

    }

    fn out<W>(self, writer: &mut W) -> IntoIter<IntoIter<u8>>
        where W: ByteWriter
    {
        Portabilization::new(
            self.out,
            self.cfg,
            writer
        ).portabilize()
    }
}