use std::vec::IntoIter;
use std::{
    ops, vec
};

use crate::core::attribute::ComponentDataType;
use crate::core::shared::{DataValue, NdVector};
use crate::core::attribute::Attribute;
use crate::debug_write;
use crate::prelude::{AttributeType, ByteWriter, ConfigType};
use crate::shared::attribute::Portable;
use crate::utils::splice_disjoint_indices;
use thiserror::Error;

#[cfg(feature = "evaluation")]
use crate::eval;

#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("Invalid attribute id: {0}")]
    InvalidAttributeId(usize),
    #[error("Invalid prediction scheme id: {0}")]
    InvalidPredictionSchemeId(usize),
    #[error("Attribute Encoder has too many encoding groups: {0}")]
    TooManyEncodingGroups(usize),
    #[error("An attribute has too many parents: {0}")]
    TooManyParents(usize),
    #[error("Unsupported data type.")]
    UnsupportedDataType,
    #[error("Attribute data has too many components; it must be less than {}, but it is {}.", 5, .0)]
    UnsupportedNumComponents(usize),
}

#[derive(Clone, Debug)]
pub struct GroupConfig {
    range: Vec<ops::Range<usize>>,

    pub prediction_scheme: prediction_scheme::Config,
    pub prediction_transform: prediction_transform::Config,
}

impl GroupConfig {
    fn default_with_size(size: usize) -> Self {
        Self {
            range: vec![0..size],
            prediction_scheme: prediction_scheme::Config::default(),
            prediction_transform: prediction_transform::Config::default(),
        }
    }

    fn default_for(att_ty: AttributeType, size: usize) -> Self {
        match att_ty {
            AttributeType::Position => Self {
                range: vec![0..size],
                prediction_scheme: prediction_scheme::Config{
                    ty: prediction_scheme::PredictionSchemeType::MeshParallelogramPrediction,
                    ..prediction_scheme::Config::default()
                },
                prediction_transform: prediction_transform::Config{
                    ty: prediction_transform::PredictionTransformType::Difference,
                    portabilization: portabilization::Config::default(),
                }
            },
            AttributeType::Normal => Self::default_with_size(size),
            AttributeType::Color => Self::default_with_size(size),
            AttributeType::TextureCoordinate => Self::default_with_size(size),
            _ => Self::default_with_size(size),
            
        }
    }
}


pub struct Config {
    group_cfgs: Vec<GroupConfig>,
}


// ToDo: THIS IMPLEMENTATION IS NOT FINAL
impl ConfigType for Config {
    fn default() -> Self {
        Self {
            group_cfgs: Vec::new(),
        }
    }
}

pub(super) struct AttributeEncoder<'parents, 'encoder, 'writer, W> 
{
	att: &'encoder Attribute,
    #[allow(unused)]
	cfg: Config,
    writer: &'writer mut W,
    parents: &'encoder[&'parents Attribute],
}

impl<'parents, 'encoder, 'writer, W> AttributeEncoder<'parents, 'encoder, 'writer, W>
    where 
        W: ByteWriter,
        'parents: 'encoder,
{
	pub(super) fn new(att: &'encoder Attribute, parents: &'encoder[&'parents Attribute], writer: &'writer mut W, cfg: Config) -> Self {
        AttributeEncoder { att, cfg, writer, parents }
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
        let cfg = Config {
            group_cfgs: vec![GroupConfig::default_for(
                self.att.get_attribute_type(),
                self.att.len()
            )],
        };
        let mut gm: GroupManager<'encoder, NdVector<N, T>> = GroupManager::compose_groups(&self.parents, cfg);
        gm.split_unpredicted_values();
        gm.compress::<WRITE_NOW,_>(&self.att, self.writer)?;
        Ok(())
    }
        
}


use crate::shared::attribute::prediction_scheme::{self, PredictionScheme};
use crate::encode::attribute::portabilization;
use crate::core::shared::Vector;
use super::prediction_transform::{self, PredictionTransform};
use crate::encode::attribute::prediction_transform::PredictionTransformImpl;

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

        let prediction_scheme = prediction_scheme::PredictionScheme::new(cfg.prediction_scheme.ty, parents);

        let prediction_transform = PredictionTransform::new(cfg.prediction_transform);

        Self { 
            prediction: prediction_scheme, 
            transform: prediction_transform
        }
    }

    fn split_unpredicted_values(&mut self, values_indices: &mut Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
        let impossible_to_predict = self.prediction
            .get_values_impossible_to_predict(values_indices);
        impossible_to_predict
    }

    fn predict_and_transform(&mut self, ranges: &Vec<ops::Range<usize>>, attribute: &Attribute) {
        for i in ranges.iter().cloned().flatten() {
            let prediction = self.prediction.predict(
                unsafe { &attribute.as_slice_unchecked()[0..i] }
            );
            self.transform.map_with_tentative_metadata(
                attribute.get::<Data>(i),
                prediction
            );
        }
    }

    fn squeeze_transformed_data<W>(&mut self, writer: &mut W)
        where W: ByteWriter
    {
        self.transform.squeeze(writer)
    }

    fn take_output<W>(self, writer: &mut W) -> IntoIter<IntoIter<u8>>
        where W: ByteWriter
    {
        self.transform.out(writer)
    }
}

