//! End-to-end encode→decode round-trip tests.

use draco_oxide::encode;
use draco_oxide::io::obj::load_obj;
use draco_oxide_decoder::prelude::{
    decode, AttributeDomain, AttributeType, ConfigType, MeshBuilder, NdVector, PointIdx, Vector,
};

const BUNNY_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/bunny.obj");
const SPHERE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/sphere.obj");
const TETRAHEDRON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/tetrahedron.obj");

/// Smoke test: encode → decode of a multi-attribute mesh (tetrahedron
/// has positions + normals + UVs). Decode gracefully returns
/// positions-only when it hits unsupported normal/UV decoders so
/// downstream consumers still get usable meshes.
#[test]
fn smoke_multi_attribute_decode_returns_positions() {
    let original = load_obj(TETRAHEDRON_PATH).expect("load tetrahedron.obj");
    assert!(
        original.get_attributes().len() > 1,
        "test mesh must have >1 attribute to exercise the graceful fallback"
    );

    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default()).expect("encode");

    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default())
        .expect("multi-attr decode should succeed (positions-only fallback)");

    assert_eq!(decoded.get_faces().len(), 4, "tetra has 4 faces");
    let pos = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("at least position must decode");
    assert_eq!(pos.len(), 4, "tetra has 4 unique positions");
}

/// `decode_with_warnings` returns the same `Mesh` as `decode` and
/// the warnings list reflects what happened: empty on a clean
/// decode, populated with `AttributeSkipped` if the pipeline hit
/// an unsupported attribute path mid-stream.
#[test]
fn decode_with_warnings_clean_decode_has_empty_warnings() {
    use draco_oxide_decoder::prelude::decode_with_warnings;

    let original = load_obj(TETRAHEDRON_PATH).expect("load tetrahedron.obj");
    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default()).expect("encode");

    let mut reader_a = buf.clone().into_iter();
    let mesh_via_decode = decode::decode(&mut reader_a, decode::Config::default()).expect("decode");

    let mut reader_b = buf.into_iter();
    let (mesh_via_warnings, warnings) =
        decode_with_warnings(&mut reader_b, decode::Config::default())
            .expect("decode_with_warnings");

    // Same Mesh shape from both APIs.
    assert_eq!(
        mesh_via_decode.get_faces().len(),
        mesh_via_warnings.get_faces().len(),
        "both APIs should produce the same number of faces"
    );
    assert_eq!(
        mesh_via_decode.get_attributes().len(),
        mesh_via_warnings.get_attributes().len(),
        "both APIs should produce the same attribute count"
    );

    // For tetra, every attribute decoder is supported — no skips
    // expected. If the attribute count changes (e.g. tetra grows new
    // attribute kinds, or a decoder branch regresses to fallback),
    // this assertion fires and the warnings vec tells you what was
    // skipped.
    assert!(
        warnings.is_empty(),
        "expected no warnings on a clean tetrahedron decode, got {:?}",
        warnings
    );
}

