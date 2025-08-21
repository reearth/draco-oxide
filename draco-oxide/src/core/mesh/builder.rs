use std::usize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use gltf::accessor::Dimensions;
use thiserror::Error;

use crate::core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType
};
use crate::core::shared::{PointIdx, VecPointIdx, Vector};
use crate::prelude::NdVector;
use super::Mesh;

pub struct MeshBuilder {
    pub attributes: Vec<Attribute>,
    faces: Vec<[usize; 3]>,
    current_id: usize,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self {
            attributes: Vec::new(),
            current_id: 0,
            faces: Vec::new(),
        }
    }

    pub fn add_attribute<Data, const N: usize>(&mut self, data: Vec<Data>, att_type: AttributeType, domain: AttributeDomain, parents: Vec<AttributeId>) -> AttributeId
        where Data: Vector<N>
    {
        let unique_id = AttributeId::new(self.current_id);
        self.attributes.push(
            Attribute::from(unique_id, data, att_type, domain, parents)
        );
        self.current_id += 1;
        unique_id
    }

    pub fn add_gltf_empty_attribute(&mut self, att_type: AttributeType, domain: AttributeDomain, component_type: ComponentDataType, ty: Dimensions) -> AttributeId {
        let num_components = ty.multiplicity();
        let unique_id = AttributeId::new(self.current_id);
        let att = Attribute::new_empty(unique_id, att_type, domain, component_type, num_components);
        self.attributes.push(att);
        self.current_id += 1;
        unique_id
    }

    pub fn add_empty_attribute(&mut self, att_type: AttributeType, domain: AttributeDomain, component_type: ComponentDataType, num_component: usize) -> AttributeId {
        let unique_id = AttributeId::new(self.current_id);
        let att = Attribute::new_empty(unique_id, att_type, domain, component_type, num_component);
        self.attributes.push(att);
        self.current_id += 1;
        unique_id
    }

    pub fn set_connectivity_attribute(&mut self, data: Vec<[usize; 3]>) {
        self.faces = data;
    }

    pub fn build(self) -> Result<Mesh, Err> {
        self.dependency_check()?;

        let Self { attributes, faces, .. } = self;

        let attributes = Self::get_sorted_attributes(attributes);

        let faces = faces.into_iter()
            .map(|[a, b, c]| [PointIdx::from(a), PointIdx::from(b), PointIdx::from(c)])
            .collect::<Vec<_>>();
        
        // Always perform vertex deduplication based on positions
        let (mut attributes, faces) = Self::deduplicate_vertices_based_on_positions(attributes, faces)?;
        
        // Remove degenerate faces
        let mut faces = faces.into_iter()
            .filter(|f| f[0]!=f[1] && f[1]!=f[2] && f[2]!=f[0]) // filter out degenerate faces
            .collect::<Vec<_>>();
    
        Self::remove_unused_vertices(&mut attributes, &mut faces)?;

        Ok(
            Mesh {
                attributes,
                faces,
                ..Mesh::new()
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
    fn get_sorted_attributes(mut original: Vec<Attribute>) -> Vec<Attribute> {
        // Find position attribute if it exists
        if let Some(pos_att_idx) = original.iter()
            .position(|att| att.get_attribute_type() == AttributeType::Position) {
            original.swap(0, pos_att_idx); // Ensure Position attribute is first
        }
        // If no position attribute exists, we'll just return the attributes as-is
        // This can happen with compressed meshes that haven't been decoded yet

        original
    }
    
    /// Removes unused vertices from the attributes. 
    /// This is done by checking the connectivity (faces) and removing any vertices that are not referenced.
    fn remove_unused_vertices(attributes: &mut Vec<Attribute>, faces: &mut Vec<[PointIdx; 3]>) -> Result<(), Err> {
        if faces.is_empty() || attributes.is_empty() {
            return Ok(());
        }

        // Find the maximum vertex index used in faces
        let max_vertex_index = faces.iter()
            .flat_map(|face| face.iter())
            .copied()
            .max()
            .unwrap_or(PointIdx::from(0));

        // Create a set of used vertices
        let mut used_vertices = VecPointIdx::from(vec![false; usize::from(max_vertex_index) + 1]);
        for face in faces.iter_mut() {
            for &mut vertex in face {
                if usize::from(vertex) < used_vertices.len() {
                    used_vertices[vertex] = true;
                }
            }
        }
        let mut unused_vertices: Vec<usize> = used_vertices.iter()
            .enumerate()
            .filter_map(|(idx, &used)| if !used { Some(idx) } else { None })
            .collect();
        unused_vertices.sort();

        for att in attributes.iter_mut() {
            // first remove any vertices greater than the maximum used vertex index
            for p in ((usize::from(max_vertex_index) + 1)..att.len()).rev() {
                let p = PointIdx::from(p);
                att.remove_dyn(p);
            }
            // Now remove the unused vertices computed above
            for &p in unused_vertices.iter().rev() {
                let p = PointIdx::from(p);
                att.remove_dyn(p);
            }
        }

        // Update faces
        // first, for each vertex v, count how many vertices are removed.
        let mut offsets = VecPointIdx::from(vec![PointIdx::from(0); used_vertices.len()]);
        let mut removed_count = 0;
        for v in 0..offsets.len() {
            let v = PointIdx::from(v);
            offsets[v] = PointIdx::from(removed_count);
            if !used_vertices[v] {
                removed_count += 1;
            }
        }
        // Now, remap the faces
        for face in faces.iter_mut() {
            for vertex in face.iter_mut() {
                *vertex = *vertex - offsets[*vertex];
            }
        }


        Ok(())
    }


    /// Deduplicate vertices by combining all attribute values and creating a mapping
    /// Only processes Position domain attributes - Corner domain attributes are left unchanged
    fn deduplicate_vertices_based_on_positions(attributes: Vec<Attribute>, faces: Vec<[PointIdx; 3]>) -> Result<(Vec<Attribute>, Vec<[PointIdx; 3]>), Err> {
        if attributes.is_empty() {
            return Ok((attributes, faces));
        }

        let num_vertices = faces.iter()
            .flat_map(|face| face.iter())
            .map(|&point_idx| usize::from(point_idx))
            .max()
            .unwrap_or(0) + 1; // +1 because PointIdx is zero-based
        if num_vertices == 0 {
            return Ok((attributes, faces));
        }

        // Create a hash map to find unique vertices (only considering Position domain attributes)
        let mut unique_points: HashMap<VertexHash, PointIdx> = HashMap::new();
        let mut point_mapping: VecPointIdx<PointIdx> = VecPointIdx::with_capacity(num_vertices);
        let mut duplicates: Vec<PointIdx> = Vec::new();
        let mut unique_count = 0;

        // Process each vertex using only Position domain attributes for hashing
        for point_idx in 0..num_vertices {
            let point_idx = PointIdx::from(point_idx);
            let vertex_hash = Self::hash_vertex(&attributes, point_idx);
            
            if let Some(&existing_idx) = unique_points.get(&vertex_hash) {
                // Vertex already exists, map to existing index
                point_mapping.push(existing_idx);
                duplicates.push(point_idx);
            } else {
                // New unique vertex
                unique_points.insert(vertex_hash, PointIdx::from(unique_count));
                point_mapping.push(PointIdx::from(unique_count));
                unique_count += 1;
            }
        }

        // If no duplicates found, return original data
        if unique_count == num_vertices {
            return Ok((attributes, faces));
        }

        // Create remapped attributes
        let mut remapped_attributes = Vec::with_capacity(attributes.len());
        for attribute in attributes {
            // Remap Position domain attributes
            let remapped_attribute = Self::remap_attribute(attribute, &point_mapping, unique_count)?;
            remapped_attributes.push(remapped_attribute);
        }

        // Remap faces
        let remapped_faces: Vec<[PointIdx; 3]> = faces.into_iter()
            .map(|[a, b, c]| [point_mapping[a], point_mapping[b], point_mapping[c]])
            .collect();

        Ok((remapped_attributes, remapped_faces))
    }

    /// Hash a vertex by combining all its attribute values
    /// Only considers the provided attributes (typically Position domain attributes)
    fn hash_vertex(attributes: &[Attribute], point_idx: PointIdx) -> VertexHash {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        
        for attribute in attributes {
            if usize::from(point_idx) < attribute.len() {
                // Hash the attribute type and metadata
                attribute.get_attribute_type().hash(&mut hasher);
                attribute.get_component_type().hash(&mut hasher);
                attribute.get_num_components().hash(&mut hasher);
                
                // Hash the raw bytes of the vertex data
                let component_size = attribute.get_component_type().size();
                let num_components = attribute.get_num_components();
                let value_size = component_size * num_components;
                let value_idx = attribute.get_unique_val_idx(point_idx);
                // Get raw bytes for this vertex
                let vertex_bytes = &attribute.get_data_as_bytes() [
                    usize::from(value_idx) * value_size..
                    usize::from(value_idx) * value_size + value_size
                ];
                vertex_bytes.hash(&mut hasher);
            }
        }
        
        VertexHash(hasher.finish())
    }

    /// Remap an attribute according to the vertex mapping by creating a new attribute
    fn remap_attribute(mut attribute: Attribute, point_mapping: &VecPointIdx<PointIdx>, unique_count: usize) -> Result<Attribute, Err> {
        // If no deduplication is needed, return the original attribute
        if unique_count == attribute.len() {
            return Ok(attribute);
        }
        
        // Find the first occurrence of each unique vertex to create reverse mapping
        let mut reverse_mapping = VecPointIdx::from(vec![PointIdx::from(0); unique_count]);
        for (old_idx, &new_idx) in point_mapping.iter().enumerate() {
            if old_idx < attribute.len() {
                reverse_mapping[new_idx] = PointIdx::from(old_idx);
            }
        }

        let mut points_met = VecPointIdx::from( vec![false; unique_count] );
        let removed_vertices: Vec<usize> = (0..point_mapping.len())
            .filter(|&v|
                if points_met[point_mapping[PointIdx::from(v)]] {
                    true
                } else {
                    points_met[point_mapping[PointIdx::from(v)]] = true;
                    false
                }
            ).collect::<Vec<_>>();

        // Create new attribute by extracting unique vertices
        // We'll handle this by copying data element by element
        match (attribute.get_component_type(), attribute.get_num_components()) {
            (ComponentDataType::F32, 3) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<NdVector<3,f32>,3>(p);
                }
            },
            (ComponentDataType::F32, 2) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<NdVector<2, f32>, 2>(p);
                }
            },
            (ComponentDataType::F32, 1) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<f32, 1>(p);
                }
            },
            (ComponentDataType::F32, 4) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<NdVector<4, f32>, 4>(p);
                }
            },
            (ComponentDataType::U32, 1) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<u32, 1>(p);
                }
            },
            (ComponentDataType::I32, 1) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<i32, 1>(p);
                }
            },
            (ComponentDataType::I8, 1) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<i8, 1>(p);
                }
            },
            (ComponentDataType::U16, 1) => {
                for p in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    let p = PointIdx::from(p);
                    attribute.remove::<u16, 1>(p);
                }
            },
            _ => return Err(Err::DeduplicationError(format!(
                "Unsupported attribute type combination: {:?} with {} components",
                attribute.get_component_type(),
                attribute.get_num_components()
            )))
        };
        Ok(attribute)
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VertexHash(u64);