struct GroupManager<'encoder, Data>
    where 
        Data: Vector + Portable,
        Data::Component: DataValue,
{
	partition: Vec<Vec<ops::Range<usize>>>,
	groups: Vec<Group<'encoder, Data>>,
}

impl <'parents, 'encoder, Data> GroupManager<'encoder, Data> 
    where 
        'parents: 'encoder,
        Data: Vector + Portable,
        Data::Component: DataValue,
{
    fn compose_groups(parents: &'encoder [&'parents Attribute], cfg: Config) -> Self {
        let mut groups = Vec::new();
        for cfg in cfg.group_cfgs.clone() {
            groups.push( Group::from(parents, cfg));
        }
        Self {
            partition: cfg.group_cfgs.iter().map(|cfg| {
                cfg.range.clone()
            }).collect(),
            groups
        }
    }

    fn split_unpredicted_values(&mut self) {
        let mut set_of_value_impossible_to_predict = Vec::new();
        for (group, indices) in &mut self.groups.iter_mut().zip(self.partition.iter_mut()) {
            let values = group.split_unpredicted_values(indices);
            set_of_value_impossible_to_predict.push(values);
        }
        let unpredicted_values = splice_disjoint_indices(set_of_value_impossible_to_predict);
        
        let cfg = prediction_transform::Config{
            ty: prediction_transform::PredictionTransformType::NoTransform,
            portabilization: portabilization::Config{
                type_: portabilization::PortabilizationType::ToBits,
                ..portabilization::Config::default()
            },
            ..prediction_transform::Config::default()
        };
        let group = Group {
            prediction: PredictionScheme::new(prediction_scheme::PredictionSchemeType::NoPrediction, &[]),
            transform: PredictionTransform::new(cfg),
        };
        self.partition.push(unpredicted_values);
        self.groups.push(group);
    }

    #[allow(dead_code)]
    fn partition_iter(&self) -> impl Iterator<Item = (ops::Range<usize>, &Group<'encoder, Data>)> {
        PartitionGroupIter::new(&self.groups, &self.partition)
    }

    #[allow(dead_code)]
    fn partition_iter_mut(&mut self) -> impl Iterator<Item = (ops::Range<usize>, &mut Group<'encoder, Data>)> {
        PartitionGroupIterMut::new(&mut self.groups, &self.partition)
    }

    fn partition_group_idx_iter<'a>(&'a self) -> PartitionGroupIdxIter<'a> {
        PartitionGroupIdxIter::new(&self.partition)
    }    

    fn compress<const WRITE_NOW: bool, W>(&mut self, attribute: &Attribute, writer: &mut W) -> Result<(), Err>
        where W: ByteWriter
    {
        debug_write!("Start of Attribute Metadata", writer);
        // write id
        let id = attribute.get_id().as_usize();
        if id >= 1 << 16 {
            return Err(Err::InvalidAttributeId(id));
        } else {
            writer.write_u16(id as u16);
        };

        // write att type
        let att_type = attribute.get_attribute_type().get_id() as u64;
        writer.write_u8(att_type as u8);
        #[cfg(feature = "evaluation")]
        eval::write_json_pair(
            "attribute type", 
            serde_json::to_value(attribute.get_attribute_type()).unwrap(), 
            writer
        );

        // write the attribbute length
        let length = attribute.len() as u64;
        writer.write_u64(length);
        // for evaluation, write the data size in bytes
        #[cfg(feature = "evaluation")]
        eval::write_json_pair(
            "data size in bytes",
            // data size in bytes
            serde_json::to_value(length * std::mem::size_of::<Data>() as u64).unwrap(), 
            writer
        );

        // write component type
        let component_type = attribute.get_component_type().get_id() as u8;
        writer.write_u8(component_type);
        #[cfg(feature = "evaluation")]
        eval::write_json_pair(
            "component type", 
            serde_json::to_value(attribute.get_component_type()).unwrap(), 
            writer
        );

        // write number of components
        let num_components = attribute.get_num_components();
        if num_components >= 1 << 8 {
            return Err(Err::UnsupportedNumComponents(num_components as usize));
        }
        writer.write_u8(num_components as u8);
        #[cfg(feature = "evaluation")]
        eval::write_json_pair(
            "number of components", 
            serde_json::to_value(num_components).unwrap(), 
            writer
        );

        // write parents
        let num_parents = attribute.get_parents().len();
        if num_parents >= 1 << 8 {
            return Err(Err::TooManyParents(num_parents as usize));
        }
        writer.write_u8(num_parents as u8);
        #[cfg(feature = "evaluation")]
        eval::write_json_pair(
            "number of parents", 
            serde_json::to_value(num_parents).unwrap(), 
            writer
        );
        
        for parent in attribute.get_parents() {
            let parent_id = parent.as_usize();
            if parent_id >= 1 << 16 {
                return Err(Err::InvalidAttributeId(parent_id));
            } else {
                writer.write_u16(parent_id as u16);
            }
        }
        #[cfg(feature = "evaluation")]
        {
            let parents = attribute.get_parents();
            eval::write_json_pair(
                "parents", 
                serde_json::to_value(parents).unwrap(), 
                writer
            );
        }

        debug_write!("End of Attribute Metadata", writer);
        
        // Prediction
        for (ranges, group) in self.partition.iter().zip(self.groups.iter_mut()) {
            group.predict_and_transform(ranges, attribute);
        }

        debug_write!("Start of Transform Metadata", writer);
        // write number of groups
        let num_groups = self.groups.len();
        if num_groups >= 1 << 8 {
            return Err(Err::TooManyEncodingGroups(num_groups));
        }
        writer.write_u8(num_groups as u8);
        // Squeeze the transformed data and write it
        let mut transform_outputs = Vec::new();
        transform_outputs.reserve(self.groups.len());


        #[cfg(feature = "evaluation")]
        eval::array_scope_begin("groups", writer);

        for (mut group, _ranges) in std::mem::take(&mut self.groups).into_iter().zip(self.partition.iter()) {
            #[cfg(feature = "evaluation")]
            {
                eval::scope_begin("group", writer);
                eval::write_json_pair("prediction", group.prediction.get_type().to_string().into(), writer);
                eval::write_json_pair("indices", format!("{:?}", _ranges).into(), writer);
            }

            // write prediction id
            let prediction_id = group.prediction.get_type().get_id();
            if prediction_id >= 1 << 4 {
                return Err(Err::InvalidPredictionSchemeId(prediction_id as usize));
            }
            writer.write_u8(prediction_id);

            debug_write!("Start of Prediction Transform Metadata", writer);
            // write transform id
            let transform_id = group.transform.get_type().get_id();
            if transform_id >= 1 << 4 {
                return Err(Err::InvalidPredictionSchemeId(transform_id as usize));
            }
            writer.write_u8(transform_id);

            
            #[cfg(feature = "evaluation")]
            eval::scope_begin("transform", writer);
            group.squeeze_transformed_data(writer);
            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);
            
            #[cfg(feature = "evaluation")]
            eval::scope_begin("portabilization", writer);
            transform_outputs.push(group.take_output(writer));
            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);

            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);
            
            debug_write!("End of Prediction Transform Metadata", writer);
        }

        #[cfg(feature = "evaluation")]
        eval::array_scope_end(writer);

        debug_write!("End of Transform Metadata", writer);

        for (range, gp_idx) in self.partition_group_idx_iter() {
            debug_write!("Start of a Range", writer);
            writer.write_u8(gp_idx as u8);
            let range_size = range.end - range.start;
            // ToDo: Reduce the size by realizing the fact that range size is always less than the attrubute size.
            writer.write_u64(range_size as u64);
            for _ in range {
                transform_outputs[gp_idx].next().unwrap().for_each(|byte| writer.write_u8(byte));
            }
        }
        Ok(())
    }
}

