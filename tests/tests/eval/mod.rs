use draco_oxide::eval::EvalWriter;
use draco_oxide::io::obj::load_obj;
#[allow(unused_imports)]
use draco_oxide::core::{
    attribute::{Attribute, AttributeType},
    bit_coder::{ByteReader, ByteWriter, FunctionalByteReader, FunctionalByteWriter},
    mesh::{builder::MeshBuilder, Mesh},
    types::{ConfigType, DataValue, NdVector, Vector},
};
use draco_oxide::encode::{self, encode};
use std::io::Write;

const MESH_NAME: &str = "tetrahedron";

#[test]
fn test_eval() {
    let original_mesh = load_obj(format!("data/{}.obj", MESH_NAME)).unwrap();

    let mut buffer = Vec::new();
    let mut writer = EvalWriter::new(&mut buffer);
    encode(
        original_mesh.clone(),
        &mut writer,
        encode::Config::default(),
    )
    .unwrap();

    // `tests/outputs/` is gitignored, so it may not exist on a fresh checkout.
    std::fs::create_dir_all("outputs").unwrap();

    let json = writer.get_result();
    let json = serde_json::to_string_pretty(&json).unwrap();
    let eval_output_path = format!("outputs/{}_eval_data.txt", MESH_NAME);
    let mut eval_file =
        std::fs::File::create(&eval_output_path).expect("Failed to create evaluation output file");
    eval_file
        .write_all(json.as_bytes())
        .expect("Failed to write evaluation data");

    let output_path = format!("outputs/{}_eval_encoded.drc", MESH_NAME);
    let mut file = std::fs::File::create(&output_path).expect("Failed to create output file");
    file.write_all(&buffer)
        .expect("Failed to write encoded data");
}
