use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::prelude::Attribute;
use crate::prelude::ByteWriter;
use crate::prelude::NdVector;
use crate::shared::attribute::Portable;

#[cfg(feature = "evaluation")]
use crate::eval;

use super::Config;
use super::PortabilizationImpl;

pub struct ToBits<Data, const N: usize>
    where Data: Vector<N>
{
    /// iterator over the attribute values.
    /// this is not 'Vec<_>' because we want nicely consume the data.
    att: Attribute,
    
    _marker: std::marker::PhantomData<Data>,
}

impl<Data, const N: usize> ToBits<Data, N> 
    where 
        Data: Vector<N> + Portable,
        Data::Component: DataValue
{
    pub fn new<W>(att: Attribute, _cfg: Config, _writer: &mut W) -> Self 
        where W: ByteWriter 
    {
        #[cfg(feature = "evaluation")]
        eval::write_json_pair("portabilization", "ToBits".into(), _writer);
        Self {
            att,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data, const N: usize> PortabilizationImpl<N> for ToBits<Data, N> 
    where
        Data: Vector<N> + Portable, 
        NdVector<N, i32>: Vector<N, Component = i32>,
        
{
    fn portabilize(self) -> Attribute {
        self.att
    }
}