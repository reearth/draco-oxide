use draco_oxide::{encode::{self, encode}, io::obj::load_obj};
use draco_oxide::prelude::ConfigType;
use std::io::Write;

const FILE_NAME: &str = "cube_quads";

#[test]
fn en() {
    let mesh = load_obj(format!("tests/data/{}.obj", FILE_NAME)).unwrap();

    let mut writer = Vec::new();
    encode(mesh.clone(), &mut writer, encode::Config::default()).unwrap();
    
    let mut file = std::fs::File::create(&format!("tests/outputs/{}.drc", FILE_NAME)).unwrap();

    file.write_all(&writer).unwrap();
}