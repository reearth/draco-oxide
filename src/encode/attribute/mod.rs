mod attribute_encoder;
mod encoding_group;
mod config;
mod err;

use std::mem;

use crate::core::attribute::ComponentDataType;
use crate::core::shared::{DataValue, NdVector};
use crate::core::{attribute::Attribute, buffer::writer::Writer};
use crate::core::buffer::MsbFirst;
use crate::shared::attribute::portabilization::quantization;
use crate::shared::attribute::prediction_scheme::NoPrediction;
use crate::shared::attribute::prediction_transform::NoPredictionTransform;
use crate::utils::splice_disjoint_indeces;

use err::Err;


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
}

struct AttributeEncoder<'a> {
	att: Attribute,
	cfg: Config,
    faces: &'a [[usize;3]],
} 

impl<'a> AttributeEncoder<'a> {
	fn new(att: Attribute, faces: &'a [[usize;3]], cfg: Config) -> Self {
        AttributeEncoder { att, cfg, faces }
    }

	/// initializes the group manager.
	fn init(&mut self, att: Attribute) {

    }
	
	fn encode(&mut self, writer: &mut Writer<MsbFirst>) -> Result<(), Err> {
        let component_type = self.att.get_component_type();
        match component_type {
            ComponentDataType::F32 => {
                self.unpack_num_components::<f32>(writer)
            }
            ComponentDataType::F64 => {
                self.unpack_num_components::<f64>(writer)
            }
            ComponentDataType::U8 => {
                self.unpack_num_components::<u8>(writer)
            }
            ComponentDataType::U16 => {
                self.unpack_num_components::<u16>(writer)
            }
            ComponentDataType::U32 => {
                self.unpack_num_components::<u32>(writer)
            }
            ComponentDataType::U64 => {
                self.unpack_num_components::<u64>(writer)
            },
            _ => {
                Err(Err::UnsupportedDataType)
            }
        }
	}

    fn unpack_num_components<T>(&mut self, writer: &mut Writer<MsbFirst>) -> Result<(), Err> 
        where 
            T: DataValue + Copy + 'static,
            NdVector<1, T>: Vector,
            NdVector<2, T>: Vector,
            NdVector<3, T>: Vector,
            NdVector<4, T>: Vector
    {
        let num_components = self.att.get_num_components();
        match num_components {
            0 => unreachable!("Vector of dimension 0 is not allowed"),
            1 => {
                let gm: GroupManager<'a, '_, NdVector<1, T>> = GroupManager::new(self.faces);
                self.encode_typed(writer, gm)
            },
            2 => {
                let gm: GroupManager<'a, '_, NdVector<2, T>> = GroupManager::new(self.faces);
                self.encode_typed(writer, gm)
            },
            3 => {
                let gm: GroupManager<'a,'_, NdVector<3, T>> = GroupManager::new(self.faces);
                self.encode_typed(writer, gm)
            },
            4 => {
                let gm: GroupManager<'a, '_, NdVector<4, T>> = GroupManager::new(self.faces);
                self.encode_typed(writer, gm)
            },
            _ => {
                Err(Err::UnsupportedNumComponents(num_components))
            }
        }
    }

    fn encode_typed<const N: usize, T>(&mut self, writer: &mut Writer<MsbFirst>, mut gm: GroupManager<NdVector<N, T>>) -> Result<(), Err> 
        where 
            T: DataValue + Copy + 'static,
            NdVector<N, T>: Vector,
    {
        let mut group_cfgs = Vec::new();
        mem::swap(&mut group_cfgs, &mut self.cfg.group_cfgs);
        gm.compose_groups(group_cfgs.into_iter());
        gm.split_unpredicted_values();
        gm.compress(&self.att, writer);
        Ok(())
    }
        
}


use crate::shared::attribute::prediction_scheme::{self, PredictionScheme};
use crate::shared::attribute::prediction_transform::{self, difference, PredictionTransform};
use crate::shared::attribute::portabilization::{self, Portabilization, PortabilizationType};
use crate::core::shared::Vector;

trait GroupTrait<Data: Vector> 
{
    fn split_unpredicted_values(&mut self, value_indeces: &mut Vec<std::ops::Range<usize>>, faces: &[[usize;3]]) -> Vec<std::ops::Range<usize>>;
    fn predict(&mut self, values_up_till_now: &[Data], parent: &Vec<&Attribute>, faces: &[[usize;3]]) -> Data;
    fn prediction_transform(&mut self, att_val: Data, prediction: Data);
    fn squeeze_transformed_data(&mut self);
    fn skim(&mut self);
    fn portabilize(&mut self, att_idx: usize, writer: &mut Writer<MsbFirst>);
}

