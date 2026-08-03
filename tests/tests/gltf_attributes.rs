//! Non-core glTF attributes survive transcoding in stock-decodable form.
//!
//! Builds synthetic GLBs around a quad strip, transcodes them, and checks the
//! Draco payload and the rewritten JSON. Feature IDs (`EXT_mesh_features`
//! `_FEATURE_ID_0`): ids fitting u16 ride an UNSIGNED_SHORT accessor, wider
//! ids ride u32 with a FLOAT accessor and must not clamp. Tangents: written
//! with the generic wire type, since a stock decoder rejects `att_type >= 5`.

use draco_oxide::core::attribute::{AttributeType, ComponentDataType};
use draco_oxide::core::types::{NdVector, Vector};
use draco_oxide::io::gltf::{glb, transcoder::GltfTranscoder};
use serde_json::{json, Value};

/// A quad strip GLB with `n` vertices carrying the given extra attributes.
fn build_glb(n: usize, feature_ids: Option<&[f32]>, tangents: Option<&[[f32; 4]]>) -> Vec<u8> {
    assert!(n >= 4 && n % 2 == 0);
    let mut positions: Vec<f32> = Vec::new();
    for i in 0..n / 2 {
        positions.extend([i as f32, 0.0, 0.0]);
        positions.extend([i as f32, 1.0, 0.0]);
    }
    let mut indices: Vec<u16> = Vec::new();
    for i in 0..(n / 2 - 1) as u16 {
        let a = 2 * i;
        indices.extend([a, a + 1, a + 2]);
        indices.extend([a + 1, a + 3, a + 2]);
    }

    let mut buffer: Vec<u8> = Vec::new();
    let pos_offset = buffer.len();
    for v in &positions {
        buffer.extend(v.to_le_bytes());
    }
    let fid_offset = buffer.len();
    for v in feature_ids.unwrap_or(&[]) {
        buffer.extend(v.to_le_bytes());
    }
    let tan_offset = buffer.len();
    for t in tangents.unwrap_or(&[]) {
        for v in t {
            buffer.extend(v.to_le_bytes());
        }
    }
    let idx_offset = buffer.len();
    for v in &indices {
        buffer.extend(v.to_le_bytes());
    }
    while buffer.len() % 4 != 0 {
        buffer.push(0);
    }

    let (min, max) =
        positions
            .chunks(3)
            .fold(([f32::MAX; 3], [f32::MIN; 3]), |(mut lo, mut hi), p| {
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
                (lo, hi)
            });

    let mut attributes = serde_json::Map::new();
    let mut accessors = vec![json!({
        "bufferView": 0, "componentType": 5126, "count": n, "type": "VEC3",
        "min": min.to_vec(), "max": max.to_vec()
    })];
    let mut buffer_views = vec![json!({
        "buffer": 0, "byteOffset": pos_offset, "byteLength": positions.len() * 4
    })];
    attributes.insert("POSITION".into(), json!(0));
    if feature_ids.is_some() {
        attributes.insert("_FEATURE_ID_0".into(), json!(accessors.len()));
        accessors.push(json!({
            "bufferView": buffer_views.len(), "componentType": 5126, "count": n, "type": "SCALAR"
        }));
        buffer_views.push(json!({
            "buffer": 0, "byteOffset": fid_offset, "byteLength": n * 4
        }));
    }
    if tangents.is_some() {
        attributes.insert("TANGENT".into(), json!(accessors.len()));
        accessors.push(json!({
            "bufferView": buffer_views.len(), "componentType": 5126, "count": n, "type": "VEC4"
        }));
        buffer_views.push(json!({
            "buffer": 0, "byteOffset": tan_offset, "byteLength": n * 16
        }));
    }
    let indices_accessor = accessors.len();
    accessors.push(json!({
        "bufferView": buffer_views.len(), "componentType": 5123, "count": indices.len(),
        "type": "SCALAR"
    }));
    buffer_views.push(json!({
        "buffer": 0, "byteOffset": idx_offset, "byteLength": indices.len() * 2
    }));

    let gltf = json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0 }],
        "meshes": [{
            "primitives": [{
                "attributes": attributes,
                "indices": indices_accessor,
                "mode": 4
            }]
        }],
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{ "byteLength": buffer.len() }]
    });

    let mut out = Vec::new();
    glb::write_glb(&mut out, gltf.to_string().as_bytes(), &buffer).unwrap();
    out
}

/// Locate Google Draco's `draco_decoder`, or `None` if it isn't available.
fn find_draco_decoder() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("DRACO_DECODER") {
        let p = std::path::PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    let default = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../third_party/draco/_build/draco_decoder");
    default.is_file().then_some(default)
}