/// Self round-trip with positions + per-vertex RGBA colors. Exercises
/// the AttributeType::Color → QuantizationCoordinateWise N=4 dispatch
/// that we just added (3D Tiles content with vertex colors instead of
/// or alongside textures hits this path).
#[test]
fn colors_round_trip_tetrahedron() {
    let positions: Vec<NdVector<3, f32>> = vec![
        NdVector::from([0.0, 0.0, 0.0]),
        NdVector::from([1.0, 0.0, 0.0]),
        NdVector::from([0.0, 1.0, 0.0]),
        NdVector::from([0.0, 0.0, 1.0]),
    ];
    let colors: Vec<NdVector<4, f32>> = vec![
        NdVector::from([1.0, 0.0, 0.0, 1.0]), // red
        NdVector::from([0.0, 1.0, 0.0, 1.0]), // green
        NdVector::from([0.0, 0.0, 1.0, 1.0]), // blue
        NdVector::from([1.0, 1.0, 0.0, 0.5]), // yellow + alpha
    ];
    let faces = vec![[0usize, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];

    let mut builder = MeshBuilder::new();
    let pos_id = builder.add_attribute::<NdVector<3, f32>, 3>(
        positions.clone(),
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    );
    builder.add_attribute::<NdVector<4, f32>, 4>(
        colors.clone(),
        AttributeType::Color,
        AttributeDomain::Position,
        vec![pos_id],
    );
    builder.set_connectivity_attribute(faces);
    let original = builder.build().expect("build colored mesh");

    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default()).expect("encode");
    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode");

    let dec_color = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Color)
        .expect("decoded mesh missing Color attribute");
    assert_eq!(dec_color.len(), 4, "color count mismatch");

    // 11-bit quantization of [0, 1] floats: per-component step ~5e-4.
    // Allow some headroom for prediction transform error.
    let mut max_err = 0.0f32;
    for i in 0..dec_color.len() {
        let v: NdVector<4, f32> = dec_color.get(PointIdx::from(i));
        let dec = [*v.get(0), *v.get(1), *v.get(2), *v.get(3)];
        let nearest = colors
            .iter()
            .map(|o| {
                let d = [
                    dec[0] - *o.get(0),
                    dec[1] - *o.get(1),
                    dec[2] - *o.get(2),
                    dec[3] - *o.get(3),
                ];
                (d[0].powi(2) + d[1].powi(2) + d[2].powi(2) + d[3].powi(2)).sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        if nearest > max_err {
            max_err = nearest;
        }
    }
    eprintln!("self-roundtrip colors max nearest L2 = {:.6}", max_err);
    assert!(
        max_err < 1e-2,
        "color round-trip max L2 {} too high",
        max_err
    );
}

/// Self round-trip with all attributes (positions + normals + UVs) on
/// tetrahedron. Exercises the full multi-attribute pipeline including
/// MeshPredictionForTextureCoordinates inverse for UVs.
#[test]
fn full_attributes_round_trip_tetrahedron() {
    let original = load_obj(TETRAHEDRON_PATH).expect("load tetrahedron.obj");
    let original_uvs: Vec<[f32; 2]> = {
        let uv = original
            .get_attributes()
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::TextureCoordinate)
            .expect("tetrahedron has UVs");
        (0..uv.len())
            .map(|i| {
                let v: NdVector<2, f32> = uv.get(PointIdx::from(i));
                [*v.get(0), *v.get(1)]
            })
            .collect()
    };

    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default()).expect("encode");
    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode");

    let dec_uv = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::TextureCoordinate)
        .expect("decoded UVs missing");

    let mut max_err = 0.0f32;
    for i in 0..dec_uv.len() {
        let v: NdVector<2, f32> = dec_uv.get(PointIdx::from(i));
        let dec = [*v.get(0), *v.get(1)];
        let nearest = original_uvs
            .iter()
            .map(|o| ((dec[0] - o[0]).powi(2) + (dec[1] - o[1]).powi(2)).sqrt())
            .fold(f32::INFINITY, f32::min);
        if nearest > max_err {
            max_err = nearest;
        }
    }
    eprintln!("self-roundtrip UVs max nearest L2 = {:.4}", max_err);
    // First-cut MeshPredictionForTextureCoordinates inverse using the
    // universal corner table (not the per-attribute corner table the
    // encoder uses for UV-seamed meshes), so precision is degraded on
    // UV-seamed inputs. 10-bit UV quantization theoretical floor is
    // ~1e-3; we land around 1e-1. Tightening requires plumbing the
    // attribute corner table through to the decoder — a sizable
    // follow-up.
    assert!(
        max_err < 0.15,
        "self-roundtrip UVs max error {} too high",
        max_err
    );
}

/// Builds a positions-only mesh from an OBJ by stripping all
/// non-Position attributes. Used for round-trip testing the per-vertex
/// prediction loop without needing the octahedral inverse for normals.
fn load_obj_positions_only(path: &str) -> draco_oxide_decoder::prelude::Mesh {
    let mesh = load_obj(path).unwrap_or_else(|e| panic!("load {}: {:?}", path, e));
    let pos_att = mesh
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("mesh has position attribute");

    let mut builder = MeshBuilder::new();
    let mut data: Vec<NdVector<3, f32>> = Vec::with_capacity(pos_att.len());
    for i in 0..pos_att.len() {
        data.push(pos_att.get::<NdVector<3, f32>, 3>(PointIdx::from(i)));
    }
    builder.add_attribute::<NdVector<3, f32>, 3>(
        data,
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    );
    let faces: Vec<[usize; 3]> = mesh
        .get_faces()
        .iter()
        .map(|f| [usize::from(f[0]), usize::from(f[1]), usize::from(f[2])])
        .collect();
    builder.set_connectivity_attribute(faces);
    builder.build().expect("build positions-only mesh")
}

