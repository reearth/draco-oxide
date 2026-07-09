//! Parity tests: decode real Google-`draco_encoder` bitstreams with the
//! draco-oxide decoder and compare against the ground-truth `.expected.obj`
//! that Google's `draco_decoder` produced for the same `.drc`. These are the
//! tests that catch decoder correctness bugs, and are self-contained (no
//! reference C++ binary required — the expected geometry is bundled).
//!
//! Ported onto the post-crate-split layout: decode entry points now live in
//! `draco_oxide_decoder`.

use draco_oxide_decoder::prelude::{
    decode, AttributeType, ConfigType, NdVector, PointIdx, Vector,
};
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/google_fixtures")
}

fn read_drc(name: &str) -> Vec<u8> {
    let path = fixture_dir().join(format!("{}_pos_cl7.drc", name));
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e))
}

/// Reads a Google-decoded `.expected.obj` (`v`/`f` lines only).
/// Returns (positions, faces-as-zero-based-vertex-ids).
fn read_expected_obj(name: &str) -> (Vec<[f32; 3]>, Vec<[usize; 3]>) {
    let path = fixture_dir().join(format!("{}_pos_cl7.expected.obj", name));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read expected obj {}: {}", path.display(), e));
    let mut positions = Vec::new();
    let mut faces = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("v ") {
            let parts: Vec<f32> = rest
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            assert!(parts.len() >= 3, "v line: {}", line);
            positions.push([parts[0], parts[1], parts[2]]);
        } else if let Some(rest) = line.strip_prefix("f ") {
            let idxs: Vec<usize> = rest
                .split_whitespace()
                .filter_map(|tok| tok.split('/').next()?.parse::<usize>().ok())
                .map(|i| i - 1)
                .collect();
            assert!(idxs.len() == 3, "f line not a triangle: {}", line);
            faces.push([idxs[0], idxs[1], idxs[2]]);
        }
    }
    (positions, faces)
}

/// L_inf nearest-neighbour distance from `dec` to any element in `originals`.
fn nearest_l_inf(dec: [f32; 3], originals: &[[f32; 3]]) -> f32 {
    originals
        .iter()
        .map(|o| {
            let d0 = (dec[0] - o[0]).abs();
            let d1 = (dec[1] - o[1]).abs();
            let d2 = (dec[2] - o[2]).abs();
            d0.max(d1).max(d2)
        })
        .fold(f32::INFINITY, f32::min)
}

fn assert_decodes_compatibly(name: &str, l_inf_tol: f32) {
    let buf = read_drc(name);
    let (expected_positions, expected_faces) = read_expected_obj(name);

    let mut reader = buf.into_iter();
    let mesh = decode::decode(&mut reader, decode::Config::default())
        .unwrap_or_else(|e| panic!("{}: decode failed: {}", name, e));

    assert_eq!(
        mesh.get_faces().len(),
        expected_faces.len(),
        "{}: face count",
        name
    );

    let pos_att = mesh
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .unwrap_or_else(|| panic!("{}: decoded mesh has no position attribute", name));
    assert_eq!(
        pos_att.len(),
        expected_positions.len(),
        "{}: position count mismatch",
        name
    );

    // Tolerant of vertex-id permutation: the SET of positions should match.
    let mut max_err: f32 = 0.0;
    for i in 0..pos_att.len() {
        let v: NdVector<3, f32> = pos_att.get(PointIdx::from(i));
        let dec = [*v.get(0), *v.get(1), *v.get(2)];
        let err = nearest_l_inf(dec, &expected_positions);
        max_err = max_err.max(err);
        assert!(
            err < l_inf_tol,
            "{}: decoded pos[{}] = {:?} not within {} of any expected (closest err = {})",
            name,
            i,
            dec,
            l_inf_tol,
            err
        );
    }
    eprintln!("{}: max per-vertex L_inf error = {:.6}", name, max_err);
}

#[test]
fn google_tetrahedron_decodes() {
    assert_decodes_compatibly("tetrahedron", 1e-3);
}

#[test]
fn google_sphere_decodes() {
    assert_decodes_compatibly("sphere", 1e-2);
}

// Torus — Valence Edgebreaker at cl=7, genus > 0 (has handles), so it
// exercises the full valence traversal + topology-split decode path.
#[test]
fn google_torus_decodes() {
    assert_decodes_compatibly("torus", 1e-2);
}
