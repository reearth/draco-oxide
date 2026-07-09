//! KHR_draco_mesh_compression extension handling.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub const EXTENSION_NAME: &str = "KHR_draco_mesh_compression";

#[derive(Debug, Clone, Default)]
pub struct DracoAttributeIds {
    pub ids: HashMap<String, u32>,
}

impl DracoAttributeIds {
    pub fn new() -> Self {
        Self {
            ids: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: &str, id: u32) {
        self.ids.insert(name.to_string(), id);
    }
}

pub fn is_draco_compressed(primitive: &Value) -> bool {
    primitive
        .get("extensions")
        .and_then(|e| e.get(EXTENSION_NAME))
        .is_some()
}

pub fn is_triangle_primitive(primitive: &Value) -> bool {
    primitive.get("mode").and_then(|m| m.as_u64()).unwrap_or(4) == 4
}

pub fn primitive_mode_name(mode: u64) -> &'static str {
    match mode {
        0 => "POINTS",
        1 => "LINES",
        2 => "LINE_LOOP",
        3 => "LINE_STRIP",
        4 => "TRIANGLES",
        5 => "TRIANGLE_STRIP",
        6 => "TRIANGLE_FAN",
        _ => "UNKNOWN",
    }
}

pub fn add_draco_extension(
    json: &mut Value,
    mesh_idx: usize,
    primitive_idx: usize,
    draco_buffer_view_idx: usize,
    attribute_ids: &DracoAttributeIds,
    indices_accessor_idx: Option<u64>,
) {
    let mut extension = Map::new();
    extension.insert("bufferView".to_string(), json!(draco_buffer_view_idx));

    let mut attributes = Map::new();
    for (name, id) in &attribute_ids.ids {
        attributes.insert(name.clone(), json!(id));
    }
    extension.insert("attributes".to_string(), Value::Object(attributes));

    let primitive = &mut json["meshes"][mesh_idx]["primitives"][primitive_idx];

    if primitive.get("extensions").is_none() {
        primitive["extensions"] = json!({});
    }
    primitive["extensions"][EXTENSION_NAME] = Value::Object(extension);

    if let Some(attrs) = primitive.get("attributes").cloned() {
        if let Some(attrs_obj) = attrs.as_object() {
            for (_, accessor_idx) in attrs_obj {
                if let Some(idx) = accessor_idx.as_u64() {
                    clear_accessor_buffer_refs(json, idx as usize);
                }
            }
        }
    }

    if let Some(idx) = indices_accessor_idx {
        clear_accessor_buffer_refs(json, idx as usize);
    }
}

fn clear_accessor_buffer_refs(json: &mut Value, accessor_idx: usize) {
    if let Some(accessor) = json
        .get_mut("accessors")
        .and_then(|a| a.get_mut(accessor_idx))
        .and_then(|a| a.as_object_mut())
    {
        accessor.remove("bufferView");
        accessor.remove("byteOffset");
    }
}

pub fn ensure_extension_declared(json: &mut Value) {
    if json.get("extensionsUsed").is_none() {
        json["extensionsUsed"] = json!([]);
    }
    if let Some(arr) = json["extensionsUsed"].as_array_mut() {
        if !arr.iter().any(|v| v.as_str() == Some(EXTENSION_NAME)) {
            arr.push(json!(EXTENSION_NAME));
        }
    }

    if json.get("extensionsRequired").is_none() {
        json["extensionsRequired"] = json!([]);
    }
    if let Some(arr) = json["extensionsRequired"].as_array_mut() {
        if !arr.iter().any(|v| v.as_str() == Some(EXTENSION_NAME)) {
            arr.push(json!(EXTENSION_NAME));
        }
    }
}

pub fn add_buffer_view(
    json: &mut Value,
    buffer_idx: usize,
    byte_offset: usize,
    byte_length: usize,
) -> usize {
    if json.get("bufferViews").is_none() {
        json["bufferViews"] = json!([]);
    }

    let buffer_view =
        json!({ "buffer": buffer_idx, "byteOffset": byte_offset, "byteLength": byte_length });
    let arr = json["bufferViews"].as_array_mut().unwrap();
    let idx = arr.len();
    arr.push(buffer_view);
    idx
}