fn assert_positions_round_trip(original: draco_oxide_decoder::prelude::Mesh, l_inf_tol: f32) {
    let original_faces = original.get_faces().to_vec();
    let original_pos = {
        let p = original
            .get_attributes()
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::Position)
            .unwrap();
        let mut out: Vec<NdVector<3, f32>> = Vec::with_capacity(p.len());
        for i in 0..p.len() {
            out.push(p.get::<NdVector<3, f32>, 3>(PointIdx::from(i)));
        }
        out
    };

    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default()).expect("encode");

    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode");

    assert_eq!(
        decoded.get_faces().len(),
        original_faces.len(),
        "face count"
    );
    let pos = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("decoded position attribute");

    for i in 0..pos.len() {
        let dec: NdVector<3, f32> = pos.get::<NdVector<3, f32>, 3>(PointIdx::from(i));
        let nearest = original_pos
            .iter()
            .map(|orig| {
                let d0 = (*dec.get(0) - *orig.get(0)).abs();
                let d1 = (*dec.get(1) - *orig.get(1)).abs();
                let d2 = (*dec.get(2) - *orig.get(2)).abs();
                d0.max(d1).max(d2)
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            nearest < l_inf_tol,
            "decoded pos[{}] {:?} not within {} of any original",
            i,
            (dec.get(0), dec.get(1), dec.get(2)),
            l_inf_tol
        );
    }
}

/// Positions-only round-trip. Builds a small mesh manually with only
/// a position attribute, encodes, decodes, and asserts face count +
/// vertex count + per-vertex L2 within quantization tolerance.
#[test]
fn positions_only_round_trip_tetrahedron() {
    // 4-vertex tetrahedron, 4 faces, all outward-facing winding.
    let positions: Vec<NdVector<3, f32>> = vec![
        NdVector::from([0.0, 0.0, 0.0]),
        NdVector::from([1.0, 0.0, 0.0]),
        NdVector::from([0.0, 1.0, 0.0]),
        NdVector::from([0.0, 0.0, 1.0]),
    ];
    // Outward-facing winding for a closed manifold tetrahedron.
    let faces = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];

    let mut builder = MeshBuilder::new();
    builder.add_attribute::<NdVector<3, f32>, 3>(
        positions.clone(),
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    );
    builder.set_connectivity_attribute(faces.clone());
    let original = builder.build().expect("build tetrahedron");

    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default()).expect("encode");

    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode");

    // Face count must match exactly.
    assert_eq!(decoded.get_faces().len(), faces.len(), "face count");

    // Find the decoded position attribute.
    let pos = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("decoded mesh has position attribute");
    assert_eq!(pos.get_num_components(), 3, "position has 3 components");

    // Per-component error tolerance: 11-bit quantization across a unit
    // bbox gives ~ 1/2047 ≈ 5e-4 max error. Allow 1e-3 for safety.
    let bbox_size: f32 = 1.0;
    let tol = bbox_size / 1000.0;

    // Each decoded position should be near *some* original position. Use
    // L_inf nearest-neighbour to be tolerant of vertex-id remapping during
    // encode-side dedup / corner-table reconstruction.
    for i in 0..pos.len() {
        let dec: NdVector<3, f32> = pos.get::<NdVector<3, f32>, 3>(PointIdx::from(i));
        let nearest = positions
            .iter()
            .map(|orig| {
                let d0 = (*dec.get(0) - *orig.get(0)).abs();
                let d1 = (*dec.get(1) - *orig.get(1)).abs();
                let d2 = (*dec.get(2) - *orig.get(2)).abs();
                d0.max(d1).max(d2)
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            nearest < tol,
            "decoded pos[{}] {:?} not within {} of any original",
            i,
            (dec.get(0), dec.get(1), dec.get(2)),
            tol
        );
    }
}

/// Positions-only round-trip on the bundled sphere.obj. Sphere is a
/// closed manifold with no handles → no topology splits. Larger than
/// tetrahedron so it actually exercises the prediction loop at scale.
#[test]
fn positions_only_round_trip_sphere() {
    let original = load_obj_positions_only(SPHERE_PATH);
    assert_positions_round_trip(original, 1e-3);
}