struct PartitionGroupIdxIter<'groups> {
    curr_pos: usize,
    ranges: &'groups Vec<Vec<ops::Range<usize>>>,
    is_done: bool,
}

impl<'groups> PartitionGroupIdxIter<'groups> {
    fn new(ranges: &'groups Vec<Vec<ops::Range<usize>>>) -> Self {
        Self {
            curr_pos: 0,
            ranges,
            is_done: false,
        }
    }
}

impl<'groups> Iterator for PartitionGroupIdxIter<'groups> {
    type Item = (ops::Range<usize>, usize);
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        let mut out = None;
        for (gp_idx, ranges) in self.ranges.iter().enumerate() {
            if let Some(range) = ranges.iter().find(|r| r.start == self.curr_pos) {
                out = Some(
                    (gp_idx, range.clone())
                );
            }
        }

        match out {
            Some((gp_idx, range)) => {
                self.curr_pos = range.end;
                Some((range, gp_idx))
            },
            None => {
                self.is_done = true;
                None
            }
        }
    }
}

struct PartitionGroupIter<'encoder, 'groups, Data> 
    where Data: Vector + Portable
{
    curr_pos: usize,
    groups: &'groups [Group<'encoder, Data>],
    ranges: &'groups Vec<Vec<ops::Range<usize>>>,
    is_done: bool,
}

