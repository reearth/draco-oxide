use std::vec::IntoIter;

use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::prelude::Attribute;
use crate::prelude::ByteWriter;
use crate::prelude::NdVector;
use crate::shared::attribute::Portable;

use super::Config;
use super::PortabilizationImpl;

#[cfg(feature = "evaluation")]
use crate::eval;

pub struct OctahedralQuantization<Data, const N: usize>
{
    /// iterator over the attribute values.
    /// this is not 'Vec<_>' because we want nicely consume the data.
    att_vals: std::vec::IntoIter<Data>,

    /// the global metadata min.
    global_metadata_min: Data,

    /// 'global_metadata_max - global_metadata_min'
    /// This is precomputed to avoid recomputing it for each data.
    range: Data,

    /// the quantization size.
    /// Each component is of float type, though its value is an integer.
    quantization_size: Data,
}

impl<Data, const N: usize> OctahedralQuantization<Data, N>
{
    pub fn new<W>(att_vals: Attribute, cfg: Config, writer: &mut W) -> Self 
        where W: ByteWriter
    {
        unimplemented!()
    }
}

impl<Data, const N: usize> PortabilizationImpl<N> for OctahedralQuantization<Data,N>
    where
        Data: Vector<N> + Portable,
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn portabilize(self) -> Attribute {
        unimplemented!()
    }
}
        


 #[cfg(all(test, not(feature = "evaluation")))]
mod tests {
    use crate::{encode::attribute::portabilization::PortabilizationType, prelude::{FunctionalByteWriter, NdVector}};

    use super::*;
    // ToDo: Add tests
}