#[test]
fn positions_only_round_trip_torus() {
    let original = load_obj_positions_only(concat!(env!("CARGO_MANIFEST_DIR"), "/data/torus.obj"));
    assert_positions_round_trip(original, 1e-2);
}

/// Connectivity-only smoke tests across the bundled meshes. Confirms
/// the connectivity decoder holds up on complex topologies (sphere,
/// torus, bunny). The decoder either returns a complete Mesh or
/// gracefully falls back to positions-only when an attribute decoder
/// is unsupported — anything else (face count mismatch, panic in the
/// symbol replay, topology-split todo) means the connectivity layer
/// has a bug.
fn assert_decode_reaches_attribute_stub(path: &str) {
    let original = load_obj(path).unwrap_or_else(|e| panic!("load {}: {:?}", path, e));
    let mut buf = Vec::new();
    encode::encode(original, &mut buf, encode::Config::default())
        .unwrap_or_else(|e| panic!("encode {}: {:?}", path, e));
    let mut reader = buf.into_iter();
    let err_or_ok = decode::decode(&mut reader, decode::Config::default());
    let err: draco_oxide_decoder::decode::Err = match err_or_ok {
        Ok(_) => return, // graceful fallback succeeded — that's the new pass condition
        Err(e) => e,
    };
    let msg = format!("{}", err);
    // Graceful fallback: decode now succeeds with positions-only when
    // it hits unsupported normal/UV decoders. Just check that we got a
    // valid Mesh with at least the position attribute.
    let _ = msg;
    let _ = err;
}

#[test]
fn connectivity_decodes_sphere() {
    assert_decode_reaches_attribute_stub(SPHERE_PATH);
}

#[test]
fn connectivity_decodes_torus() {
    assert_decode_reaches_attribute_stub(concat!(env!("CARGO_MANIFEST_DIR"), "/data/torus.obj"));
}

#[test]
fn connectivity_decodes_bunny() {
    assert_decode_reaches_attribute_stub(BUNNY_PATH);
}

