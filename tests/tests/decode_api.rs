//! API-shape checks on the decoder entry points that the profile harness
//! cannot express.

use draco_oxide::core::types::ConfigType;
use draco_oxide::{
    encode::{self, encode_mesh},
    io::obj::load_obj,
};

#[test]
fn generic_decode_yields_the_mesh_variant() {
    let mesh = load_obj("data/tetrahedron.obj").expect("load obj");
    let mut buf = Vec::new();
    encode_mesh(mesh, &mut buf, encode::Config::default()).expect("encode");
    assert!(matches!(
        draco_oxide::decode::decode(&buf),
        Ok(draco_oxide::decode::Geometry::Mesh(_))
    ));
}
