use std::usize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use gltf::accessor::Dimensions;
use thiserror::Error;

use crate::core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType
};
use crate::core::shared::Vector;
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

        let Self { attributes, mut faces, .. } = self;

        let mut attributes = Self::get_sorted_attributes(attributes);

        Self::remove_unused_vertices(&mut attributes, &mut faces)?;

        // Always perform vertex deduplication based on positions
        let (attributes, faces) = Self::deduplicate_vertices_based_on_positions(attributes, faces)?;

        // Ensure that the connectivity is valid
        Self::assert_connectivity_validity(&faces);
        // Ensure that the attributes are valid
        Self::assert_attributes_validity(&attributes);
        
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
        let pos_att_idx = original.iter()
            .position(|att| att.get_attribute_type() == AttributeType::Position)
            .unwrap(); // TODO: Handle this error properly

        original.swap(0, pos_att_idx); // Ensure Position attribute is first

        original
    }

    pub(crate) fn assert_connectivity_validity(faces: &[[usize; 3]]) {
        let mut vertices = vec![false; faces.iter()
            .flat_map(|face| face.iter())
            .copied()
            .max()
            .unwrap_or(0) + 1];
        for &face in faces {
            for &v in face.iter() {
                vertices[v] = true;
            }
        }
        assert!(vertices.iter().all(|&v| v), 
            "After removing unused vertices, some vertices are still not used in faces: \n
            unused vertices: {:?} \n faces: {:?}", 
            vertices.iter().enumerate().filter_map(|(i, &v)| if !v { Some(i) } else { None }).collect::<Vec<_>>(),
            faces
        );
    }

    pub(crate) fn assert_attributes_validity(attributes: &[Attribute]) 
    {
        for att in attributes.iter() {
            let mut values = vec![false; att.num_unique_values()];
            for v in 0..att.len() {
                let value_idx = att.get_att_idx(v);
                values[value_idx] = true;
            }
            assert!(values.iter().all(|&v| v), 
                "After removing unused vertices, some attribute values are not pointed by any vertex: {:?}", 
                values.iter().enumerate().filter_map(|(i, &v)| if !v { Some(i) } else { None }).collect::<Vec<_>>()
            );
        }
    }


    /// Removes unused vertices from the attributes. 
    /// This is done by checking the connectivity (faces) and removing any vertices that are not referenced.
    fn remove_unused_vertices(attributes: &mut Vec<Attribute>, faces: &mut Vec<[usize; 3]>) -> Result<(), Err> {
        if faces.is_empty() || attributes.is_empty() {
            return Ok(());
        }

        // Find the maximum vertex index used in faces
        let max_vertex_index = faces.iter()
            .flat_map(|face| face.iter())
            .copied()
            .max()
            .unwrap_or(0);

        // Create a set of used vertices
        let mut used_vertices = vec![false; max_vertex_index + 1];
        for face in faces.iter_mut() {
            for &mut vertex in face {
                if vertex < used_vertices.len() {
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
            for v in ((max_vertex_index + 1)..att.len()).rev() {
                att.remove_dyn(v);
            }
            // Now remove the unused vertices computed above
            for &v in unused_vertices.iter().rev() {
                // Remove the vertex from the attribute
                att.remove_dyn(v);
            }
        }

        // Update faces
        // first, for each vertex v, count how many vertices are removed.
        let mut offsets = vec![0; used_vertices.len()];
        let mut removed_count = 0;
        for v in 0..offsets.len() {
            offsets[v] = removed_count;
            if !used_vertices[v] {
                removed_count += 1;
            }
        }
        // Now, remap the faces
        for face in faces.iter_mut() {
            for vertex in face.iter_mut() {
                *vertex -= offsets[*vertex];
            }
        }


        Ok(())
    }


    /// Deduplicate vertices by combining all attribute values and creating a mapping
    /// Only processes Position domain attributes - Corner domain attributes are left unchanged
    fn deduplicate_vertices_based_on_positions(attributes: Vec<Attribute>, faces: Vec<[usize; 3]>) -> Result<(Vec<Attribute>, Vec<[usize; 3]>), Err> {
        if attributes.is_empty() {
            return Ok((attributes, faces));
        }

        let num_vertices = attributes.iter().map(|a| a.len()).max().unwrap_or(0);
        if num_vertices == 0 {
            return Ok((attributes, faces));
        }

        // Create a hash map to find unique vertices (only considering Position domain attributes)
        let mut unique_vertices: HashMap<VertexHash, usize> = HashMap::new();
        let mut vertex_mapping: Vec<usize> = Vec::with_capacity(num_vertices);
        let mut duplicates: Vec<usize> = Vec::new();
        let mut unique_count = 0;

        // Process each vertex using only Position domain attributes for hashing
        for vertex_idx in 0..num_vertices {
            let vertex_hash = Self::hash_vertex(&attributes, vertex_idx);
            
            if let Some(&existing_idx) = unique_vertices.get(&vertex_hash) {
                // Vertex already exists, map to existing index
                vertex_mapping.push(existing_idx);
                duplicates.push(vertex_idx);
            } else {
                // New unique vertex
                unique_vertices.insert(vertex_hash, unique_count);
                vertex_mapping.push(unique_count);
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
            let remapped_attribute = Self::remap_attribute(attribute, &vertex_mapping, unique_count)?;
            remapped_attributes.push(remapped_attribute);
        }

        // Remap faces
        let remapped_faces = faces.into_iter()
            .map(|[a, b, c]| [vertex_mapping[a], vertex_mapping[b], vertex_mapping[c]])
            .collect();

        Ok((remapped_attributes, remapped_faces))
    }

    /// Hash a vertex by combining all its attribute values
    /// Only considers the provided attributes (typically Position domain attributes)
    fn hash_vertex(attributes: &[Attribute], vertex_idx: usize) -> VertexHash {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        
        for attribute in attributes {
            if vertex_idx < attribute.len() {
                // Hash the attribute type and metadata
                attribute.get_attribute_type().hash(&mut hasher);
                attribute.get_component_type().hash(&mut hasher);
                attribute.get_num_components().hash(&mut hasher);
                
                // Hash the raw bytes of the vertex data
                let component_size = attribute.get_component_type().size();
                let num_components = attribute.get_num_components();
                let value_size = component_size * num_components;
                let value_idx = attribute.get_att_idx(vertex_idx);
                // Get raw bytes for this vertex
                let vertex_bytes = &attribute.get_data_as_bytes() [
                    value_idx * value_size..
                    value_idx * value_size + value_size
                ];
                vertex_bytes.hash(&mut hasher);
            }
        }
        
        VertexHash(hasher.finish())
    }

    /// Remap an attribute according to the vertex mapping by creating a new attribute
    fn remap_attribute(mut attribute: Attribute, vertex_mapping: &[usize], unique_count: usize) -> Result<Attribute, Err> {
        // If no deduplication is needed, return the original attribute
        if unique_count == attribute.len() {
            return Ok(attribute);
        }
        
        // Find the first occurrence of each unique vertex to create reverse mapping
        let mut reverse_mapping = vec![0; unique_count];
        for (old_idx, &new_idx) in vertex_mapping.iter().enumerate() {
            if old_idx < attribute.len() {
                reverse_mapping[new_idx] = old_idx;
            }
        }

        let mut vertices_met = vec![false; unique_count];
        let removed_vertices: Vec<usize> = (0..vertex_mapping.len())
            .filter(|&v|
                if vertices_met[vertex_mapping[v]] {
                    true
                } else {
                    vertices_met[vertex_mapping[v]] = true;
                    false
                }
            ).collect::<Vec<_>>();

        // Create new attribute by extracting unique vertices
        // We'll handle this by copying data element by element
        match (attribute.get_component_type(), attribute.get_num_components()) {
            (ComponentDataType::F32, 3) => {
                for v in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    attribute.remove::<NdVector<3,f32>,3>(v);
                }
            },
            (ComponentDataType::F32, 2) => {
                for v in removed_vertices.into_iter().rev() {
                    // Remove the vertex from the mapping
                    attribute.remove::<NdVector<2, f32>, 2>(v);
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
    fn test_two_faces_six_vertices_with_two_duplicate_pairs() {
        let mut builder = MeshBuilder::new();
        
        // Create 6 vertices where:
        // - vertex 3 is a duplicate of vertex 0
        // - vertex 4 is a duplicate of vertex 1
        // So we have 2 pairs of duplicates, resulting in 4 unique vertices
        let positions = vec![
            NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 0 (unique)
            NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 1 (unique)
            NdVector::from([0.5f32, 1.0, 0.0]),  // vertex 2 (unique)
            NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 3 (duplicate of vertex 0)
            NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 4 (duplicate of vertex 1)
            NdVector::from([2.0f32, 0.0, 0.0]),  // vertex 5 (unique)
        ];
        
        builder.add_attribute(
            positions,
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );
        
        // Two faces using all 6 vertices
        let faces = vec![
            [0, 1, 2],  // first triangle
            [3, 4, 5],  // second triangle (vertices 3,4 are duplicates)
        ];
        builder.set_connectivity_attribute(faces);
        
        let mesh = builder.build().expect("Failed to build mesh");
        
        // After deduplication, should have 4 unique vertices
        // (vertex 3 merged with vertex 0, vertex 4 merged with vertex 1)
        let position_attrs: Vec<_> = mesh.get_attributes().iter()
            .filter(|attr| attr.get_attribute_type() == AttributeType::Position)
            .collect();
        
        assert_eq!(position_attrs.len(), 1, "Should have exactly one position attribute");
        assert_eq!(position_attrs[0].len(), 4, "Should have 4 unique vertices after deduplication");
        
        // Check that faces are correctly remapped
        let mesh_faces = mesh.get_faces();
        assert_eq!(mesh_faces.len(), 2, "Should have 2 faces");
        assert_eq!(mesh_faces[0], [0, 1, 2], "First face should remain unchanged");
        assert_eq!(mesh_faces[1], [0, 1, 3], "Second face should have vertices 3,4 remapped to 0,1 and vertex 5 becomes 3");
    }

    #[test]
    fn test_two_faces_six_vertices_with_duplicate_pairs_and_position_domain_attributes() {
        let mut builder = MeshBuilder::new();
        
        // Same vertex pattern: 6 vertices with 2 pairs of duplicates
        let positions = vec![
            NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 0 (unique)
            NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 1 (unique)
            NdVector::from([0.5f32, 1.0, 0.0]),  // vertex 2 (unique)
            NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 3 (duplicate of vertex 0)
            NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 4 (duplicate of vertex 1)
            NdVector::from([2.0f32, 0.0, 0.0]),  // vertex 5 (unique)
        ];
        
        let pos_id = AttributeId::new(0);
        builder.add_attribute(
            positions,
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );
        
        // Add normals (Position domain) - matching duplication pattern
        let normals = vec![
            NdVector::from([0.0f32, 0.0, 1.0]),  // normal 0
            NdVector::from([1.0f32, 0.0, 0.0]),  // normal 1
            NdVector::from([0.0f32, 1.0, 0.0]),  // normal 2
            NdVector::from([0.0f32, 0.0, 1.0]),  // normal 3 (duplicate of normal 0)
            NdVector::from([1.0f32, 0.0, 0.0]),  // normal 4 (duplicate of normal 1)
            NdVector::from([-1.0f32, 0.0, 0.0]), // normal 5
        ];
        
        builder.add_attribute(
            normals,
            AttributeType::Normal,
            AttributeDomain::Position,
            vec![pos_id],
        );
        
        // Add texture coordinates (Position domain)
        // No duplicates
        let tex_coords = vec![
            NdVector::from([0.0f32, 0.0]),  // texcoord 0
            NdVector::from([1.0f32, 0.0]),  // texcoord 1
            NdVector::from([0.5f32, 1.0]),  // texcoord 2
            NdVector::from([0.0f32, 1.0]),  // texcoord 3
            NdVector::from([1.0f32, 1.0]),  // texcoord 4
            NdVector::from([2.0f32, 0.0]),  // texcoord 5
        ];
        
        builder.add_attribute(
            tex_coords,
            AttributeType::TextureCoordinate,
            AttributeDomain::Position,
            vec![pos_id],
        );
        
        // Add colors (Position domain) - matching duplication pattern
        let colors = vec![
            NdVector::from([1.0f32, 0.0, 0.0]),  // red (vertex 0)
            NdVector::from([0.0f32, 1.0, 0.0]),  // green (vertex 1)
            NdVector::from([0.0f32, 0.0, 1.0]),  // blue (vertex 2)
            NdVector::from([1.0f32, 0.0, 0.0]),  // red (vertex 3, duplicate of vertex 0)
            NdVector::from([0.0f32, 1.0, 0.0]),  // green (vertex 4, duplicate of vertex 1)
            NdVector::from([1.0f32, 1.0, 0.0]),  // yellow (vertex 5)
        ];
        
        builder.add_attribute(
            colors,
            AttributeType::Color,
            AttributeDomain::Position,
            vec![pos_id],
        );
        
        let faces = vec![
            [0, 1, 2],
            [3, 4, 5],
        ];
        builder.set_connectivity_attribute(faces);
        
        let mesh = builder.build().expect("Failed to build mesh");
        
        // All Position domain attributes should be deduplicated to 4 vertices
        let position_attrs: Vec<_> = mesh.get_attributes().iter()
            .filter(|attr| attr.get_attribute_type() == AttributeType::Position)
            .collect();
        let normal_attrs: Vec<_> = mesh.get_attributes().iter()
            .filter(|attr| attr.get_attribute_type() == AttributeType::Normal)
            .collect();
        let texcoord_attrs: Vec<_> = mesh.get_attributes().iter()
            .filter(|attr| attr.get_attribute_type() == AttributeType::TextureCoordinate)
            .collect();
        let color_attrs: Vec<_> = mesh.get_attributes().iter()
            .filter(|attr| attr.get_attribute_type() == AttributeType::Color)
            .collect();
        
        assert_eq!(position_attrs[0].len(), 6);
        assert_eq!(normal_attrs[0].len(), 6);
        assert_eq!(texcoord_attrs[0].len(), 6);
        assert_eq!(color_attrs[0].len(), 6);
        
        // Check face remapping
        let mesh_faces = mesh.get_faces();
        assert_eq!(mesh_faces[0], [0, 1, 2]);
        assert_eq!(mesh_faces[1], [3, 4, 5]);
    }


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

    #[test]
    fn test_remove_unused_vertices() {
        let att1_data = vec![
            NdVector::from([0.0f32, 0.0, 0.0]),
            NdVector::from([1.0f32, 0.0, 0.0]),
            NdVector::from([2.0f32, 0.0, 0.0]),
            NdVector::from([0.0f32, 1.0, 0.0]),
            NdVector::from([1.0f32, 1.0, 0.0]),
        ];
        let att2_data = vec![
            NdVector::from([0.0f32, 1.0, 0.0]),
            NdVector::from([1.0f32, 1.0, 0.0]),
            NdVector::from([2.0f32, 1.0, 0.0]),
            NdVector::from([0.0f32, 1.0, 0.0]),
            NdVector::from([1.0f32, 1.0, 0.0]),
        ];
        let mut builder = MeshBuilder::new();
        let pos_id = builder.add_attribute(
            att1_data,
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );
        builder.add_attribute(
            att2_data,
            AttributeType::Normal,
            AttributeDomain::Position,
            vec![pos_id],
        );
        builder.set_connectivity_attribute(vec![
            [0, 1, 2],
            [1, 4, 2],
            // Note that 3 is unused
        ]);

        MeshBuilder::remove_unused_vertices(&mut builder.attributes, &mut builder.faces).expect("Failed to remove unused vertices");

        // Check that unused vertex (index 3) is removed from both attributes
        for att in &builder.attributes {
            assert_eq!(att.len(), 4, "Each attribute should have 4 vertices after removing unused vertices");
        }

        // Check that faces are updated correctly
        assert_eq!(builder.faces.len(), 2, "Faces should remain unchanged in count");
        assert_eq!(builder.faces[0], [0, 1, 2], "First face should remain unchanged");
        assert_eq!(builder.faces[1], [1, 3, 2]);
    }
}