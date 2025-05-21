use std::io::Write;
use draco::{eval::EvalWriter, prelude::*};

const MESH_NAME: &str = "sphere";

#[test]
fn test_eval() {
    let (bunny,_) = tobj::load_obj(
        format!("tests/data/{}.obj", MESH_NAME), 
        &tobj::GPU_LOAD_OPTIONS
    ).unwrap();
    let bunny = &bunny[0];
    let mesh = &bunny.mesh;

    let mut faces = mesh.indices.chunks(3)
        .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
        .collect::<Vec<_>>();
    faces.iter_mut().for_each(|x| x.sort());
    faces.sort();

    let points = mesh.positions.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();

    let original_mesh = {
        let mut builder = MeshBuilder::new();
        let ref_face_att = builder.add_connectivity_attribute(faces, Vec::new());
        builder.add_attribute(points, AttributeType::Position, vec![ref_face_att]);
        builder.build().unwrap()
    };
    
    let mut buff_writer = buffer::writer::Writer::new();
    let mut writer = |input: (u8, u64)| {
        buff_writer.next(input);
    };
    let mut eval_writer = EvalWriter::new(&mut writer);
    let mut writer = |input: (u8, u64)| {
        eval_writer.write(input);
    };
    println!("Encoding...");
    encode(original_mesh.clone(), &mut writer, encode::Config::default()).unwrap();
    println!("Encoding done.");

    // write json
    let mut eval_file = std::fs::File::create(
    format!("tests/outputs/{}_eval.json", MESH_NAME)
    ).unwrap();
    let data = eval_writer.get_result();
    let data = serde_json::to_string_pretty(&data).unwrap();
    eval_file.write_all(data.as_bytes()).unwrap();


    let data: Buffer = buff_writer.into();

    let mut file = std::fs::File::create(
        format!("tests/outputs/{}_compressed.draco", MESH_NAME)
    ).unwrap();
    let out = data.as_slice();
    file.write_all(out).unwrap();

    let mut buff_reader = data.into_reader();
    let mut bit_counter: usize = 0;
    let mut reader = |size| {
        bit_counter += size as usize;
        // println!("bit_counter = {}  reading {} bits", bit_counter, size);
        buff_reader.next(size)
    };
    println!("Decoding...");
    let mesh = decode(&mut reader, decode::Config::default()).unwrap();
    println!("Decoding done.");

    let mut obj_file = std::fs::File::create(
        format!("tests/outputs/{}_recovered.obj", MESH_NAME)
    ).unwrap();
    let mut file_writer = std::io::BufWriter::new(&mut obj_file);

    for point in mesh.get_attributes()[1].as_slice::<[f32; 3]>() {
        writeln!(file_writer, "v {} {} {}", point[0], point[1], point[2]).unwrap();
    }

    for face in mesh.get_attributes()[0].as_slice::<[usize; 3]>() {
        writeln!(file_writer, "f {} {} {}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
    }
}