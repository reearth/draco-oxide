use draco_oxide::{encode::{self, encode}, io::obj::load_obj};
use draco_oxide::prelude::ConfigType;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create mesh from an obj file, for example.
    let mesh = load_obj("mesh.obj").unwrap();

    // Create a buffer that we write the encoded data to.
    // This time we use 'Vec<u8>' as the output buffer, but draco-oxide can stream-write to anything 
    // that implements draco_oxide::prelude::ByteWriter.
    let mut buffer = Vec::new();
    
    // Encode the mesh into the buffer.
    encode(mesh, &mut buffer, encode::Config::default()).unwrap();

    let mut file = std::fs::File::create("output.drc").unwrap();
    file.write_all(&buffer)?;
    Ok(())
}