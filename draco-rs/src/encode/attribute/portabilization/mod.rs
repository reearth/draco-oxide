pub mod quantization_coordinate_wise;
pub mod octahedral_quantization;
pub mod to_bits;

use crate::core::shared::{ConfigType, Vector};
use crate::debug_write;
use crate::core::bit_coder::ByteWriter;
use crate::prelude::{Attribute, NdVector};
use crate::shared::attribute::Portable;

pub enum Portabilization<Data, const N: usize, const MakeOutputPositive: bool>
    where Data: Vector<N> + Portable
{
    QuantizationCoordinateWise(quantization_coordinate_wise::QuantizationCoordinateWise<Data, N, MakeOutputPositive>),
    OctahedralQuantization(octahedral_quantization::OctahedralQuantization<Data, N>),
    ToBits(to_bits::ToBits<Data, N>),
}

impl<Data, const N: usize, const MakeOutputPositive: bool> Portabilization<Data, N, MakeOutputPositive> 
    where 
        Data: Vector<N> + Portable,
        NdVector<N, i32>: Vector<N, Component = i32>,
            
{
    /// creates a new instance of the portabilization, computes the metadata, and 
    /// writes the metadata to the stream.
    // enum_dispatch does not support associated functions, we explicitly write the
    // constructor.
    pub fn new<W>(att: Attribute, cfg: Config, writer: &mut W) -> Self
        where W: ByteWriter
    {
        debug_write!("Start of Portabilization Metadata", writer);
        // cfg.type_.write_to(writer);
        let out = match cfg.type_ {
            PortabilizationType::QuantizationCoordinateWise => {
                Portabilization::QuantizationCoordinateWise(
                    quantization_coordinate_wise::QuantizationCoordinateWise::<_,N,MakeOutputPositive>::new(att, cfg, writer)
                )
            },
            PortabilizationType::QuantizationRectangleArray => {
                Portabilization::OctahedralQuantization(
                    octahedral_quantization::OctahedralQuantization::new(att, cfg, writer)
                )
            },
            PortabilizationType::ToBits => {
                Portabilization::ToBits(
                    to_bits::ToBits::new(att, cfg, writer)
                )
            },
        };
        debug_write!("End of Portabilization Metadata", writer);
        out
    }

    pub fn portabilize(self) -> Attribute {
        match self {
            Portabilization::QuantizationCoordinateWise(qcw) => qcw.portabilize(),
            Portabilization::OctahedralQuantization(oct) => oct.portabilize(),
            Portabilization::ToBits(tb) => tb.portabilize(),
        }
    }
}

pub trait PortabilizationImpl<const N: usize>
    where 
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// portabilizes the whole data.
    fn portabilize(self) -> Attribute;
}

#[remain::sorted]
#[derive(Clone, Copy, Debug)]
pub enum PortabilizationType {
    QuantizationCoordinateWise,
    QuantizationRectangleArray,
    ToBits,
}

impl PortabilizationType {
    pub(crate) fn get_id(&self) -> u8 {
        match self {
            PortabilizationType::QuantizationCoordinateWise => 0,
            PortabilizationType::QuantizationRectangleArray => 1,
            PortabilizationType::ToBits => 2,
        }
    }

    pub(crate) fn write_to<W>(&self, writer: &mut W) 
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
            type_: PortabilizationType::QuantizationCoordinateWise,
            resolution: Resolution::DivisionSize(1000),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    DivisionSize(u64),
    UnitCubeSize(f64)
}