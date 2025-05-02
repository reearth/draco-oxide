use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;

use super::PortabilizationImpl;

pub struct QuantizationSpherical<Data>
    where Data: Vector,
{
    center: Data,
    radius: Data::Component,
}

impl<Data> QuantizationSpherical<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    pub fn new() -> Self {
        Self {
            center: Data::zero(), 
            radius: Data::Component::zero(),
        }
    }

    fn metadata_into_writable_format(&self) -> WritableFormat {
        unimplemented!("Metadata into writable format for QuantizationSpherical is not implemented yet.");
    }

    fn linearize(&self, _data: Vec<Data>, _center: Data) -> WritableFormat {
        unimplemented!("Linearization for QuantizationSpherical is not implemented yet.");
    }
}

impl<Data> PortabilizationImpl for QuantizationSpherical<Data> 
    where 
        Data: Vector + Copy,
{
    type Data = Data;
    const PORTABILIZATION_ID: usize = 1;

    fn portabilize(&mut self, _att_vals: Vec<Self::Data>) -> (WritableFormat, WritableFormat) {
        unimplemented!("Portabilization for QuantizationSpherical is not implemented yet.");
    }
}