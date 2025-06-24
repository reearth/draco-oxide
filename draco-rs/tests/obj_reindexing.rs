use draco::{prelude::{NdVector, Vector}};
use std::io::Write;

#[test]
fn en() {
    let (bunny, _) = tobj::load_obj(
        // "tests/data/tetrahedron.obj",
        "tests/data/sphere.obj",
        &tobj::GPU_LOAD_OPTIONS
    ).unwrap();
    let bunny = &bunny[0];
    let mesh = &bunny.mesh;

    let faces = mesh.indices.chunks(3)
        .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
        .collect::<Vec<_>>();

    let points = mesh.positions.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();

    let normals = mesh.normals.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();

    // create the obj file and write the mesh to it
    let mut file = std::fs::File::create("tests/data/sphere_reindexed.obj").unwrap();
    let mut file_writer = std::io::BufWriter::new(&mut file);
    for point in points.iter() {
        writeln!(file_writer, "v {} {} {}", point.get(0), point.get(1), point.get(2)).unwrap();
    }
    for normal in normals.iter() {
        writeln!(file_writer, "vn {} {} {}", normal.get(0), normal.get(1), normal.get(2)).unwrap();
    }
    for face in faces.iter() {
        writeln!(file_writer, "f {0}//{0} {1}//{1} {2}//{2}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
    }
}