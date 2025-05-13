use std::{
    ops, vec
};

use crate::core::attribute::ComponentDataType;
use crate::core::shared::{DataValue, NdVector};
use crate::core::attribute::Attribute;
use crate::debug_write;
use crate::prelude::ConfigType;
use crate::shared::attribute::Portable;
use crate::utils::splice_disjoint_indices;
use thiserror::Error;

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

pub(super) struct AttributeEncoder<'parents, 'encoder, 'writer, F> 
{
	att: &'encoder Attribute,
	cfg: Config,
    writer: &'writer mut F,
    parents: &'encoder[&'parents Attribute],
}

impl<'parents, 'encoder, 'writer, F> AttributeEncoder<'parents, 'encoder, 'writer, F>
    where 
        F: FnMut((u8, u64)),
        'parents: 'encoder,
{
	pub(super) fn new(att: &'encoder Attribute, parents: &'encoder[&'parents Attribute], writer: &'writer mut F, cfg: Config) -> Self {
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
            group_cfgs: vec![GroupConfig::default_with_size( self.att.len() )],
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
use super::WritableFormat;
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

    fn predict(&mut self, values_up_till_now: &[Data]) -> Data {
        self.prediction
            .predict(values_up_till_now)
    }

    fn prediction_transform(&mut self, att_val: Data, prediction: Data) {
        self.transform.map_with_tentative_metadata(att_val, prediction);
    }

    fn squeeze_transformed_data<F>(&mut self, writer: &mut F)
        where F: FnMut((u8, u64))
    {
        self.transform.squeeze(writer)
    }

    fn get_transform_output<F>(self, writer: &mut F) -> vec::IntoIter<WritableFormat>
        where F: FnMut((u8, u64))
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

    fn partition_iter(&self) -> impl Iterator<Item = (ops::Range<usize>, &Group<'encoder, Data>)> {
        PartitionGroupIter::new(&self.groups, &self.partition)
    }

    fn partition_iter_mut(&mut self) -> impl Iterator<Item = (ops::Range<usize>, &mut Group<'encoder, Data>)> {
        PartitionGroupIterMut::new(&mut self.groups, &self.partition)
    }

    fn partition_group_idx_iter<'a>(&'a self) -> PartitionGroupIdxIter<'a> {
        PartitionGroupIdxIter::new(&self.partition)
    }    

    fn compress<const WRITE_NOW: bool, F>(&mut self, attribute: &Attribute, writer: &mut F) -> Result<(), Err>
        where F: FnMut((u8, u64))
    {
        debug_write!("Start of Attribute Metadata", writer);
        // write id
        let id = attribute.get_id().as_usize();
        if id >= 1 << 16 {
            return Err(Err::InvalidAttributeId(id));
        } else {
            writer((16, id as u64));
        };

        // write att type
        let att_type = attribute.get_attribute_type().get_id() as u64;
        writer((8, att_type));

        // write length
        let length = attribute.len() as u64;
        writer((64, length));

        // write component type
        let component_type = attribute.get_component_type().get_id() as u64;
        writer((8, component_type));

        // write number of components
        let num_components = attribute.get_num_components() as u64;
        if num_components >= 1 << 8 {
            return Err(Err::UnsupportedNumComponents(num_components as usize));
        }
        writer((8, num_components));

        // write parents
        let num_parents = attribute.get_parents().len() as u64;
        if num_parents >= 1 << 8 {
            return Err(Err::TooManyParents(num_parents as usize));
        }
        writer((8, num_parents));
        for parent in attribute.get_parents() {
            let parent_id = parent.as_usize();
            if parent_id >= 1 << 16 {
                return Err(Err::InvalidAttributeId(parent_id));
            } else {
                writer((16, parent_id as u64));
            }
        }

        debug_write!("End of Attribute Metadata", writer);

        let mut predictions = Vec::new();
        predictions.reserve(attribute.len());
        
        // Prediction
        for (ranges, group) in self.partition.iter().zip(self.groups.iter_mut()) {
            for att_val_idx in ranges.iter().cloned().flatten() {
                let prediction = group.predict(unsafe {
                    &attribute.as_slice_unchecked()[0..att_val_idx]
                });
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
        
        debug_write!("Start of Transform Metadata", writer);
        // write number of groups
        let num_groups = self.groups.len() as u64;
        if num_groups >= 1 << 8 {
            return Err(Err::TooManyEncodingGroups(num_groups as usize));
        }
        writer((8, num_groups));
        // Squeeze the transformed data and write it
        let mut transform_outputs = Vec::new();
        transform_outputs.reserve(self.groups.len());
        for mut group in std::mem::take(&mut self.groups) {
            // write prediction id
            let prediction_id = group.prediction.get_type().get_id() as u64;
            if prediction_id >= 1 << 4 {
                return Err(Err::InvalidPredictionSchemeId(prediction_id as usize));
            }
            writer((4, prediction_id));

            debug_write!("Start of Prediction Transform Metadata", writer);
            // write transform id
            let transform_id = group.transform.get_type().get_id() as u64;
            if transform_id >= 1 << 4 {
                return Err(Err::InvalidPredictionSchemeId(transform_id as usize));
            }
            writer((4, transform_id));

            group.squeeze_transformed_data(writer);

            transform_outputs.push(group.get_transform_output(writer));
            debug_write!("End of Prediction Transform Metadata", writer);
        }
        debug_write!("End of Transform Metadata", writer);

        


        for (range, idx) in self.partition_group_idx_iter() {
            debug_write!("Start of a Range", writer);
            writer((8, idx as u64));
            let range_size = range.end - range.start;
            // ToDo: Reduce the size by realizing the fact that range size is always less than the attrubute size.
            writer((64, range_size as u64));
            for _ in range {
                transform_outputs[idx].next().unwrap().write(writer);
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