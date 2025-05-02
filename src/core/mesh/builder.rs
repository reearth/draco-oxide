use std::mem;

use thiserror::Error;

use crate::core::attribute::{
    Attribute, 
    AttributeType,
    AttributeId,
};
use crate::core::shared::Vector;
use super::Mesh;

pub struct MeshBuilder {
    attributes: Vec<Attribute>,
    current_id: usize,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self {
            attributes: Vec::new(),
            current_id: 0,
        }
    }

    pub fn add_attribute<Data: Vector>(&mut self, data: Vec<Data>, att_type: AttributeType, parents: Vec<AttributeId>) -> AttributeId {
        self.attributes.push(
            Attribute::from(AttributeId::new(self.current_id), data, att_type, parents)
        );
        let id = self.current_id;
        self.current_id += 1;
        AttributeId::new(id)
    }

    pub fn add_connectivity_attribute(&mut self, data: Vec<[usize; 3]>, parents: Vec<AttributeId>) -> AttributeId {
        self.attributes.push(
            Attribute::from_faces(AttributeId::new(self.current_id), data, parents)
        );
        let id = self.current_id;
        self.current_id += 1;
        AttributeId::new(id)
    }

    pub fn build(self) -> Result<Mesh, Err> {
        let mut attributes = self.get_sorted_attributes();
        Self::preprocess_connectivity(&mut attributes);

        // check if it has the point attribute
        if attributes.iter().any(|att| att.get_attribute_type() != AttributeType::Position) {
            return Err(Err::NoPointAttribute);
        }
        
        Ok(
            Mesh {
                attributes,
            }
        )
    }


    /// Sorts the attributes in a way that the parent attributes are before their children.
    /// Furthermore, all connectivity attributes are moved to the front of the list.
    fn get_sorted_attributes(mut self) -> Vec<Attribute> {
        let mut sorted = Vec::new();
        let mut original = mem::take(&mut self.attributes);

        // First, we move all connectivity attributes to the front of the list
        original = original
            .into_iter()
            .filter_map(|att| {
                if att.get_attribute_type() == AttributeType::Connectivity {
                    sorted.push(att);
                    None
                } else {
                    Some(att)
                }
            })
            .collect();

        while !original.is_empty() {
            original = original
                .into_iter()
                .filter_map(|att| {
                    if att.get_parents().iter().all(|&p| sorted.iter().any(|att| p == att.get_id())) {
                        sorted.push(att);
                        None
                    } else {
                        Some(att)
                    }
                })
                .collect();            
        }

        sorted
    }

    fn preprocess_connectivity(atts: &mut Vec<Attribute>) {
        for conn_att in atts
                .iter_mut()
                .take_while(|att| att.get_attribute_type() == AttributeType::Connectivity) 
        {
            let faces = unsafe { conn_att.as_slice_unchecked_mut::<[usize; 3]>() };

            let mut vertices = faces.iter()
                .flat_map(|face| face)
                .copied()
                .collect::<Vec<_>>();
            vertices.sort();
            vertices.dedup();

            for face in faces    {
                for v in face {
                    let new_v = vertices.binary_search(v).unwrap();
                    *v = new_v;
                }
            }
        }
    }
}


#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("The mesh does not have a point attribute.")]
    NoPointAttribute,
}