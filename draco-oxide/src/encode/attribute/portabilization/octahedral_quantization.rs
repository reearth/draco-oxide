use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::attribute::AttributeType;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::geom::float_vector_to_oct;
use draco_oxide_core::codec::attribute::geom::oct_center;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::safety_assert;
use draco_oxide_core::types::AttributeValueIdx;
use draco_oxide_core::types::NdVector;
use draco_oxide_core::types::Vector;

use super::Config;
use super::PortabilizationImpl;

pub struct OctahedralQuantization<Data, const N: usize> {
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
    where
        W: ByteWriter,
    {
        assert!(
            att.get_attribute_type() == AttributeType::Normal,
            "Octahedral quantization can only be applied to normal attributes."
        );

        // Normals are bits-only (octahedral error is angular, not coordinate);
        // `resolve` never consults the range here.
        let quantization_bits = cfg.quantization.resolve(0.0);

        // encode the quantization bits.
        writer.write_u8(quantization_bits);

        Self {
            att,
            quantization_bits,
            _marker: std::marker::PhantomData,
        }
    }

    fn portabilize_value(&mut self, val: Data) -> NdVector<2, i32> {
        let out = float_vector_to_oct(val, oct_center(self.quantization_bits));
        safety_assert!(
            *out.get(0) >= 0 && *out.get(1) >= 0,
            "Octahedral quantized value out of bounds: {:?}",
            out
        );
        out
    }
}

impl<Data, const N: usize> PortabilizationImpl<N> for OctahedralQuantization<Data, N>
where
    Data: Vector<N> + Portable,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn portabilize(mut self) -> Attribute {
        let mut out = Vec::new();
        for i in 0..self.att.num_unique_values() {
            let i = AttributeValueIdx::from(i);
            out.push(self.portabilize_value(self.att.get_unique_val::<Data, N>(i)));
        }
        let mut port_att = Attribute::from_without_removing_duplicates(
            self.att.get_id(),
            out,
            self.att.get_attribute_type(),
            self.att.get_domain(),
            self.att.get_parents().clone(),
        );
        port_att.set_point_to_att_val_map(self.att.take_point_to_att_val_map());
        port_att
    }
}
