pub mod quantization;
use crate::core::shared::{ConfigType, Vector};
use crate::core::buffer::{writer::Writer, MsbFirst};

pub trait Portabilization {
    type Data: Vector;
    fn skim(&mut self, data: &[Self::Data]);
    fn portabilize_and_encode(&mut self, att_val: Self::Data, writer: &mut Writer<MsbFirst>);
}

#[derive(Clone, Copy)]
pub enum PortabilizationType {
    Quantization,
}

#[derive(Clone, Copy)]
pub struct Config {
    pub portabilization: PortabilizationType,
    pub bit_length: u8,
}

impl ConfigType for Config {
    fn default()-> Self {
        Config {
            portabilization: PortabilizationType::Quantization,
            bit_length: 8,
        }
    }
}