#[remain::sorted]
#[derive(Error, Debug, Clone)]
pub enum Err {
    #[error("The attribute {0} has {1} values, but the parent attribute {2} has a size of {3}.")]
    AttributeSizeError(usize, usize, usize, usize),

    #[error("Failed to deduplicate vertices: {0}")]
    DeduplicationError(String),

    #[error("Duplicate attribute ID: {0:?}")]
    DuplicateAttributeId(AttributeId),

    #[error("One of the attributes does not meet the minimum dependency; {:?} must depend on {:?}.", .0, .1)]
    MinimumDependencyError(AttributeType, AttributeType),
    
    #[error("The connectivity attribute and the position attribute are not compatible; the connectivity attribute has a maximum index of {0} and the position attribute has a length of {1}.")]
    PositionAndConnectivityNotCompatible(usize, usize),

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::shared::NdVector;

    #[test]
    fn test_with_tetrahedron() {
        let faces = [[0,1,2], [3,4,5], [6,7,8], [9,10,11]];
        let pos = vec![
            NdVector::from([0.0f32, 0.0, 0.0]),
            NdVector::from([1.0f32, 0.0, 0.0]),
            NdVector::from([2.0f32, 0.0, 0.0]),
            
            NdVector::from([0.0f32, 0.0, 0.0]),
            NdVector::from([3.0f32, 0.0, 0.0]),
            NdVector::from([1.0f32, 0.0, 0.0]),
            
            NdVector::from([1.0f32, 0.0, 0.0]),
            NdVector::from([3.0f32, 0.0, 0.0]),
            NdVector::from([2.0f32, 0.0, 0.0]),

            NdVector::from([0.0f32, 0.0, 0.0]),
            NdVector::from([2.0f32, 0.0, 0.0]),
            NdVector::from([3.0f32, 0.0, 0.0]),            
        ];
        let mut builder = MeshBuilder::new();
        builder.set_connectivity_attribute(faces.to_vec());
        builder.add_attribute(
            pos,
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );
        let mesh = builder.build().expect("Failed to build mesh");
        assert_eq!(mesh.get_faces().len(), 4, "Mesh should have 4 faces");
        assert_eq!(mesh.get_attributes().len(), 1, "Mesh should have 1 attribute");
        assert_eq!(mesh.get_attributes()[0].len(), 4, "Position attribute should have 4 vertices as duplicates are merged");
    }
}