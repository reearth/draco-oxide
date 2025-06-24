use draco::{encode::{self, encode}, io::obj::load_obj};
use draco::prelude::ConfigType;
use std::io::Write;

const FILE_NAME: &str = "sphere_reindexed";

#[test]
fn en() {
    let mesh = load_obj(format!("tests/data/{}.obj", FILE_NAME)).unwrap();

    let mut writer = Vec::new();
    println!("Encoding...");
    encode(mesh.clone(), &mut writer, encode::Config::default()).unwrap();
    println!("Encoding done.");
    
    // let mut file = std::fs::File::create("tests/outputs/tetrahedron_compressed.drc").unwrap();
    let mut file = std::fs::File::create(&format!("tests/outputs/{}.drc", FILE_NAME)).unwrap();

    file.write_all(&writer).unwrap();
}