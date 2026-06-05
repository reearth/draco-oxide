//! Integration tests for `encode::Config::set_attribute_explicit_quantization`.
//!
//! Verifies the contract that tiled-output emitters rely on: when the
//! caller supplies an explicit `(origin, range, quantization_bits)`, those
//! values appear verbatim in the bitstream metadata regardless of the
//! per-mesh attribute values. Two encodes of different meshes under the
//! same explicit-quantization config produce identical quantization
//! metadata bytes — the property that makes cross-tile vertex
//! determinism possible.

use draco_oxide::encode::{self, encode};
use draco_oxide::io::obj::load_obj;
use draco_oxide::prelude::{AttributeType, ConfigType};

/// Builds the 17-byte little-endian metadata blob that
/// `QuantizationCoordinateWise::new` writes for a 3D POSITION attribute:
/// `min_values[3] as f32 LE`, then `range as f32 LE`, then `bits as u8`.
fn expected_metadata_blob(origin: &[f32; 3], range: f32, bits: u8) -> Vec<u8> {
    let mut blob = Vec::with_capacity(17);
    for c in origin {
        blob.extend_from_slice(&c.to_le_bytes());
    }
    blob.extend_from_slice(&range.to_le_bytes());
    blob.push(bits);
    blob
}

fn contains_blob(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The supplied (origin, range, bits) must appear verbatim in the
/// encoded bitstream. This is the basic "the values you gave us are
/// what we wrote" check.
#[test]
fn explicit_quantization_writes_supplied_metadata() {
    let mesh = load_obj("tests/data/cube_quads.obj").unwrap();

    // Values deliberately wider than the cube_quads bbox so they can't
    // collide with what the per-mesh scan would have produced.
    let origin = [-100.0_f32, -100.0, -100.0];
    let range = 250.0_f32;
    let bits: u8 = 14;

    let mut cfg = encode::Config::default();
    cfg.set_attribute_explicit_quantization(
        AttributeType::Position,
        bits as i32,
        3,
        &origin,
        range,
    );

    let mut buf = Vec::new();
    encode(mesh, &mut buf, cfg).unwrap();

    let blob = expected_metadata_blob(&origin, range, bits);
    assert!(
        contains_blob(&buf, &blob),
        "supplied (origin={:?}, range={}, bits={}) blob ({} bytes) not found in encoded bitstream ({} bytes)",
        origin,
        range,
        bits,
        blob.len(),
        buf.len(),
    );
}

/// Two different meshes encoded with the same explicit-quantization
/// config must both carry the same supplied metadata blob. This is the
/// cross-tile determinism contract: identical lattice across encodes.
#[test]
fn explicit_quantization_is_deterministic_across_meshes() {
    let mesh_a = load_obj("tests/data/cube_quads.obj").unwrap();
    let mesh_b = load_obj("tests/data/tetrahedron.obj").unwrap();

    let origin = [-100.0_f32, -100.0, -100.0];
    let range = 250.0_f32;
    let bits: u8 = 14;

    let mut cfg_a = encode::Config::default();
    cfg_a.set_attribute_explicit_quantization(
        AttributeType::Position,
        bits as i32,
        3,
        &origin,
        range,
    );
    let cfg_b = cfg_a.clone();

    let mut buf_a = Vec::new();
    encode(mesh_a, &mut buf_a, cfg_a).unwrap();
    let mut buf_b = Vec::new();
    encode(mesh_b, &mut buf_b, cfg_b).unwrap();

    // The two bitstreams will differ overall (different meshes); only the
    // quantization metadata is required to match.
    assert_ne!(
        buf_a, buf_b,
        "different meshes should produce different bitstreams"
    );

    let blob = expected_metadata_blob(&origin, range, bits);
    assert!(
        contains_blob(&buf_a, &blob),
        "supplied metadata blob not found in mesh-A bitstream"
    );
    assert!(
        contains_blob(&buf_b, &blob),
        "supplied metadata blob not found in mesh-B bitstream"
    );
}

/// Without the explicit-quantization setter, the encoder falls back to
/// the per-mesh scan and produces a different bitstream than the
/// explicit-quant path. This guards against a regression where the
/// scan-path-fallback silently breaks.
#[test]
fn explicit_quantization_changes_bitstream_vs_default() {
    let mesh = load_obj("tests/data/cube_quads.obj").unwrap();

    let mut buf_default = Vec::new();
    encode(mesh.clone(), &mut buf_default, encode::Config::default()).unwrap();

    let origin = [-100.0_f32, -100.0, -100.0];
    let range = 250.0_f32;
    let bits: u8 = 14;
    let mut cfg = encode::Config::default();
    cfg.set_attribute_explicit_quantization(
        AttributeType::Position,
        bits as i32,
        3,
        &origin,
        range,
    );
    let mut buf_explicit = Vec::new();
    encode(mesh, &mut buf_explicit, cfg).unwrap();

    assert_ne!(
        buf_default, buf_explicit,
        "explicit-quant config must produce a different bitstream than default"
    );
}
