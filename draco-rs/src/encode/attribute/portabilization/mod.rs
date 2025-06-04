pub mod quantization_rect_array;
pub mod to_bits;

use std::vec::IntoIter;

use crate::core::shared::{ConfigType, Vector};
use crate::debug_write;
use crate::core::bit_coder::ByteWriter;
use crate::shared::attribute::Portable;

#[enum_dispatch::enum_dispatch(PortabilizationImpl)]
pub enum Portabilization<Data>
    where Data: Vector + Portable,
{
    QuantizationRectangleArray(quantization_rect_array::QuantizationRectangleArray<Data>),
    ToBits(to_bits::ToBits<Data>),
}

impl<Data> Portabilization<Data> 
    where Data: Vector + Portable,
{
    /// creates a new instance of the portabilization, computes the metadata, and 
    /// writes the metadata to the stream.
    // enum_dispatch does not support associated functions, we explicitly write the
    // constructor.
    pub fn new<W>(att_vals: Vec<Data>, cfg: Config, writer: &mut W) -> Self
        where W: ByteWriter
    {
        debug_write!("Start of Portabilization Metadata", writer);
        cfg.type_.write_id(writer);
        let out = match cfg.type_ {
            PortabilizationType::QuantizationRectangleArray => {
                Portabilization::QuantizationRectangleArray(
                    quantization_rect_array::QuantizationRectangleArray::new(att_vals, cfg, writer)
                )
            },
            PortabilizationType::ToBits => {
                Portabilization::ToBits(
                    to_bits::ToBits::new(att_vals, cfg, writer)
                )
            },
        };
        debug_write!("End of Portabilization Metadata", writer);
        out
    }
}

#[enum_dispatch::enum_dispatch]
pub trait PortabilizationImpl
{
    /// portabilizes the whole data.
    fn portabilize(self) -> IntoIter<IntoIter<u8>>;
}

#[remain::sorted]
#[derive(Clone, Copy, Debug)]
pub enum PortabilizationType {
    QuantizationRectangleArray,
    ToBits,
}

impl PortabilizationType {
    pub(crate) fn get_id(&self) -> u8 {
        match self {
            PortabilizationType::QuantizationRectangleArray => 0,
            PortabilizationType::ToBits => 1,
        }
    }

    pub(crate) fn write_id<W>(&self, writer: &mut W) 
        where W: ByteWriter
    {
        let id = self.get_id();
        writer.write_u8(id);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub type_: PortabilizationType,
    pub resolution: Resolution,
}

impl ConfigType for Config {
    fn default()-> Self {
        Config {
            type_: PortabilizationType::QuantizationRectangleArray,
            resolution: Resolution::DivisionSize(1000),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    DivisionSize(u64),
    UnitCubeSize(f64)
}