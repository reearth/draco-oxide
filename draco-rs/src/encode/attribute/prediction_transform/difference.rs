use crate::core::shared::{DataValue, Vector};
use crate::encode::attribute::portabilization::{Portabilization, PortabilizationImpl};
use crate::encode::attribute::WritableFormat;
use crate::shared::attribute::Portable;
use super::{FinalMetadata, PredictionTransformImpl};
use crate::core::shared::Max;


pub struct Difference<Data> 
    where Data: Vector + Portable
{
    cfg: super::Config,
    out: Vec<Data>,
    final_metadata: FinalMetadata<Data>,
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
            final_metadata: FinalMetadata::Global(metadata.clone()),
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

    fn squeeze<F>(&mut self, writer: &mut F)
        where F: FnMut((u8, u64))
    {
        self.out.iter_mut()
            .for_each(|v|
                *v -= self.metadata
            );
        let final_metadata = FinalMetadata::Global(self.metadata);

        // write metadata
        WritableFormat::from(final_metadata).write(writer);
    }

    fn out<F>(self, writer: &mut F) -> std::vec::IntoIter<WritableFormat>
        where F: FnMut((u8, u64))
    {
        Portabilization::new(
            self.out,
            self.cfg.portabilization,
            writer
        ).portabilize()
    }
}