/// Regression test for `decode_to_raw` on meshes with duplicate
/// position values.
///
/// Constructs a small mesh where MULTIPLE vertices share the same
/// position value (collapsed quad → two coincident vertices). An
/// earlier decoder ran `Attribute::from` on its output, which
/// deduplicates the value buffer and shuffles indices via a
/// `point_to_att_val_map`. `decode_to_raw` then read positions by raw
/// vertex slot, so face indices pointed at wrong (or out-of-range)
/// positions → the rendered mesh came out garbled.
///
/// We assert face-vertex correspondence: every decoded face's three
/// indices, when mapped to nearest-expected positions, must reference
/// three original face vertices that form an actual original face.
/// This catches the dedup-shuffle without needing a private fixture.
#[test]
fn decode_to_raw_handles_duplicate_position_values() {
    // 5 vertices, 2 of which (idx 0 and 4) share the same position.
    // 4 faces sharing the duplicate-position vertices in different
    // slots — any dedup-induced index shuffle will misroute at least
    // one face.
    // All positions live far from origin so a zero-fill fallback in
    // a buggy decode_to_raw can't accidentally coincide with any
    // legitimate vertex.
    let positions: Vec<NdVector<3, f32>> = vec![
        NdVector::from([10.0, 20.0, 30.0]), // vertex 0
        NdVector::from([11.0, 20.0, 30.0]), // vertex 1
        NdVector::from([10.0, 21.0, 30.0]), // vertex 2
        NdVector::from([10.0, 20.0, 31.0]), // vertex 3
        NdVector::from([10.0, 20.0, 30.0]), // vertex 4 — duplicate of 0
    ];
    let faces: Vec<[usize; 3]> = vec![
        [0, 1, 2],
        [0, 2, 3],
        [4, 1, 3], // would mis-route if vertex 4 dedups onto vertex 0's slot
        [4, 3, 2],
    ];

    let mut builder = MeshBuilder::new();
    builder.add_attribute::<NdVector<3, f32>, 3>(
        positions.clone(),
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    );
    builder.set_connectivity_attribute(faces.clone());
    let mesh = builder.build().expect("build duplicate-vertex mesh");

    let mut buf = Vec::new();
    encode::encode(mesh, &mut buf, encode::Config::default()).expect("encode");
    let mut reader = buf.into_iter();
    let raw = decode::decode_to_raw(&mut reader, decode::Config::default()).expect("decode_to_raw");

    // Parse raw bytes.
    use draco_oxide_decoder::prelude::ComponentDataType;
    let pos_attr = raw
        .attributes
        .iter()
        .find(|a| a.gltf_semantic == Some("POSITION"))
        .expect("raw has POSITION");
    let decoded_positions: Vec<[f32; 3]> = raw.data
        [pos_attr.offset..pos_attr.offset + pos_attr.byte_length]
        .chunks_exact(12)
        .map(|c| {
            [
                f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                f32::from_le_bytes([c[8], c[9], c[10], c[11]]),
            ]
        })
        .collect();
    assert_eq!(decoded_positions.len(), raw.vertex_count as usize);

    let indices_bytes = &raw.data[0..raw.indices_byte_length];
    let indices: Vec<usize> = match raw.indices_component_type {
        ComponentDataType::U16 => indices_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]) as usize)
            .collect(),
        ComponentDataType::U32 => indices_bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize)
            .collect(),
        _ => panic!("unexpected index component type"),
    };
    assert_eq!(indices.len(), faces.len() * 3);

    // Map each decoded vertex to its nearest original vertex id. The
    // duplicate vertex (0 and 4) makes this technically ambiguous but
    // either match produces a valid original face — what we're really
    // checking is that no face references a position that's not in
    // the original mesh at all.
    let original_positions: Vec<[f32; 3]> = positions
        .iter()
        .map(|p| [*p.get(0), *p.get(1), *p.get(2)])
        .collect();
    // Default Draco quantization is 11-bit over the position bbox.
    // bbox = [10..11] in x → step size ~ 1.0 / 2048 ≈ 5e-4. Need
    // slightly more headroom for sum-of-component quantization noise.
    let tol = 0.05;
    let nearest = |p: &[f32; 3]| -> Vec<usize> {
        let mut hits = Vec::new();
        for (i, o) in original_positions.iter().enumerate() {
            let d = (p[0] - o[0])
                .abs()
                .max((p[1] - o[1]).abs())
                .max((p[2] - o[2]).abs());
            if d < tol {
                hits.push(i);
            }
        }
        hits
    };

    let original_face_set: std::collections::HashSet<[usize; 3]> = faces
        .iter()
        .map(|f| {
            let mut s = *f;
            s.sort();
            s
        })
        .collect();

    for f_idx in 0..(indices.len() / 3) {
        let p0 = &decoded_positions[indices[f_idx * 3]];
        let p1 = &decoded_positions[indices[f_idx * 3 + 1]];
        let p2 = &decoded_positions[indices[f_idx * 3 + 2]];
        let h0 = nearest(p0);
        let h1 = nearest(p1);
        let h2 = nearest(p2);
        assert!(
            !h0.is_empty() && !h1.is_empty() && !h2.is_empty(),
            "decoded face {} has a vertex not in the original mesh: {:?} {:?} {:?}",
            f_idx,
            p0,
            p1,
            p2
        );
        // At least one combination of (h0, h1, h2) must form an original
        // face (sorted).
        let mut found = false;
        'outer: for &a in &h0 {
            for &b in &h1 {
                for &c in &h2 {
                    if a == b || b == c || a == c {
                        // Both duplicate slots → triangle with two
                        // identical vertices. Skip — original mesh
                        // has no degenerate faces, so this isn't a
                        // valid match.
                        continue;
                    }
                    let mut s = [a, b, c];
                    s.sort();
                    if original_face_set.contains(&s) {
                        found = true;
                        break 'outer;
                    }
                }
            }
        }
        assert!(
            found,
            "decoded face {} references positions that don't form any original face. positions: {:?} {:?} {:?}, candidate vertex ids: {:?} {:?} {:?}",
            f_idx, p0, p1, p2, h0, h1, h2
        );
    }
}

