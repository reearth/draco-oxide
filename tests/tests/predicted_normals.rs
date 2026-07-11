//! Round-trip test for the zero-CPU "trust the prediction" normal mode
//! (`NormalEncoding::PredictedOnly`).
//!
//! In this mode the encoder never reads the input normal values — it emits an
//! all-zero octahedral correction stream and neutral (all-false) sign flips, so
//! Google Draco's decoder reconstructs exactly the normals it predicts from the
//! geometry. This test encodes the normal-bearing test meshes in that mode and
//! confirms the reference `draco_decoder` accepts the bitstream and emits
//! normals, and that the correction-free stream is no larger than the default
//! (quantized) normal path.
//!
//! Like `draco_decode.rs`, the reference-decoder half is **auto-skipped** when
//! `draco_decoder` isn't available (build it with `scripts/build-draco.sh`, or
//! point `DRACO_DECODER` at a binary).

use std::path::{Path, PathBuf};
use std::process::Command;

use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{self, encode, NormalEncoding};
use draco_oxide::io::obj::load_obj;

/// Test meshes from `tests/data/` that carry per-corner normals.
const MESHES: &[&str] = &[
    "tetrahedron",
    "cube_quads",
    "sphere",
    "punctured_sphere",
    "bunny",
];

fn find_draco_decoder() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DRACO_DECODER") {
        let p = PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    let default =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../third_party/draco/_build/draco_decoder");
    default.is_file().then_some(default)
}

fn encode_predicted_only(name: &str) -> Vec<u8> {
    let mesh = load_obj(&format!("data/{name}.obj")).expect("load_obj");
    let mut buf = Vec::new();
    encode(
        mesh,
        &mut buf,
        encode::Config::default().with_normals(NormalEncoding::PredictedOnly),
    )
    .expect("encode (predicted-only normals)");
    buf
}

fn encode_default(name: &str) -> Vec<u8> {
    let mesh = load_obj(&format!("data/{name}.obj")).expect("load_obj");
    let mut buf = Vec::new();
    encode(mesh, &mut buf, encode::Config::default()).expect("encode (default)");
    buf
}

/// The all-zero correction stream must never be *larger* than the fully
/// quantized normal stream for the same mesh — it carries no per-value entropy.
/// This runs with or without the reference decoder present.
#[test]
fn predicted_only_normals_are_not_larger_than_quantized() {
    for &name in MESHES {
        let predicted = encode_predicted_only(name).len();
        let default = encode_default(name).len();
        eprintln!("{name}: predicted-only={predicted} B, default={default} B");
        assert!(
            predicted <= default,
            "{name}: predicted-only normals ({predicted} B) larger than default ({default} B)"
        );
    }
}

#[test]
fn google_draco_decodes_predicted_only_normals() {
    let Some(decoder) = find_draco_decoder() else {
        eprintln!(
            "SKIP google_draco_decodes_predicted_only_normals: Google Draco `draco_decoder` not \
             found.\n      Build it with `scripts/build-draco.sh`, or set DRACO_DECODER=<path>."
        );
        return;
    };
    eprintln!("Using draco_decoder: {}", decoder.display());

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/predicted_normals");
    std::fs::create_dir_all(&out_dir).expect("create output dir");

    let mut failures = Vec::new();
    for &name in MESHES {
        let buf = encode_predicted_only(name);
        let drc_path = out_dir.join(format!("{name}.drc"));
        std::fs::write(&drc_path, &buf).expect("write .drc");

        let decoded_path = out_dir.join(format!("{name}.decoded.obj"));
        let output = Command::new(&decoder)
            .arg("-i")
            .arg(&drc_path)
            .arg("-o")
            .arg(&decoded_path)
            .output()
            .expect("spawn draco_decoder");

        if !output.status.success() {
            failures.push(format!(
                "{name}: draco_decoder failed with {}\n      stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
            continue;
        }

        // The decoded mesh must carry normals (the whole point of the mode).
        let decoded = std::fs::read_to_string(&decoded_path).expect("read decoded obj");
        if !decoded.lines().any(|l| l.starts_with("vn ")) {
            failures.push(format!("{name}: decoded mesh has no normals"));
        }
    }

    assert!(
        failures.is_empty(),
        "predicted-only normal round-trip failed for {}/{} mesh(es):\n    - {}",
        failures.len(),
        MESHES.len(),
        failures.join("\n    - "),
    );
}
