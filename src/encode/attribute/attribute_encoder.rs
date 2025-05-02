use std::{
    mem,
    ops
};

use crate::core::attribute::ComponentDataType;
use crate::core::shared::{DataValue, NdVector};
use crate::core::attribute::Attribute;
use crate::prelude::ConfigType;
use crate::shared::attribute::prediction_scheme::NoPrediction;
use crate::encode::attribute::prediction_transform::{oct_reflection, PredictionTransformType};
use crate::shared::attribute::Portable;
use crate::utils::splice_disjoint_indeces;
use thiserror::Error;

#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("Unsupported data type.")]
    UnsupportedDataType,
    #[error("Input data has too many connected components; it must be less than {}, but it is {}.", 5, .0)]
    UnsupportedNumComponents(usize),
}

#[derive(Clone)]
pub struct GroupConfig {
    /// Attribute ID
    id: usize,

    range: Vec<ops::Range<usize>>,

    pub prediction_scheme: prediction_scheme::Config,
    pub prediction_transform: prediction_transform::Config,
    pub portabilization: portabilization::Config,
}

pub struct Config {
    group_cfgs: Vec<GroupConfig>,
    max_quantization_cube_side_length: f64,
}


// ToDo: THIS IMPLEMENTATION IS NOT FINAL
impl ConfigType for Config {
    fn default() -> Self {
        Self {
            group_cfgs: Vec::new(),
            max_quantization_cube_side_length: 0.0,
        }
    }
}

pub(super) struct AttributeEncoder<'parents, 'encoder, 'b, F> 
{
	att: &'encoder Attribute,
	cfg: Config,
    writer: &'b mut F,
    parents: &'encoder[&'parents Attribute],
}

impl<'parents, 'encoder, 'b, F> AttributeEncoder<'parents, 'encoder, 'b, F>
    where 
        F: FnMut((u8, u64)),
        'parents: 'encoder,
{
	pub(super) fn new(att: &'encoder Attribute, parents: &'encoder[&'parents Attribute], writer: &'b mut F, cfg: Config) -> Self {
        AttributeEncoder { att, cfg, writer, parents }
    }

	/// initializes the group manager.
	pub(super) fn init(&mut self) {

    }
	
	pub(super) fn encode<const WRITE_NOW: bool>(self) -> Result<(), Err> {
        let component_type = self.att.get_component_type();
        match component_type {
            ComponentDataType::F32 => {
                self.unpack_num_components::<WRITE_NOW, f32>()
            }
            ComponentDataType::F64 => {
                self.unpack_num_components::<WRITE_NOW, f64>()
            }
            ComponentDataType::U8 => {
                self.unpack_num_components::<WRITE_NOW, u8>()
            }
            ComponentDataType::U16 => {
                self.unpack_num_components::<WRITE_NOW, u16>()
            }
            ComponentDataType::U32 => {
                self.unpack_num_components::<WRITE_NOW, u32>()
            }
            ComponentDataType::U64 => {
                self.unpack_num_components::<WRITE_NOW, u64>()
            },
            _ => {
                Err(Err::UnsupportedDataType)
            }
        }
	}

    fn unpack_num_components<const WRITE_NOW: bool, T>(self) -> Result<(), Err> 
        where 
            T: DataValue + Copy,
            NdVector<1, T>: Vector,
            NdVector<2, T>: Vector,
            NdVector<3, T>: Vector,
            NdVector<4, T>: Vector
    {
        let num_components = self.att.get_num_components();
        match num_components {
            0 => unreachable!("Vector of dimension 0 is not allowed"),
            1 => {
                self.encode_typed::<WRITE_NOW,1,_>()
            },
            2 => {
                self.encode_typed::<WRITE_NOW,2,_>()
            },
            3 => {
                self.encode_typed::<WRITE_NOW,3,_>()
            },
            4 => {
                self.encode_typed::<WRITE_NOW,4,_>()
            },
            _ => {
                Err(Err::UnsupportedNumComponents(num_components))
            }
        }
    }

    fn encode_typed<const WRITE_NOW: bool, const N: usize, T>(self) -> Result<(), Err> 
        where 
            T: DataValue + Copy,
            NdVector<N, T>: Vector + Portable,
    {
        let mut gm: GroupManager<'encoder, NdVector<N, T>> = GroupManager::compose_groups(&self.parents, self.cfg);
        gm.split_unpredicted_values();
        gm.compress::<WRITE_NOW,_>(&self.att, self.writer);
        Ok(())
    }
        
}


use crate::shared::attribute::prediction_scheme::{self, PredictionScheme};
use crate::encode::attribute::portabilization::{self, Portabilization};
use crate::core::shared::Vector;
use super::prediction_transform::{self, difference, oct_difference, oct_orthogonal, orthogonal, PredictionTransform};
use super::WritableFormat;

struct Group<'encoder, Data>
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
	/// Prediction
	prediction: PredictionScheme<'encoder, Data>, 
    transform: PredictionTransform<Data>,
}