impl<'encoder, 'groups, Data> PartitionGroupIter<'encoder, 'groups, Data> 
    where 
        Data: Vector + Portable,
        'encoder: 'groups,
{
    fn new(groups: &'groups [Group<'encoder, Data>], ranges: &'groups Vec<Vec<ops::Range<usize>>>) -> Self {
        Self {
            curr_pos: 0,
            groups,
            ranges,
            is_done: false,
        }
    }
}

impl<'encoder, 'groups, Data> Iterator for PartitionGroupIter<'encoder, 'groups, Data> 
    where Data: Vector + Portable,
{
    type Item = (ops::Range<usize>, &'groups Group<'encoder, Data>);
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        let mut out = None;
        for (gp_idx, ranges) in self.ranges.iter().enumerate() {
            if let Some(range) = ranges.iter().find(|r| r.start == self.curr_pos) {
                out = Some(
                    (gp_idx, range.clone())
                );
            }
        }

        match out {
            Some((gp_idx, range)) => {
                self.curr_pos = range.end;
                Some((range, &self.groups[gp_idx]))
            },
            None => {
                self.is_done = true;
                None
            }
        }
    }
}


struct PartitionGroupIterMut<'encoder, 'groups, Data> 
    where Data: Vector + Portable
{
    curr_pos: usize,
    groups: &'groups mut [Group<'encoder, Data>],
    ranges: &'groups Vec<Vec<ops::Range<usize>>>,
    is_done: bool,
}

impl<'encoder, 'groups, Data> PartitionGroupIterMut<'encoder, 'groups, Data> 
    where 
        Data: Vector + Portable,
        'encoder: 'groups,
{
    fn new(groups: &'groups mut [Group<'encoder, Data>], ranges: &'groups Vec<Vec<ops::Range<usize>>>) -> Self {
        Self {
            curr_pos: 0,
            groups,
            ranges,
            is_done: false,
        }
    }
}

impl<'encoder, 'groups, Data> Iterator for PartitionGroupIterMut<'encoder, 'groups, Data> 
    where 
        Data: Vector + Portable,
        'encoder: 'groups,
{
    type Item = (ops::Range<usize>, &'groups mut Group<'encoder, Data>);
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        let mut out = None;
        for (gp_idx, ranges) in self.ranges.iter().enumerate() {
            if let Some(range) = ranges.iter().find(|r| r.start == self.curr_pos) {
                out = Some(
                    (gp_idx, range.clone())
                );
            }
        }

        match out {
            Some((gp_idx, range)) => {
                self.curr_pos = range.end;
                let group = &mut self.groups[gp_idx] as *mut Group<'encoder, Data>;
                // SAFETY: We ensure that the mutable reference is not used elsewhere.
                Some((range, unsafe { &mut *group }))
            },
            None => {
                self.is_done = true;
                None
            }
        }
    }
}