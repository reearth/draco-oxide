use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::attribute::ComponentDataType;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::types::DataValue;
use draco_oxide_core::types::NdVector;
use draco_oxide_core::types::{AttributeValueIdx, Vector};

use super::Config;
use super::PortabilizationImpl;

pub struct ToBits<Data, const N: usize>
where
    Data: Vector<N>,
{
    /// The attribute to portabilize.
    att: Attribute,

    _marker: std::marker::PhantomData<Data>,
}

impl<Data, const N: usize> ToBits<Data, N>
where
    Data: Vector<N> + Portable,
    Data::Component: DataValue,
{
    pub fn new<W>(att: Attribute, _cfg: Config, _writer: &mut W) -> Self
    where
        W: ByteWriter,
    {
        Self {
            att,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data, const N: usize> PortabilizationImpl<N> for ToBits<Data, N>
where
    Data: Vector<N> + Portable,
    Data::Component: DataValue,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn portabilize(self) -> Attribute {
        // Convert all values to i32 if it is not already i32.

        if self.att.get_component_type() == ComponentDataType::I32 {
            return self.att;
        }

        let mut out = Vec::with_capacity(self.att.num_unique_values());
        for i in 0..self.att.num_unique_values() {
            let i = AttributeValueIdx::from(i);
            let val: Data = self.att.get_unique_val(i);
            let mut converted = NdVector::<N, i32>::zero();
            for j in 0..N {
                *converted.get_mut(j) = val.get(j).to_i64() as i32;
            }
            out.push(converted);
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
