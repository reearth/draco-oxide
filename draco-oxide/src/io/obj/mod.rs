use crate::core::attribute::AttributeDomain;
// use tobj to load the obj file and convert it to our internal mesh representation
use crate::prelude::{AttributeType, MeshBuilder, NdVector};
use crate::Mesh;
use std::fmt::Debug;
use std::path::Path;

#[derive(Debug, thiserror::Error, Clone)]
pub enum Err {
    #[error("Mesh Builder Error: {0}")]
    MeshBuilderError(#[from] crate::core::mesh::builder::Err),
}

pub fn load_obj<P: AsRef<Path> + Debug>(path: P) -> Result<Mesh, Err> {
    let op = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };

    let (models, _materials) = tobj::load_obj(path, &op).expect("Failed to load OBJ file");
    let model: &tobj::Model = &models[0];
    let pos = model.mesh.positions.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();
    let faces = model.mesh.indices.chunks(3)
        .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
        .collect::<Vec<_>>();
    let (normals, normals_domain_ty) = load_normals(&model.mesh);
    let (tex_coords, tex_coords_domain_ty) = load_tex_coords(&model.mesh);
    let mut builder = MeshBuilder::new();
    builder.set_connectivity_attribute(faces);
    let pos_att_id = builder.add_attribute(pos, AttributeType::Position, AttributeDomain::Position, vec![]);
    if !normals.is_empty() {
        builder.add_attribute(normals, AttributeType::Normal, normals_domain_ty, vec![pos_att_id]);
    }
    if !tex_coords.is_empty() {
        builder.add_attribute(tex_coords, AttributeType::TextureCoordinate, tex_coords_domain_ty, vec![pos_att_id]);
    }

    Ok(builder.build()?)
}

fn load_normals(mesh: &tobj::Mesh) -> (Vec<NdVector<3,f32>>, AttributeDomain) {
    if mesh.normals.is_empty() {
        return (vec![], AttributeDomain::Position)
    }
    let normals = mesh.normals.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();
    (normals, AttributeDomain::Corner)
}


fn load_tex_coords(mesh: &tobj::Mesh) -> (Vec<NdVector<2, f32>>, AttributeDomain) {
    if mesh.texcoords.is_empty() {
        return (vec![], AttributeDomain::Position)
    }
    let tex_coords = mesh.texcoords.chunks(2)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32]))
        .collect::<Vec<_>>();

    (tex_coords, AttributeDomain::Corner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_reindexed() {
        let mesh  = load_obj("tests/data/sphere.obj").unwrap();
        let mesh_reindexed = load_obj("tests/data/sphere_reindexed.obj").unwrap();
        assert_eq!(mesh.get_faces(), mesh_reindexed.get_faces());
        assert_eq!(mesh.attributes.len(), mesh_reindexed.attributes.len());
        assert_eq!(mesh.attributes[0].unique_vals_as_slice::<NdVector<3,f32>>(), mesh_reindexed.attributes[0].unique_vals_as_slice());
        assert_eq!(mesh.attributes[1].unique_vals_as_slice::<NdVector<3,f32>>(), mesh_reindexed.attributes[1].unique_vals_as_slice());
    }

    #[test]
    fn tetrahedron() {
        let mesh = load_obj("tests/data/tetrahedron.obj").unwrap();
        assert_eq!(mesh.get_faces(),
            vec![
                [0, 1, 2], [0, 3, 1], [0, 2, 4], [1, 5, 2]
            ]
        );
        assert_eq!(mesh.attributes.len(), 3);
        assert_eq!(mesh.attributes[0].get_attribute_type(), AttributeType::Position);
        assert_eq!(mesh.attributes[0].get_domain(), AttributeDomain::Position);
        assert_eq!(mesh.attributes[0].get_num_components(), 3);
        assert_eq!(mesh.attributes[0].num_unique_values(), 4);
        assert_eq!(mesh.attributes[0].len(), 6);
    }
}