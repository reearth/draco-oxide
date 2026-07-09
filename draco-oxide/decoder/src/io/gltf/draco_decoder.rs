//! glTF / GLB Draco decoder.
//!
//! Reads a `.glb` buffer (or pre-extracted JSON + binary buffer), finds
//! every primitive with the `KHR_draco_mesh_compression` extension,
//! extracts the Draco-compressed bufferView bytes, and decodes them via
//! [`crate::decode::decode`]. Returns one [`draco_oxide_core::mesh::Mesh`]
//! per Draco-compressed primitive, in the order they appear in the glTF
//! `meshes[*].primitives[*]` traversal.

use serde_json::{json, Value};

use draco_oxide_core::attribute::ComponentDataType;
use draco_oxide_core::mesh::Mesh;
use draco_oxide_core::types::ConfigType;
use crate::decode;
use crate::io::gltf::{draco_extension, glb};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GLB parse error: {0}")]
    Glb(#[from] glb::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Decode error: {0}")]
    Decode(#[from] decode::Err),
    #[error("Missing field {0} on JSON path {1}")]
    MissingField(&'static str, String),
    #[error("BufferView {0} extends past the end of the binary buffer")]
    BufferViewOutOfRange(usize),
    #[error("BufferView {0} references buffer {1} but only buffer 0 (the GLB chunk) is supported")]
    NonZeroBuffer(usize, usize),
}

/// One decoded primitive with its provenance for routing back into the
/// glTF graph.
#[derive(Debug)]
pub struct DecodedPrimitive {
    /// Index of the source mesh in `glTF.meshes`.
    pub mesh_idx: usize,
    /// Index of the primitive within `glTF.meshes[mesh_idx].primitives`.
    pub primitive_idx: usize,
    /// The decoded geometry. Position is always present; normals + UVs
    /// are best-effort (see decoder graceful-fallback notes).
    pub mesh: Mesh,
}

/// Decode every Draco-compressed primitive in the GLB.
pub fn decode_glb(input: &[u8]) -> Result<Vec<DecodedPrimitive>, Error> {
    let glb_data = glb::parse_glb(input)?;
    let json: Value = serde_json::from_slice(&glb_data.json)?;
    decode_with_buffer(&json, &glb_data.buffer)
}

/// Decode every Draco-compressed primitive given pre-parsed glTF JSON
/// + the binary buffer (bufferView byte source). Useful when the caller
/// already has the GLB unpacked or when reading from non-GLB sources.
pub fn decode_with_buffer(
    json: &Value,
    binary_buffer: &[u8],
) -> Result<Vec<DecodedPrimitive>, Error> {
    let mut out = Vec::new();
    let meshes = match json.get("meshes").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return Ok(out),
    };

    for (mesh_idx, mesh) in meshes.iter().enumerate() {
        let prims = match mesh.get("primitives").and_then(|p| p.as_array()) {
            Some(p) => p,
            None => continue,
        };
        for (primitive_idx, primitive) in prims.iter().enumerate() {
            if !draco_extension::is_draco_compressed(primitive) {
                continue;
            }
            let buffer_view_idx = primitive
                .get("extensions")
                .and_then(|e| e.get(draco_extension::EXTENSION_NAME))
                .and_then(|d| d.get("bufferView"))
                .and_then(|v| v.as_u64())
                .ok_or(Error::MissingField(
                    "bufferView",
                    format!(
                        "meshes[{}].primitives[{}].extensions.{}",
                        mesh_idx,
                        primitive_idx,
                        draco_extension::EXTENSION_NAME
                    ),
                ))? as usize;

            let bytes = extract_buffer_view(json, binary_buffer, buffer_view_idx)?;
            let mut reader = bytes.into_iter();
            let mesh = decode::decode(&mut reader, decode::Config::default())?;
            out.push(DecodedPrimitive {
                mesh_idx,
                primitive_idx,
                mesh,
            });
        }
    }

    Ok(out)
}

/// Take a Draco-bearing GLB and return a Draco-free GLB ready for any
/// vanilla glTF loader (bevy_gltf, three.js, gltf-rs, ...). Decompress
/// every primitive, splice the decoded buffers back into the BIN as
/// plain bufferViews, patch the accessors that the primitives already
/// point at (count + componentType + bufferView + byteOffset), drop
/// the extension reference. Pass-through for GLBs that don't use the
/// extension (returns the input bytes verbatim).
pub fn splice_glb_remove_draco(input: &[u8]) -> Result<Vec<u8>, Error> {
    let glb_data = glb::parse_glb(input)?;
    let mut json: Value = serde_json::from_slice(&glb_data.json)?;

    if !json_has_draco_primitive(&json) {
        return Ok(input.to_vec());
    }

    let bin_bytes = &glb_data.buffer;

    let mut new_bin: Vec<u8> = bin_bytes.to_vec();
    align_to_4(&mut new_bin);

    let mut buffer_views = json
        .get("bufferViews")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut accessors = json
        .get("accessors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let target_buffer_index: usize = 0;

    let meshes_owned = json
        .get("meshes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut meshes_new: Vec<Value> = Vec::with_capacity(meshes_owned.len());

    for mut mesh in meshes_owned.into_iter() {
        let prims_owned = mesh
            .get("primitives")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut prims_new: Vec<Value> = Vec::with_capacity(prims_owned.len());

        for mut prim in prims_owned.into_iter() {
            let draco_ext = prim
                .get("extensions")
                .and_then(|e| e.get(draco_extension::EXTENSION_NAME))
                .cloned();
            let Some(ext) = draco_ext else {
                prims_new.push(prim);
                continue;
            };

            let old_index_accessor = prim
                .get("indices")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let old_attr_accessors: std::collections::BTreeMap<String, usize> = prim
                .get("attributes")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_u64().map(|u| (k.clone(), u as usize)))
                        .collect()
                })
                .unwrap_or_default();

            let ext_bv_index =
                ext.get("bufferView")
                    .and_then(|v| v.as_u64())
                    .ok_or(Error::MissingField(
                        "bufferView",
                        format!("primitive.extensions.{}", draco_extension::EXTENSION_NAME),
                    ))? as usize;
            let (encoded_offset, encoded_len) =
                read_buffer_view_range(&buffer_views, ext_bv_index)?;
            if encoded_offset + encoded_len > bin_bytes.len() {
                return Err(Error::BufferViewOutOfRange(ext_bv_index));
            }
            let encoded = &bin_bytes[encoded_offset..encoded_offset + encoded_len];

            let mut reader: &[u8] = encoded;
            let raw = decode::decode_to_raw(&mut reader, decode::Config::default())?;

            align_to_4(&mut new_bin);
            let block_start = new_bin.len();
            new_bin.extend_from_slice(&raw.data);

            // Indices.
            let index_bv_index = buffer_views.len();
            buffer_views.push(json!({
                "buffer": target_buffer_index,
                "byteOffset": block_start + raw.indices_offset,
                "byteLength": raw.indices_byte_length,
                "target": 34963u32, // ELEMENT_ARRAY_BUFFER
            }));
            let index_component_type = raw
                .indices_component_type
                .to_gltf_component_type()
                .unwrap_or(5125);
            if let Some(idx) = old_index_accessor {
                if let Some(acc) = accessors.get_mut(idx).and_then(|v| v.as_object_mut()) {
                    acc.insert("bufferView".into(), json!(index_bv_index));
                    acc.insert("byteOffset".into(), json!(0));
                    acc.insert("componentType".into(), json!(index_component_type));
                    acc.insert("count".into(), json!(raw.index_count));
                    acc.insert("type".into(), json!("SCALAR"));
                }
            }

            // Map: gltf semantic name → draco unique_id (from extension).
            // Inverted to look up the gltf accessor for each decoded attribute.
            let attr_map = ext
                .get("attributes")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let mut by_unique_id: std::collections::HashMap<u32, String> =
                std::collections::HashMap::new();
            for (gltf_name, draco_id_v) in attr_map.into_iter() {
                if let Some(uid) = draco_id_v.as_u64() {
                    by_unique_id.insert(uid as u32, gltf_name);
                }
            }

            for attr in &raw.attributes {
                let semantic_owned: Option<String> = attr.gltf_semantic.map(|s| s.to_string());
                let gltf_name = match by_unique_id
                    .get(&attr.unique_id)
                    .cloned()
                    .or(semantic_owned)
                {
                    Some(n) => n,
                    None => continue,
                };
                let Some(&existing_acc_idx) = old_attr_accessors.get(&gltf_name) else {
                    continue;
                };
                let bv_index = buffer_views.len();
                buffer_views.push(json!({
                    "buffer": target_buffer_index,
                    "byteOffset": block_start + attr.offset,
                    "byteLength": attr.byte_length,
                    "target": 34962u32, // ARRAY_BUFFER
                }));
                let component_type = attr.component_type.to_gltf_component_type().unwrap_or(5126);
                let accessor_type = accessor_type_from_dim(attr.dim);
                if let Some(acc) = accessors
                    .get_mut(existing_acc_idx)
                    .and_then(|v| v.as_object_mut())
                {
                    acc.insert("bufferView".into(), json!(bv_index));
                    acc.insert("byteOffset".into(), json!(0));
                    acc.insert("componentType".into(), json!(component_type));
                    acc.insert("count".into(), json!(raw.vertex_count));
                    acc.insert("type".into(), json!(accessor_type));
                    if gltf_name == "POSITION" && attr.component_type == ComponentDataType::F32 {
                        let pos_bytes = &raw.data[attr.offset..attr.offset + attr.byte_length];
                        if let Some((mins, maxs)) =
                            compute_position_min_max(pos_bytes, attr.dim as usize)
                        {
                            acc.insert("min".into(), json!(mins));
                            acc.insert("max".into(), json!(maxs));
                        }
                    }
                }
            }

            if let Some(exts) = prim.get_mut("extensions").and_then(|v| v.as_object_mut()) {
                exts.remove(draco_extension::EXTENSION_NAME);
                if exts.is_empty() {
                    if let Some(prim_obj) = prim.as_object_mut() {
                        prim_obj.remove("extensions");
                    }
                }
            }

            prims_new.push(prim);
        }

        if let Some(obj) = mesh.as_object_mut() {
            obj.insert("primitives".into(), Value::Array(prims_new));
        }
        meshes_new.push(mesh);
    }

    json["meshes"] = Value::Array(meshes_new);
    json["bufferViews"] = Value::Array(buffer_views);
    json["accessors"] = Value::Array(accessors);

    if let Some(buffers) = json.get_mut("buffers").and_then(|v| v.as_array_mut()) {
        if let Some(buf0) = buffers.get_mut(target_buffer_index) {
            buf0["byteLength"] = json!(new_bin.len());
        }
    } else {
        json["buffers"] = json!([{ "byteLength": new_bin.len() }]);
    }

    strip_ext_from_array(&mut json, "extensionsRequired");
    strip_ext_from_array(&mut json, "extensionsUsed");

    let new_json_bytes = serde_json::to_vec(&json)?;
    let mut out = Vec::with_capacity(20 + new_json_bytes.len() + new_bin.len());
    glb::write_glb(&mut out, &new_json_bytes, &new_bin)?;
    Ok(out)
}

