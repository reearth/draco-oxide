use std::ops;

use crate::core::attribute::{
    Attribute, AttributeId, AttributeType, ComponentDataType, MaybeInitAttribute 
};

use crate::core::shared::{DataValue, NdVector, Vector};
use crate::debug_expect;
use crate::prelude::ConfigType;
use crate::shared::attribute::{prediction_scheme::PredictionScheme, Portable};

use super::inverse_prediction_transform::InversePredictionTransform;

pub(super) struct AttributeDecoder<'parents, 'stream_in, F> {
    att: MaybeInitAttribute,
	cfg: Config,
    stream_in: &'stream_in mut F,
    parents: Vec<&'parents Attribute>,
}

impl<'parents, 'stream_in, F> AttributeDecoder<'parents, 'stream_in, F> 
    where F: FnMut(u8) -> u64,
{
    pub(super) fn new_and_init(
        cfg: Config,
        stream_in: &'stream_in mut F,
        parents_pool: &'parents [Attribute],
    ) -> Result<Self, Err> {
        debug_expect!("Start of Attribute Metadata", stream_in);
        let id = AttributeId::new(stream_in(16) as usize);
        let att_type = AttributeType::from_id(stream_in(8) as usize);
        let len = stream_in(64) as usize;
        let component_type = ComponentDataType::from_id(stream_in(8) as usize)
            .map_err(|_| Err::ComponentUnwrapErr)?;
        let num_components = stream_in(8) as usize;
        let num_parents = stream_in(8) as usize;
        let mut parents = Vec::with_capacity(num_parents);
        let mut parents_ids = Vec::with_capacity(num_parents);
        for _ in 0..num_parents {
            let parent_id = stream_in(16) as usize;
            parents_ids.push(AttributeId::new(parent_id));
            parents.push(&parents_pool[parent_id]);
        }
        debug_expect!("End of Attribute Metadata", stream_in);

        let att = MaybeInitAttribute::new(
            id,
            att_type,
            len,
            component_type,
            num_components,
            parents_ids,
        );

        Ok( 
            Self {
                att,
                cfg,
                stream_in,
                parents,
            }
        )
    }

    pub(super) fn decode(mut self) -> Result<Attribute, Err> {
        match self.att.get_component_type() {
            ComponentDataType::F32 => {
                self.unpack_num_components::<f32>()?
            }
            ComponentDataType::F64 => {
                self.unpack_num_components::<f64>()?
            }
            ComponentDataType::U8 => {
                self.unpack_num_components::<u8>()?
            }
            ComponentDataType::U16 => {
                self.unpack_num_components::<u16>()?
            }
            ComponentDataType::U32 => {
                self.unpack_num_components::<u32>()?
            }
            ComponentDataType::U64 => {
                self.unpack_num_components::<u64>()?
            }
        };

        Ok(<Attribute as From<MaybeInitAttribute>>::from(self.att))
    }

    fn unpack_num_components<T>(&mut self) -> Result<(), Err>
        where 
            T: DataValue + Copy,
            NdVector<1, T>: Vector,
            NdVector<2, T>: Vector,
            NdVector<3, T>: Vector,
            NdVector<4, T>: Vector
    {
        match self.att.get_num_components() {
            1 => {
                self.decode_impl::<1,_>()
            }
            2 => {
                self.decode_impl::<2,_>()
            }
            3 => {
                self.decode_impl::<3,_>()
            }
            4 => {
                self.decode_impl::<4,_>()
            }
            num_components => {
                Err(Err::GotUnsupportedNumComponents(num_components))
            }
        }
    }

    fn decode_impl<const N: usize, T>(&mut self) -> Result<(), Err>
        where 
            T: DataValue + Copy,
            NdVector<N, T>: Vector + Portable,
    {

        // get groups
        debug_expect!("Start of Transform Metadata", self.stream_in);
        let num_groups = (self.stream_in)(8) as usize;
        let mut groups: Vec<Group<'_, NdVector<N,T>>> = Vec::new();
        for _ in 0..num_groups {
            let group = Group::new(self.stream_in, &self.parents)?;
            groups.push(group);
        }
        debug_expect!("End of Transform Metadata", self.stream_in);

        let mut num_encoded_values = 0;
        while num_encoded_values < self.att.len() {
            debug_expect!("Start of a Range", self.stream_in);
            let group_id = (self.stream_in)(8) as usize;
            let group = &mut groups[group_id];
            let range_size = (self.stream_in)(64) as usize;
            let range = num_encoded_values..(num_encoded_values + range_size);
            group.inverse(range, &mut self.att, self.stream_in);
            num_encoded_values += range_size;
        }
        Ok(())
    }
}

struct Group<'parents, Data> 
    where Data: Vector + Portable
{
    prediction: PredictionScheme<'parents, Data>,
    inverse_transform: InversePredictionTransform<Data>,
}

impl<'parents, Data> Group<'parents, Data> 
    where Data: Vector + Portable
{
    fn new<F>(stream_in: &mut F, parents: &'parents [&Attribute]) -> Result<Self, Err> 
        where F: FnMut(u8) -> u64
    {
        let prediction = PredictionScheme::new_from_stream(stream_in, parents)
            .map_err(|x| Err::InvalidPredictionSchemeId(x))?;
        let inverse_transform = InversePredictionTransform::new(stream_in)
            .map_err(|err| Err::InvalidInversePredictionTransformId(err))?;
        
        Ok(
            Self {
                prediction,
                inverse_transform,
            }
        )
    }

    fn inverse<F>(
        &mut self,
        range: ops::Range<usize>,
        att: &mut MaybeInitAttribute,
        stream_in: &mut F
    )
        where F: FnMut(u8) -> u64
    {
        for idx in range {
            let slice =  unsafe{ &att.as_slice_unchecked::<Data>()[..idx] };
            let pred = self.prediction.predict(slice);
            att.write(idx, self.inverse_transform.inverse(pred, stream_in));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    leave_quantized: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            leave_quantized: false,
        }
    }
}

#[remain::sorted]
#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Component Type Unwrap Failed.")]
    ComponentUnwrapErr,
    #[error("Got unsupported number of components: {0}")]
    GotUnsupportedNumComponents(usize),
    #[error("Invalid Inverse Prediction Transform ID: {0}")]
    InvalidInversePredictionTransformId(#[from] super::inverse_prediction_transform::Err),
    #[error("Invalid Prediction Scheme ID: {0}")]
    InvalidPredictionSchemeId(usize),
}
