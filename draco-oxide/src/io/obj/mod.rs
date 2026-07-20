use draco_oxide_core::attribute::AttributeDomain;
// use tobj to load the obj file and convert it to our internal mesh representation
use draco_oxide_core::attribute::{Attribute, AttributeType};
use draco_oxide_core::mesh::builder::MeshBuilder;
use draco_oxide_core::mesh::Mesh;
use draco_oxide_core::types::{NdVector, PointIdx, Vector};
use std::fmt::Debug;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Mesh Builder Error: {0}")]
    MeshBuilderError(#[from] draco_oxide_core::mesh::builder::Err),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("mesh has no position attribute")]
    MissingPosition,

    #[cfg(feature = "decoder")]
    #[error("Draco decode error: {0}")]
    Decode(#[from] crate::decode::Err),
}

pub fn load_obj<P: AsRef<Path> + Debug>(path: P) -> Result<Mesh, Err> {
    let op = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };

    let (models, _materials) = tobj::load_obj(path, &op).expect("Failed to load OBJ file");
    let model: &tobj::Model = &models[0];
    let pos = model
        .mesh
        .positions
        .chunks(3)
        .map(|x| NdVector::from([x[0], x[1], x[2]]))
        .collect::<Vec<_>>();
    let faces = model
        .mesh
        .indices
        .chunks(3)
        .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
        .collect::<Vec<_>>();
    let (normals, normals_domain_ty) = load_normals(&model.mesh);
    let (tex_coords, tex_coords_domain_ty) = load_tex_coords(&model.mesh);
    let mut builder = MeshBuilder::new();
    builder.set_connectivity_attribute(faces);
    let pos_att_id = builder.add_attribute(
        pos,
        AttributeType::Position,
        AttributeDomain::Position,
        vec![],
    );
    if !normals.is_empty() {
        builder.add_attribute(
            normals,
            AttributeType::Normal,
            normals_domain_ty,
            vec![pos_att_id],
        );
    }
    if !tex_coords.is_empty() {
        builder.add_attribute(
            tex_coords,
            AttributeType::TextureCoordinate,
            tex_coords_domain_ty,
            vec![pos_att_id],
        );
    }

    Ok(builder.build()?)
}

/// The first attribute of type `ty`, if the mesh carries one.
fn find_attribute(mesh: &Mesh, ty: AttributeType) -> Option<&Attribute> {
    mesh.attributes
        .iter()
        .find(|a| a.get_attribute_type() == ty)
}

/// Writes `mesh` as Wavefront OBJ.
///
/// One `v`/`vt`/`vn` record is emitted per point, so a face indexes the same
/// ordinal in every channel it references. Values are written as decoded, in
/// the mesh's own point order.
pub fn write_obj<P: AsRef<Path>>(mesh: &Mesh, path: P) -> Result<(), Err> {
    let file = std::fs::File::create(path)?;
    let mut w = BufWriter::new(file);

    let pos = find_attribute(mesh, AttributeType::Position).ok_or(Err::MissingPosition)?;
    let num_points = pos.len();
    for p in 0..num_points {
        let v = pos.get::<NdVector<3, f32>, 3>(PointIdx::from(p));
        writeln!(w, "v {} {} {}", v.get(0), v.get(1), v.get(2))?;
    }

    let tex = find_attribute(mesh, AttributeType::TextureCoordinate);
    if let Some(t) = tex {
        for p in 0..num_points {
            let v = t.get::<NdVector<2, f32>, 2>(PointIdx::from(p));
            writeln!(w, "vt {} {}", v.get(0), v.get(1))?;
        }
    }

    let normal = find_attribute(mesh, AttributeType::Normal);
    if let Some(n) = normal {
        for p in 0..num_points {
            let v = n.get::<NdVector<3, f32>, 3>(PointIdx::from(p));
            writeln!(w, "vn {} {} {}", v.get(0), v.get(1), v.get(2))?;
        }
    }

    for face in mesh.get_faces() {
        write!(w, "f")?;
        for corner in face {
            // OBJ indices are 1-based.
            let i = usize::from(*corner) + 1;
            match (tex.is_some(), normal.is_some()) {
                (true, true) => write!(w, " {i}/{i}/{i}")?,
                (true, false) => write!(w, " {i}/{i}")?,
                (false, true) => write!(w, " {i}//{i}")?,
                (false, false) => write!(w, " {i}")?,
            }
        }
        writeln!(w)?;
    }

    w.flush()?;
    Ok(())
}

/// Decodes a Draco `.drc` file with draco-oxide's decoder and writes the result
/// as Wavefront OBJ.
#[cfg(feature = "decoder")]
pub fn decode_drc_to_obj<P: AsRef<Path>, Q: AsRef<Path>>(drc: P, obj: Q) -> Result<(), Err> {
    let bytes = std::fs::read(drc)?;
    let mesh = crate::decode::decode(draco_oxide_core::bit_coder::SliceReader::new(&bytes))?;
    write_obj(&mesh, obj)
}

fn load_normals(mesh: &tobj::Mesh) -> (Vec<NdVector<3, f32>>, AttributeDomain) {
    if mesh.normals.is_empty() {
        return (vec![], AttributeDomain::Position);
    }
    let normals = mesh
        .normals
        .chunks(3)
        .map(|x| NdVector::from([x[0], x[1], x[2]]))
        .collect::<Vec<_>>();
    (normals, AttributeDomain::Corner)
}

fn load_tex_coords(mesh: &tobj::Mesh) -> (Vec<NdVector<2, f32>>, AttributeDomain) {
    if mesh.texcoords.is_empty() {
        return (vec![], AttributeDomain::Position);
    }
    let tex_coords = mesh
        .texcoords
        .chunks(2)
        .map(|x| NdVector::from([x[0], x[1]]))
        .collect::<Vec<_>>();

    (tex_coords, AttributeDomain::Corner)
}

#[cfg(test)]
mod tests {
    use draco_oxide_core::types::PointIdx;

    use super::*;

    #[test]
    fn tetrahedron() {
        let mesh = load_obj("../tests/data/tetrahedron.obj").unwrap();
        assert_eq!(
            mesh.get_faces(),
            vec![
                [PointIdx::from(0), PointIdx::from(1), PointIdx::from(2)],
                [PointIdx::from(0), PointIdx::from(3), PointIdx::from(1)],
                [PointIdx::from(0), PointIdx::from(2), PointIdx::from(4)],
                [PointIdx::from(1), PointIdx::from(5), PointIdx::from(2)]
            ]
        );
        assert_eq!(mesh.attributes.len(), 3);
        assert_eq!(
            mesh.attributes[0].get_attribute_type(),
            AttributeType::Position
        );
        assert_eq!(mesh.attributes[0].get_domain(), AttributeDomain::Position);
        assert_eq!(mesh.attributes[0].get_num_components(), 3);
        assert_eq!(mesh.attributes[0].num_unique_values(), 4);
        assert_eq!(mesh.attributes[0].len(), 6);
    }
}
