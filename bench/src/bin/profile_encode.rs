//! Encode profiling harness: loads a mesh, then encodes it `reps` times inside
//! `encode_region`, a never-inlined function that callgrind can toggle
//! collection on so mesh loading stays out of the profile.
//!
//! Usage: profile-encode <mesh.obj> [reps]

use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{Config, Encoder};
use draco_oxide::io::obj::load_obj;
use std::time::Instant;

#[inline(never)]
fn encode_region(mesh: &Mesh, reps: usize) -> usize {
    let mut last = 0;
    for _ in 0..reps {
        // A fresh Encoder per rep: the instance may come to hold reusable
        // resources across encodes, and the profile must keep measuring the
        // cold single-run cost.
        let mut buffer = Vec::new();
        Encoder::new()
            .encode(mesh.clone(), &mut buffer, Config::default())
            .expect("encode");
        last = buffer.len();
        std::hint::black_box(&buffer);
    }
    last
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: profile-encode <mesh.obj> [reps]");
    let reps: usize = args.next().map_or(1, |s| s.parse().expect("reps"));

    let mesh = load_obj(&path).expect("load obj");
    let faces = mesh.faces.len();
    let attrs = mesh.attributes.len();

    let t = Instant::now();
    let bytes = encode_region(&mesh, reps);
    let total_ms = t.elapsed().as_secs_f64() * 1e3;

    println!(
        "{path}: {faces} faces, {attrs} attrs, {bytes} bytes, {:.2} ms/encode ({reps} reps)",
        total_ms / reps as f64
    );
}
