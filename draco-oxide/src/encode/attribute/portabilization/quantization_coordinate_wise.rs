use crate::core::shared::{AttributeValueIdx, DataValue, Vector};
use crate::prelude::{Attribute, ByteWriter, NdVector};
use crate::shared::attribute::Portable;

use super::{Config, PortabilizationImpl};

pub(crate) struct QuantizationCoordinateWise<Data, const N: usize> 
    where Data: Vector<N>
{
    att: Attribute,
    range_size: f32,
    min_values: NdVector<N, f32>,
    quantization_bits: u8,
    _phantom: std::marker::PhantomData<Data>,
}

impl<Data, const N: usize> QuantizationCoordinateWise<Data, N>
    where 
        NdVector<N, i32>: Vector<N, Component = i32>,
        NdVector<N, f32>: Vector<N, Component = f32> + Portable,
        Data: Vector<N> + Portable,
        Data::Component: DataValue
{
    pub fn new<W>(att: Attribute, cfg: Config, writer: &mut W) -> Self
    where
        W: ByteWriter,
    {
        let mut min_values = NdVector::<N,f32>::zero();
        for val in att.unique_vals_as_slice::<Data>() {
            for i in 0..N {
                let component = val.get(i).to_f64() as f32;
                if component < *min_values.get(i) {
                    *min_values.get_mut(i) = component;
                }
            }
        }

        let mut max_values = NdVector::<N,f32>::zero();
        for val in att.unique_vals_as_slice::<Data>() {
            for i in 0..N {
                let component = val.get(i).to_f64() as f32;
                if component > *max_values.get(i) {
                    *max_values.get_mut(i) = component;
                }
            }
        }

        let mut delta_max = 0.0;
        for i in 0..N {
            let delta = *max_values.get(i) - *min_values.get(i);
            if delta > delta_max {
                delta_max = delta;
            }
        }

        // write metadata
        min_values.write_to(writer);
        delta_max.write_to(writer);
        writer.write_u8(cfg.quantization_bits);

        Self {
            att,
            range_size: delta_max,
            min_values,
            quantization_bits: cfg.quantization_bits,
            _phantom: std::marker::PhantomData,
        }
    }

    fn portabilize_value(&mut self, val: Data) -> NdVector<N, i32> {
        // convert value to float vector TODO: implement the vector conversion so that this will be one line
        let val: NdVector<N, f32> = {
            let mut out = NdVector::<N, f32>::zero();
            for i in 0..N {
                *out.get_mut(i) = val.get(i).to_f64() as f32;
            }
            out
        };
        let diff = val - self.min_values;
        let normalized = if self.range_size==0.0 {
            diff 
        } else {
            diff / self.range_size
        };
        let quantized = normalized * f32::from_u64((1<<self.quantization_bits)-1);
        let mut out = NdVector::<N, i32>::zero();
        for i in 0..N {
            *out.get_mut(i) = (*quantized.get(i) + 0.5).to_i64() as i32;
        }
        out
    }
}

impl<Data, const N: usize> PortabilizationImpl<N> for QuantizationCoordinateWise<Data, N>
    where
        NdVector<N, i32>: Vector<N, Component = i32>,
        NdVector<N, f32>: Vector<N, Component = f32> + Portable,
        Data: Vector<N> + Portable,
{
    fn portabilize(mut self) -> Attribute {
        let mut out = Vec::new();
        for i in 0..self.att.num_unique_values() {
            let i = AttributeValueIdx::from(i);
            out.push(self.portabilize_value(
                self.att.get_unique_val::<Data, N>(i)
            ));
        }
        let mut port_att = Attribute::from_without_removing_duplicates(
            self.att.get_id(),
            out, 
            self.att.get_attribute_type(),
            self.att.get_domain(), 
            self.att.get_parents().clone()
        );
        port_att.set_point_to_att_val_map(self.att.take_point_to_att_val_map());
        port_att
    }
}