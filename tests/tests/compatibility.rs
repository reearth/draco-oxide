use draco_oxide::prelude::ConfigType;
use draco_oxide::{
    encode::{self, encode},
    io::obj::load_obj,
};
use std::io::Write;

const FILE_NAME: &str = "cube_quads";

#[test]
fn en() {
    let mesh = load_obj(format!("data/{}.obj", FILE_NAME)).unwrap();

    let mut writer = Vec::new();
    encode(mesh.clone(), &mut writer, encode::Config::default()).unwrap();

    let mut file = std::fs::File::create(&format!("outputs/{}.drc", FILE_NAME)).unwrap();

    file.write_all(&writer).unwrap();
}
