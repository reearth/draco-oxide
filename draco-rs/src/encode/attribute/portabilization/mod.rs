pub mod quantization_rect_array;
pub mod quantization_rect_spiral;
pub mod quantization_spherical;
pub mod to_bits;

use crate::{core::shared::{ConfigType, Vector}, debug_write, shared::attribute::Portable};
use super::WritableFormat;

#[enum_dispatch::enum_dispatch(PortabilizationImpl)]
pub enum Portabilization<Data>
    where Data: Vector + Portable,
{
    QuantizationRectangleArray(quantization_rect_array::QuantizationRectangleArray<Data>),
    QuantizationRectangleSpiral(quantization_rect_spiral::QuantizationRectangleSpiral<Data>),
    QuantizationSpherical(quantization_spherical::QuantizationSpherical<Data>),
    ToBits(to_bits::ToBits<Data>),
}

impl<Data> Portabilization<Data> 
    where Data: Vector + Portable,
{
    /// creates a new instance of the portabilization, computes the metadata, and 
    /// writes the metadata to the stream.
    // enum_dispatch does not support associated functions, we explicitly write the
    // constructor.
    pub fn new<F>(att_vals: Vec<Data>, cfg: Config, writer: &mut F) -> Self
        where F: FnMut((u8, u64))
    {
        debug_write!("Start of Portabilization Metadata", writer);
        cfg.type_.write_id(writer);
        let out = match cfg.type_ {
            PortabilizationType::QuantizationRectangleArray => {
                Portabilization::QuantizationRectangleArray(
                    quantization_rect_array::QuantizationRectangleArray::new(att_vals, cfg, writer)
                )
            },
            PortabilizationType::QuantizationRectangleSpiral => {
                Portabilization::QuantizationRectangleSpiral(
                    quantization_rect_spiral::QuantizationRectangleSpiral::new(att_vals, cfg, writer)
                )
            },
            PortabilizationType::QuantizationSpherical => {
                Portabilization::QuantizationSpherical(
                    quantization_spherical::QuantizationSpherical::new(att_vals, cfg, writer)
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
    fn portabilize(self) -> std::vec::IntoIter<WritableFormat>;
}

#[remain::sorted]
#[derive(Clone, Copy, Debug)]
pub enum PortabilizationType {
    QuantizationRectangleArray,
    QuantizationRectangleSpiral,
    QuantizationSpherical,
    ToBits,
}

impl PortabilizationType {
    pub(crate) fn get_id(&self) -> u8 {
        match self {
            PortabilizationType::QuantizationRectangleArray => 0,
            PortabilizationType::QuantizationRectangleSpiral => 1,
            PortabilizationType::QuantizationSpherical => 2,
            PortabilizationType::ToBits => 3,
        }
    }

    pub(crate) fn write_id<F>(&self, writer: &mut F) 
        where F: FnMut((u8, u64)) 
    {
        let id = self.get_id();
        writer((4, id as u64));
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