struct Group<S, T, P>
    where 
	    S: PredictionScheme,
	    T: PredictionTransform<Data=S::Data>,
	    P: Portabilization,
{
	/// Prediction. Maybe enabled/disabled at user's discretion.
	prediction: Option<(S, T)>,

	portabilization: P,

    /// this srores the corrections from the prediction transform.
    /// it is into iter but not vec; in this way we can nicely
    /// deallocate the memory when portabilization is done.
    prediction_correction: std::vec::IntoIter<T::Correction>,
}



use core::panic;
use std::ops;

impl<S, T, P, Data> GroupTrait<Data> for Group <S, T, P>
    where 
        S: PredictionScheme<Data = Data>,
        T: PredictionTransform<Data = Data>,
        P: Portabilization<Data = T::Correction>,
        Data: Vector,
        Data::Component: DataValue,
{
    fn split_unpredicted_values(&mut self, values_indeces: &mut Vec<std::ops::Range<usize>>, faces: &[[usize;3]]) -> Vec<std::ops::Range<usize>> {
        let mut impossible_to_predict = self.prediction
            .as_mut()
            .unwrap()
            .0
            .get_values_impossible_to_predict(values_indeces, faces);
        impossible_to_predict
    }
    fn predict(&mut self, values_up_till_now: &[Data], parents: &Vec<&Attribute>, faces: &[[usize;3]]) -> Data {
        self.prediction
            .as_mut()
            .unwrap()
            .0
            .predict(values_up_till_now, parents, faces)
    }

    fn prediction_transform(&mut self, att_val: Data, prediction: Data) {
        self.prediction
            .as_mut()
            .unwrap()
            .1
            .map_with_tentative_metadata(att_val, prediction);
    }

    fn squeeze_transformed_data(&mut self) {
        self.prediction_correction = self.prediction
            .as_mut()
            .unwrap()
            .1
            .squeeze()
            .into_iter();
    }

    fn skim(&mut self) {
        self.portabilization
            .skim(&self.prediction_correction.as_slice());
    }

    fn portabilize(&mut self, att_idx: usize, writer: &mut Writer<MsbFirst>) {
        let att_val = self.prediction_correction.next().unwrap();
        self.portabilization.portabilize_and_encode(att_val, writer);
    }
}

struct GroupManager<'a, 'b, Data> {
	partition: Vec<(usize, Vec<ops::Range<usize>>)>,
	groups: Vec<Box<dyn GroupTrait<Data>>>,
    values_up_till_now: Vec<Data>,
    parents: Vec<&'b Attribute>,
    faces: &'a [[usize; 3]],
}

impl<'a, 'b, Data> GroupManager<'a, 'b, Data> 
    where 
        Data: Vector,
        Data::Component: DataValue,
{
    fn new(faces: &'a [[usize; 3]]) -> Self {
        GroupManager { 
            partition: Vec::new(), 
            groups: Vec::new(), 
            values_up_till_now: Vec::new(),
            parents: Vec::new(), 
            faces 
        }
    }
}

