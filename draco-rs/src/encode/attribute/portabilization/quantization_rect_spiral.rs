use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;

use super::PortabilizationImpl;

pub struct QuantizationRectangleSpiral<Data>
    where Data: Vector,
{
    center: Data,
    radius: Data::Component,
}

impl<Data> QuantizationRectangleSpiral<Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    pub fn new<F>(_att_vals: Vec<Data>, _cfg: super::Config, _writer: &mut F) -> Self 
        where F:FnMut((u8, u64)) 
    {
        unimplemented!()
    }

    fn metadata_into_writable_format(&self) -> WritableFormat {
        unimplemented!()

        // let mut out = Vec::new();
        // out.reserve(Data::NUM_COMPONENTS * 2);
        // for i in 0..Data::NUM_COMPONENTS {
        //     out.push((64, self.center.get(i).to_bits() as usize));
        // }
        // out.push((64, self.radius.to_bits() as usize));
        // for i in 0..Data::NUM_COMPONENTS {
        //     out.push((32, self.quantization_size[i]));
        // }
        // WritableFormat::from(out)
    }

    fn linearize(&self, _data: Vec<Data>, _center: Data) -> WritableFormat {
        unimplemented!()
        // let size = self.quantization_size.iter().product::<usize>();
        // let out = data.into_iter()
        //     .map(|x| {
        //         let mut val = 0;
        //         for i in 0..Data::NUM_COMPONENTS {
        //             let component = *x.get(i);
        //             let max_component = self.quantization_size[i];
        //             val += component.to_u64() as usize * max_component;
        //         }
        //         (size, val)
        //     })
        //     .collect::<Vec<_>>();

        // WritableFormat::from(out)
    }
}

impl<Data> PortabilizationImpl for QuantizationRectangleSpiral<Data> 
    where Data: Vector + Copy,
{
    fn portabilize(self) -> std::vec::IntoIter<WritableFormat> {
        unimplemented!()
    }
}