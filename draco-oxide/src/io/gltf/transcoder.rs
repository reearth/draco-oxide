//! Passthrough glTF transcoder with Draco compression.
//!
//! This transcoder compresses geometry while preserving all other glTF data
//! (materials, textures, animations, extensions) unchanged.

use crate::encode::Config as DracoConfig;
use draco_oxide_core::attribute::{AttributeDomain, AttributeType};
use draco_oxide_core::mesh::builder::MeshBuilder;
use draco_oxide_core::types::ConfigType;
use draco_oxide_core::types::NdVector;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::buffer_builder::BufferBuilder;
use super::draco_extension::{self, DracoAttributeIds};
use super::geometry_extractor::{
    self, read_accessor_as_scalar_f32, read_accessor_as_u32, read_accessor_as_vec2,
    read_accessor_as_vec3, read_accessor_as_vec4,
};
use super::glb;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GLB parse error: {0}")]
    GlbParse(#[from] glb::Error),
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),
    #[error("Geometry extraction error: {0}")]
    GeometryExtraction(#[from] geometry_extractor::Error),
    #[error("Mesh build error: {0}")]
    MeshBuild(#[from] draco_oxide_core::mesh::builder::Err),
    #[error("Draco encode error: {0}")]
    DracoEncode(#[from] crate::encode::Err),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported: {0}")]
    Unsupported(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

/// Configuration for the transcoder.
#[derive(Debug, Clone)]
pub struct TranscoderConfig {
    /// Draco compression configuration.
    pub draco: DracoConfig,
}

impl Default for TranscoderConfig {
    fn default() -> Self {
        Self {
            draco: DracoConfig::default(),
        }
    }
}

/// Output format for transcoded glTF.
#[derive(Debug, Clone)]
pub enum OutputFormat {
    /// GLB binary format (single file).
    Glb,
    /// glTF with separate .bin file.
    Gltf { bin_filename: String },
}

/// Result of transcoding.
#[derive(Debug)]
pub struct TranscodeResult {
    /// JSON content.
    pub json: Vec<u8>,
    /// Binary buffer content.
    pub buffer: Vec<u8>,
    /// Warnings generated during transcoding.
    pub warnings: Vec<String>,
}

/// Passthrough glTF transcoder.
///
/// Compresses geometry with Draco while preserving all other data unchanged.
pub struct GltfTranscoder {
    config: TranscoderConfig,
}

impl Default for GltfTranscoder {
    fn default() -> Self {
        Self::new(TranscoderConfig::default())
    }
}

impl GltfTranscoder {
    /// Create a new transcoder with the given configuration.
    pub fn new(config: TranscoderConfig) -> Self {
        Self { config }
    }

    /// Transcode GLB input to GLB output.
    pub fn transcode_to_glb(&self, input: &[u8]) -> Result<(Vec<u8>, Vec<String>), Error> {
        let result = self.transcode(input, &OutputFormat::Glb)?;

        let mut output = Vec::new();
        glb::write_glb(&mut output, &result.json, &result.buffer)?;

        Ok((output, result.warnings))
    }

    /// Transcode GLB input and write to a file.
    ///
    /// Output format is determined by file extension (.glb or .gltf).
    pub fn transcode_to_file(
        &self,
        input: &[u8],
        output_path: &Path,
    ) -> Result<Vec<String>, Error> {
        let extension = output_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let format = match extension.as_str() {
            "glb" => OutputFormat::Glb,
            "gltf" => {
                let bin_name = output_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| format!("{}.bin", s))
                    .unwrap_or_else(|| "buffer.bin".to_string());
                OutputFormat::Gltf {
                    bin_filename: bin_name,
                }
            }
            _ => {
                return Err(Error::InvalidInput(format!(
                    "Unknown output extension: {}",
                    extension
                )))
            }
        };

        let result = self.transcode(input, &format)?;

        match &format {
            OutputFormat::Glb => {
                let mut file = std::fs::File::create(output_path)?;
                glb::write_glb(&mut file, &result.json, &result.buffer)?;
            }
            OutputFormat::Gltf { bin_filename } => {
                // Write JSON
                std::fs::write(output_path, &result.json)?;

                // Write binary buffer
                if !result.buffer.is_empty() {
                    let bin_path = output_path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(bin_filename);
                    std::fs::write(bin_path, &result.buffer)?;
                }
            }
        }

        Ok(result.warnings)
    }

    /// Transcode GLB input to separate JSON and buffer.
    pub fn transcode(&self, input: &[u8], format: &OutputFormat) -> Result<TranscodeResult, Error> {
        // Step 1: Parse GLB
        let glb_data = glb::parse_glb(input)?;
        let mut json: Value = serde_json::from_slice(&glb_data.json)?;
        let original_buffer = &glb_data.buffer;

        let mut warnings = Vec::new();

        // Check for external buffer URIs (not supported)
        if let Some(buffers) = json.get("buffers").and_then(|b| b.as_array()) {
            for (i, buffer) in buffers.iter().enumerate() {
                if buffer.get("uri").is_some() && i == 0 {
                    // First buffer in GLB shouldn't have URI, but we check anyway
                }
                if i > 0 {
                    return Err(Error::Unsupported("Multiple buffers not supported".into()));
                }
            }
        }

        // Step 2: Identify geometry bufferViews vs non-geometry bufferViews
        let (geometry_views, _non_geometry_views) = categorize_buffer_views(&json);

        // Step 3: Process each mesh primitive
        let mut new_buffer = BufferBuilder::new();
        let mut compressed_data: Vec<CompressedPrimitive> = Vec::new();

        if let Some(meshes) = json.get("meshes").and_then(|m| m.as_array()).cloned() {
            for (mesh_idx, mesh) in meshes.iter().enumerate() {
                if let Some(primitives) = mesh.get("primitives").and_then(|p| p.as_array()) {
                    for (prim_idx, primitive) in primitives.iter().enumerate() {
                        match self.process_primitive(
                            &json,
                            original_buffer,
                            primitive,
                            &mut new_buffer,
                        ) {
                            Ok(Some(compressed)) => {
                                compressed_data.push(CompressedPrimitive {
                                    mesh_idx,
                                    prim_idx,
                                    buffer_view_offset: compressed.buffer_view_offset,
                                    buffer_view_length: compressed.buffer_view_length,
                                    attribute_ids: compressed.attribute_ids,
                                    indices_accessor_idx: compressed.indices_accessor_idx,
                                    feature_id_accessor_indices: compressed
                                        .feature_id_accessor_indices,
                                    original_accessor_indices: compressed.original_accessor_indices,
                                    vertex_count: compressed.vertex_count,
                                    indices_count: compressed.indices_count,
                                });
                            }
                            Ok(None) => {
                                // Primitive was skipped (already compressed, non-triangle, etc.)
                            }
                            Err(SkipReason::AlreadyCompressed) => {
                                warnings.push(format!(
                                    "Mesh {} primitive {}: already Draco-compressed, skipping",
                                    mesh_idx, prim_idx
                                ));
                            }
                            Err(SkipReason::NonTriangle(mode)) => {
                                warnings.push(format!(
                                    "Mesh {} primitive {}: non-triangle mode ({}), skipping",
                                    mesh_idx,
                                    prim_idx,
                                    draco_extension::primitive_mode_name(mode)
                                ));
                            }
                            Err(SkipReason::Error(e)) => {
                                return Err(e);
                            }
                        }
                    }
                }
            }
        }

        // Step 3.5: Handle shared accessors
        // When multiple primitives share the same accessor but have different Draco bufferViews,
        // we need to duplicate the accessor so each primitive has its own.
        let mut accessor_remappings: HashMap<(usize, usize), HashMap<u64, usize>> = HashMap::new();

        // Build accessor usage map: accessor_idx -> list of (mesh_idx, prim_idx, count)
        // For vertex attributes, count is vertex_count; for indices, count is indices_count
        let mut accessor_usage: HashMap<u64, Vec<(usize, usize, usize)>> = HashMap::new();
        for compressed in &compressed_data {
            for &accessor_idx in compressed.original_accessor_indices.values() {
                accessor_usage.entry(accessor_idx).or_default().push((
                    compressed.mesh_idx,
                    compressed.prim_idx,
                    compressed.vertex_count,
                ));
            }
            // Also track indices accessor if present
            if let Some(idx) = compressed.indices_accessor_idx {
                accessor_usage.entry(idx).or_default().push((
                    compressed.mesh_idx,
                    compressed.prim_idx,
                    compressed.indices_count,
                ));
            }
        }

        // For shared accessors, duplicate for all primitives except the first
        for (accessor_idx, users) in &accessor_usage {
            if users.len() > 1 {
                // This accessor is shared - duplicate for each primitive
                for (i, &(mesh_idx, prim_idx, vertex_count)) in users.iter().enumerate() {
                    let remapping = accessor_remappings.entry((mesh_idx, prim_idx)).or_default();

                    if i == 0 {
                        // First user keeps the original accessor, but we update its count
                        if vertex_count > 0 {
                            draco_extension::update_accessor_count(
                                &mut json,
                                *accessor_idx as usize,
                                vertex_count,
                            );
                        }
                        remapping.insert(*accessor_idx, *accessor_idx as usize);
                    } else {
                        // Other users get duplicated accessors
                        let new_idx = draco_extension::duplicate_accessor(
                            &mut json,
                            *accessor_idx as usize,
                            vertex_count,
                        );
                        remapping.insert(*accessor_idx, new_idx);
                    }
                }
            } else if let Some(&(mesh_idx, prim_idx, vertex_count)) = users.first() {
                // Single user - just update the count if needed
                if vertex_count > 0 {
                    draco_extension::update_accessor_count(
                        &mut json,
                        *accessor_idx as usize,
                        vertex_count,
                    );
                }
                let remapping = accessor_remappings.entry((mesh_idx, prim_idx)).or_default();
                remapping.insert(*accessor_idx, *accessor_idx as usize);
            }
        }

        // Update primitive attributes to use new accessor indices
        for compressed in &compressed_data {
            if let Some(remapping) =
                accessor_remappings.get(&(compressed.mesh_idx, compressed.prim_idx))
            {
                for (attr_name, &original_idx) in &compressed.original_accessor_indices {
                    if let Some(&new_idx) = remapping.get(&original_idx) {
                        if new_idx != original_idx as usize {
                            draco_extension::update_primitive_attribute(
                                &mut json,
                                compressed.mesh_idx,
                                compressed.prim_idx,
                                attr_name,
                                new_idx,
                            );
                        }
                    }
                }
                // Update indices accessor if remapped
                if let Some(original_indices_idx) = compressed.indices_accessor_idx {
                    if let Some(&new_idx) = remapping.get(&original_indices_idx) {
                        if new_idx != original_indices_idx as usize {
                            draco_extension::update_primitive_indices(
                                &mut json,
                                compressed.mesh_idx,
                                compressed.prim_idx,
                                new_idx,
                            );
                        }
                    }
                }
            }
        }

        // Step 4: Copy non-geometry bufferViews to new buffer
        let mut view_offset_map: HashMap<usize, usize> = HashMap::new();

        // Determine which bufferViews need 8-byte alignment (INT64/FLOAT64 metadata)
        let views_needing_8byte_align = get_8byte_aligned_buffer_views(&json);

        if let Some(buffer_views) = json.get("bufferViews").and_then(|b| b.as_array()) {
            for (old_idx, bv) in buffer_views.iter().enumerate() {
                if !geometry_views.contains(&old_idx) {
                    // Non-geometry bufferView - copy to new buffer
                    let byte_offset =
                        bv.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let byte_length =
                        bv.get("byteLength").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                    if byte_offset + byte_length <= original_buffer.len() {
                        let data = &original_buffer[byte_offset..byte_offset + byte_length];
                        // Use 8-byte alignment for INT64/FLOAT64 data, 4-byte otherwise
                        let alignment = if views_needing_8byte_align.contains(&old_idx) {
                            8
                        } else {
                            4
                        };
                        let (new_offset, _) = new_buffer.append(data, alignment);
                        view_offset_map.insert(old_idx, new_offset);
                    }
                }
            }
        }

        // Step 5: Patch JSON

        // Update non-geometry bufferView offsets (before removing views, since indices will change)
        for (old_idx, new_offset) in &view_offset_map {
            draco_extension::update_buffer_view_offset(&mut json, *old_idx, *new_offset);
        }

        // Clear bufferView/byteOffset for ALL accessors referencing geometry bufferViews
        // (including orphan accessors not used by any primitive)
        draco_extension::clear_accessors_referencing_views(&mut json, &geometry_views);

        // Remove geometry bufferViews and remap all references
        let _old_to_new = draco_extension::remove_buffer_views(&mut json, &geometry_views);

        // Add new bufferViews for Draco data and add extensions to primitives
        for compressed in &compressed_data {
            let new_bv_idx = draco_extension::add_buffer_view(
                &mut json,
                0, // buffer index
                compressed.buffer_view_offset,
                compressed.buffer_view_length,
            );

            // Get remapped indices accessor index if available
            let remapping = accessor_remappings.get(&(compressed.mesh_idx, compressed.prim_idx));
            let indices_accessor_idx = compressed.indices_accessor_idx.map(|orig| {
                remapping
                    .and_then(|r| r.get(&orig))
                    .copied()
                    .map(|idx| idx as u64)
                    .unwrap_or(orig)
            });

            draco_extension::add_draco_extension(
                &mut json,
                compressed.mesh_idx,
                compressed.prim_idx,
                new_bv_idx,
                &compressed.attribute_ids,
                indices_accessor_idx,
            );

            // Update feature ID accessor componentType from FLOAT to UNSIGNED_SHORT
            // since we encode feature IDs as u16 for Draco compatibility
            // Use remapped accessor indices if available
            let remapping = accessor_remappings.get(&(compressed.mesh_idx, compressed.prim_idx));
            for &original_accessor_idx in &compressed.feature_id_accessor_indices {
                let actual_idx = remapping
                    .and_then(|r| r.get(&original_accessor_idx))
                    .copied()
                    .unwrap_or(original_accessor_idx as usize);
                draco_extension::update_accessor_component_type(
                    &mut json,
                    actual_idx as u64,
                    draco_extension::COMPONENT_TYPE_UNSIGNED_SHORT,
                );
            }
        }

        // Update buffer length (pad to 8-byte alignment for INT64/FLOAT64 typed array compatibility)
        let mut final_buffer = new_buffer.finish();
        let padding = (8 - (final_buffer.len() % 8)) % 8;
        final_buffer.extend(std::iter::repeat_n(0u8, padding));
        draco_extension::update_buffer_length(&mut json, 0, final_buffer.len());

        // Ensure extension is declared
        if !compressed_data.is_empty() {
            draco_extension::ensure_extension_declared(&mut json);
        }

        // Set buffer URI based on format
        match format {
            OutputFormat::Glb => {
                draco_extension::set_buffer_uri(&mut json, 0, None);
            }
            OutputFormat::Gltf { bin_filename } => {
                draco_extension::set_buffer_uri(&mut json, 0, Some(bin_filename));
            }
        }

        // Serialize JSON
        let json_bytes = serde_json::to_vec(&json)?;

        Ok(TranscodeResult {
            json: json_bytes,
            buffer: final_buffer,
            warnings,
        })
    }

    /// Process a single primitive.
    fn process_primitive(
        &self,
        json: &Value,
        buffer: &[u8],
        primitive: &Value,
        output_buffer: &mut BufferBuilder,
    ) -> Result<Option<CompressedPrimitiveData>, SkipReason> {
        // Check if already Draco-compressed
        if draco_extension::is_draco_compressed(primitive) {
            return Err(SkipReason::AlreadyCompressed);
        }

        // Check if triangles
        let mode = primitive.get("mode").and_then(|m| m.as_u64()).unwrap_or(4);
        if mode != 4 {
            return Err(SkipReason::NonTriangle(mode));
        }

        // Extract geometry
        let mut geometry = self.extract_geometry(json, buffer, primitive)?;

        // Capture counts before building mesh
        let vertex_count = geometry.positions.len();
        let indices_count = geometry.indices.len();

        // Build Mesh (also assigns draco_attribute_ids in correct order)
        let mesh = self.build_mesh(&mut geometry)?;

        // Compress
        let mut compressed = Vec::new();
        crate::encode::encode(mesh, &mut compressed, self.config.draco.clone())
            .map_err(|e| SkipReason::Error(Error::DracoEncode(e)))?;

        // Append to buffer
        let (offset, length) = output_buffer.append(&compressed, 4);

        Ok(Some(CompressedPrimitiveData {
            buffer_view_offset: offset,
            buffer_view_length: length,
            attribute_ids: geometry.draco_attribute_ids,
            indices_accessor_idx: geometry.indices_accessor_idx,
            feature_id_accessor_indices: geometry
                .feature_id_accessor_indices
                .into_iter()
                .map(|(_, idx)| idx)
                .collect(),
            original_accessor_indices: geometry.original_accessor_indices,
            vertex_count,
            indices_count,
        }))
    }

    /// Extract geometry from a primitive.
    fn extract_geometry(
        &self,
        json: &Value,
        buffer: &[u8],
        primitive: &Value,
    ) -> Result<ExtractedGeometry, SkipReason> {
        let mut geometry = ExtractedGeometry::default();

        // Extract indices
        if let Some(idx) = primitive.get("indices").and_then(|i| i.as_u64()) {
            geometry.indices = read_accessor_as_u32(json, buffer, idx)
                .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?;
            geometry.indices_accessor_idx = Some(idx);
        }

        // Extract attributes (draco_attribute_ids will be assigned in build_mesh)
        if let Some(attrs) = primitive.get("attributes").and_then(|a| a.as_object()) {
            for (name, accessor_idx) in attrs {
                let idx = accessor_idx.as_u64().ok_or_else(|| {
                    SkipReason::Error(Error::InvalidInput(format!(
                        "Invalid accessor index for {}",
                        name
                    )))
                })?;

                // Track original accessor index for shared accessor detection
                geometry
                    .original_accessor_indices
                    .insert(name.to_string(), idx);

                match name.as_str() {
                    "POSITION" => {
                        geometry.positions = read_accessor_as_vec3(json, buffer, idx)
                            .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?;
                    }
                    "NORMAL" => {
                        geometry.normals = Some(
                            read_accessor_as_vec3(json, buffer, idx)
                                .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?,
                        );
                    }
                    name if name.starts_with("TEXCOORD_") => {
                        let texcoords = read_accessor_as_vec2(json, buffer, idx)
                            .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?;
                        geometry.texcoords.push((name.to_string(), texcoords));
                    }
                    name if name.starts_with("COLOR_") => {
                        // Try VEC4 first, fall back to VEC3
                        let colors = read_accessor_as_vec4(json, buffer, idx)
                            .or_else(|_| {
                                read_accessor_as_vec3(json, buffer, idx).map(|v| {
                                    v.into_iter().map(|c| [c[0], c[1], c[2], 1.0]).collect()
                                })
                            })
                            .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?;
                        geometry.colors.push((name.to_string(), colors));
                    }
                    "TANGENT" => {
                        geometry.tangents = Some(
                            read_accessor_as_vec4(json, buffer, idx)
                                .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?,
                        );
                    }
                    name if name.starts_with("_FEATURE_ID_") => {
                        let feature_ids = read_accessor_as_scalar_f32(json, buffer, idx)
                            .map_err(|e| SkipReason::Error(Error::GeometryExtraction(e)))?;
                        geometry.feature_ids.push((name.to_string(), feature_ids));
                        // Track accessor index so we can update componentType to U32 after encoding
                        geometry
                            .feature_id_accessor_indices
                            .push((name.to_string(), idx));
                    }
                    _ => {
                        // Skip unknown attributes for now
                        // Could add support for custom attributes here
                    }
                }
            }
        }

        if geometry.positions.is_empty() {
            return Err(SkipReason::Error(Error::InvalidInput(
                "Primitive has no POSITION attribute".into(),
            )));
        }

        Ok(geometry)
    }

    /// Build a Draco Mesh from extracted geometry.
    /// Also populates the draco_attribute_ids based on the actual order attributes are added.
    fn build_mesh(
        &self,
        geometry: &mut ExtractedGeometry,
    ) -> Result<draco_oxide_core::mesh::Mesh, SkipReason> {
        let mut builder = MeshBuilder::new();

        // Set faces from indices
        let faces: Vec<[usize; 3]> = if geometry.indices.is_empty() {
            // No indices - generate sequential faces
            (0..geometry.positions.len() / 3)
                .map(|i| [i * 3, i * 3 + 1, i * 3 + 2])
                .collect()
        } else {
            geometry
                .indices
                .chunks(3)
                .map(|c| [c[0] as usize, c[1] as usize, c[2] as usize])
                .collect()
        };
        builder.set_connectivity_attribute(faces);

        // Clear and rebuild draco_attribute_ids in the correct order
        geometry.draco_attribute_ids = DracoAttributeIds::new();
        let mut draco_id = 0u32;

        // Add position attribute (always first, gets ID 0)
        let positions: Vec<NdVector<3, f32>> = geometry
            .positions
            .iter()
            .map(|p| NdVector::from(*p))
            .collect();
        let pos_id = builder.add_attribute(
            positions,
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );
        geometry.draco_attribute_ids.insert("POSITION", draco_id);
        draco_id += 1;

        // Add normal attribute
        if geometry.normals.is_some() {
            let normals: Vec<NdVector<3, f32>> = geometry
                .normals
                .as_ref()
                .unwrap()
                .iter()
                .map(|n| NdVector::from(*n))
                .collect();
            builder.add_attribute(
                normals,
                AttributeType::Normal,
                AttributeDomain::Corner,
                vec![pos_id],
            );
            geometry.draco_attribute_ids.insert("NORMAL", draco_id);
            draco_id += 1;
        }

        // Add texture coordinates
        for (name, texcoords) in &geometry.texcoords {
            let texcoords: Vec<NdVector<2, f32>> =
                texcoords.iter().map(|t| NdVector::from(*t)).collect();
            builder.add_attribute(
                texcoords,
                AttributeType::TextureCoordinate,
                AttributeDomain::Corner,
                vec![pos_id],
            );
            geometry.draco_attribute_ids.insert(name, draco_id);
            draco_id += 1;
        }

        // Add colors
        for (name, colors) in &geometry.colors {
            let colors: Vec<NdVector<4, f32>> = colors.iter().map(|c| NdVector::from(*c)).collect();
            builder.add_attribute(
                colors,
                AttributeType::Color,
                AttributeDomain::Corner,
                vec![pos_id],
            );
            geometry.draco_attribute_ids.insert(name, draco_id);
            draco_id += 1;
        }

        // Add tangents
        if geometry.tangents.is_some() {
            let tangents: Vec<NdVector<4, f32>> = geometry
                .tangents
                .as_ref()
                .unwrap()
                .iter()
                .map(|t| NdVector::from(*t))
                .collect();
            builder.add_attribute(
                tangents,
                AttributeType::Tangent,
                AttributeDomain::Corner,
                vec![pos_id],
            );
            geometry.draco_attribute_ids.insert("TANGENT", draco_id);
            draco_id += 1;
        }

        // Add feature IDs (from EXT_mesh_features)
        // Encode as u16 for Draco compatibility (Draco GENERIC attributes work better with integers)
        // The glTF accessor componentType will be updated to U16 after encoding
        for (name, feature_ids) in &geometry.feature_ids {
            let feature_ids: Vec<NdVector<1, u16>> = feature_ids
                .iter()
                .map(|&id| NdVector::from([id as u16]))
                .collect();
            builder.add_attribute(
                feature_ids,
                AttributeType::Custom,
                AttributeDomain::Corner,
                vec![pos_id],
            );
            geometry.draco_attribute_ids.insert(name, draco_id);
            draco_id += 1;
        }

        // Silence unused variable warning
        let _ = draco_id;

        builder
            .build()
            .map_err(|e| SkipReason::Error(Error::MeshBuild(e)))
    }
}

/// Categorize bufferViews into geometry vs non-geometry.
fn categorize_buffer_views(json: &Value) -> (HashSet<usize>, HashSet<usize>) {
    let mut geometry_views = HashSet::new();

    // Collect all bufferView indices referenced by mesh primitive accessors
    if let Some(meshes) = json.get("meshes").and_then(|m| m.as_array()) {
        for mesh in meshes {
            if let Some(primitives) = mesh.get("primitives").and_then(|p| p.as_array()) {
                for primitive in primitives {
                    // Skip already-compressed primitives
                    if draco_extension::is_draco_compressed(primitive) {
                        continue;
                    }

                    // Indices accessor
                    if let Some(idx) = primitive.get("indices").and_then(|i| i.as_u64()) {
                        if let Some(bv) = get_accessor_buffer_view(json, idx as usize) {
                            geometry_views.insert(bv);
                        }
                    }

                    // Attribute accessors
                    if let Some(attrs) = primitive.get("attributes").and_then(|a| a.as_object()) {
                        for (_, accessor_idx) in attrs {
                            if let Some(idx) = accessor_idx.as_u64() {
                                if let Some(bv) = get_accessor_buffer_view(json, idx as usize) {
                                    geometry_views.insert(bv);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // All other bufferViews are non-geometry
    let num_views = json
        .get("bufferViews")
        .and_then(|b| b.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let non_geometry_views: HashSet<usize> = (0..num_views)
        .filter(|i| !geometry_views.contains(i))
        .collect();

    (geometry_views, non_geometry_views)
}

/// Get the bufferView index for an accessor.
fn get_accessor_buffer_view(json: &Value, accessor_idx: usize) -> Option<usize> {
    json.get("accessors")
        .and_then(|a| a.get(accessor_idx))
        .and_then(|a| a.get("bufferView"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

/// Get bufferView indices that require 8-byte alignment (INT64/FLOAT64 data).
/// This checks EXT_structural_metadata property tables for properties with
/// componentType INT64 or FLOAT64.
fn get_8byte_aligned_buffer_views(json: &Value) -> HashSet<usize> {
    let mut result = HashSet::new();

    let ext = match json
        .get("extensions")
        .and_then(|e| e.get("EXT_structural_metadata"))
    {
        Some(ext) => ext,
        None => return result,
    };

    // Get schema to find property component types
    let schema_classes = ext
        .get("schema")
        .and_then(|s| s.get("classes"))
        .and_then(|c| c.as_object());

    let schema_classes = match schema_classes {
        Some(c) => c,
        None => return result,
    };

    // Get property tables
    let tables = match ext.get("propertyTables").and_then(|t| t.as_array()) {
        Some(t) => t,
        None => return result,
    };

    for table in tables {
        let class_name = match table.get("class").and_then(|c| c.as_str()) {
            Some(c) => c,
            None => continue,
        };

        let class_schema = match schema_classes.get(class_name) {
            Some(c) => c,
            None => continue,
        };

        let schema_props = match class_schema.get("properties").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => continue,
        };

        let table_props = match table.get("properties").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => continue,
        };

        for (prop_name, prop_data) in table_props {
            // Check if this property's componentType requires 8-byte alignment
            let needs_8byte = schema_props
                .get(prop_name)
                .and_then(|s| s.get("componentType"))
                .and_then(|c| c.as_str())
                .map(|c| c == "INT64" || c == "FLOAT64")
                .unwrap_or(false);

            if needs_8byte {
                // Add the "values" bufferView
                if let Some(bv_idx) = prop_data.get("values").and_then(|v| v.as_u64()) {
                    result.insert(bv_idx as usize);
                }
            }
        }
    }

    result
}

/// Reason for skipping a primitive.
enum SkipReason {
    AlreadyCompressed,
    NonTriangle(u64),
    Error(Error),
}

/// Data about a compressed primitive.
struct CompressedPrimitive {
    mesh_idx: usize,
    prim_idx: usize,
    buffer_view_offset: usize,
    buffer_view_length: usize,
    attribute_ids: DracoAttributeIds,
    indices_accessor_idx: Option<u64>,
    /// Feature ID accessor indices that need their componentType updated to U32
    feature_id_accessor_indices: Vec<u64>,
    /// Maps attribute name to original accessor index (for detecting shared accessors)
    original_accessor_indices: HashMap<String, u64>,
    /// Number of vertices in this primitive (for updating accessor count after duplication)
    vertex_count: usize,
    /// Number of indices in this primitive (for updating indices accessor count after duplication)
    indices_count: usize,
}

struct CompressedPrimitiveData {
    buffer_view_offset: usize,
    buffer_view_length: usize,
    attribute_ids: DracoAttributeIds,
    indices_accessor_idx: Option<u64>,
    /// Feature ID accessor indices that need their componentType updated to U32
    feature_id_accessor_indices: Vec<u64>,
    /// Maps attribute name to original accessor index (for detecting shared accessors)
    original_accessor_indices: HashMap<String, u64>,
    /// Number of vertices in this primitive (for updating accessor count after duplication)
    vertex_count: usize,
    /// Number of indices in this primitive (for updating indices accessor count after duplication)
    indices_count: usize,
}

/// Extracted geometry from a primitive.
#[derive(Default)]
struct ExtractedGeometry {
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    texcoords: Vec<(String, Vec<[f32; 2]>)>,
    colors: Vec<(String, Vec<[f32; 4]>)>,
    tangents: Option<Vec<[f32; 4]>>,
    feature_ids: Vec<(String, Vec<f32>)>,
    /// Maps feature ID attribute name to its accessor index (for updating componentType after encoding)
    feature_id_accessor_indices: Vec<(String, u64)>,
    indices: Vec<u32>,
    indices_accessor_idx: Option<u64>,
    draco_attribute_ids: DracoAttributeIds,
    /// Maps attribute name to original accessor index (for detecting shared accessors)
    original_accessor_indices: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_categorize_buffer_views() {
        let json = json!({
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0, "NORMAL": 1 },
                    "indices": 2
                }]
            }],
            "accessors": [
                { "bufferView": 0 },
                { "bufferView": 1 },
                { "bufferView": 2 }
            ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": 100 },
                { "buffer": 0, "byteOffset": 100, "byteLength": 100 },
                { "buffer": 0, "byteOffset": 200, "byteLength": 50 },
                { "buffer": 0, "byteOffset": 250, "byteLength": 1000 }  // Image data
            ]
        });

        let (geometry, non_geometry) = categorize_buffer_views(&json);

        assert!(geometry.contains(&0)); // POSITION
        assert!(geometry.contains(&1)); // NORMAL
        assert!(geometry.contains(&2)); // indices
        assert!(!geometry.contains(&3)); // image

        assert!(non_geometry.contains(&3));
        assert!(!non_geometry.contains(&0));
    }

    #[test]
    fn test_transcode_duck_glb() {
        let test_path = "../tests/data/Duck/Duck.glb";
        let input = match std::fs::read(test_path) {
            Ok(data) => data,
            Err(_) => {
                println!("Test file {} not found, skipping", test_path);
                return;
            }
        };

        let transcoder = GltfTranscoder::default();
        let (output, warnings) = transcoder
            .transcode_to_glb(&input)
            .expect("Transcoding failed");

        // Output should be non-empty
        assert!(!output.is_empty(), "Output should not be empty");

        // Output should be smaller than input (compressed)
        println!("Input size: {} bytes", input.len());
        println!("Output size: {} bytes", output.len());
        println!(
            "Compression ratio: {:.2}%",
            (output.len() as f64 / input.len() as f64) * 100.0
        );

        for warning in &warnings {
            println!("Warning: {}", warning);
        }

        // Output should be valid GLB (can parse header)
        let parsed = super::glb::parse_glb(&output).expect("Output is not valid GLB");
        assert!(!parsed.json.is_empty(), "JSON chunk should not be empty");

        // JSON should contain KHR_draco_mesh_compression extension
        let json_str = String::from_utf8_lossy(&parsed.json);
        assert!(
            json_str.contains("KHR_draco_mesh_compression"),
            "Output should contain Draco extension"
        );
    }

    #[test]
    fn test_transcode_deterministic() {
        let test_path = "../tests/data/Duck/Duck.glb";
        let input = match std::fs::read(test_path) {
            Ok(data) => data,
            Err(_) => {
                println!("Test file {} not found, skipping", test_path);
                return;
            }
        };

        let transcoder = GltfTranscoder::default();

        // Run transcoding multiple times
        let mut outputs = Vec::new();
        for _ in 0..5 {
            let (output, _) = transcoder
                .transcode_to_glb(&input)
                .expect("Transcoding failed");
            outputs.push(output);
        }

        // All outputs should be identical
        for (i, output) in outputs.iter().enumerate().skip(1) {
            assert_eq!(
                outputs[0].len(),
                output.len(),
                "Output {} has different length",
                i
            );
            assert_eq!(&outputs[0], output, "Output {} differs", i);
        }

        println!(
            "Determinism test passed: {} runs produced identical output",
            outputs.len()
        );
    }
}
