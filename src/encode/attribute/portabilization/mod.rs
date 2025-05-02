pub mod quantization_rect_array;
pub mod quantization_rect_spiral;
pub mod quantization_spherical;

use crate::{core::shared::{ConfigType, Vector}, shared::attribute::Portable};

use super::WritableFormat;

pub enum Portabilization<Data> 
    where Data: Vector
{
    QuantizationRectangleArray(quantization_rect_array::QuantizationRectangleArray<Data>),
    QuantizationRectangleSpiral(quantization_rect_spiral::QuantizationRectangleSpiral<Data>),
    QuantizationSpherical(quantization_spherical::QuantizationSpherical<Data>),
}


impl<Data> Portabilization<Data> 
    where Data: Vector + Portable,
{
    pub(crate) fn new(cfg: Config) -> Self {
        match cfg.type_ {
            PortabilizationType::QuantizationRectangleArray => {
                Portabilization::QuantizationRectangleArray(quantization_rect_array::QuantizationRectangleArray::new(cfg.unit_cube_size))
            }
            PortabilizationType::QuantizationRectangleSpiral => {
                Portabilization::QuantizationRectangleSpiral(quantization_rect_spiral::QuantizationRectangleSpiral::new())
            }
            PortabilizationType::QuantizationSpherical => {
                Portabilization::QuantizationSpherical(quantization_spherical::QuantizationSpherical::new())
            }
        }
    }
    pub(crate) fn portabilize(&mut self, att_vals: Vec<Data>) -> (WritableFormat, WritableFormat) {
        match self {
            Portabilization::QuantizationRectangleArray(x) => x.portabilize(att_vals),
            Portabilization::QuantizationRectangleSpiral(x) => x.portabilize(att_vals),
            Portabilization::QuantizationSpherical(x) => x.portabilize(att_vals),
        }
    }
        
    pub(crate) fn portabilize_and_write_metadata<F>(&mut self, att_vals: Vec<Data>, writer: &mut F) -> WritableFormat
        where F: FnMut((u8, u64))
    {
        match self {
            Portabilization::QuantizationRectangleArray(x) => x.portabilize_and_write_metadata(att_vals, writer),
            Portabilization::QuantizationRectangleSpiral(x) => x.portabilize_and_write_metadata(att_vals, writer),
            Portabilization::QuantizationSpherical(x) => x.portabilize_and_write_metadata(att_vals, writer),
        }
    }
}

pub trait PortabilizationImpl {
    type Data: Vector;

    const PORTABILIZATION_ID: usize = 0;

    /// portabilize the transform output.
    /// The outputs are (output data, metadata)
    fn portabilize(&mut self, att_vals: Vec<Self::Data>) -> (WritableFormat, WritableFormat);
    
    /// portabilize the transform output and write the metadata to the writer.
    fn portabilize_and_write_metadata<F>(&mut self, att_vals: Vec<Self::Data>, writer: &mut F) -> WritableFormat
        where F: FnMut((u8, u64))
        {
            let (output, mut metadata) = self.portabilize(att_vals);
            metadata.write(writer);
            output
        }
}

#[derive(Clone, Copy)]
pub enum PortabilizationType {
    QuantizationRectangleArray,
    QuantizationRectangleSpiral,
    QuantizationSpherical,
}

#[derive(Clone)]
pub struct Config {
    pub type_: PortabilizationType,
    pub unit_cube_size: f64,
}

impl ConfigType for Config {
    fn default()-> Self {
        Config {
            type_: PortabilizationType::QuantizationRectangleArray,
            unit_cube_size: 1e-6,
        }
    }
}