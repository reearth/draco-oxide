mod attribute_encoder;
mod encoding_group;
mod config;
mod err;

use crate::core::attribute::ComponentDataType;
use crate::core::shared::{ConfigType, DataValue, NdVector};
use crate::core::{attribute::Attribute, buffer::writer::Writer};
use crate::core::buffer::MsbFirst;

use err::Err;

struct AttributeEncoder<'a> {
	att: Attribute,
	cfg: Config,
    faces: &'a [[usize;3]],
} 

impl<'a> AttributeEncoder<'a> {
	fn new(att: Attribute, faces: &'a [[usize;3]]) -> Self {
        AttributeEncoder { att, cfg: Config::default(), faces }
    }

	/// initializes the group manager.
	fn init(&mut self, att: Attribute) {

    }
	
	fn encode(&mut self, writer: &mut Writer<MsbFirst>) -> Result<(), Err> {
		let component_type = self.att.get_component_type();
        match component_type {
            ComponentDataType::F32 => {
                self.unpack_num_components::<f32>(writer)?
            }
            ComponentDataType::F64 => {
                self.unpack_num_components::<f64>(writer)?
            }
            ComponentDataType::U8 => {
                self.unpack_num_components::<u8>(writer)?
            }
            ComponentDataType::U16 => {
                self.unpack_num_components::<u16>(writer)?
            }
            ComponentDataType::U32 => {
                self.unpack_num_components::<u32>(writer)?
            }
            ComponentDataType::U64 => {
                self.unpack_num_components::<u64>(writer)?
            },
            _ => {
                return Err(Err::UnsupportedDataType)
            }
        };
        Ok(())
	}

    fn unpack_num_components<T: DataValue>(&mut self, writer: &mut Writer<MsbFirst>) -> Result<(), Err> {
        let num_components = self.att.get_num_components();
        match num_components {
            0 => unreachable!("Vector of dimension 0 is not allowed"),
            1 => {
                let mut gm: GroupManager<'a, NdVector<1, T>> = GroupManager::new(self.faces);
                gm.compress(&self.att, writer)
            },
            2 => {
                let mut gm: GroupManager<'a, NdVector<2, T>> = GroupManager::new(self.faces);
                gm.compress(&self.att, writer)
            },
            3 => {
                let mut gm: GroupManager<'a, NdVector<3, T>> = GroupManager::new(self.faces);
                gm.compress(&self.att, writer)
            },
            4 => {
                let mut gm: GroupManager<'a, NdVector<4, T>> = GroupManager::new(self.faces);
                gm.compress(&self.att, writer)
            },
            5 => {
                let mut gm: GroupManager<'a, NdVector<5, T>> = GroupManager::new(self.faces);
                gm.compress(&self.att, writer)
            },
            _ => {
                return Err(Err::UnsupportedNumComponents(num_components))
            }
        };
        Ok(())
    }
}

struct Config {

}

impl ConfigType for Config {
    fn default() -> Self {
        Config {}
    }
}


use crate::shared::attribute::prediction_scheme::PredictionScheme;
use crate::shared::attribute::prediction_transform::PredictionTransform;
use crate::shared::attribute::portabilization::Portabilization;
use crate::core::shared::Vector;

trait GroupTrait<Data: Vector> 
{
    fn predict(&mut self, att_val: Data, values_up_till_now: &[Data], parent: Vec<&Attribute>, faces: &[[usize;3]]) -> Data;
    fn prediction_transform(&mut self, att_val: Data) -> Vec<u8>;
    fn portabilize(&mut self, att_val: Data) -> Vec<u8>;
}

struct Group<S, T, P>
    where 
	    S: PredictionScheme,
	    T: PredictionTransform<Data=S::Data>,
	    P: Portabilization,
{
	/// Prediction. Maybe enabled/disabled at user's discretion.
	prediction: Option<(S, T)>,

	portabilization: P
}



use std::ops;

impl<S, T, P, Data> GroupTrait<Data> for Group <S, T, P>
    where 
        S: PredictionScheme<Data = Data>,
        T: PredictionTransform<Data = S::Data>,
        P: Portabilization,
        Data: Vector,
        Data::Component: DataValue,
{
    fn predict(&mut self, att_val: Data, values_up_till_now: &[Data], parent: Vec<&Attribute>, faces: &[[usize;3]]) -> Data {
        self.prediction
            .as_mut()
            .unwrap()
            .0
            .predict(values_up_till_now, parent, faces)
    }

    fn prediction_transform(&mut self, att_val: Data) -> Vec<u8> {
        unimplemented!();
    }

    fn portabilize(&mut self, att_val: Data) -> Vec<u8> {
        unimplemented!();
    }
}

struct GroupManager<'a, Data> {
	partition: Vec<(usize, ops::Range<usize>)>,
	groups: Vec<Box<dyn GroupTrait<Data>>>,
    values_up_till_now: Vec<Data>,
    faces: &'a [[usize; 3]],
}

impl<'a, Data> GroupManager<'a, Data> 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    fn new(faces: &'a [[usize; 3]]) -> Self {
        GroupManager { partition: Vec::new(), groups: Vec::new(), values_up_till_now: Vec::new(), faces }
    }

    fn compress(&mut self, attribute: &Attribute, writer: &mut Writer<MsbFirst>) {
        unimplemented!();
        // for (g_idx, range) in self.partition.iter() {
        //     let group = &mut self.groups[*g_idx];
        //     for att_val_idx in range.clone() {
        //         let att_val = attribute.get::<Data>(att_val_idx);
        //         group.predict(att_val, &self.values_up_till_now, parent, self.faces);
        //     }
        // }

        // for (g_idx, range) in self.partition.iter() {
        //     let group = &mut self.groups[*g_idx];
        //     for att_val_idx in range.clone() {
        //         let att_val = attribute.get::<Data>(att_val_idx);
        //         group.prediction_transform(att_val);
        //     }
        // }

        // for (g_idx, range) in self.partition.iter() {
        //     let group = &mut self.groups[*g_idx];
        //     for att_val_idx in range.clone() {
        //         let att_val = attribute.get::<Data>(att_val_idx);
        //         group.portabilize(att_val);
        //     }
        // }
    }
}


