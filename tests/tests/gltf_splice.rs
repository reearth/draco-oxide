//! Integration coverage for the decoder's glTF / GLB
//! `KHR_draco_mesh_compression` helpers (`draco-oxide-decoder`'s `gltf`
//! feature): `decode_glb` (decode every Draco primitive) and
//! `splice_glb_remove_draco` (rewrite the GLB with the Draco extension removed
//! and the geometry inlined uncompressed).
//!
//! Driven by the bundled real 3D-Tiles `.b3dm` tile, whose embedded GLB carries
//! Draco-compressed primitives with POSITION + NORMAL + TEXCOORD_0.

use draco_oxide_decoder::io::gltf::draco_decoder::{decode_glb, splice_glb_remove_draco};
use draco_oxide_decoder::prelude::AttributeType;
use std::path::PathBuf;

fn b3dm_glb() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/b3dm/skyline_mesh_1252.b3dm");
    let b3dm = std::fs::read(&path).expect("read bundled b3dm fixture");
    // Strip the 28-byte b3dm header + feature/batch tables to reach the GLB.
    assert_eq!(&b3dm[0..4], b"b3dm", "fixture is not a b3dm");
    let u32_at = |o: usize| u32::from_le_bytes([b3dm[o], b3dm[o + 1], b3dm[o + 2], b3dm[o + 3]]) as usize;
    let glb_start = 28 + u32_at(12) + u32_at(16) + u32_at(20) + u32_at(24);
    b3dm[glb_start..].to_vec()
}

#[test]
fn decode_glb_returns_positioned_primitives() {
    let glb = b3dm_glb();
    let prims = decode_glb(&glb).expect("decode_glb");
    assert!(!prims.is_empty(), "expected at least one Draco primitive");
    for p in &prims {
        assert!(
            !p.mesh.get_faces().is_empty(),
            "primitive (mesh {}, prim {}) decoded no faces",
            p.mesh_idx,
            p.primitive_idx
        );
        let has_pos = p
            .mesh
            .get_attributes()
            .iter()
            .any(|a| a.get_attribute_type() == AttributeType::Position);
        assert!(has_pos, "primitive missing POSITION");
    }
    eprintln!("decode_glb: {} Draco primitive(s) decoded", prims.len());
}

#[test]
fn splice_removes_draco_and_stays_valid_glb() {
    let glb = b3dm_glb();
    let out = splice_glb_remove_draco(&glb).expect("splice_glb_remove_draco");
    // Still a GLB container.
    assert_eq!(&out[0..4], b"glTF", "spliced output is not a GLB");
    // The Draco extension must be gone from the JSON chunk.
    let text = String::from_utf8_lossy(&out);
    assert!(
        !text.contains("KHR_draco_mesh_compression"),
        "spliced GLB still references KHR_draco_mesh_compression"
    );
    // Uncompressed geometry: the spliced GLB should be larger than the input.
    assert!(
        out.len() > glb.len(),
        "expected inlined (uncompressed) GLB to grow: {} -> {}",
        glb.len(),
        out.len()
    );
    // And it should still round-trip through decode_glb as a no-op (no Draco left).
    let prims = decode_glb(&out).expect("decode spliced glb");
    assert!(
        prims.is_empty(),
        "spliced GLB should have no Draco primitives left, found {}",
        prims.len()
    );
}