fn json_has_draco_primitive(json: &Value) -> bool {
    let Some(meshes) = json.get("meshes").and_then(|v| v.as_array()) else {
        return false;
    };
    meshes.iter().any(|mesh| {
        mesh.get("primitives")
            .and_then(|v| v.as_array())
            .map(|prims| prims.iter().any(draco_extension::is_draco_compressed))
            .unwrap_or(false)
    })
}

fn read_buffer_view_range(buffer_views: &[Value], idx: usize) -> Result<(usize, usize), Error> {
    let bv = buffer_views
        .get(idx)
        .ok_or(Error::BufferViewOutOfRange(idx))?;
    let byte_offset = bv.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let byte_length = bv.get("byteLength").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    Ok((byte_offset, byte_length))
}

fn strip_ext_from_array(json: &mut Value, key: &str) {
    let Some(arr) = json.get_mut(key).and_then(|v| v.as_array_mut()) else {
        return;
    };
    arr.retain(|v| v.as_str() != Some(draco_extension::EXTENSION_NAME));
    if arr.is_empty() {
        if let Some(obj) = json.as_object_mut() {
            obj.remove(key);
        }
    }
}

fn align_to_4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

fn accessor_type_from_dim(dim: u8) -> &'static str {
    match dim {
        1 => "SCALAR",
        2 => "VEC2",
        3 => "VEC3",
        4 => "VEC4",
        _ => "SCALAR",
    }
}

