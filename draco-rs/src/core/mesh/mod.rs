pub mod builder;
pub mod metadata;

use std::mem;

use super::{attribute::{AttributeType, ComponentDataType, Attribute}, shared::{Float, Vector}};
use crate::{core::shared::{NdVector, VertexIdx}, utils::geom::point_to_face_distance_3d};

#[derive(Debug, Clone)]
pub struct Mesh {
    pub(crate) faces: Vec<[VertexIdx; 3]>,
	pub(crate) attributes: Vec<Attribute>,
}

impl Mesh {
    pub fn get_attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    pub fn get_faces(&self) -> &[[VertexIdx; 3]] {
        &self.faces
    }

    pub fn get_attributes_mut(&mut self) -> &mut[Attribute] {
        &mut self.attributes
    }

    pub fn get_attributes_mut_by_indices<'a>(&'a mut self, indices: &[usize]) -> Vec<&'a mut Attribute> {
        let out = indices.iter()
            .map(|i| &mut self.attributes[*i] as *mut Attribute)
            .collect::<Vec<_>>();

        unsafe {
            let out = out.iter()
                .map(|i| *i)
                .collect::<Vec<_>>();
            std::mem::transmute::<Vec<*mut Attribute>, Vec<&mut Attribute>>(out)
        }
    }

    pub(crate) fn take_attributes(&mut self) -> Vec<Attribute>{
        mem::take(&mut self.attributes)
    }

    pub fn new() -> Self {
        Self {
            faces: Vec::new(),
            attributes: Vec::new(),
        }
    }

    pub(crate) fn add_attribute(&mut self, att: Attribute) {
        if att.get_attribute_type() == AttributeType::Connectivity {
            let mut tmp = vec![att];
            mem::swap(&mut tmp, &mut self.attributes);
            self.attributes.append(&mut tmp);
        } else {
            self.attributes.push(att);
        }
    }

    pub fn diff_l2_norm(&self, other: &Self) -> f64 {
        let pos_att_iter = self.attributes.iter()
            .enumerate()
            .filter(|(_,att)| att.get_attribute_type() == AttributeType::Position);
        let other_pos_att_iter = other.attributes.iter()
            .enumerate()
            .filter(|(_,att)| att.get_attribute_type() == AttributeType::Position);

        let mut num_points = 0;
        let mut sum_of_squared_dist = 0.0;
        for ((_, pos_att), (_, other_pos_att)) in pos_att_iter.zip(other_pos_att_iter) {
            if pos_att.get_num_components() != 3 {
                panic!("Position attribute must have 3 components, but the first mesh has {} components", pos_att.get_num_components());
            }

            let faces = self.attributes.iter()
                .filter(|att| att.get_attribute_type() == AttributeType::Connectivity)
                .find(|att| !att.get_parents().is_empty() )
                .unwrap();
            let faces = unsafe{ faces.as_slice_unchecked::<[usize; 3]>() };

            let other_faces = other.attributes.iter()
                .filter(|att| att.get_attribute_type() == AttributeType::Connectivity)
                .find(|att| !att.get_parents().is_empty() )
                .unwrap();
            let other_faces = unsafe{ other_faces.as_slice_unchecked::<[usize; 3]>() };

            num_points += pos_att.len();
            num_points += other_pos_att.len();
            sum_of_squared_dist += sum_of_squared_dist_unpack_datatype(
                pos_att, 
                faces,
                other_pos_att,
                other_faces
            );
        }

        sum_of_squared_dist.sqrt()/ num_points as f64
    }
}


fn sum_of_squared_dist_unpack_datatype(
    position_att: &Attribute, 
    faces: &[[usize;3]], 
    other_position_att: &Attribute, 
    other_faces: &[[usize;3]]
) -> f64 {
    // Safety:
    // 1. The number of components is checked to be 3.
    // 2. The component type is checked to be f32 or f64.
    unsafe {
        match position_att.get_component_type() {
            ComponentDataType::F32 => sum_of_squared_dist_impl::<f32>(
                position_att, 
                faces,
                other_position_att,
                other_faces
            ) as f64,
            ComponentDataType::F64 => sum_of_squared_dist_impl::<f64>(
                position_att, 
                faces,
                other_position_att,
                other_faces
            ),
            _ => panic!("Position Attribute is not of type f32 or f64")
        }
    }
}

// # Safety: it must be safe to cast the first argument to &[Data]
unsafe fn sum_of_squared_dist_impl<F>(
    self_pos_att: &Attribute, 
    self_faces: &[[usize;3]], 
    other_pos_att: &Attribute, 
    other_faces: &[[usize;3]]
) -> F
    where
        F: Float,
        NdVector<3, F>: Vector<Component = F>,
{
    assert!( 
        other_pos_att.get_component_type() == self_pos_att.get_component_type(),
        "Component types must match, but the first mesh has {:?} and the second mesh has {:?}",
        self_pos_att.get_component_type(),
        other_pos_att.get_component_type()
    );

    if other_pos_att.get_num_components() != 3 {
        panic!("Position attribute must have 3 components, but the second mesh has {} components", other_pos_att.get_num_components());
    }

    // Safety: upheld
    let self_pos_att = self_pos_att.as_slice_unchecked::<NdVector<3, F>>();
    // Satety: Just checked
    let other_pos_att = unsafe{ other_pos_att.as_slice_unchecked::<NdVector<3,F>>() };
        

    let mut sum_of_squared_dist = F::zero();
    for pos in self_pos_att.iter() {
        let min_dist = min_dist_point_to_faces(*pos, other_faces, other_pos_att);
        sum_of_squared_dist += min_dist * min_dist;
    }
    for pos in other_pos_att.iter() {
        let min_dist = min_dist_point_to_faces(*pos, self_faces, self_pos_att);
        sum_of_squared_dist += min_dist * min_dist;
    };

    sum_of_squared_dist.sqrt()
}

fn min_dist_point_to_faces<F>(p: NdVector<3,F>, faces: &[[usize;3]], pos_att: &[NdVector<3,F>]) -> F 
    where 
        F: Float,
{
    let mut min_dist = F::MAX_VALUE;
    for face in faces {
        let v0 = pos_att[face[0]];
        let v1 = pos_att[face[1]];
        let v2 = pos_att[face[2]];
        let dist = point_to_face_distance_3d(p, [v0, v1, v2]);
        if dist < min_dist {
            min_dist = dist;
        }
    }
    min_dist
}