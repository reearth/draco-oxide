//! End-to-end encode-output stability gate.
//!
//! Locks the encoded byte stream for a set of meshes so the O(n^2) perf fixes
//! in `compute_sequence` (Edgebreaker traversal order) and
//! `MeshParallelogramPrediction::predict` (attribute prediction) cannot
//! silently change output. draco-oxide's encoded bytes are consumed by
//! Google's Draco decoder in the field, so they must stay byte-identical
//! across these optimizations.
//!
//! The fingerprints were captured from the pre-optimization implementation and
//! confirmed byte-identical on the full 930-tile HighPoly corpus.

use draco_oxide::prelude::ConfigType;
use draco_oxide::{
    encode::{self, encode},
    io::obj::load_obj,
};

/// FNV-1a over the encoded byte stream. Deterministic, dependency-free.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn encode_fingerprint(obj: &str) -> (usize, u64) {
    let mesh = load_obj(obj).unwrap();
    let mut buf = Vec::new();
    encode(mesh, &mut buf, encode::Config::default()).unwrap();
    (buf.len(), fnv1a(&buf))
}

#[test]
fn encode_output_is_byte_stable() {
    // (obj, expected_len, expected_fnv1a). tetrahedron carries position +
    // normal + texcoord attributes, exercising all three mesh prediction
    // schemes; sphere/torus/bunny exercise position (parallelogram) at scale
    // and over handle topology (torus).
    let cases: &[(&str, usize, u64)] = &[
        ("tests/data/tetrahedron.obj", EXPECT_TETRA_LEN, EXPECT_TETRA_HASH),
        ("tests/data/sphere.obj", EXPECT_SPHERE_LEN, EXPECT_SPHERE_HASH),
        ("tests/data/torus.obj", EXPECT_TORUS_LEN, EXPECT_TORUS_HASH),
        ("tests/data/bunny.obj", EXPECT_BUNNY_LEN, EXPECT_BUNNY_HASH),
    ];
    let dump = std::env::var("DUMP_ENCODE_FINGERPRINTS").is_ok();
    for (obj, exp_len, exp_hash) in cases {
        let (len, hash) = encode_fingerprint(obj);
        if dump {
            eprintln!("{obj} => len={len} hash={hash}");
            continue;
        }
        assert_eq!(
            (len, hash),
            (*exp_len, *exp_hash),
            "encoded output changed for {obj}"
        );
    }
}

const EXPECT_TETRA_LEN: usize = 846;
const EXPECT_TETRA_HASH: u64 = 620833109304232433;
const EXPECT_SPHERE_LEN: usize = 1962;
const EXPECT_SPHERE_HASH: u64 = 11855373330842349000;
const EXPECT_TORUS_LEN: usize = 4181;
const EXPECT_TORUS_HASH: u64 = 8060327790296598891;
const EXPECT_BUNNY_LEN: usize = 78933;
const EXPECT_BUNNY_HASH: u64 = 6236277559521459486;
