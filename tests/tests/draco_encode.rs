//! Round-trip gate for streams produced by Google Draco: encode each mesh with
//! the reference `draco_encoder`, decode it with draco-oxide, and check the
//! geometry against the source OBJ. The sibling `draco_decode` suite covers the
//! other direction, so together they pin both ends of the bitstream.
//!
//! The mesh list spans both symbol-coding methods the reference encoder picks
//! between: it takes the LengthCoded (tagged) path on the small and low-entropy
//! meshes and the DirectCoded (raw) path on the large ones.
//!
//! The test is **auto-skipped** when the reference encoder isn't available, so
//! `cargo test` still passes on machines that haven't built Google Draco. To
//! enable it, build the encoder with `scripts/build-draco.sh`, or point the
//! `DRACO_ENCODER` environment variable at an existing `draco_encoder` binary.

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;

use draco_oxide::core::attribute::AttributeType;
use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::{NdVector, PointIdx, Vector};
use draco_oxide::io::obj::load_obj;

/// Meshes from `tests/data/`, with the per-vertex position tolerance each may
/// drift by under the reference encoder's default quantization.
const CASES: &[(&str, f32)] = &[
    ("tetrahedron", 1e-3),
    ("cube_quads", 1e-3),
    ("cube_flat", 1e-3),
    ("cube_flat_random_normals", 1e-3),
    ("open_box", 1e-3),
    ("groove_fan", 1e-3),
    ("mobius", 1e-2),
    ("sphere", 1e-2),
    ("punctured_sphere", 1e-2),
    ("torus", 1e-2),
    ("bldg_894e93d9", 1e-2),
    ("Duck", 1e-2),
    ("bunny", 1e-2),
];

/// Locate Google Draco's `draco_encoder`, or `None` if it isn't available.
fn find_draco_encoder() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DRACO_ENCODER") {
        let p = PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    let default =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../third_party/draco/_build/draco_encoder");
    default.is_file().then_some(default)
}

fn positions(mesh: &Mesh) -> Vec<NdVector<3, f32>> {
    let att = mesh
        .attributes
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("decoded mesh has a position attribute");
    (0..att.len())
        .map(|i| att.get::<NdVector<3, f32>, 3>(PointIdx::from(i)))
        .collect()
}

/// The axis-aligned extent of a point set, used to normalize the tolerance.
fn bbox_diagonal(pts: &[NdVector<3, f32>]) -> f32 {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in pts {
        for c in 0..3 {
            lo[c] = lo[c].min(*p.get(c));
            hi[c] = hi[c].max(*p.get(c));
        }
    }
    ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt()
}

fn roundtrip_one(name: &str, tol: f32, encoder: &Path, out_dir: &Path) -> Result<(), String> {
    let obj_path = format!("data/{name}.obj");
    let original = load_obj(&obj_path).map_err(|e| format!("{name}: load_obj failed: {e}"))?;

    let drc_path = out_dir.join(format!("{name}.drc"));
    let output = Command::new(encoder)
        .arg("-i")
        .arg(&obj_path)
        .arg("-o")
        .arg(&drc_path)
        .output()
        .map_err(|e| format!("{name}: failed to spawn draco_encoder: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{name}: draco_encoder failed with {}\n      stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }

    let bytes =
        std::fs::read(&drc_path).map_err(|e| format!("{name}: reading .drc failed: {e}"))?;
    let decoded =
        draco_oxide::decode::decode(&bytes).map_err(|e| format!("{name}: decode failed: {e:?}"))?;

    let orig = positions(&original);
    let got = positions(&decoded);
    if got.is_empty() {
        return Err(format!("{name}: decoded mesh has no position values"));
    }
    if decoded.faces.is_empty() {
        return Err(format!("{name}: decoded mesh has no faces"));
    }

    // Every decoded position must coincide with some input position. This
    // catches a desynchronized value stream, which reshuffles values without
    // changing their count.
    let eps = tol * bbox_diagonal(&orig).max(1e-6);
    let cell = |p: &NdVector<3, f32>| {
        [
            (*p.get(0) / eps).floor() as i64,
            (*p.get(1) / eps).floor() as i64,
            (*p.get(2) / eps).floor() as i64,
        ]
    };
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (i, o) in orig.iter().enumerate() {
        grid.entry(cell(o)).or_default().push(i);
    }
    for (i, g) in got.iter().enumerate() {
        let [cx, cy, cz] = cell(g);
        let mut nearest = f32::MAX;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &j in grid.get(&[cx + dx, cy + dy, cz + dz]).into_iter().flatten() {
                        let d = (0..3)
                            .map(|c| (*g.get(c) - *orig[j].get(c)).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        nearest = nearest.min(d);
                    }
                }
            }
        }
        if nearest > eps {
            return Err(format!(
                "{name}: decoded position {i} is {nearest} from the nearest input \
                 position (tolerance {eps})"
            ));
        }
    }
    Ok(())
}

#[test]
fn draco_oxide_decodes_google_draco_output() {
    let Some(encoder) = find_draco_encoder() else {
        eprintln!(
            "SKIP draco_oxide_decodes_google_draco_output: Google Draco `draco_encoder` not \
             found.\n      Build it with `scripts/build-draco.sh`, or set DRACO_ENCODER=<path>."
        );
        return;
    };
    eprintln!("Using draco_encoder: {}", encoder.display());

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/draco_encode");
    std::fs::create_dir_all(&out_dir).expect("failed to create output dir");

    let mut failures = Vec::new();
    for &(name, tol) in CASES {
        match std::panic::catch_unwind(AssertUnwindSafe(|| {
            roundtrip_one(name, tol, &encoder, &out_dir)
        })) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(msg),
            Err(_) => failures.push(format!(
                "{name}: PANIC during encode/decode (see panic output above)"
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "draco-oxide failed to decode {}/{} Google Draco stream(s):\n    - {}",
        failures.len(),
        CASES.len(),
        failures.join("\n    - "),
    );
}