pub fn update_buffer_length(json: &mut Value, buffer_idx: usize, byte_length: usize) {
    if let Some(buffer) = json
        .get_mut("buffers")
        .and_then(|b| b.get_mut(buffer_idx))
        .and_then(|b| b.as_object_mut())
    {
        buffer.insert("byteLength".to_string(), json!(byte_length));
    }
}

pub fn set_buffer_uri(json: &mut Value, buffer_idx: usize, uri: Option<&str>) {
    if let Some(buffer) = json
        .get_mut("buffers")
        .and_then(|b| b.get_mut(buffer_idx))
        .and_then(|b| b.as_object_mut())
    {
        match uri {
            Some(u) => {
                buffer.insert("uri".to_string(), json!(u));
            }
            None => {
                buffer.remove("uri");
            }
        }
    }
}

pub fn update_buffer_view_offset(json: &mut Value, buffer_view_idx: usize, new_offset: usize) {
    if let Some(bv) = json
        .get_mut("bufferViews")
        .and_then(|b| b.get_mut(buffer_view_idx))
        .and_then(|b| b.as_object_mut())
    {
        bv.insert("byteOffset".to_string(), json!(new_offset));
    }
}

/// Clear bufferView and byteOffset for all accessors that reference any of the given bufferViews.
/// This handles orphan accessors that aren't used by any primitive but still reference geometry bufferViews.
pub fn clear_accessors_referencing_views(
    json: &mut Value,
    views_to_clear: &std::collections::HashSet<usize>,
) {
    if let Some(accessors) = json.get_mut("accessors").and_then(|a| a.as_array_mut()) {
        for accessor in accessors.iter_mut() {
            if let Some(bv_idx) = accessor.get("bufferView").and_then(|v| v.as_u64()) {
                if views_to_clear.contains(&(bv_idx as usize)) {
                    if let Some(obj) = accessor.as_object_mut() {
                        obj.remove("bufferView");
                        obj.remove("byteOffset");
                    }
                }
            }
        }
    }
}

/// Remove geometry bufferViews and remap all bufferView references.
/// Returns a mapping from old indices to new indices.
pub fn remove_buffer_views(
    json: &mut Value,
    views_to_remove: &std::collections::HashSet<usize>,
) -> std::collections::HashMap<usize, usize> {
    let mut old_to_new: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();

    if let Some(buffer_views) = json.get_mut("bufferViews").and_then(|b| b.as_array_mut()) {
        // Build the new array and mapping
        let mut new_views = Vec::new();
        for (old_idx, bv) in buffer_views.iter().enumerate() {
            if !views_to_remove.contains(&old_idx) {
                old_to_new.insert(old_idx, new_views.len());
                new_views.push(bv.clone());
            }
        }
        *buffer_views = new_views;
    }

    // Remap bufferView references in images
    if let Some(images) = json.get_mut("images").and_then(|i| i.as_array_mut()) {
        for image in images.iter_mut() {
            if let Some(bv_idx) = image.get("bufferView").and_then(|v| v.as_u64()) {
                if let Some(&new_idx) = old_to_new.get(&(bv_idx as usize)) {
                    image
                        .as_object_mut()
                        .unwrap()
                        .insert("bufferView".to_string(), json!(new_idx));
                }
            }
        }
    }

    // Remap bufferView references in EXT_structural_metadata property tables
    if let Some(ext) = json
        .get_mut("extensions")
        .and_then(|e| e.get_mut("EXT_structural_metadata"))
    {
        if let Some(tables) = ext.get_mut("propertyTables").and_then(|t| t.as_array_mut()) {
            for table in tables.iter_mut() {
                if let Some(properties) =
                    table.get_mut("properties").and_then(|p| p.as_object_mut())
                {
                    for (_, prop) in properties.iter_mut() {
                        // Remap "values" bufferView reference
                        if let Some(bv_idx) = prop.get("values").and_then(|v| v.as_u64()) {
                            if let Some(&new_idx) = old_to_new.get(&(bv_idx as usize)) {
                                prop.as_object_mut()
                                    .unwrap()
                                    .insert("values".to_string(), json!(new_idx));
                            }
                        }
                        // Remap "stringOffsets" bufferView reference
                        if let Some(bv_idx) = prop.get("stringOffsets").and_then(|v| v.as_u64()) {
                            if let Some(&new_idx) = old_to_new.get(&(bv_idx as usize)) {
                                prop.as_object_mut()
                                    .unwrap()
                                    .insert("stringOffsets".to_string(), json!(new_idx));
                            }
                        }
                        // Remap "arrayOffsets" bufferView reference (for variable-length arrays)
                        if let Some(bv_idx) = prop.get("arrayOffsets").and_then(|v| v.as_u64()) {
                            if let Some(&new_idx) = old_to_new.get(&(bv_idx as usize)) {
                                prop.as_object_mut()
                                    .unwrap()
                                    .insert("arrayOffsets".to_string(), json!(new_idx));
                            }
                        }
                    }
                }
            }
        }
    }

    old_to_new
}

