use std::vec::IntoIter;

use crate::core::attribute::ComponentDataType;
use crate::core::shared::{DataValue, Vector};
use crate::prelude::{Attribute, ByteWriter, NdVector};
use crate::shared::attribute::Portable;

use super::{Config, PortabilizationImpl};

pub(crate) struct QuantizationCoordinateWise<Data, const N: usize, const MAKE_OUTPUT_POSITIVE: bool> 
    where Data: Vector<N>
{
    att_vals: IntoIter<Data>,
    port_att: Attribute,
    range_size: Data::Component,
    min_values: Data,
    quantization_bits: usize,
}

impl<Data, const N: usize, const MAKE_OUTPUT_POSITIVE: bool> QuantizationCoordinateWise<Data, N, MAKE_OUTPUT_POSITIVE>
    where 
        NdVector<N, i32>: Vector<N, Component = i32>,
        Data: Vector<N> + Portable,
        Data::Component: DataValue
{
    pub fn new<W>(att: Attribute, _cfg: Config, writer: &mut W) -> Self
    where
        W: ByteWriter,
    {
        let (att_vals, mut empty_att) = att.into_parts::<Data, N>();
        assert!(
            !att_vals.is_empty(),
            "No attribute values provided for quantization."
        );

        empty_att.set_component_type(ComponentDataType::I32);

        let mut min_values = Data::zero();
        for val in &att_vals {
            for i in 0..N {
                let component = val.get(i);
                if component < min_values.get(i) {
                    *min_values.get_mut(i) = *component;
                }
            }
        }

        let mut max_values = Data::zero();
        for val in &att_vals {
            for i in 0..N {
                let component = val.get(i);
                if component > max_values.get(i) {
                    *max_values.get_mut(i) = *component;
                }
            }
        }

        let mut delta_max = Data::Component::zero();
        for i in 0..N {
            let delta = *max_values.get(i) - *min_values.get(i);
            if delta > delta_max {
                delta_max = delta;
            }
        }

        let quantization_bits = 11;

        // write metadata
        println!("min_values: {:?}", min_values);
        min_values.write_to(writer);
        println!("delta_max (range): {:?}", delta_max);
        delta_max.write_to(writer);
        println!("quantization_bits: {}", quantization_bits);
        writer.write_u8(quantization_bits as u8);

        Self {
            att_vals: att_vals.into_iter(),
            port_att: empty_att,
            range_size: delta_max,
            min_values,
            quantization_bits,
        }
    }

    fn portabilize_next(&mut self) -> NdVector<N, i32> {
        let val = self.att_vals.next().expect("No more data values.");
        let diff = val - self.min_values;
        let normalized = diff / self.range_size;
        let quantized = normalized * Data::Component::from_u64((1<<self.quantization_bits)-1);
        let mut out = NdVector::<N, i32>::zero();
        for i in 0..N {
            *out.get_mut(i) = quantized.get(i).to_i64() as i32;
        }
        out
    }
}

impl<Data, const N: usize, const MAKE_OUTPUT_POSITIVE: bool> PortabilizationImpl<N> for QuantizationCoordinateWise<Data, N, MAKE_OUTPUT_POSITIVE>
    where
        NdVector<N, i32>: Vector<N, Component = i32>,
        Data: Vector<N> + Portable,
{
    fn portabilize(mut self) -> Attribute {
        let mut out = Vec::new();
        for _ in 0..self.att_vals.len() {
            out.push(self.portabilize_next());
        }
        self.port_att.set_values(out);
        self.port_att
    }
}