/// (mins, maxs) per component over an f32 attribute buffer. Required by
/// glTF 2.0 spec for POSITION accessors.
fn compute_position_min_max(bytes: &[u8], dim: usize) -> Option<(Vec<f32>, Vec<f32>)> {
    if dim == 0 {
        return None;
    }
    let stride = dim * 4;
    if bytes.is_empty() || bytes.len() % stride != 0 {
        return None;
    }
    let mut mins = vec![f32::INFINITY; dim];
    let mut maxs = vec![f32::NEG_INFINITY; dim];
    for vertex in bytes.chunks_exact(stride) {
        for (i, comp) in vertex.chunks_exact(4).enumerate() {
            let v = f32::from_le_bytes([comp[0], comp[1], comp[2], comp[3]]);
            if v < mins[i] {
                mins[i] = v;
            }
            if v > maxs[i] {
                maxs[i] = v;
            }
        }
    }
    if mins.iter().any(|v| !v.is_finite()) || maxs.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some((mins, maxs))
}

/// Slice the bytes for a specific bufferView out of the binary buffer.
fn extract_buffer_view(
    json: &Value,
    binary_buffer: &[u8],
    view_idx: usize,
) -> Result<Vec<u8>, Error> {
    let view = json
        .get("bufferViews")
        .and_then(|v| v.as_array())
        .and_then(|v| v.get(view_idx))
        .ok_or(Error::MissingField(
            "bufferViews[idx]",
            format!("bufferViews[{}]", view_idx),
        ))?;
    let buffer = view.get("buffer").and_then(|b| b.as_u64()).unwrap_or(0) as usize;
    if buffer != 0 {
        return Err(Error::NonZeroBuffer(view_idx, buffer));
    }
    let offset = view.get("byteOffset").and_then(|o| o.as_u64()).unwrap_or(0) as usize;
    let length = view
        .get("byteLength")
        .and_then(|l| l.as_u64())
        .ok_or(Error::MissingField(
            "byteLength",
            format!("bufferViews[{}]", view_idx),
        ))? as usize;
    let end = offset
        .checked_add(length)
        .ok_or(Error::BufferViewOutOfRange(view_idx))?;
    if end > binary_buffer.len() {
        return Err(Error::BufferViewOutOfRange(view_idx));
    }
    Ok(binary_buffer[offset..end].to_vec())
}


// NOTE: the encode->splice->decode integration tests that lived here used the
// encoder's `GltfTranscoder` to synthesize test GLBs. They now belong in the
// workspace `tests/` crate (which can depend on both the encoder and this
// decoder). Tracked as a follow-up.
