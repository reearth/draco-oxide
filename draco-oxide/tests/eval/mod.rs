use std::io::Write;
use draco::eval::EvalWriter;
use draco::io::obj::load_obj;
use draco::prelude::*;

const MESH_NAME: &str = "tetrahedron";

#[test]
fn test_eval() {
    let original_mesh = load_obj(format!("tests/data/{}.obj", MESH_NAME)).unwrap();
    
    let mut buffer = Vec::new();
    let mut writer = EvalWriter::new(&mut buffer);
    encode(original_mesh.clone(), &mut writer, encode::Config::default()).unwrap();
    
    // Write the evaluation data to a separate file
    let json = writer.get_result();
    let json = serde_json::to_string_pretty(&json).unwrap();
    let eval_output_path = format!("tests/outputs/{}_eval_data.txt", MESH_NAME);
    let mut eval_file = std::fs::File::create(&eval_output_path)
        .expect("Failed to create evaluation output file");
    eval_file.write_all(json.as_bytes())
        .expect("Failed to write evaluation data");

    // Write the encoded data to a temporary file
    let output_path = format!("tests/outputs/{}_eval_encoded.drc", MESH_NAME);
    let mut file = std::fs::File::create(&output_path)
        .expect("Failed to create output file");
    file.write_all(&buffer)
        .expect("Failed to write encoded data");

}