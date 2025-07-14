use crate::core::shared::Vector;
use crate::encode::attribute::prediction_transform::geom::into_faithful_oct_quantization;
use crate::encode::attribute::prediction_transform::geom::octahedral_transform;
use crate::prelude::Attribute;
use crate::prelude::AttributeType;
use crate::prelude::ByteWriter;
use crate::prelude::NdVector;
use crate::shared::attribute::Portable;

use super::Config;
use super::PortabilizationImpl;

pub struct OctahedralQuantization<Data, const N: usize>
{
    /// iterator over the attribute values.
    /// this is not 'Vec<_>' because we want to nicely consume the data.
    att: Attribute,

    /// the size of the quantization
    quantization_bits: u8,

    _marker: std::marker::PhantomData<Data>,
}

impl<Data, const N: usize> OctahedralQuantization<Data, N>
    where 
        Data: Vector<N>,
        NdVector<N, i32>: Vector<N, Component = i32>, 
{
    pub fn new<W>(att: Attribute, cfg: Config, writer: &mut W) -> Self 
        where W: ByteWriter
    {
        assert!(
            att.get_attribute_type()==AttributeType::Normal, 
            "Octahedral quantization can only be applied to normal attributes."
        );

        // encode the quantization bits.
        writer.write_u8(cfg.quantization_bits);

        Self {
            att,
            quantization_bits: cfg.quantization_bits,
            _marker: std::marker::PhantomData,
        }
    }

    fn portabilize_value(&mut self, val: Data) -> NdVector<2, i32> {
        let val_oct = octahedral_transform(val) + NdVector::<2, f32>::from([1.0,1.0]);
        debug_assert!(
            *val_oct.get(0) >= 0.0 && *val_oct.get(0) <= 2.0 &&
            *val_oct.get(1) >= 0.0 && *val_oct.get(1) <= 2.0,
            "Octahedral transformed value out of bounds: {:?}",
            val_oct
        );
        let quantized = val_oct * ((1<<self.quantization_bits-1)-1) as f32;
        let mut out = NdVector::<2, i32>::zero();
        for i in 0..2 {
            *out.get_mut(i) = *quantized.get(i) as i32;
        }
        let out = into_faithful_oct_quantization(out);
        out
    }
}

impl<Data, const N: usize> PortabilizationImpl<N> for OctahedralQuantization<Data,N>
    where
        Data: Vector<N> + Portable,
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn portabilize(mut self) -> Attribute {
        let mut out = Vec::new();
        for i in 0..self.att.num_unique_values() {
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
        port_att.set_vertex_to_att_val_map(self.att.take_vertex_to_att_val_map());
        port_att
    }
}
        