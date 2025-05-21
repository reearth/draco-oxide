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
        self.dependency_check()?;

        let mut attributes = self.get_sorted_attributes();
        Self::check_attribute_size(&attributes)?;
        Self::check_position_and_connectivity(&mut attributes)?;
        
        Ok(
            Mesh {
                attributes,
            }
        )
    }


    /// Checks if attributes have a valid dependency structure.
    fn dependency_check(&self) -> Result<(), Err> {
        // Check if all attributes has at least minimal dependencies
        for att in &self.attributes {
            if let Some(d) = att.get_attribute_type()
                .get_minimum_dependency()
                .iter() // for each minimum dependency, ...
                .find(|ty| 
                    att.get_parents()
                        .iter() // for each parent id, ...
                        .map(|parent_id| self.attributes.iter().find(|att| &att.get_id() == parent_id ).unwrap()) // for each parent attribute, ...
                        .all(|parent| parent.get_attribute_type() != **ty)
                ) 
            {
                return Err(Err::MinimumDependencyError(att.get_attribute_type(), *d));
            }
        }
        Ok(())
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

    /// Checks if the position attribute values are large enough to be used by the connectivity attributes.
    fn check_position_and_connectivity(atts: &mut Vec<Attribute>) -> Result<(), Err> {
        for pos_att in atts.iter().filter(|att| att.get_attribute_type() == AttributeType::Position) {
            if let Some(conn_att) = pos_att.get_parents().iter().find_map(|parent_id| {
                atts.iter().find(|att| att.get_id() == *parent_id && att.get_attribute_type() == AttributeType::Connectivity)
            }) {
                // Safety: conn_att is a connectivity attribute.
                let conn_att = unsafe{ conn_att.as_slice_unchecked::<[usize;3]>() };
                let max_idx = conn_att.iter().flat_map(|face| face.iter()).copied().max().unwrap_or(0);
                if max_idx >= pos_att.len() {
                    return Err(Err::PositionAndConnectivityNotCompatible(max_idx, pos_att.len()));
                }
            }
        }

        Ok(())
    }


    /// attribute size check
    fn check_attribute_size(attributes: &[Attribute]) -> Result<(), Err> {
        for att in attributes {
            let parents = att.get_parents();
            for parent in parents {
                let parent_att = attributes.iter().find(|att| att.get_id() == *parent).unwrap();
                let parent_len = if parent_att.get_attribute_type() == AttributeType::Connectivity {
                    parent_att.as_slice::<[usize; 3]>().iter()
                        .flat_map(|face| *face)
                        .map(|v|v+1)
                        .max()
                        .unwrap_or(0)
                } else {
                    parent_att.len()
                };
                if att.len() != parent_len {
                    return Err(Err::AttributeSizeError(att.get_id().as_usize(), att.len(), parent_att.get_id().as_usize(), parent_len));
                }
            }
        }
        Ok(())
    }
}


#[remain::sorted]
#[derive(Error, Debug)]
pub enum Err {
    #[error("The attribute {0} has {1} values, but the parent attribute {2} has a size of {3}.")]
    AttributeSizeError(usize, usize, usize, usize),

    #[error("One of the attributes does not meet the minimum dependency; {:?} must depend on {:?}.", .0, .1)]
    MinimumDependencyError(AttributeType, AttributeType),
    
    #[error("The connectivity attribute and the position attribute are not compatible; the connectivity attribute has a maximum index of {0} and the position attribute has a length of {1}.")]
    PositionAndConnectivityNotCompatible(usize, usize),

}