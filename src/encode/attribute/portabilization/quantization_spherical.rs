use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;

use super::Config;
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
    pub fn new<F>(_att_vals: Vec<Data>, _cfg: Config, _writer: &mut F) -> Self 
        where F:FnMut((u8, u64)) 
    {
        unimplemented!()
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
    fn portabilize(self) -> std::vec::IntoIter<WritableFormat> {
        unimplemented!()
    }
}