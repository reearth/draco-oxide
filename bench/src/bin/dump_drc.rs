//! Encodes OBJ files to .drc with the default config, for harnesses (e.g. the
//! wasm decode bench) that consume pre-encoded streams.
//!
//! Usage: dump-drc <out_dir> <mesh.obj> [<mesh.obj> ...]

use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode_mesh, Config};
use draco_oxide::io::obj::load_obj;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = args.next().expect("usage: dump-drc <out_dir> <obj>...");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    for path in args {
        let mesh = load_obj(&path).expect("load obj");
        let mut buffer = Vec::new();
        encode_mesh(mesh, &mut buffer, Config::default()).expect("encode");
        let stem = std::path::Path::new(&path)
            .file_stem()
            .unwrap()
            .to_string_lossy();
        let out = format!("{out_dir}/{stem}.drc");
        std::fs::write(&out, &buffer).expect("write drc");
        eprintln!("{out}: {} bytes", buffer.len());
    }
}
