use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::shared::attribute::Portable;

use super::Linealizer;

pub(crate) struct RectangleArrayLinealizer<Data> 
{
    max: Data,
}

impl<Data, const N: usize> Linealizer<Data, N> for RectangleArrayLinealizer<Data> 
    where  Data: Vector<N> + Portable
{
    type Metadata = Data;

    fn init(&mut self, max: Self::Metadata) {
        self.max = max;
    }

    fn linearize(&self, data: Vec<Data>) -> Vec<u64> {
        data.into_iter()
            .map(|x| {
                let mut val = 0;
                for i in 0..N {
                    let component = unsafe { x.get_unchecked(i) };
                    let max_component = unsafe { self.max.get_unchecked(i) };
                    val += (*component * *max_component).to_u64();
                }
                val
            })
            .collect()
    }

    fn inverse_linialize(&self, data: &Vec<usize>) -> Vec<usize> {
        unimplemented!()
    }
}
