//! Minimal decode timing harness (no profiler overhead): encodes each mesh once,
//! then reports median ms/decode over many repeats.
//!
//! Usage: time-decode <mesh.obj> [<mesh.obj> ...]

use std::time::Instant;

use draco_oxide::core::bit_coder::SliceReader;
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode, Config};
use draco_oxide::io::obj::load_obj;

fn median_ms<F: FnMut()>(mut f: F) -> f64 {
    f();
    let mut samples = Vec::new();
    let start = Instant::now();
    while samples.len() < 20 || (start.elapsed().as_secs_f64() < 2.0 && samples.len() < 400) {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_secs_f64() * 1e3);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn main() {
    for path in std::env::args().skip(1) {
        let mesh = load_obj(&path).expect("load obj");
        let faces = mesh.faces.len();
        let mut buffer = Vec::new();
        encode(mesh, &mut buffer, Config::default()).expect("encode");

        let ms = median_ms(|| {
            let reader = SliceReader::new(&buffer);
            let decoded = draco_oxide::decode::decode(reader).expect("decode");
            std::hint::black_box(&decoded);
        });
        println!("{path}: {faces} faces, {} bytes, {ms:.4} ms/decode", buffer.len());
    }
}