impl<'encoder, Data> Group<'encoder, Data>
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{

    fn from<'parents>(parents: &'encoder[&'parents Attribute], cfg: GroupConfig) -> Self 
        where 'parents: 'encoder
    {
        let attribute = cfg.id;
        let range = cfg.range.clone();

        let prediction_scheme = prediction_scheme::PredictionScheme::new(cfg.prediction_scheme, parents);

        let prediction_transform_cfg = cfg.prediction_transform.clone();
        let prediction_transform = match prediction_transform_cfg.prediction_transform {
            PredictionTransformType::Difference => {
                let prediction_transform = difference::Difference::<Data>::new(cfg.portabilization);
                PredictionTransform::Difference(prediction_transform)
            },
            PredictionTransformType::OctahedralDifference => {
                let prediction_transform = oct_difference::OctahedronDifferenceTransform::<Data>::new(cfg.portabilization);
                PredictionTransform::OctahedralDifference(prediction_transform)
            },
            PredictionTransformType::OctahedralReflection => {
                let prediction_transform = oct_reflection::OctahedronReflectionTransform::<Data>::new(cfg.portabilization);
                PredictionTransform::OctahedralReflection(prediction_transform)
            },
            PredictionTransformType::OctahedralOrthogonal => {
                let prediction_transform = oct_orthogonal::OctahedronOrthogonalTransform::<Data>::new(cfg.portabilization);
                PredictionTransform::OctahedralOrthogonal(prediction_transform)
            },
            PredictionTransformType::Orthogonal => {
                let prediction_transform = orthogonal::OrthogonalTransform::<Data>::new(cfg.portabilization);
                PredictionTransform::Orthogonal(prediction_transform)
            },
            PredictionTransformType::NoTransform => {
                unreachable!("Internal error: prediction transform not supported");
            },
        };

        Self { 
            prediction: prediction_scheme, 
            transform: prediction_transform
        }
    }

    fn split_unpredicted_values(&mut self, values_indeces: &mut Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
        let impossible_to_predict = self.prediction
            .get_values_impossible_to_predict(values_indeces);
        impossible_to_predict
    }

    fn predict(&mut self, values_up_till_now: &[Data]) -> Data {
        self.prediction
            .predict(values_up_till_now)
    }

    fn prediction_transform(&mut self, att_val: Data, prediction: Data) {
        self.transform.map_with_tentative_metadata(att_val, prediction);
    }

    fn squeeze_transformed_data(&mut self) -> (WritableFormat, WritableFormat) {
        self.transform.squeeze()
    }

    fn squeeze_and_write_transformed_data<F>(&mut self, writer: &mut F) -> WritableFormat
        where F: FnMut((u8, u64))
    {
        self.transform.squeeze_and_write(writer)
    }
}

struct GroupManager<'encoder, Data>
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
	partition: Vec<Vec<ops::Range<usize>>>,
	groups: Vec<Group<'encoder, Data>>,
    values_up_till_now: Vec<Data>,
    max_quantization_cube_side_length: f64,
}

impl <'parents, 'encoder, Data> GroupManager<'encoder, Data> 
    where 
        'parents: 'encoder,
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    fn compose_groups(parents: &'encoder [&'parents Attribute], mut cfg: Config) -> Self {
        let mut groups = Vec::new();
        for cfg in mem::take(&mut cfg.group_cfgs) {
            groups.push( Group::from(parents, cfg));
        }
        Self {
            partition: cfg.group_cfgs.iter().map(|cfg| cfg.range.clone()).collect(),
            groups,
            values_up_till_now: Vec::new(),
            max_quantization_cube_side_length: cfg.max_quantization_cube_side_length,
        }
    }

    fn split_unpredicted_values(&mut self) {
        let mut set_of_value_impossible_to_predict = Vec::new();
        for (group, indeces) in &mut self.groups.iter_mut().zip(self.partition.iter_mut()) {
            let values = group.split_unpredicted_values(indeces);
            set_of_value_impossible_to_predict.push(values);
        }
        let unpredicted_values = splice_disjoint_indeces(set_of_value_impossible_to_predict);
        let portabilization = portabilization::quantization_rect_array::QuantizationRectangleArray::<Data>::new(self.max_quantization_cube_side_length);
        let portabilization = Portabilization::QuantizationRectangleArray(portabilization);

        let group = Group {
            prediction: PredictionScheme::NoPrediction(NoPrediction::<Data>::new()),
            transform: PredictionTransform::NoTransform(prediction_transform::NoPredictionTransform::<Data>::new_with_portabilization(portabilization)),
        };
        self.partition.push(unpredicted_values);
        self.groups.push(group);
    }

    fn compress<const WRITE_NOW: bool, F>(&mut self, attribute: &Attribute, writer: &mut F) 
        where F: FnMut((u8, u64))
    {
        let mut writable_metadta = Vec::new();

        let mut predictions = Vec::new();
        predictions.reserve(attribute.len());
        
        // Prediction
        for (range, group) in self.partition.iter().zip(self.groups.iter_mut()) {
            for _att_val_idx in range.clone() {
                let prediction = group.predict(&self.values_up_till_now);
                predictions.push(prediction);
            }
        }

        // Prediction Transform
        let mut prediction_it = predictions.into_iter();
        for (range, group) in self.partition.iter().zip(self.groups.iter_mut()) {
            for att_val_idx in range.iter().cloned().flatten() {
                let att_val = attribute.get::<Data>(att_val_idx);
                let prediction_val = prediction_it.next().unwrap();
                group.prediction_transform(att_val, prediction_val);
            }
        }
        

        let mut transform_outputs = Vec::new();
        transform_outputs.reserve(self.groups.len());
        for group in &mut self.groups {
            let transform_output = if WRITE_NOW {
                group.squeeze_and_write_transformed_data(writer)
            } else {
                let (buffer, metadata) = group.squeeze_transformed_data();
                writable_metadta.push(metadata);
                buffer
            };
            transform_outputs.push(transform_output);
        }
    }
}
