use std::vec::IntoIter;

use crate::core::shared::{DataValue, Vector};
use crate::encode::attribute::portabilization::{Portabilization, PortabilizationImpl};
use crate::prelude::ByteWriter;
use crate::shared::attribute::Portable;
use super::PredictionTransformImpl;
use crate::core::shared::Max;

#[cfg(feature = "evaluation")]
use crate::eval;

pub struct Difference<Data> 
    where Data: Vector + Portable
{
    cfg: super::Config,
    out: Vec<Data>,
    metadata: Data,
}

impl<Data> Difference<Data> 
    where Data: Vector + Portable
{
    pub fn new(cfg: super::Config) -> Self {
        let mut metadata = Data::zero();
        for i in 0..Data::NUM_COMPONENTS {
            // Safety:
            // iterating over a constant-sized array
            unsafe{
                *metadata.get_unchecked_mut(i) = Data::Component::MAX_VALUE;
            }
        }
        Self {
            cfg,
            out: Vec::new(),
            metadata,
        }
    }
}

impl<Data> PredictionTransformImpl<Data> for Difference<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{

    fn map_with_tentative_metadata(&mut self, orig: Data, pred: Data) {
        let corr = orig - pred;
        self.out.push(corr);
        // update metadata
        for i in 0..Data::NUM_COMPONENTS {
            unsafe{
                if self.metadata.get_unchecked(i) > corr.get_unchecked(i) {
                    *self.metadata.get_unchecked_mut(i) = *corr.get_unchecked(i);
                }
            }
        }
    }

    fn squeeze<W>(&mut self, writer: &mut W)
        where W: ByteWriter
    {
        self.out.iter_mut()
            .for_each(|v|
                *v -= self.metadata
            );

        #[cfg(feature = "evaluation")]
        {
            eval::write_json_pair("transform  type", "Difference".into(), writer);
            eval::write_json_pair("metadata type", "Global".into(), writer);
            eval::write_json_pair("metadata", self.metadata.into(), writer);
            eval::array_scope_begin("transformed data", writer);
            for &x in self.out.iter() {
                eval::write_arr_elem(x.into(), writer);
            }
            eval::array_scope_end(writer);
        }

        // write metadata
        self.metadata.write_to(writer);
    }

    fn out<W>(self, writer: &mut W) -> IntoIter<IntoIter<u8>>
        where W: ByteWriter
    {
        Portabilization::new(
            self.out,
            self.cfg.portabilization,
            writer
        ).portabilize()
    }
}