/// Transcodes and returns the output JSON together with the decoded feature ID
/// values of the (single) primitive's Draco payload.
fn transcode_and_decode(feature_ids: &[f32], label: &str) -> (Value, Vec<i32>) {
    let input = build_glb(feature_ids.len(), Some(feature_ids), None);
    let (json, mesh) = transcode_and_decode_payload(&input, label);
    let mut ids = Vec::new();
    for att in mesh.get_attributes() {
        if att.get_attribute_type() == AttributeType::Custom && att.get_num_components() == 1 {
            ids = (0..att.num_unique_values())
                .map(|i| match att.get_component_type() {
                    ComponentDataType::U16 => {
                        *att.get_unique_val::<NdVector<1, u16>, 1>(i.into()).get(0) as i32
                    }
                    ComponentDataType::U32 => {
                        *att.get_unique_val::<NdVector<1, u32>, 1>(i.into()).get(0) as i32
                    }
                    other => panic!("unexpected feature ID component type {other:?}"),
                })
                .collect();
        }
    }
    assert!(!ids.is_empty(), "feature ID attribute missing from payload");
    (json, ids)
}

/// Transcodes a GLB and returns the output JSON together with the oxide
/// decode of the (single) primitive's Draco payload, after checking the
/// reference decoder accepts that payload.
fn transcode_and_decode_payload(input: &[u8], label: &str) -> (Value, draco_oxide::Mesh) {
    let (output, warnings) = GltfTranscoder::default()
        .transcode_to_glb(input)
        .expect("transcode succeeds");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    let out = glb::parse_glb(&output).expect("output GLB parses");
    let json: Value = serde_json::from_slice(&out.json).unwrap();

    let ext = &json["meshes"][0]["primitives"][0]["extensions"]["KHR_draco_mesh_compression"];
    let view_idx = ext["bufferView"].as_u64().expect("draco extension present") as usize;
    let view = &json["bufferViews"][view_idx];
    let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let length = view["byteLength"].as_u64().unwrap() as usize;
    let drc = &out.buffer[offset..offset + length];

    // The reference decoder accepts the payload.
    if let Some(decoder) = find_draco_decoder() {
        let out_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/gltf_attributes");
        std::fs::create_dir_all(&out_dir).unwrap();
        let drc_path = out_dir.join(format!("{label}.drc"));
        std::fs::write(&drc_path, drc).unwrap();
        let output = std::process::Command::new(&decoder)
            .arg("-i")
            .arg(&drc_path)
            .arg("-o")
            .arg(out_dir.join(format!("{label}.ply")))
            .output()
            .expect("draco_decoder runs");
        assert!(
            output.status.success(),
            "draco_decoder rejected the transcoded payload: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let mesh = draco_oxide::decode::decode(drc).expect("draco payload decodes");
    (json, mesh)
}

#[test]
fn tangents_transcode_to_stock_decodable_streams() {
    // A stock decoder rejects att_type >= 5, so tangents must ride the
    // generic wire type; values round-trip within quantization error.
    let n = 16;
    let tangents: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32;
            let (s, c) = (std::f32::consts::PI * a).sin_cos();
            [c, s, 0.0, if i % 2 == 0 { 1.0 } else { -1.0 }]
        })
        .collect();
    let input = build_glb(n, None, Some(&tangents));
    let (_json, mesh) = transcode_and_decode_payload(&input, "tangents");

    let mut checked = false;
    for att in mesh.get_attributes() {
        if att.get_num_components() == 4 {
            assert_eq!(att.get_attribute_type(), AttributeType::Custom);
            let vals: Vec<[f32; 4]> = (0..att.num_unique_values())
                .map(|i| {
                    let v: NdVector<4, f32> = att.get_unique_val(i.into());
                    [*v.get(0), *v.get(1), *v.get(2), *v.get(3)]
                })
                .collect();
            for t in &tangents {
                let hit = vals
                    .iter()
                    .any(|v| v.iter().zip(t).all(|(a, b)| (a - b).abs() < 2e-3));
                assert!(hit, "tangent {t:?} missing from decoded values");
            }
            checked = true;
        }
    }
    assert!(checked, "tangent attribute missing from payload");
}

#[test]
fn small_feature_ids_ride_unsigned_short() {
    let feature_ids: Vec<f32> = (0..16).map(|i| (i * 100) as f32).collect();
    let (json, ids) = transcode_and_decode(&feature_ids, "small_ids");
    assert_eq!(json["accessors"][1]["componentType"].as_u64(), Some(5123));
    for &expect in &feature_ids {
        assert!(ids.contains(&(expect as i32)), "id {expect} missing");
    }
}

#[test]
fn wide_feature_ids_do_not_clamp() {
    // Ids beyond u16::MAX previously saturated to 65535.
    let feature_ids: Vec<f32> = (0..16).map(|i| 70000.0 + (i * 1000) as f32).collect();
    let (json, ids) = transcode_and_decode(&feature_ids, "wide_ids");
    assert_eq!(json["accessors"][1]["componentType"].as_u64(), Some(5126));
    for &expect in &feature_ids {
        assert!(ids.contains(&(expect as i32)), "id {expect} missing");
    }
}
