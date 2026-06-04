//! Smoke test: encode meshes with draco-oxide, then decode them with Google
//! Draco's reference `draco_decoder` to confirm the produced bitstream
//! round-trips without an error / panic / crash. This does *not* check that the
//! decoded geometry is correct — only that the reference decoder accepts it.
//!
//! The test is **auto-skipped** when the reference decoder isn't available, so
//! `cargo test` still passes on machines that haven't built Google Draco. To
//! enable it, build the decoder with `scripts/build-draco.sh`, or point the
//! `DRACO_DECODER` environment variable at an existing `draco_decoder` binary.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;

use draco_oxide::encode::{self, encode};
use draco_oxide::io::obj::load_obj;
use draco_oxide::prelude::ConfigType;

/// Meshes from `tests/data/` to round-trip through the reference decoder.
const MESHES: &[&str] = &[
    "cube_quads",
    "tetrahedron",
    "sphere",
    "punctured_sphere",
    "torus",
    "bunny",
];

/// Locate Google Draco's `draco_decoder`, or `None` if it isn't available.
fn find_draco_decoder() -> Option<PathBuf> {
    // 1. Explicit override.
    if let Ok(path) = std::env::var("DRACO_DECODER") {
        let p = PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    // 2. Default location produced by scripts/build-draco.sh. The repo root is
    //    the parent of this crate's manifest dir.
    let default =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../third_party/draco/_build/draco_decoder");
    default.is_file().then_some(default)
}

/// Encode one mesh with draco-oxide and decode it with `draco_decoder`.
/// Returns `Err(message)` describing any failure.
fn roundtrip_one(name: &str, decoder: &Path, out_dir: &Path) -> Result<(), String> {
    let obj_path = format!("data/{name}.obj");
    let mesh = load_obj(&obj_path).map_err(|e| format!("{name}: load_obj failed: {e}"))?;

    let mut buf = Vec::new();
    encode(mesh, &mut buf, encode::Config::default())
        .map_err(|e| format!("{name}: draco-oxide encode failed: {e:?}"))?;

    let drc_path = out_dir.join(format!("{name}.drc"));
    std::fs::write(&drc_path, &buf).map_err(|e| format!("{name}: writing .drc failed: {e}"))?;

    // draco_decoder infers the output format from the extension (.obj/.ply).
    let decoded_path = out_dir.join(format!("{name}.decoded.obj"));
    let output = Command::new(decoder)
        .arg("-i")
        .arg(&drc_path)
        .arg("-o")
        .arg(&decoded_path)
        .output()
        .map_err(|e| format!("{name}: failed to spawn draco_decoder: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        // A non-zero exit code is a decode error; a terminating signal
        // (e.g. SIGSEGV) shows up in the Display of `status`.
        Err(format!(
            "{name}: draco_decoder failed with {}\n      stdout: {}\n      stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        ))
    }
}

#[test]
fn google_draco_decodes_draco_oxide_output() {
    let Some(decoder) = find_draco_decoder() else {
        eprintln!(
            "SKIP google_draco_decodes_draco_oxide_output: Google Draco `draco_decoder` not \
             found.\n      Build it with `scripts/build-draco.sh`, or set DRACO_DECODER=<path>."
        );
        return;
    };
    eprintln!("Using draco_decoder: {}", decoder.display());

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/draco_decode");
    std::fs::create_dir_all(&out_dir).expect("failed to create output dir");

    let mut failures = Vec::new();
    for &name in MESHES {
        // Isolate each mesh so a panic in draco-oxide's (WIP) encoder is
        // reported as a failure rather than aborting the whole test run.
        match std::panic::catch_unwind(AssertUnwindSafe(|| roundtrip_one(name, &decoder, &out_dir)))
        {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(msg),
            Err(_) => failures.push(format!(
                "{name}: PANIC during encode/decode (see panic output above)"
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "Google Draco failed to decode {}/{} mesh(es):\n    - {}",
        failures.len(),
        MESHES.len(),
        failures.join("\n    - "),
    );
}
