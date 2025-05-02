use std::mem;
use crate::core::shared::{DataValue, Vector};
use crate::encode::attribute::portabilization;
use crate::encode::attribute::{portabilization::Portabilization, WritableFormat};
use crate::shared::attribute::Portable;
use super::{FinalMetadata, PredictionTransformImpl};
use crate::core::shared::Max;


pub struct Difference<Data> 
    where Data: Vector + Portable
{
    out: Vec<Data>,
    final_metadata: FinalMetadata<Data>,
    _metadata: Data,
    portabilization: Portabilization<<Self as PredictionTransformImpl>::Correction>,
}

impl<Data> Difference<Data> 
    where Data: Vector + Portable
{
    pub fn new(cfg: portabilization::Config) -> Self {
        let mut _metadata = Data::zero();
        for i in 0..Data::NUM_COMPONENTS {
            // Safety:
            // iterating over a constant-sized array
            unsafe{
                *_metadata.get_unchecked_mut(i) = Data::Component::MAX_VALUE;
            }
        }

        let portabilization = Portabilization::new(cfg);

        Self {
            out: Vec::new(),
            final_metadata: FinalMetadata::Global(_metadata.clone()),
            _metadata,
            portabilization
        }
    }
}

impl<Data> PredictionTransformImpl for Difference<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    const ID: usize = 1;

    type Data = Data;
    type Correction = Data;
    type Metadata = Data;

    fn map(orig: Self::Data, pred: Self::Data, metadata: Self::Metadata) -> Self::Correction {
        orig - pred - metadata
    }

    fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data) {
        let corr = orig - pred;
        self.out.push(corr);
        // update metadata
        for i in 0..Data::NUM_COMPONENTS {
            unsafe{
                if self._metadata.get_unchecked(i) > corr.get_unchecked(i) {
                    *self._metadata.get_unchecked_mut(i) = *corr.get_unchecked(i);
                }
            }
        }
    }

    fn squeeze_impl(&mut self) {
        self.out.iter_mut()
            .for_each(|v|
                *v -= self._metadata
            );
        self.final_metadata = FinalMetadata::Global(self._metadata);
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
        WritableFormat::from(self.final_metadata.clone()) // ToDo: Think of a way to avoid cloning
    }
}