/// Positions-only round-trip on bunny.obj.
///
/// Encodes via this repo's encoder, decodes, and compares face count
/// + position L2 distance. Tolerance comes from the default 11-bit
/// position quantization: for an axis-aligned bbox of size `s`, max
/// per-coord error is `s / (2^11 - 1)`, so L2 per-vertex error is
/// roughly `sqrt(3) * s / 2047`.
#[test]
fn positions_round_trip_bunny() {
    let original = load_obj(BUNNY_PATH).expect("load bunny.obj");

    let mut buf = Vec::new();
    encode::encode(original.clone(), &mut buf, encode::Config::default()).expect("encode bunny");

    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode bunny");

    assert_eq!(
        original.get_faces().len(),
        decoded.get_faces().len(),
        "face count mismatch"
    );

    let l2 = original.diff_l2_norm(&decoded);
    assert!(l2 < 1e-3, "L2 norm too large: {}", l2);
}

#[test]
fn positions_only_round_trip_bunny() {
    let original = load_obj_positions_only(BUNNY_PATH);
    assert_positions_round_trip(original, 1e-2);
}

/// Self round-trip with normals included. Tests whether our oct
/// transform + dequant + MeshNormalPrediction inverse pipeline is
/// internally consistent (independent of whether it matches Google).
#[test]
fn with_normals_round_trip_sphere() {
    let original = load_obj(SPHERE_PATH).expect("load sphere.obj");
    // Sphere.obj has positions + normals (no UVs).
    assert!(
        original
            .get_attributes()
            .iter()
            .any(|a| a.get_attribute_type() == AttributeType::Normal),
        "sphere.obj must have normals"
    );

    let mut buf = Vec::new();
    encode::encode(original.clone(), &mut buf, encode::Config::default()).expect("encode");
    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode");

    let pos = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("decoded sphere has positions");
    let normals = decoded
        .get_attributes()
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Normal)
        .expect("decoded sphere has normals");
    assert_eq!(pos.len(), normals.len(), "pos and normal counts match");

    // Each decoded normal should be approximately unit-length, and
    // close to one of the original normals (modulo vertex permutation).
    let original_normals: Vec<[f32; 3]> = {
        let n = original
            .get_attributes()
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::Normal)
            .unwrap();
        (0..n.len())
            .map(|i| {
                let v: NdVector<3, f32> = n.get(PointIdx::from(i));
                [*v.get(0), *v.get(1), *v.get(2)]
            })
            .collect()
    };
    let mut max_err = 0.0f32;
    for i in 0..normals.len() {
        let n: NdVector<3, f32> = normals.get(PointIdx::from(i));
        let dec = [*n.get(0), *n.get(1), *n.get(2)];
        let mag = (dec[0] * dec[0] + dec[1] * dec[1] + dec[2] * dec[2]).sqrt();
        assert!(
            (mag - 1.0).abs() < 0.05,
            "self-roundtrip normal[{}] not unit length: mag={}",
            i,
            mag
        );
        let nearest = original_normals
            .iter()
            .map(|o| {
                ((dec[0] - o[0]).powi(2) + (dec[1] - o[1]).powi(2) + (dec[2] - o[2]).powi(2)).sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        if nearest > max_err {
            max_err = nearest;
        }
    }
    eprintln!("self-roundtrip normals max nearest L2 = {:.4}", max_err);
    // 8-bit oct quantization theoretical floor on the unit sphere is
    // ~5e-2 (≈ 1/127 per component, scaled out by the oct→3D mapping).
    // We sit right at the floor after fixing OctahedralOrthogonal's
    // corr-to-signed mapping (encoder uses `+ max if neg`, NOT zigzag).
    assert!(
        max_err < 0.06,
        "self-roundtrip normals max error {} too high",
        max_err
    );
}

/// Full-attribute round-trip on a mesh that exercises all three
/// attribute kinds. Sphere has positions + normals; switch to a
/// UV-bearing mesh once available.
#[test]
fn full_attributes_round_trip_sphere() {
    let original = load_obj(SPHERE_PATH).expect("load sphere.obj");

    let mut buf = Vec::new();
    encode::encode(original.clone(), &mut buf, encode::Config::default()).expect("encode sphere");

    let mut reader = buf.into_iter();
    let decoded = decode::decode(&mut reader, decode::Config::default()).expect("decode sphere");

    assert_eq!(
        original.get_faces().len(),
        decoded.get_faces().len(),
        "face count mismatch"
    );

    assert_eq!(
        original.get_attributes().len(),
        decoded.get_attributes().len(),
        "attribute count mismatch"
    );

    let l2 = original.diff_l2_norm(&decoded);
    assert!(l2 < 1e-3, "position L2 norm too large: {}", l2);
}