impl <'a, 'b, const N: usize, D> GroupManager<'a, 'b, NdVector<N, D>> 
    where 
        D: DataValue + 'static,   
        NdVector<N,D>: Vector,
{
    fn compose_groups(&mut self, cfgs: impl Iterator<Item = GroupConfig>) {
        for cfg in cfgs {
            self.unpack_stp(cfg);
        }
    }

    fn unpack_stp(
        &mut self,
        cfg: GroupConfig,
    ) 
        where 
            NdVector<N,D>: Vector,
    {
        let prediction_scheme_cfg = cfg.prediction_scheme.clone();
        match prediction_scheme_cfg.prediction_scheme {
            prediction_scheme::PredictionSchemeType::DeltaPrediction => {
                use prediction_scheme::delta_prediction::DeltaPrediction;
                let prediction_scheme: DeltaPrediction<NdVector<N,D>> = DeltaPrediction::new();
                self.unpack_tp(prediction_scheme, cfg);
            },
            prediction_scheme::PredictionSchemeType::MeshParallelogramPrediction => {
                use prediction_scheme::mesh_parallelogram_prediction::MeshParallelogramPrediction;
                let prediction_scheme: MeshParallelogramPrediction<NdVector<N,D>> = MeshParallelogramPrediction::new();
                self.unpack_tp(prediction_scheme, cfg);
            },
            prediction_scheme::PredictionSchemeType::MeshMultiParallelogramPrediction => {
                use prediction_scheme::mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction;
                let prediction_scheme: MeshMultiParallelogramPrediction<NdVector<N, D>> = MeshMultiParallelogramPrediction::new();
                self.unpack_tp(prediction_scheme, cfg);
            },
            _ => {
                panic!("Unsupported prediction scheme");
            }
        }
    }

    fn unpack_tp<S>(
        &mut self,
        prediction_scheme: S, 
        cfg: GroupConfig
    ) 
        where 
            S: PredictionScheme<Data = NdVector<N,D>> + 'static,
            D: 'static,
            NdVector<N,D>: Vector,
            
    {
        let prediction_transform_cfg = cfg.prediction_transform.clone();
        match prediction_transform_cfg.prediction_transform {
            prediction_transform::PredictionTransformType::Difference => {
                let prediction_transform: difference::Difference<_> = difference::Difference::new();
                self.unpack_p(prediction_scheme, prediction_transform, cfg);
            },
            _ => {
                panic!("Unsupported prediction transform");
            }
        };
    }


    fn unpack_p<S,T>(
        &mut self,
        prediction_scheme: S, 
        prediction_transform: T, 
        cfg: GroupConfig
    ) 
        where 
            S: PredictionScheme<Data = NdVector<N,D>> + 'static,
            T: PredictionTransform<Data = NdVector<N,D>> + 'static,
            NdVector<N,D>: Vector,
    {
        let portabilization_cfg = cfg.portabilization.clone();
        match portabilization_cfg.portabilization {
            PortabilizationType::Quantization => {
                let portabilization = quantization::Quantization::<T::Correction>::new(
                    portabilization_cfg.bit_length
                );
                self.create_group(prediction_scheme, prediction_transform, portabilization, cfg);
            },
            _ => {
                panic!("Unsupported portabilization");
            }
        };
    }

    fn create_group<'c, S,T,P>(
        &'c mut self,
        prediction_scheme: S,
        prediction_transform: T,
        portabilization: P,
        cfg: GroupConfig
    ) 
        where 
            S: PredictionScheme<Data = NdVector<N,D>> + 'static,
            T: PredictionTransform<Data = NdVector<N,D>> + 'static,
            P: Portabilization<Data = T::Correction> + 'static,
    {
        let group = Group {
            prediction: Some((prediction_scheme, prediction_transform)),
            portabilization,
            prediction_correction: Vec::new().into_iter(),
        };
        self.groups.push(Box::new(group));
        self.partition.push((cfg.id, cfg.range));
    }

    fn create_group_without_prediction<P>(
        &mut self,
        portabilization: P,
        cfg: GroupConfig
    ) 
        where 
            P: Portabilization<Data = NdVector<N,D>> + 'static,
    {
        let group: Group<NoPrediction<NdVector<N,D>>, NoPredictionTransform<NdVector<N,D>>, P> = Group {
            prediction: None,
            portabilization,
            prediction_correction: Vec::new().into_iter(),
        };
        self.groups.push(Box::new(group));
        self.partition.push((cfg.id, cfg.range));
    }

    fn create_group_without_prediction_with_ranges<P>(
        &mut self,
        portabilization: P,
        ranges: Vec<ops::Range<usize>>,
    ) 
        where 
            P: Portabilization<Data = NdVector<N,D>> + 'static,
    {
        let group: Group<NoPrediction<NdVector<N,D>>, NoPredictionTransform<NdVector<N,D>>, P> = Group {
            prediction: None,
            portabilization,
            prediction_correction: Vec::new().into_iter(),
        };
        self.groups.push(Box::new(group));
        self.partition.push((self.groups.len(), ranges));
    }

    fn split_unpredicted_values(&mut self) {
        let mut set_of_value_impossible_to_predict = Vec::new();
        for (group, indeces) in &mut self.groups.iter_mut().zip(self.partition.iter_mut().map(|x| &mut x.1)) {
            let values = group.split_unpredicted_values(indeces, self.faces);
            set_of_value_impossible_to_predict.push(values);
        }
        let unpredicted_values = splice_disjoint_indeces(set_of_value_impossible_to_predict);
        let portabilization = portabilization::quantization::Quantization::<NdVector<N,D>>::new(8);
        self.create_group_without_prediction_with_ranges(portabilization, unpredicted_values);
    }

    fn compress(&mut self, attribute: &Attribute, writer: &mut Writer<MsbFirst>) {
        let mut predictions = Vec::new();
        predictions.reserve(attribute.len());
        
        // Prediction
        for (g_idx, range) in self.partition.iter() {
            let group = &mut self.groups[*g_idx];
            for _att_val_idx in range.clone() {
                let prediction = group.predict(&self.values_up_till_now, &self.parents, self.faces);
                predictions.push(prediction);
            }
        }

        // Prediction Transform
        let mut prediction_it = predictions.into_iter();
        for (g_idx, range) in self.partition.iter() {
            let group = &mut self.groups[*g_idx];
            for att_val_idx in range.iter().cloned().flatten() {
                let att_val = attribute.get::<NdVector<N,D>>(att_val_idx);
                let prediction_val = prediction_it.next().unwrap();
                group.prediction_transform(att_val, prediction_val);
            }
        }
        
        for group in &mut self.groups {
            group.squeeze_transformed_data();
        }

        for group in &mut self.groups {
            group.skim();
        }

        // Portabilization
        for (g_idx, range) in self.partition.iter() {
            let group = &mut self.groups[*g_idx];
            for att_val_idx in range.clone().into_iter().flatten() {
                group.portabilize(att_val_idx, writer);
            }
        }
    }
}