/// glTF componentType values
pub const COMPONENT_TYPE_UNSIGNED_SHORT: u64 = 5123;

/// Update accessor componentType to UNSIGNED_SHORT (5123).
/// This is needed for feature ID attributes that are encoded as u16 in Draco
/// but originally declared as FLOAT in glTF.
pub fn update_accessor_component_type(json: &mut Value, accessor_idx: u64, component_type: u64) {
    if let Some(accessor) = json
        .get_mut("accessors")
        .and_then(|a| a.get_mut(accessor_idx as usize))
        .and_then(|a| a.as_object_mut())
    {
        accessor.insert("componentType".to_string(), json!(component_type));
    }
}

/// Duplicate an accessor with a new count.
/// Returns the index of the new accessor.
pub fn duplicate_accessor(json: &mut Value, original_idx: usize, new_count: usize) -> usize {
    let accessors = json
        .get_mut("accessors")
        .and_then(|a| a.as_array_mut())
        .expect("accessors array should exist");

    // Clone the original accessor
    let mut new_accessor = accessors[original_idx].clone();

    // Update the count
    if let Some(obj) = new_accessor.as_object_mut() {
        obj.insert("count".to_string(), json!(new_count));
        // Also clear bufferView and byteOffset since this will be Draco-compressed
        obj.remove("bufferView");
        obj.remove("byteOffset");
    }

    // Add the new accessor and return its index
    let new_idx = accessors.len();
    accessors.push(new_accessor);
    new_idx
}

/// Update accessor count.
pub fn update_accessor_count(json: &mut Value, accessor_idx: usize, new_count: usize) {
    if let Some(accessor) = json
        .get_mut("accessors")
        .and_then(|a| a.get_mut(accessor_idx))
        .and_then(|a| a.as_object_mut())
    {
        accessor.insert("count".to_string(), json!(new_count));
    }
}

/// Update a primitive's attribute accessor index.
pub fn update_primitive_attribute(
    json: &mut Value,
    mesh_idx: usize,
    prim_idx: usize,
    attr_name: &str,
    new_accessor_idx: usize,
) {
    if let Some(primitive) = json
        .get_mut("meshes")
        .and_then(|m| m.get_mut(mesh_idx))
        .and_then(|m| m.get_mut("primitives"))
        .and_then(|p| p.get_mut(prim_idx))
    {
        if let Some(attrs) = primitive
            .get_mut("attributes")
            .and_then(|a| a.as_object_mut())
        {
            attrs.insert(attr_name.to_string(), json!(new_accessor_idx));
        }
    }
}

/// Update a primitive's indices accessor index.
pub fn update_primitive_indices(
    json: &mut Value,
    mesh_idx: usize,
    prim_idx: usize,
    new_accessor_idx: usize,
) {
    if let Some(primitive) = json
        .get_mut("meshes")
        .and_then(|m| m.get_mut(mesh_idx))
        .and_then(|m| m.get_mut("primitives"))
        .and_then(|p| p.get_mut(prim_idx))
        .and_then(|p| p.as_object_mut())
    {
        primitive.insert("indices".to_string(), json!(new_accessor_idx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_draco_compressed() {
        let compressed =
            json!({ "extensions": { "KHR_draco_mesh_compression": { "bufferView": 0 } } });
        let uncompressed = json!({ "attributes": { "POSITION": 0 } });
        assert!(is_draco_compressed(&compressed));
        assert!(!is_draco_compressed(&uncompressed));
    }

    #[test]
    fn test_is_triangle_primitive() {
        assert!(is_triangle_primitive(&json!({})));
        assert!(is_triangle_primitive(&json!({"mode": 4})));
        assert!(!is_triangle_primitive(&json!({"mode": 0})));
    }
}
