use std::collections::HashMap;
use gltf::accessor::Dimensions;
use gltf::Accessor;
use gltf::Primitive;
use gltf::Semantic;

use crate::core::attribute::{AttributeDomain, AttributeId};
use crate::core::mesh::Mesh;
use crate::core::mesh::meh_features::MeshFeatures;
use crate::core::attribute::ComponentDataType;
use crate::core::scene::Matrix4d;
use crate::core::scene::Scene;
use crate::core::scene::TrsMatrix;
use crate::core::texture::TextureMap;
use crate::prelude::AttributeType;
use crate::prelude::MeshBuilder;
use crate::core::shared::NdVector;

/// Holds extension attributes that the gltf crate doesn't recognize
#[derive(Debug, Clone)]
pub struct ExtensionAttributes {
    /// Map from attribute name to accessor information
    pub attributes: HashMap<String, AccessorInfo>,
    /// Extensions data for each primitive
    pub extensions: HashMap<String, serde_json::Value>,
    /// Complete EXT_structural_metadata from original JSON
    pub structural_metadata: Option<serde_json::Value>,
}

/// Stores accessor information extracted from original JSON
#[derive(Debug, Clone)]
pub struct AccessorInfo {
    pub buffer_view_index: usize,
    pub byte_offset: usize,
    pub component_type: u32,
    pub count: usize,
    pub data_type: String,
    pub buffer_view_info: BufferViewInfo,
}

/// Stores buffer view information extracted from original JSON  
#[derive(Debug, Clone)]
pub struct BufferViewInfo {
    pub buffer: usize,
    pub byte_offset: usize,
    pub byte_length: usize,
    pub byte_stride: Option<usize>,
}

impl Default for ExtensionAttributes {
    fn default() -> Self {
        Self {
            attributes: HashMap::new(),
            extensions: HashMap::new(),
            structural_metadata: None,
        }
    }
}

// Placeholder for glTF value type
type GltfValue = serde_json::Value;

// Placeholder constants for texture constants - these would come from tinygltf
pub const TINYGLTF_TEXTURE_WRAP_CLAMP_TO_EDGE: i32 = 33071;
pub const TINYGLTF_TEXTURE_WRAP_MIRRORED_REPEAT: i32 = 33648;
pub const TINYGLTF_TEXTURE_WRAP_REPEAT: i32 = 10497;
pub const TINYGLTF_TEXTURE_FILTER_NEAREST: i32 = 9728;
pub const TINYGLTF_TEXTURE_FILTER_LINEAR: i32 = 9729;
pub const TINYGLTF_TEXTURE_FILTER_NEAREST_MIPMAP_NEAREST: i32 = 9984;
pub const TINYGLTF_TEXTURE_FILTER_LINEAR_MIPMAP_NEAREST: i32 = 9985;
pub const TINYGLTF_TEXTURE_FILTER_NEAREST_MIPMAP_LINEAR: i32 = 9986;
pub const TINYGLTF_TEXTURE_FILTER_LINEAR_MIPMAP_LINEAR: i32 = 9987;


/// Scene graph can be loaded either as a tree or a general directed acyclic
/// graph (DAG) that allows multiple parent nodes. By default, we decode the
/// scene graph as a tree. If the tree mode is selected and the input contains
/// nodes with multiple parents, these nodes are duplicated to form a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GltfSceneGraphMode {
    Tree,
    Dag,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Err {
    #[error("Failed to load glTF file: {0}")]
    LoadError(String),
    #[error("Conversion Error: {0}")]
    ConversionError(String),
    #[error("IO Error: {0}")]
    IoError(String),
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("Mesh Builder Error: {0}")]
    MeshBuilderError(#[from] crate::core::mesh::builder::Err),
}

/// Data used when decoding the entire glTF asset into a single draco::Mesh.
/// The struct tracks the total number of elements across all matching
/// attributes and it ensures all matching attributes are compatible.
#[derive(Debug, Clone)]
pub struct MeshAttributeData {
    pub component_type: ComponentDataType,
    pub data_type: Dimensions,
    pub normalized: bool,
    pub total_attribute_counts: i32,
}

impl Default for MeshAttributeData {
    fn default() -> Self {
        Self {
            component_type: ComponentDataType::Invalid,
            data_type: Dimensions::Scalar,
            normalized: false,
            total_attribute_counts: 0,
        }
    }
}

/// Functionality for deduping primitives on decode.
#[derive(Debug, Clone)]
pub struct PrimitiveSignature {
    // TODO: Replace with actual primitive type once gltf library is added
    pub primitive_id: usize,
}

impl PrimitiveSignature {
    pub fn new(primitive_id: usize) -> Self {
        Self { primitive_id }
    }
}

impl PartialEq for PrimitiveSignature {
    fn eq(&self, other: &Self) -> bool {
        self.primitive_id == other.primitive_id
    }
}

impl Eq for PrimitiveSignature {}

impl std::hash::Hash for PrimitiveSignature {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.primitive_id.hash(state);
    }
}

/// Decodes a glTF file and returns a draco::Mesh. All of the mesh's attributes
/// will be merged into one draco::Mesh
pub struct GltfDecoder {
    // Data structure that stores the glTF data.
    gltf_model: Option<gltf::Document>,

    // Buffers used by the glTF model.
    buffers: Option<Vec<gltf::buffer::Data>>,

    // Path to the glTF file.
    input_file_name: String,

    // Class used to build the Draco mesh.
    // TODO: Replace with actual builder types once implemented
    mb: Option<MeshBuilder>, // TriangleSoupMeshBuilder
    pb: Option<()>, // PointCloudBuilder

    // Map from the index in a feature ID vertex attribute name like _FEATURE_ID_5
    // to the corresponding attribute index in the current geometry builder.
    feature_id_attribute_indices: HashMap<i32, i32>,

    // Next face index used when adding attribute data to the Draco mesh.
    next_face_id: i32,

    // Next point index used when adding attribute data to the point cloud.
    next_point_id: i32,

    // Total number of indices from all the meshes and primitives.
    total_face_indices_count: i32,
    total_point_indices_count: i32,

    // This is the id of the GeometryAttribute::MATERIAL attribute added to the
    // Draco mesh.
    material_att_id: i32,

    // Map of glTF attribute name to attribute component type.
    mesh_attribute_data: HashMap<String, MeshAttributeData>,

    // Map of glTF attribute name to Draco mesh attribute id.
    attribute_name_to_draco_mesh_attribute_id: HashMap<String, i32>,

    // Map of glTF material to Draco material index.
    gltf_primitive_material_to_draco_material: HashMap<i32, i32>,

    // Map of glTF material index to transformation scales of primitives.
    gltf_primitive_material_to_scales: HashMap<i32, Vec<f32>>,

    // Map of glTF image to Draco textures.
    // TODO: Replace with actual texture type once implemented
    gltf_image_to_draco_texture: HashMap<i32, ()>,

    scene: Option<Scene>,

    // Selected mode of the decoded scene graph.
    gltf_scene_graph_mode: GltfSceneGraphMode,

    // Whether vertices should be deduplicated after loading.
    deduplicate_vertices: bool,

    // Extension attributes extracted from raw JSON
    extension_attributes: Vec<Vec<ExtensionAttributes>>,
    
    // WebP image information for restoration during encoding
    webp_info: Option<String>,
    
    // Temporary storage for document-level structural metadata
    temp_document_structural_metadata: Option<serde_json::Value>,
    
    // Temporary storage for mesh features JSON
    temp_mesh_features_json: Option<String>,
}

impl Default for GltfDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl GltfDecoder {
    pub fn new() -> Self {
        Self {
            gltf_model: None,
            buffers: None,
            input_file_name: String::new(),
            mb: None,
            pb: None,
            feature_id_attribute_indices: HashMap::new(),
            next_face_id: 0,
            next_point_id: 0,
            total_face_indices_count: 0,
            total_point_indices_count: 0,
            material_att_id: 0,
            mesh_attribute_data: HashMap::new(),
            attribute_name_to_draco_mesh_attribute_id: HashMap::new(),
            gltf_primitive_material_to_draco_material: HashMap::new(),
            gltf_primitive_material_to_scales: HashMap::new(),
            gltf_image_to_draco_texture: HashMap::new(),
            scene: None,
            gltf_scene_graph_mode: GltfSceneGraphMode::Tree,
            deduplicate_vertices: true,
            extension_attributes: Vec::new(),
            webp_info: None,
            temp_document_structural_metadata: None,
            temp_mesh_features_json: None,
        }
    }

    /// Decodes a glTF file stored in the input file_name to a Mesh.
    pub fn decode_from_file(&mut self, file_name: &str) -> Result<Mesh, Err> {
        // Load the glTF file
        self.load_file(file_name, None)?;

        // Build the mesh from the loaded glTF model
        self.build_mesh()
        
    }

    /// Decodes a glTF file stored in the input file_name to a Mesh.
    /// Returns a vector of files used as input to the mesh during the decoding process.
    pub fn decode_from_file_with_files(&mut self, file_name: &str, input_files: Option<&mut Vec<String>>) -> Result<Mesh, Err> {
        // Load the glTF file and track input files if requested
        self.load_file(file_name, input_files)?;

        // Build the mesh from the loaded glTF model
        self.build_mesh()
    }

    /// Decodes a glTF file stored in the input buffer to a Mesh.
    pub fn decode_from_buffer(&mut self, buffer: &[u8]) -> Result<Mesh, Err> {
        // Load the glTF model from the buffer
        self.load_buffer(buffer)?;

        // Build the mesh from the loaded glTF model
        self.build_mesh()
    }

    /// Decodes a glTF file stored in the input file_name to a Scene.
    pub fn decode_from_file_to_scene(&mut self, file_name: &str) -> Result<Scene, Err> {
        // Call the version with files tracking, but ignore the files
        let input_files = Vec::new();
        self.decode_from_file_to_scene_with_files(file_name, input_files)
    }

    /// Decodes a glTF file stored in the input file_name to a Scene.
    /// Returns a vector of files used as input to the scene during the decoding process.
    pub fn decode_from_file_to_scene_with_files(&mut self, file_name: &str, mut input_files: Vec<String>) -> Result<Scene, Err> {
        // Load the glTF file and track input files
        self.load_file(file_name, Some(&mut input_files))?;

        // Create a new scene
        let mut scene = Scene::new();

        // Decode the glTF model into the scene
        self.decode_gltf_to_scene(&mut scene)?;

        Ok(scene)
    }

    /// Decodes a glTF file stored in the input buffer to a Scene.
    pub fn decode_from_buffer_to_scene(&mut self, buffer: &[u8]) -> Result<Scene, Err> {
        // Load the glTF model from the buffer
        self.load_buffer(buffer)?;

        // Create a new scene
        let mut scene = Scene::new();

        // Decode the glTF model into the scene
        self.decode_gltf_to_scene(&mut scene)?;

        Ok(scene)
    }

    /// Sets the scene graph mode
    pub fn set_scene_graph_mode(&mut self, mode: GltfSceneGraphMode) {
        self.gltf_scene_graph_mode = mode;
    }

    /// By default, the decoder will attempt to deduplicate vertices after decoding
    /// the mesh. This means lower memory usage and smaller output glTFs after
    /// reencoding. However, for very large meshes, this may become an expensive
    /// operation. If that becomes an issue, you might want to consider disabling
    /// deduplication.
    ///
    /// Note that at this moment, disabling deduplication works ONLY for point clouds.
    pub fn set_deduplicate_vertices(&mut self, deduplicate_vertices: bool) {
        self.deduplicate_vertices = deduplicate_vertices;
    }

    // Private methods

    /// Loads file_name into gltf_model. Fills input_files with paths to all
    /// input files when provided.
    fn load_file(&mut self, file_name: &str, input_files: Option<&mut Vec<String>>) -> Result<(), Err> {
        use std::path::Path;
        use std::fs::File;
        use std::io::Read;
        
        // Try to load glTF document using the standard import
        let (gltf, buffers, _images) = match gltf::import(file_name) {
            Ok(result) => result,
            Err(e) => {
                // If validation fails, load the file manually without validation
                let mut file = File::open(file_name)
                    .map_err(|io_e| Err::IoError(format!("Failed to open file: {}", io_e)))?;
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)
                    .map_err(|io_e| Err::IoError(format!("Failed to read file: {}", io_e)))?;
                
                // Check if it's a binary glTF (.glb) file
                if file_name.ends_with(".glb") {
                    // Parse as GLB
                    let glb = gltf::Glb::from_slice(&contents)
                        .map_err(|glb_e| Err::IoError(format!("Failed to parse GLB: {}", glb_e)))?;
                    
                    // Load without validation
                    let gltf = gltf::Gltf::from_slice_without_validation(&glb.json)
                        .map_err(|gltf_e| Err::IoError(format!("Failed to load glTF: {} (validation error: {})", gltf_e, e)))?;
                    
                    // Parse extension attributes from the raw JSON
                    self.parse_extension_attributes_from_json(&glb.json)?;
                    
                    let buffers = glb.bin
                        .map(|data| vec![gltf::buffer::Data(data.to_vec())])
                        .unwrap_or_default();
                    
                    (gltf.document, buffers, vec![])
                } else {
                    // Parse as JSON glTF
                    let gltf = gltf::Gltf::from_slice_without_validation(&contents)
                        .map_err(|gltf_e| Err::IoError(format!("Failed to load glTF: {} (validation error: {})", gltf_e, e)))?;
                    
                    // Parse extension attributes from the raw JSON
                    self.parse_extension_attributes_from_json(&contents)?;
                    
                    // Import buffers separately
                    let base_path = Path::new(file_name).parent()
                        .ok_or_else(|| Err::IoError("Invalid file path".to_string()))?;
                    
                    let buffers = gltf::import_buffers(&gltf.document, Some(base_path), None)
                        .map_err(|buf_e| Err::IoError(format!("Failed to import buffers: {}", buf_e)))?;
                    
                    (gltf.document, buffers, vec![])
                }
            }
        };


        // Track input files if requested
        if let Some(input_files) = input_files {
            input_files.push(file_name.to_string());
            
            // Add any additional files referenced by the glTF (images, bin files)
            for image in gltf.images() {
                if let gltf::image::Source::Uri { uri, .. } = image.source() {
                    // Resolve relative paths
                    if let Some(parent) = Path::new(file_name).parent() {
                        let image_path = parent.join(uri);
                        if let Some(path_str) = image_path.to_str() {
                            input_files.push(path_str.to_string());
                        }
                    }
                }
            }
            
            for buffer in gltf.buffers() {
                if let gltf::buffer::Source::Uri(uri) = buffer.source() {
                    // Resolve relative paths  
                    if let Some(parent) = Path::new(file_name).parent() {
                        let buffer_path = parent.join(uri);
                        if let Some(path_str) = buffer_path.to_str() {
                            input_files.push(path_str.to_string());
                        }
                    }
                }
            }
        }

        // Store the loaded glTF document
        self.gltf_model = Some(gltf);

        // Store the buffers
        self.buffers = Some(buffers);

        // Check for unsupported features
        self.check_unsupported_features()?;
        
        // Store the input file name
        self.input_file_name = file_name.to_string();
        
        Ok(())
    }

    /// Loads gltf_model from buffer in GLB format.
    fn load_buffer(&mut self, buffer: &[u8]) -> Result<(), Err> {
        // Try to load glTF document from binary buffer, following the same pattern as load_file
        // First attempt with validation using gltf::Gltf::from_slice, then without if it fails
        let (gltf, buffers, _images): (gltf::Document, Vec<gltf::buffer::Data>, Vec<gltf::image::Data>) = match gltf::Gltf::from_slice(buffer) {
            Ok(gltf) => {
                // Successfully loaded with validation
                let gltf_doc = gltf.document;
                let gltf_buffers = gltf.blob.map(|blob| vec![gltf::buffer::Data(blob)]).unwrap_or_default();
                (gltf_doc, gltf_buffers, vec![])
            }
            Err(e) => {
                // If validation fails, load manually without validation (same as load_file)
                // Check if it's a binary glTF (.glb) file
                if buffer.len() >= 12 && &buffer[0..4] == b"glTF" {
                    // Parse as GLB using the same method as load_file
                    let glb = gltf::Glb::from_slice(buffer)
                        .map_err(|glb_e| Err::IoError(format!("Failed to parse GLB: {}", glb_e)))?;
                    
                    // Load without validation
                    let gltf = gltf::Gltf::from_slice_without_validation(&glb.json)
                        .map_err(|gltf_e| Err::IoError(format!("Failed to load glTF: {} (validation error: {})", gltf_e, e)))?;
                    
                    // Parse extension attributes from the raw JSON
                    self.parse_extension_attributes_from_json(&glb.json)?;
                    
                    let buffers = glb.bin
                        .map(|data| vec![gltf::buffer::Data(data.to_vec())])
                        .unwrap_or_default();
                    
                    (gltf.document, buffers, vec![])
                } else {
                    // Parse as JSON glTF
                    let gltf = gltf::Gltf::from_slice_without_validation(buffer)
                        .map_err(|gltf_e| Err::IoError(format!("Failed to load glTF: {} (validation error: {})", gltf_e, e)))?;
                    
                    // Parse extension attributes from the raw JSON
                    self.parse_extension_attributes_from_json(buffer)?;
                    
                    // For JSON glTF from buffer, we can't import external buffers
                    // so we just use empty buffer list
                    (gltf.document, vec![], vec![])
                }
            }
        };

        // Store the loaded glTF document
        self.gltf_model = Some(gltf);

        // Store the buffers
        self.buffers = Some(buffers);

        // Check for unsupported features
        self.check_unsupported_features()?;
        
        // Clear the input file name since we're loading from buffer
        self.input_file_name.clear();
        
        Ok(())
    }

    /// Parse extension attributes from raw glTF JSON
    fn parse_extension_attributes_from_json(&mut self, json_data: &[u8]) -> Result<(), Err> {
        // Parse the JSON
        let json: serde_json::Value = serde_json::from_slice(json_data)
            .map_err(|e| Err::LoadError(format!("Failed to parse glTF JSON: {}", e)))?;
        
        // Clear existing extension attributes
        self.extension_attributes.clear();
        
        // Extract document-level extensions, especially EXT_structural_metadata
        if let Some(extensions) = json.get("extensions").and_then(|e| e.as_object()) {
            if let Some(structural_metadata) = extensions.get("EXT_structural_metadata") {
                // Store the structural metadata as a JSON string in a temporary location
                // We'll add it to the scene later in add_structural_metadata_to_scene
                if let Ok(_metadata_string) = serde_json::to_string(structural_metadata) {
                    // Create a temporary field to hold this until we can add it to the scene
                    // For now, we'll store it in the first primitive's extension attributes
                    // This is a bit of a hack, but it works within the current architecture
                    self.temp_document_structural_metadata = Some(structural_metadata.clone());
                }
            }
        }
        
        // Get meshes array from JSON
        if let Some(meshes) = json.get("meshes").and_then(|m| m.as_array()) {
            for (_mesh_index, mesh) in meshes.iter().enumerate() {
                let mut mesh_primitives = Vec::new();
                
                // Get primitives array for this mesh
                if let Some(primitives) = mesh.get("primitives").and_then(|p| p.as_array()) {
                    for (_primitive_index, primitive) in primitives.iter().enumerate() {
                        let mut ext_attrs = ExtensionAttributes::default();
                        
                        // Extract custom attributes (those starting with _)
                        if let Some(attributes) = primitive.get("attributes").and_then(|a| a.as_object()) {
                            for (attr_name, accessor_index) in attributes {
                                if attr_name.starts_with("_") {
                                    // This is a custom attribute, extract its accessor info
                                    if let Some(accessor_idx) = accessor_index.as_u64() {
                                        if let Some(accessor_info) = self.extract_accessor_info(&json, accessor_idx as usize)? {
                                            ext_attrs.attributes.insert(attr_name.clone(), accessor_info);
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Extract extensions for this primitive
                        if let Some(extensions) = primitive.get("extensions").and_then(|e| e.as_object()) {
                            for (ext_name, ext_data) in extensions {
                                ext_attrs.extensions.insert(ext_name.clone(), ext_data.clone());
                            }
                        }
                        
                        mesh_primitives.push(ext_attrs);
                    }
                }
                
                self.extension_attributes.push(mesh_primitives);
            }
        }
        
        Ok(())
    }

    /// Extract accessor information from JSON for a given accessor index
    fn extract_accessor_info(&self, json: &serde_json::Value, accessor_index: usize) -> Result<Option<AccessorInfo>, Err> {
        // Get accessors array
        let accessors = json.get("accessors")
            .and_then(|a| a.as_array())
            .ok_or_else(|| Err::LoadError("No accessors found in glTF".to_string()))?;
        
        if accessor_index >= accessors.len() {
            return Ok(None);
        }
        
        let accessor = &accessors[accessor_index];
        
        // Extract accessor properties
        let buffer_view_index = accessor.get("bufferView")
            .and_then(|bv| bv.as_u64())
            .ok_or_else(|| Err::LoadError("Accessor missing bufferView".to_string()))? as usize;
        
        let byte_offset = accessor.get("byteOffset")
            .and_then(|bo| bo.as_u64())
            .unwrap_or(0) as usize;
        
        let component_type = accessor.get("componentType")
            .and_then(|ct| ct.as_u64())
            .ok_or_else(|| Err::LoadError("Accessor missing componentType".to_string()))? as u32;
        
        let count = accessor.get("count")
            .and_then(|c| c.as_u64())
            .ok_or_else(|| Err::LoadError("Accessor missing count".to_string()))? as usize;
        
        let data_type = accessor.get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| Err::LoadError("Accessor missing type".to_string()))?
            .to_string();
        
        // Extract buffer view info
        let buffer_view_info = self.extract_buffer_view_info(json, buffer_view_index)?;
        
        Ok(Some(AccessorInfo {
            buffer_view_index,
            byte_offset,
            component_type,
            count,
            data_type,
            buffer_view_info,
        }))
    }

    /// Extract buffer view information from JSON
    fn extract_buffer_view_info(&self, json: &serde_json::Value, buffer_view_index: usize) -> Result<BufferViewInfo, Err> {
        let buffer_views = json.get("bufferViews")
            .and_then(|bv| bv.as_array())
            .ok_or_else(|| Err::LoadError("No bufferViews found in glTF".to_string()))?;
        
        if buffer_view_index >= buffer_views.len() {
            return Err(Err::LoadError("BufferView index out of range".to_string()));
        }
        
        let buffer_view = &buffer_views[buffer_view_index];
        
        let buffer = buffer_view.get("buffer")
            .and_then(|b| b.as_u64())
            .ok_or_else(|| Err::LoadError("BufferView missing buffer".to_string()))? as usize;
        
        let byte_offset = buffer_view.get("byteOffset")
            .and_then(|bo| bo.as_u64())
            .unwrap_or(0) as usize;
        
        let byte_length = buffer_view.get("byteLength")
            .and_then(|bl| bl.as_u64())
            .ok_or_else(|| Err::LoadError("BufferView missing byteLength".to_string()))? as usize;
        
        let byte_stride = buffer_view.get("byteStride")
            .and_then(|bs| bs.as_u64())
            .map(|bs| bs as usize);
        
        Ok(BufferViewInfo {
            buffer,
            byte_offset,
            byte_length,
            byte_stride,
        })
    }

    /// Builds mesh from gltf_model.
    fn build_mesh(&mut self) -> Result<Mesh, Err> {
        // Gather statistics about attributes and materials
        self.gather_attribute_and_material_stats()?;
        
        // Add extension attributes to mesh_attribute_data
        self.add_extension_attributes_to_stats()?;
        
        // Check for mixed primitive types (triangles and points)
        if self.total_face_indices_count > 0 && self.total_point_indices_count > 0 {
            return Err(Err::LoadError(
                "Decoding to mesh can't handle triangle and point primitives at the same time.".to_string()
            ));
        }

        // Initialize the appropriate builder based on primitive type
        if self.total_face_indices_count > 0 {
            // Triangle mesh builder
            self.mb = Some(MeshBuilder::new());
            self.add_attributes_to_draco_mesh_triangle()?;
        } else {
            // Point cloud builder
            self.pb = Some(self.create_point_cloud_builder(self.total_point_indices_count)?);
            self.add_attributes_to_draco_mesh_point()?;
        }

        // Clear attribute indices before populating attributes
        self.feature_id_attribute_indices.clear();

        // Process all scenes and their nodes
        if self.gltf_model.is_some() {
            let parent_matrix = Matrix4d::identity();
            // Take ownership temporarily to avoid borrow checker issues
            let gltf_model = self.gltf_model.take().unwrap();
            
            for scene in gltf_model.scenes() {
                for node in scene.nodes() {
                    self.decode_node(&node, &parent_matrix)?;
                }
            }
            
            // Put it back
            self.gltf_model = Some(gltf_model);
        } else {
            return Err(Err::LoadError("No glTF model loaded".to_string()));
        }

        // Build the final mesh from the builder
        let mut mesh = self.mb.take().unwrap().build()?;

        // Add additional data to the mesh
        self.copy_textures(&mut mesh)?;
        self.set_attribute_properties_on_draco_mesh(&mut mesh);
        self.add_materials_to_draco_mesh(&mut mesh)?;
        self.add_primitive_extensions_to_draco_mesh(&mut mesh)?;
        self.add_structural_metadata_to_geometry(&mut ())?;
        Self::move_non_material_textures_from_mesh(&mut mesh);
        self.add_asset_metadata_to_mesh(&mut mesh)?;

        Ok(mesh)
    }

    /// Helper method to create point cloud builder
    fn create_point_cloud_builder(&self, num_points: i32) -> Result<(), Err> {
        // TODO: Implement point cloud builder creation
        // This would initialize the point cloud builder with the specified number of points
        let _ = num_points;
        Ok(())
    }


    /// Add attributes to triangle mesh builder
    fn add_attributes_to_draco_mesh_triangle(&mut self) -> Result<(), Err> {
        let mut curr_att_id = 0;
        for att in &mut self.mesh_attribute_data {
            let draco_att_type = AttributeType::Invalid; // Simplified - was gltf_attribute_to_draco_attribute(att.0);
            if draco_att_type == AttributeType::Invalid {
                *self.attribute_name_to_draco_mesh_attribute_id.get_mut(att.0).unwrap() = -1;
                continue;
            }
            self.mb.as_mut().unwrap().add_gltf_empty_attribute(
                draco_att_type,
                AttributeDomain::Position,
                att.1.component_type,
                att.1.data_type,
            );
            *self.attribute_name_to_draco_mesh_attribute_id.get_mut(att.0).unwrap() = curr_att_id as i32;
            curr_att_id += 1;
        }

        // Add the material attribute.
        if self.gltf_model.as_mut().unwrap().materials().len() > 1 {
            let mut component_type = ComponentDataType::U32;
            if self.gltf_model.as_mut().unwrap().materials().len() < 256 {
                component_type = ComponentDataType::I8;
            } else if self.gltf_model.as_mut().unwrap().materials().len() < (1 << 16) {
                component_type = ComponentDataType::U16;
            }
            self.material_att_id = curr_att_id as i32;
            self.mb.as_mut().unwrap().add_empty_attribute(
                AttributeType::Material, 
                AttributeDomain::Position, 
                component_type,
                1,
            );
        }

        Ok(())
    }

    /// Add attributes to point cloud builder
    fn add_attributes_to_draco_mesh_point(&mut self) -> Result<(), Err> {
        // TODO: Implement attribute addition for point cloud
        unimplemented!("Point cloud attribute addition not yet implemented")
    }

    /// Checks gltf_model for unsupported features. If gltf_model contains
    /// unsupported features then the function will return an error.
    fn check_unsupported_features(&self) -> Result<(), Err> {
        if let Some(ref gltf_model) = self.gltf_model {
            // Check for morph targets.
            for mesh in gltf_model.meshes() {
                for primitive in mesh.primitives() {
                    if !primitive.morph_targets().next().is_none() {
                        return Err(Err::LoadError("Morph targets are unsupported.".to_string()));
                    }
                }
            }

            // Check for sparse accessors.
            for accessor in gltf_model.accessors() {
                if accessor.sparse().is_some() {
                    return Err(Err::LoadError("Sparse accessors are unsupported.".to_string()));
                }
            }

            // Check for required extensions.
            for extension in gltf_model.extensions_required() {
                match extension {
                    "KHR_materials_unlit" | "KHR_texture_transform" | "KHR_draco_mesh_compression" | "EXT_mesh_features" => {
                        // These extensions are supported
                    }
                    _ => {
                        return Err(Err::LoadError(format!("{} is unsupported.", extension)));
                    }
                }
            }
        } else {
            return Err(Err::LoadError("No glTF model loaded".to_string()));
        }

        Ok(())
    }

    /// Decodes a glTF Node as well as any child Nodes. If node contains a mesh
    /// it will process all of the mesh's primitives.
    fn decode_node(&mut self, node: &gltf::Node, parent_matrix: &Matrix4d) -> Result<(), Err> {
        let trsm: TrsMatrix = Self::get_node_transformation_matrix(&node);
        let node_matrix: Matrix4d =
            parent_matrix.clone() * trsm.compute_transformation_matrix();

        if let Some(mesh) = node.mesh() {
            let mesh_index = mesh.index();
            for (primitive_index, primitive) in mesh.primitives().enumerate() {
                self.decode_primitive(&primitive, &node_matrix, mesh_index, primitive_index)?;
            }
        }
        for c in node.children() {
            self.decode_node(&c, &node_matrix)?;
        }
        Ok(())
    }

    /// Decodes the number of entries in the first attribute of a given glTF primitive.
    /// Note that all attributes have the same entry count according to glTF 2.0 spec.
    fn decode_primitive_attribute_count(&self, primitive: &Primitive) -> Result<i32, Err> {
        // Use the first primitive attribute as all attributes have the same entry
        // count according to glTF 2.0 spec.
        // Try standard semantics first to avoid potential panics from custom attributes
        if let Some(positions) = primitive.get(&Semantic::Positions) {
            return Ok(positions.count() as i32);
        }
        if let Some(normals) = primitive.get(&Semantic::Normals) {
            return Ok(normals.count() as i32);
        }
        for i in 0..8 {
            if let Some(texcoords) = primitive.get(&Semantic::TexCoords(i)) {
                return Ok(texcoords.count() as i32);
            }
        }
        
        // If no standard attributes found, try the iterator approach (might panic)
        let mut attributes = primitive.attributes();
        if let Some((_, accessor)) = attributes.next() {
            Ok(accessor.count() as i32)
        } else {
            Err(Err::LoadError("Primitive has no attributes.".to_string()))
        }
    }

    /// Decodes indices property of a given glTF primitive. If primitive's
    /// indices property is not defined, the indices are generated based on entry
    /// count of a primitive attribute.
    fn decode_primitive_indices(&self, primitive: &Primitive) -> Result<Vec<u32>, Err> {
        let mut indices_data = Vec::new();
        
        match primitive.indices() {
            Some(indices_accessor) => {
                // Get indices from the primitive's indices property.
                if indices_accessor.count() == 0 {
                    return Err(Err::LoadError("Could not convert indices.".to_string()));
                }
                
                // Copy data as u32 from the accessor  
                if let Some(ref buffers) = self.buffers {
                    indices_data = copy_data_as_uint32(&indices_accessor, buffers)?;
                } else {
                    return Err(Err::LoadError("No buffers available for index data".to_string()));
                }
            }
            None => {
                // Primitive has implicit indices [0, 1, 2, 3, ...]. Create indices based on
                // entry count of a primitive attribute.
                let num_vertices = self.decode_primitive_attribute_count(primitive)?;
                indices_data.reserve(num_vertices as usize);
                for i in 0..num_vertices {
                    indices_data.push(i as u32);
                }
            }
        }
        
        Ok(indices_data)
    }

    /// Decodes a glTF Primitive. All of the primitive's attributes will be
    /// merged into the draco::Mesh output if they are of the same type that
    /// already has been decoded.
    fn decode_primitive(&mut self, primitive: &Primitive, transform_matrix: &Matrix4d, mesh_index: usize, primitive_index: usize) -> Result<(), Err> {
        use gltf::mesh::Mode;
        
        // Check primitive mode
        if primitive.mode() != Mode::Triangles && primitive.mode() != Mode::Points {
            return Err(Err::LoadError(
                "Primitive does not contain triangles or points.".to_string()
            ));
        }

        // Store the transformation scale of this primitive loading as draco::Mesh.
        if self.scene.is_none() {
            // TODO: Do something for non-uniform scaling.
            let scale = (transform_matrix.data[0][0].powi(2) + 
                        transform_matrix.data[1][0].powi(2) + 
                        transform_matrix.data[2][0].powi(2)).sqrt() as f32;
            
            let material_index = primitive.material().index().unwrap_or(0) as i32;
            self.gltf_primitive_material_to_scales
                .entry(material_index)
                .or_insert_with(Vec::new)
                .push(scale);
        }

        // Handle indices first.
        let indices_data = self.decode_primitive_indices(primitive)?;
        let number_of_faces = indices_data.len() as i32 / 3;
        let number_of_points = indices_data.len() as i32;

        // Process standard attributes
        // Collect attributes safely to avoid panics on custom attributes
        let mut safe_attributes = Vec::new();
        
        // Try standard semantics
        if let Some(positions) = primitive.get(&Semantic::Positions) {
            safe_attributes.push((Semantic::Positions, positions));
        }
        if let Some(normals) = primitive.get(&Semantic::Normals) {
            safe_attributes.push((Semantic::Normals, normals));
        }
        for i in 0..8 {
            if let Some(texcoords) = primitive.get(&Semantic::TexCoords(i)) {
                safe_attributes.push((Semantic::TexCoords(i), texcoords));
            }
            if let Some(colors) = primitive.get(&Semantic::Colors(i)) {
                safe_attributes.push((Semantic::Colors(i), colors));
            }
            if let Some(joints) = primitive.get(&Semantic::Joints(i)) {
                safe_attributes.push((Semantic::Joints(i), joints));
            }
            if let Some(weights) = primitive.get(&Semantic::Weights(i)) {
                safe_attributes.push((Semantic::Weights(i), weights));
            }
        }
        if let Some(tangents) = primitive.get(&Semantic::Tangents) {
            safe_attributes.push((Semantic::Tangents, tangents));
        }
        
        for (attribute_name, accessor) in safe_attributes {
            let att_id = self.attribute_name_to_draco_mesh_attribute_id
                .get(attribute_name.to_string().as_str())
                .cloned()
                .unwrap_or(-1);
            
            if att_id == -1 {
                continue;
            }

            if primitive.mode() == Mode::Triangles {
                // Add to triangle mesh builder  
                if self.mb.is_some() {
                    // We need to work around borrow checker issues by temporarily taking the builder
                    let mut mb = self.mb.take().unwrap();
                    let result = self.add_attribute_values_to_builder(
                        &attribute_name.to_string(),
                        &accessor,
                        &indices_data,
                        att_id,
                        number_of_faces,
                        &transform_matrix.data,
                        &mut mb,
                    );
                    self.mb = Some(mb);
                    result?;
                }
            } else {
                // Add to point cloud builder
                if let Some(ref mut _pb) = self.pb {
                    unimplemented!("Point cloud attribute addition not yet implemented");
                    // self.add_attribute_values_to_builder(
                    //     &attribute_name.to_string(),
                    //     &accessor,
                    //     &indices_data,
                    //     att_id,
                    //     number_of_points,
                    //     &transform_matrix.data,
                    //     pb,
                    // )?;
                }
            }
        }

        // Process extension attributes that weren't included in primitive.attributes()
        if mesh_index < self.extension_attributes.len() && primitive_index < self.extension_attributes[mesh_index].len() {
            let ext_attrs = self.extension_attributes[mesh_index][primitive_index].clone();
            
            // Collect extension attribute processing data first to avoid borrow conflicts
            let mut ext_attr_data = Vec::new();
            for (attr_name, accessor_info) in &ext_attrs.attributes {
                let att_id = self.attribute_name_to_draco_mesh_attribute_id
                    .get(attr_name)
                    .cloned()
                    .unwrap_or(-1);
                
                if att_id != -1 {
                    ext_attr_data.push((attr_name.clone(), accessor_info.clone(), att_id));
                }
            }
            
            // Process the collected extension attributes
            for (attr_name, accessor_info, _att_id) in ext_attr_data {
                if primitive.mode() == Mode::Triangles {
                    if self.mb.is_some() {
                        // Extract data directly using stored accessor information
                        if let Some(ref buffers) = self.buffers {
                            if let Ok(feature_data) = Self::extract_uint32_from_accessor_info(&accessor_info, buffers) {
                                // Convert to NdVector<1> for scalar values
                                let feature_data_vectors: Vec<_> = feature_data
                                    .into_iter()
                                    .map(|val| NdVector::from([val]))
                                    .collect();
                                
                                // Add as Custom attribute to the mesh builder
                                let mut mb = self.mb.take().unwrap();
                                let attribute_id = mb.add_attribute(
                                    feature_data_vectors,
                                    AttributeType::Custom,
                                    AttributeDomain::Corner,
                                    vec![],
                                );
                                
                                // Set the attribute name so it can be used during encoding
                                if let Some(attribute) = mb.attributes.last_mut() {
                                    attribute.set_name(attr_name.to_string());
                                }
                                self.mb = Some(mb);
                                
                                // Map extension attribute indices for later use in mesh features
                                if let Some(feature_id_str) = attr_name.strip_prefix("_FEATURE_ID_") {
                                    if let Ok(feature_id) = feature_id_str.parse::<i32>() {
                                        self.feature_id_attribute_indices.insert(feature_id, attribute_id.as_usize() as i32);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Add the material data only if there is more than one material.
        if self.gltf_primitive_material_to_draco_material.len() > 1 {
            let material_index = primitive.material().index().unwrap_or(0) as i32;
            if let Some(&draco_material_index) = self.gltf_primitive_material_to_draco_material.get(&material_index) {
                if primitive.mode() == Mode::Triangles {
                    if self.mb.is_some() {
                        let mut mb = self.mb.take().unwrap();
                        let result = self.add_material_data_to_builder(draco_material_index, number_of_faces, &mut mb);
                        self.mb = Some(mb);
                        result?;
                    }
                } else {
                    if let Some(ref mut _pb) = self.pb {
                        unimplemented!("Point cloud material addition not yet implemented");
                        // self.add_material_data_to_builder(draco_material_index, number_of_points, pb)?;
                    }
                }
            }
        }

        // Extension attributes are now processed through the normal attribute flow
        // via add_extension_attributes_to_stats(), so no special handling needed here

        self.next_face_id += number_of_faces;
        self.next_point_id += number_of_points;
        
        Ok(())
    }

    /// gltf::Node version of node_gather_attribute_and_material_stats
    fn node_gather_attribute_and_material_stats_gltf(&mut self, scene_idx: usize, node_idx: usize) -> Result<(), Err> {
        // Collect primitive data first to avoid borrow checker issues
        let mut primitives_to_process = Vec::new();
        
        if let Some(ref gltf_model) = self.gltf_model {
            if let Some(node) = gltf_model.scenes().nth(scene_idx).and_then(|scene| scene.nodes().nth(node_idx)) {
                if let Some(mesh) = node.mesh() {
                    for primitive in mesh.primitives() {
                        primitives_to_process.push(primitive);
                    }
                }
            }
        }
        
        // Process collected primitives
        for primitive in primitives_to_process {
            // self.accumulate_primitive_stats_gltf(&primitive)?;

            let material_index = primitive.material().index().unwrap_or(0) as i32;
            if !self.gltf_primitive_material_to_draco_material.contains_key(&material_index) {
                let draco_material_index = self.gltf_primitive_material_to_draco_material.len() as i32;
                self.gltf_primitive_material_to_draco_material.insert(material_index, draco_material_index);
            }
        }

        // TODO: Handle child nodes recursively
        
        Ok(())
    }

    /// Add extension attributes to mesh_attribute_data so they get processed like normal attributes
    fn add_extension_attributes_to_stats(&mut self) -> Result<(), Err> {
        // Collect extension attribute data first to avoid borrow conflicts
        let mut ext_attr_info = Vec::new();
        
        // Extract extension attributes and corresponding accessor info
        for (_mesh_index, mesh_ext_attrs) in self.extension_attributes.iter().enumerate() {
            for (_primitive_index, ext_attrs) in mesh_ext_attrs.iter().enumerate() {
                // println!("DEBUG: ext_attrs: {:?}", ext_attrs);
                for (attr_name, accessor_info) in &ext_attrs.attributes {
                    // Only process feature ID and other extension attributes
                    // println!("DEBUG: Processing extension attribute: {}", attr_name);
                    if attr_name.starts_with("_FEATURE_ID_") || attr_name.starts_with("_") {
                        // Use the stored accessor information instead of looking it up in the sanitized model
                        let component_type = match accessor_info.component_type {
                            5120 => 1, // GL_BYTE
                            5121 => 2, // GL_UNSIGNED_BYTE
                            5122 => 3, // GL_SHORT
                            5123 => 4, // GL_UNSIGNED_SHORT
                            5125 => 5, // GL_UNSIGNED_INT
                            5126 => 6, // GL_FLOAT
                            _ => 6,    // Default to float
                        };
                        
                        let data_type = match accessor_info.data_type.as_str() {
                            "SCALAR" => 1,
                            "VEC2" => 2,
                            "VEC3" => 3,
                            "VEC4" => 4,
                            "MAT2" => 9,
                            "MAT3" => 16,
                            "MAT4" => 16,
                            _ => 1, // Default to scalar
                        };
                        
                        ext_attr_info.push((attr_name.clone(), component_type, data_type, false, accessor_info.count as i32, accessor_info.buffer_view_index));
                    }
                }
            }
        }
        
        // Now process the collected data without holding any references
        for (attr_name, component_type, data_type, normalized, count, _buffer_view_index) in ext_attr_info {
            // Add to mesh_attribute_data using check_types which handles first-time registration
            self.check_types(&attr_name, component_type, data_type, normalized)?;
            
            // Sum the attribute counts
            self.sum_attribute_stats(&attr_name, count);
        }
        
        Ok(())
    }

    /// Sums the number of elements per attribute for all of the meshes and primitives.
    fn gather_attribute_and_material_stats(&mut self) -> Result<(), Err> {
        if let Some(ref gltf_model) = self.gltf_model {
            let num_scenes = gltf_model.scenes().map(|s| s.nodes().len()).collect::<Vec<_>>();
            for si in 0..gltf_model.scenes().len() {
                let n_scenes = num_scenes[si];
                for ni in 0..n_scenes {
                    self.node_gather_attribute_and_material_stats_gltf(si, ni)?;
                }
            }
        } else {
            return Err(Err::LoadError("No glTF model loaded".to_string()));
        }
        Ok(())
    }

    /// Sums the attribute counts into total_attribute_counts.
    fn sum_attribute_stats(&mut self, attribute_name: &str, count: i32) {
        // We know that there must be a valid entry for |attribute_name| at this time.
        if let Some(mad) = self.mesh_attribute_data.get_mut(attribute_name) {
            mad.total_attribute_counts += count;
        }
    }

    /// Checks that all the same glTF attribute types in different meshes and
    /// primitives contain the same characteristics.
    fn check_types(&mut self, attribute_name: &str, component_type: i32, type_: i32, normalized: bool) -> Result<(), Err> {
        if let Some(mad) = self.mesh_attribute_data.get(attribute_name) {
            // Allow different component types for feature ID attributes since they can vary per primitive
            if mad.component_type as i32 != component_type && !attribute_name.starts_with("_FEATURE_ID_") {
                return Err(Err::LoadError(format!(
                    "{} attribute component type does not match previous.",
                    attribute_name
                )));
            }
            // Allow different data types for feature ID attributes since they can vary per primitive
            if mad.data_type as i32 != type_ && !attribute_name.starts_with("_FEATURE_ID_") {
                return Err(Err::LoadError(format!(
                    "{} attribute type does not match previous.",
                    attribute_name
                )));
            }
            // Allow different normalized properties for feature ID attributes since they can vary per primitive
            if mad.normalized != normalized && !attribute_name.starts_with("_FEATURE_ID_") {
                return Err(Err::LoadError(format!(
                    "{} attribute normalized property does not match previous.",
                    attribute_name
                )));
            }
        } else {
            // First time seeing this attribute, create new entry
            let mad = MeshAttributeData {
                component_type: match component_type {
                    1 => ComponentDataType::I8,
                    2 => ComponentDataType::U8,
                    3 => ComponentDataType::I16,
                    4 => ComponentDataType::U16,
                    5 => ComponentDataType::U32,
                    6 => ComponentDataType::F32,
                    _ => ComponentDataType::Invalid,
                },
                data_type: match type_ {
                    1 => Dimensions::Scalar,
                    2 => Dimensions::Vec2,
                    3 => Dimensions::Vec3,
                    4 => Dimensions::Vec4,
                    9 => Dimensions::Mat2,
                    16 => Dimensions::Mat3,
                    _ => Dimensions::Scalar,
                },
                normalized,
                total_attribute_counts: 0,
            };
            self.mesh_attribute_data.insert(attribute_name.to_string(), mad);
            // Initialize the attribute ID mapping with -1 (will be updated later)
            self.attribute_name_to_draco_mesh_attribute_id.insert(attribute_name.to_string(), -1);
        }
        Ok(())
    }

    /// Copies attribute data from accessor and adds it to a Draco mesh using the
    /// geometry builder.
    fn add_attribute_values_to_builder(
        &mut self,
        attribute_name: &str,
        accessor: &Accessor,
        indices_data: &[u32],
        att_id: i32,
        number_of_elements: i32,
        transform_matrix: &[[f64; 4]; 4],
        builder: &mut MeshBuilder,
    ) -> Result<(), Err> {
        // Calculate determinant to check for reverse winding
        let determinant = transform_matrix[0][0] * (transform_matrix[1][1] * transform_matrix[2][2] - transform_matrix[1][2] * transform_matrix[2][1])
                        - transform_matrix[0][1] * (transform_matrix[1][0] * transform_matrix[2][2] - transform_matrix[1][2] * transform_matrix[2][0])
                        + transform_matrix[0][2] * (transform_matrix[1][0] * transform_matrix[2][1] - transform_matrix[1][1] * transform_matrix[2][0]);
        let reverse_winding = determinant < 0.0;
        
        match attribute_name {
            "TEXCOORD_0" | "TEXCOORD_1" => {
                self.add_tex_coord_to_builder(
                    &(), // TODO: Use proper accessor
                    indices_data,
                    att_id,
                    number_of_elements,
                    reverse_winding,
                    &mut (), // TODO: Use proper builder
                )
            }
            "TANGENT" => {
                // For tangents, we need to update the matrix for normals
                self.add_tangent_to_builder(
                    &(), // TODO: Use proper accessor
                    indices_data,
                    att_id,
                    number_of_elements,
                    transform_matrix,
                    reverse_winding,
                    builder,
                )
            }
            "POSITION" | "NORMAL" => {
                let normalize = attribute_name == "NORMAL";
                self.add_transformed_data_to_builder(
                    &(), // TODO: Use proper accessor  
                    indices_data,
                    att_id,
                    number_of_elements,
                    transform_matrix,
                    normalize,
                    reverse_winding,
                    &mut (), // TODO: Use proper builder
                )
            }
            name if name.starts_with("_FEATURE_ID_") => {
                self.add_feature_id_to_builder(
                    accessor,
                    attribute_name,
                    builder,
                )
            }
            name if name.starts_with('_') => {
                // Structural metadata property attribute
                self.add_property_attribute_to_builder(
                    &(), // TODO: Use proper accessor
                    indices_data,
                    att_id,
                    number_of_elements,
                    reverse_winding,
                    attribute_name,
                    &mut (), // TODO: Use proper builder
                )
            }
            _ => {
                // Generic attribute handling
                self.add_attribute_data_by_types(
                    &(), // TODO: Use proper accessor
                    indices_data,
                    att_id,
                    number_of_elements,
                    reverse_winding,
                    &mut (), // TODO: Use proper builder
                )
            }
        }
    }

    /// Copies the tangent attribute data from accessor and adds it to a Draco mesh.
    /// This function will transform all of the data by transform_matrix and then
    /// normalize before adding the data to the Draco mesh.
    fn add_tangent_to_builder(
        &mut self,
        _accessor: &(),
        _indices_data: &[u32],
        _att_id: i32,
        _number_of_elements: i32,
        _transform_matrix: &[[f64; 4]; 4],
        _reverse_winding: bool,
        _builder: &mut MeshBuilder,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    /// Copies the texture coordinate attribute data from accessor and adds it to
    /// a Draco mesh. This function will flip the data on the horizontal axis as
    /// Draco meshes store the texture coordinates differently than glTF.
    fn add_tex_coord_to_builder(
        &mut self,
        _accessor: &(),
        _indices_data: &[u32],
        _att_id: i32,
        _number_of_elements: i32,
        _reverse_winding: bool,
        _builder: &mut (),
    ) -> Result<(), Err> {
        unimplemented!()
    }

    /// Copies the mesh feature ID attribute data from accessor and adds it to a Draco mesh.
    fn add_feature_id_to_builder(
        &mut self,
        accessor: &gltf::Accessor,
        attribute_name: &str,
        builder: &mut MeshBuilder,
    ) -> Result<(), Err> {
        // Extract feature ID data as u32 values
        if let Some(ref buffers) = self.buffers {
            if let Ok(feature_data) = copy_data_as_uint32(accessor, buffers) {
                // Convert to NdVector<1> for scalar values
                let feature_data_vectors: Vec<_> = feature_data
                    .into_iter()
                    .map(|val| NdVector::from([val]))
                    .collect();
                
                // Add as Custom attribute to the mesh builder
                let attribute_id = builder.add_attribute(
                    feature_data_vectors,
                    AttributeType::Custom,
                    AttributeDomain::Corner,
                    vec![],
                );
                
                // Set the attribute name so it can be used during encoding
                if let Some(attribute) = builder.attributes.last_mut() {
                    attribute.set_name(attribute_name.to_string());
                }
                
                // Map extension attribute indices for later use in mesh features
                if let Some(feature_id_str) = attribute_name.strip_prefix("_FEATURE_ID_") {
                    if let Ok(feature_id) = feature_id_str.parse::<i32>() {
                        self.feature_id_attribute_indices.insert(feature_id, attribute_id.as_usize() as i32);
                    }
                }
                return Ok(());
            }
        }
        
        Err(Err::LoadError(format!("Failed to extract feature ID data for {}", attribute_name)))
    }

    /// Copies the property attribute data from accessor and adds it to a Draco mesh.
    fn add_property_attribute_to_builder(
        &mut self,
        _accessor: &(),
        _indices_data: &[u32],
        _att_id: i32,
        _number_of_elements: i32,
        _reverse_winding: bool,
        _attribute_name: &str,
        _builder: &mut (),
    ) -> Result<(), Err> {
        unimplemented!()
    }

    /// Copies the attribute data from accessor and adds it to a Draco mesh.
    /// This function will transform all of the data by transform_matrix before
    /// adding the data to the Draco mesh.
    fn add_transformed_data_to_builder(
        &mut self,
        _accessor: &(),
        _indices_data: &[u32],
        _att_id: i32,
        _number_of_elements: i32,
        _transform_matrix: &[[f64; 4]; 4],
        _normalize: bool,
        _reverse_winding: bool,
        _builder: &mut (),
    ) -> Result<(), Err> {
        unimplemented!()
    }


    /// Adds the attribute data in accessor to builder for unique attribute att_id.
    fn add_attribute_data_by_types(
        &mut self,
        _accessor: &(),
        _indices_data: &[u32],
        _att_id: i32,
        _number_of_elements: i32,
        _reverse_winding: bool,
        _builder: &mut (),
    ) -> Result<(), Err> {
        unimplemented!()
    }

    /// Adds the textures to owner.
    fn copy_textures(&mut self, owner: &mut Mesh) -> Result<(), Err> {
        if let Some(ref gltf_model) = self.gltf_model {
            for (i, image) in gltf_model.images().enumerate() {
                // Create a new texture
                let mut texture = crate::core::texture::Texture::new();
                
                // Update mapping between glTF images and textures in the texture library.
                self.gltf_image_to_draco_texture.insert(i as i32, ());
                
                // Get source image from glTF image
                let source_image = self.get_source_image(gltf_model, &image)?;
                
                // Set the source image on the texture
                texture.set_source_image(source_image);
                
                // Add texture to the material library
                let texture_library = owner.get_material_library_mut().get_texture_library_mut();
                texture_library.push(texture);
            }
        }
        Ok(())
    }

    /// Creates a SourceImage from a glTF image, handling both embedded and external image files.
    fn get_source_image(&self, _gltf_model: &gltf::Document, image: &gltf::Image) -> Result<crate::core::texture::Image, Err> {
        let mut source_image = crate::core::texture::Image::new();
        
        // Check if the image is embedded in a buffer view or is an external file
        if let Some(source) = Some(image.source()) {
            match source {
                gltf::image::Source::View { view, mime_type } => {
                    // Embedded image - extract data from buffer view
                    let buffer_data = self.copy_data_from_buffer_view(&view)?;
                    *source_image.get_encoded_data_mut() = buffer_data;
                    source_image.set_mime_type(mime_type.to_string());
                }
                gltf::image::Source::Uri { uri, mime_type } => {
                    // External image file
                    if uri.starts_with("data:") {
                        // Data URI - decode base64 data
                        if let Some(comma_pos) = uri.find(',') {
                            let data_part = &uri[comma_pos + 1..];
                            use base64::Engine;
                            if let Ok(decoded_data) = base64::engine::general_purpose::STANDARD.decode(data_part) {
                                *source_image.get_encoded_data_mut() = decoded_data;
                                if let Some(mime_type) = mime_type {
                                    source_image.set_mime_type(mime_type.to_string());
                                }
                            } else {
                                return Err(Err::InvalidInput("Failed to decode base64 data URI".to_string()));
                            }
                        }
                    } else {
                        // External file URI - resolve path relative to glTF file
                        let absolute_path = self.resolve_image_path(uri)?;
                        
                        // Load the image file
                        let image_data = std::fs::read(&absolute_path).map_err(|e| {
                            Err::IoError(format!("Failed to read image file {}: {}", absolute_path, e))
                        })?;
                        
                        *source_image.get_encoded_data_mut() = image_data;
                        source_image.set_filename(absolute_path);
                        
                        // Set MIME type based on file extension if not provided
                        let mime_type = mime_type.unwrap_or_else(|| {
                            let extension = std::path::Path::new(uri)
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .unwrap_or("");
                            match extension.to_lowercase().as_str() {
                                "png" => "image/png",
                                "jpg" | "jpeg" => "image/jpeg", 
                                "webp" => "image/webp",
                                "ktx2" => "image/ktx2",
                                _ => "application/octet-stream",
                            }
                        });
                        source_image.set_mime_type(mime_type.to_string());
                    }
                }
            }
        }
        
        Ok(source_image)
    }

    /// Resolves an image URI to an absolute path relative to the glTF file location.
    fn resolve_image_path(&self, uri: &str) -> Result<String, Err> {
        if let Some(parent_dir) = std::path::Path::new(&self.input_file_name).parent() {
            let image_path = parent_dir.join(uri);
            Ok(image_path.to_string_lossy().to_string())
        } else {
            // If no parent directory, use current directory
            Ok(uri.to_string())
        }
    }

    /// Copies data from a glTF buffer view to a Vec<u8>.
    fn copy_data_from_buffer_view(&self, view: &gltf::buffer::View) -> Result<Vec<u8>, Err> {
        let buffer_data = self.buffers.as_ref()
            .and_then(|buffers| buffers.get(view.buffer().index()))
            .ok_or_else(|| Err::InvalidInput("Buffer not found for buffer view".to_string()))?;
        
        let start = view.offset();
        let end = start + view.length();
        
        if end > buffer_data.len() {
            return Err(Err::InvalidInput("Buffer view extends beyond buffer data".to_string()));
        }
        
        Ok(buffer_data[start..end].to_vec())
    }

    /// Sets extra attribute properties on a constructed draco mesh.
    fn set_attribute_properties_on_draco_mesh(&self, _mesh: &mut Mesh) {
        for (attribute_name, mad) in &self.mesh_attribute_data {
            if let Some(&att_id) = self.attribute_name_to_draco_mesh_attribute_id.get(attribute_name) {
                if att_id == -1 {
                    continue;
                }
                if mad.normalized {
                    // Set normalized property on the attribute if available
                    // For now, we'll skip this as the mesh attribute API may not be fully defined
                    // mesh.attribute_mut(att_id as usize).set_normalized(true);
                }
            }
        }
    }

    /// Adds the materials to mesh.
    fn add_materials_to_draco_mesh(&mut self, _mesh: &mut Mesh) -> Result<(), Err> {
        if let Some(ref gltf_model) = self.gltf_model {
            // Find default material index if it exists
            let _default_material_index = self.gltf_primitive_material_to_draco_material.get(&-1).copied();
            
            // Process each material in the glTF model
            for (input_material_index, _material) in gltf_model.materials().enumerate() {
                let input_material_index = input_material_index as i32;
                
                // Check if this material is actually used
                if let Some(&output_material_index) = self.gltf_primitive_material_to_draco_material.get(&input_material_index) {
                    // For now, just create a basic material entry
                    // In a full implementation, we would:
                    // 1. Create a Material in the mesh's material library
                    // 2. Call add_gltf_material to populate it
                    // 3. Handle texture maps and normal maps
                    let _ = output_material_index; // Suppress unused warning
                }
            }
        }
        Ok(())
    }

    /// Adds the material data for the GeometryAttribute::MATERIAL attribute to the Draco mesh.
    fn add_material_data_to_builder(&mut self, material_value: i32, number_of_elements: i32, builder: &mut MeshBuilder) -> Result<(), Err> {
        let num_materials = self.gltf_primitive_material_to_draco_material.len();
        
        if num_materials < 256 {
            let typed_material_value = material_value as u8;
            self.add_material_data_to_triangle_builder_internal(typed_material_value, number_of_elements, builder)
        } else if num_materials < (1 << 16) {
            let typed_material_value = material_value as u16;
            self.add_material_data_to_triangle_builder_internal(typed_material_value, number_of_elements, builder)
        } else {
            let typed_material_value = material_value as u32;
            self.add_material_data_to_triangle_builder_internal(typed_material_value, number_of_elements, builder)
        }
    }

    /// Adds the material data for the GeometryAttribute::MATERIAL attribute to the triangle builder.
    fn add_material_data_to_triangle_builder_internal<T>(&mut self, material_value: T, number_of_faces: i32, builder: &mut MeshBuilder) -> Result<(), Err> {
        // For now, we'll provide a basic implementation
        // In a full implementation, this would add material data for each face/vertex
        let _ = (material_value, number_of_faces, builder);
        Ok(())
    }

    /// Decode glTF file to scene.
    fn decode_gltf_to_scene(&mut self, scene: &mut Scene) -> Result<(), Err> {
        // Store the scene reference for use by other methods
        // Note: We can't store a mutable reference in self, so we'll pass it through method calls
        
        // Gather statistics about attributes and materials
        self.gather_attribute_and_material_stats()?;
        
        // Add extension attributes to mesh_attribute_data
        self.add_extension_attributes_to_stats()?;

        // Add lights to the scene (skip as per instructions - no animation/skin/point cloud)
        // self.add_lights_to_scene(scene)?;

        // Add material variants names to the scene
        self.add_materials_variants_names_to_scene(scene)?;

        // Add structural metadata to the scene
        self.add_structural_metadata_to_scene(scene)?;
        
        // Add mesh features to the scene
        self.add_mesh_features_to_scene(scene)?;
        
        // Add WebP information to the scene metadata for restoration during encoding
        if let Some(ref webp_info) = self.webp_info {
            scene.metadata_mut().add_entry("webp_info".to_string(), webp_info.clone());
        }

        // Copy textures to the scene
        self.copy_textures_to_scene(scene)?;

        // Process all scene nodes to build the scene graph
        self.decode_scene_nodes(scene)?;

        // Skip animations as per instructions
        // self.add_animations_to_scene(scene)?;

        // Add materials to the scene
        self.add_materials_to_scene(scene)?;

        // Skip skins as per instructions
        // self.add_skins_to_scene(scene)?;

        // Move non-material textures from material texture library to non-material texture library
        Self::move_non_material_textures_from_scene(scene);

        // Add various asset metadata to the scene
        self.add_asset_metadata_to_scene(scene)?;

        Ok(())
    }

    /// Decodes glTF materials variants names into a scene.
    fn add_materials_variants_names_to_scene(&mut self, scene: &mut Scene) -> Result<(), Err> {
        // For now, this is a placeholder since material variants are an advanced feature
        // In a full implementation, this would read material variants from glTF extensions
        let _ = scene;
        Ok(())
    }

    /// Decode extensions on all primitives of all scenes and add their contents to mesh.
    fn add_primitive_extensions_to_draco_mesh(&mut self, mesh: &mut Mesh) -> Result<(), Err> {
        for scene in 0..self.gltf_model.as_ref().unwrap().scenes().len() {
            let scene = self.gltf_model.as_ref().unwrap().scenes().nth(scene).unwrap();
            for i in 0..scene.nodes().len() {
                self.add_primitive_extensions_to_draco_mesh_for_node(i as i32, mesh)?;
            }
        }
        Ok(())
    }

    /// Decode extensions on all primitives of all scenes and add their contents to mesh.
    fn add_primitive_extensions_to_draco_mesh_for_node(&mut self, node_index: i32, mesh: &mut Mesh) -> Result<(), Err> {
        let gltf_model = self.gltf_model.clone();
        let node = gltf_model
            .as_ref()
            .ok_or_else(|| Err::LoadError("No glTF model loaded".into()))?
            .nodes()
            .nth(node_index as usize)
            .ok_or_else(|| Err::LoadError("Node index out of range".into()))?
            .clone(); // TODO: Look into borrowing issues here

        if let Some(gltf_mesh) = node.mesh() {
            let mesh_index = gltf_mesh.index();
            for (primitive_index, primitive) in gltf_mesh.primitives().enumerate() {
                self.add_primitive_extensions_to_draco_mesh_for_primitive(
                    &primitive,
                    mesh,
                    mesh_index,
                    primitive_index,
                )?;
            }
        }

        for child in node.children() {
            self.add_primitive_extensions_to_draco_mesh_for_node(child.index() as i32, mesh)?;
        }
        Ok(())
    }

    /// Decode extensions on primitive and add their contents to mesh.
    fn add_primitive_extensions_to_draco_mesh_for_primitive(&mut self, _primitive: &Primitive, mesh: &mut Mesh, mesh_index: usize, primitive_index: usize) -> Result<(), Err> {
        // Use extension attributes from our parsed JSON instead of relying on gltf crate
        if mesh_index < self.extension_attributes.len() && primitive_index < self.extension_attributes[mesh_index].len() {
            // Clone the extension data to avoid borrowing conflicts
            let ext_attrs = self.extension_attributes[mesh_index][primitive_index].clone();
            
            // Decode mesh feature ID sets if present
            if let Some(mesh_features_ext) = ext_attrs.extensions.get("EXT_mesh_features") {
                self.decode_mesh_features_from_extension_attrs(mesh_features_ext, mesh)?;
            }

            // Decode structural metadata if present
            if let Some(structural_metadata_ext) = ext_attrs.extensions.get("EXT_structural_metadata") {
                self.decode_structural_metadata_from_extension_attrs(structural_metadata_ext, mesh)?;
            }
        }

        Ok(())
    }

    /// Decodes glTF structural metadata from glTF model and adds it to geometry.
    fn add_structural_metadata_to_geometry(&mut self, geometry: &mut ()) -> Result<(), Err> {
        // Placeholder for structural metadata processing
        // In full implementation, this would process glTF extensions for metadata
        let _ = geometry;
        Ok(())
    }

    /// Decodes glTF structural metadata from glTF model and adds it to scene.
    fn add_structural_metadata_to_scene(&mut self, scene: &mut Scene) -> Result<(), Err> {
        // First, try to use document-level structural metadata if available
        if let Some(ref structural_metadata) = self.temp_document_structural_metadata {
            // Store the JSON as a string in the scene's metadata
            scene.metadata_mut().add_entry("structural_metadata_json".to_string(), 
                serde_json::to_string(structural_metadata).unwrap_or_default());
        } else {
            // Fallback: Extract structural metadata from the first primitive's extension attributes
            // (assuming all primitives in the first mesh have the same structural metadata)
            if let Some(first_mesh_extensions) = self.extension_attributes.first() {
                if let Some(first_primitive_extensions) = first_mesh_extensions.first() {
                    if let Some(ref structural_metadata) = first_primitive_extensions.structural_metadata {
                        // Store the JSON as a string in the scene's metadata
                        // Since the scene's structural metadata uses custom types, we'll need to 
                        // create a public method to store this for the encoder to access
                        
                        // For now, we'll store it as a scene metadata entry
                        // This is a workaround since the StructuralMetadata type doesn't support JSON directly
                        scene.metadata_mut().add_entry("structural_metadata_json".to_string(), 
                            serde_json::to_string(structural_metadata).unwrap_or_default());
                    }
                }
            }
        }
        Ok(())
    }

    /// Copy textures to the scene.
    fn copy_textures_to_scene(&mut self, scene: &mut Scene) -> Result<(), Err> {
        if let Some(ref gltf_model) = self.gltf_model {
            
            for (i, image) in gltf_model.images().enumerate() {
                
                // Create a new texture
                let mut texture = crate::core::texture::Texture::new();
                
                // Update mapping between glTF images and textures in the scene
                self.gltf_image_to_draco_texture.insert(i as i32, ());
                
                // Get source image from glTF image
                let source_image = self.get_source_image(gltf_model, &image)?;
                
                // Set the source image on the texture
                texture.set_source_image(source_image);
                
                // Add texture to the scene's material library
                let texture_library = scene.material_library_mut().get_texture_library_mut();
                texture_library.push(texture);
            }
            
            // Now create materials that use these textures
            self.copy_materials_to_scene(scene)?;
        }
        Ok(())
    }
    
    /// Copy materials from glTF to the scene and link them with textures.
    fn copy_materials_to_scene(&mut self, scene: &mut Scene) -> Result<(), Err> {
        if let Some(ref gltf_model) = self.gltf_model {
            
            for (i, gltf_material) in gltf_model.materials().enumerate() {
                
                let mut material = crate::core::material::Material::new();
                material.set_name(gltf_material.name().unwrap_or(&format!("Material_{}", i)).to_string());
                
                // Read PBR material properties
                let pbr = gltf_material.pbr_metallic_roughness();
                material.set_metallic_factor(pbr.metallic_factor());
                material.set_roughness_factor(pbr.roughness_factor());
                
                // Set base color factor
                let base_color = pbr.base_color_factor();
                material.set_color_factor(crate::prelude::NdVector::from([
                    base_color[0],
                    base_color[1], 
                    base_color[2],
                    base_color[3]
                ]));
                
                // Process base color texture
                if let Some(base_color_texture) = gltf_material.pbr_metallic_roughness().base_color_texture() {
                    let texture_index = base_color_texture.texture().index();
                    
                    // Create a texture map for the base color
                    let mut texture_map = crate::core::texture::TextureMap::new();
                    
                    // Get sampler properties from the glTF texture
                    let gltf_texture = base_color_texture.texture();
                    let sampler = gltf_texture.sampler();
                    let (wrapping_mode, min_filter, mag_filter) = {
                        let wrap_s = match sampler.wrap_s() {
                            gltf::texture::WrappingMode::ClampToEdge => crate::core::texture::AxisWrappingMode::ClampToEdge,
                            gltf::texture::WrappingMode::MirroredRepeat => crate::core::texture::AxisWrappingMode::MirroredRepeat,
                            gltf::texture::WrappingMode::Repeat => crate::core::texture::AxisWrappingMode::Repeat,
                        };
                        let wrap_t = match sampler.wrap_t() {
                            gltf::texture::WrappingMode::ClampToEdge => crate::core::texture::AxisWrappingMode::ClampToEdge,
                            gltf::texture::WrappingMode::MirroredRepeat => crate::core::texture::AxisWrappingMode::MirroredRepeat,
                            gltf::texture::WrappingMode::Repeat => crate::core::texture::AxisWrappingMode::Repeat,
                        };
                        let wrapping_mode = crate::core::texture::WrappingMode::new(wrap_s, wrap_t);
                        
                        let min_filter = sampler.min_filter().map(|f| match f {
                            gltf::texture::MinFilter::Nearest => crate::core::texture::FilterType::Nearest,
                            gltf::texture::MinFilter::Linear => crate::core::texture::FilterType::Linear,
                            gltf::texture::MinFilter::NearestMipmapNearest => crate::core::texture::FilterType::NearestMipmapNearest,
                            gltf::texture::MinFilter::LinearMipmapNearest => crate::core::texture::FilterType::LinearMipmapNearest,
                            gltf::texture::MinFilter::NearestMipmapLinear => crate::core::texture::FilterType::NearestMipmapLinear,
                            gltf::texture::MinFilter::LinearMipmapLinear => crate::core::texture::FilterType::LinearMipmapLinear,
                        });
                        
                        let mag_filter = sampler.mag_filter().map(|f| match f {
                            gltf::texture::MagFilter::Nearest => crate::core::texture::FilterType::Nearest,
                            gltf::texture::MagFilter::Linear => crate::core::texture::FilterType::Linear,
                        });
                        
                        (Some(wrapping_mode), min_filter, mag_filter)
                    };
                    
                    texture_map.set_properties(
                        crate::core::texture::Type::Color,
                        wrapping_mode,
                        Some(base_color_texture.tex_coord() as isize),
                        min_filter,
                        mag_filter
                    );
                    
                    // Get the texture from the scene's texture library
                    if let Some(texture) = scene.material_library().get_texture_library().get(texture_index) {
                        texture_map.set_texture(texture.clone());
                        material.set_texture_map(crate::core::texture::Type::Color, texture_map);
                    }
                }
                
                // Process normal texture
                if let Some(normal_texture) = gltf_material.normal_texture() {
                    let texture_index = normal_texture.texture().index();
                    
                    let mut texture_map = crate::core::texture::TextureMap::new();
                    
                    // Get sampler properties from the glTF texture
                    let gltf_texture = normal_texture.texture();
                    let sampler = gltf_texture.sampler();
                    let (wrapping_mode, min_filter, mag_filter) = {
                        let wrap_s = match sampler.wrap_s() {
                            gltf::texture::WrappingMode::ClampToEdge => crate::core::texture::AxisWrappingMode::ClampToEdge,
                            gltf::texture::WrappingMode::MirroredRepeat => crate::core::texture::AxisWrappingMode::MirroredRepeat,
                            gltf::texture::WrappingMode::Repeat => crate::core::texture::AxisWrappingMode::Repeat,
                        };
                        let wrap_t = match sampler.wrap_t() {
                            gltf::texture::WrappingMode::ClampToEdge => crate::core::texture::AxisWrappingMode::ClampToEdge,
                            gltf::texture::WrappingMode::MirroredRepeat => crate::core::texture::AxisWrappingMode::MirroredRepeat,
                            gltf::texture::WrappingMode::Repeat => crate::core::texture::AxisWrappingMode::Repeat,
                        };
                        let wrapping_mode = crate::core::texture::WrappingMode::new(wrap_s, wrap_t);
                        
                        let min_filter = sampler.min_filter().map(|f| match f {
                            gltf::texture::MinFilter::Nearest => crate::core::texture::FilterType::Nearest,
                            gltf::texture::MinFilter::Linear => crate::core::texture::FilterType::Linear,
                            gltf::texture::MinFilter::NearestMipmapNearest => crate::core::texture::FilterType::NearestMipmapNearest,
                            gltf::texture::MinFilter::LinearMipmapNearest => crate::core::texture::FilterType::LinearMipmapNearest,
                            gltf::texture::MinFilter::NearestMipmapLinear => crate::core::texture::FilterType::NearestMipmapLinear,
                            gltf::texture::MinFilter::LinearMipmapLinear => crate::core::texture::FilterType::LinearMipmapLinear,
                        });
                        
                        let mag_filter = sampler.mag_filter().map(|f| match f {
                            gltf::texture::MagFilter::Nearest => crate::core::texture::FilterType::Nearest,
                            gltf::texture::MagFilter::Linear => crate::core::texture::FilterType::Linear,
                        });
                        
                        (Some(wrapping_mode), min_filter, mag_filter)
                    };
                    
                    texture_map.set_properties(
                        crate::core::texture::Type::NormalTangentSpace,
                        wrapping_mode,
                        Some(normal_texture.tex_coord() as isize),
                        min_filter,
                        mag_filter
                    );
                    
                    if let Some(texture) = scene.material_library().get_texture_library().get(texture_index) {
                        texture_map.set_texture(texture.clone());
                        material.set_texture_map(crate::core::texture::Type::NormalTangentSpace, texture_map);
                    }
                }
                
                // Process metallic-roughness texture
                if let Some(mr_texture) = gltf_material.pbr_metallic_roughness().metallic_roughness_texture() {
                    let texture_index = mr_texture.texture().index();
                    
                    let mut texture_map = crate::core::texture::TextureMap::new();
                    
                    // Get sampler properties from the glTF texture
                    let gltf_texture = mr_texture.texture();
                    let sampler = gltf_texture.sampler();
                    let (wrapping_mode, min_filter, mag_filter) = {
                        let wrap_s = match sampler.wrap_s() {
                            gltf::texture::WrappingMode::ClampToEdge => crate::core::texture::AxisWrappingMode::ClampToEdge,
                            gltf::texture::WrappingMode::MirroredRepeat => crate::core::texture::AxisWrappingMode::MirroredRepeat,
                            gltf::texture::WrappingMode::Repeat => crate::core::texture::AxisWrappingMode::Repeat,
                        };
                        let wrap_t = match sampler.wrap_t() {
                            gltf::texture::WrappingMode::ClampToEdge => crate::core::texture::AxisWrappingMode::ClampToEdge,
                            gltf::texture::WrappingMode::MirroredRepeat => crate::core::texture::AxisWrappingMode::MirroredRepeat,
                            gltf::texture::WrappingMode::Repeat => crate::core::texture::AxisWrappingMode::Repeat,
                        };
                        let wrapping_mode = crate::core::texture::WrappingMode::new(wrap_s, wrap_t);
                        
                        let min_filter = sampler.min_filter().map(|f| match f {
                            gltf::texture::MinFilter::Nearest => crate::core::texture::FilterType::Nearest,
                            gltf::texture::MinFilter::Linear => crate::core::texture::FilterType::Linear,
                            gltf::texture::MinFilter::NearestMipmapNearest => crate::core::texture::FilterType::NearestMipmapNearest,
                            gltf::texture::MinFilter::LinearMipmapNearest => crate::core::texture::FilterType::LinearMipmapNearest,
                            gltf::texture::MinFilter::NearestMipmapLinear => crate::core::texture::FilterType::NearestMipmapLinear,
                            gltf::texture::MinFilter::LinearMipmapLinear => crate::core::texture::FilterType::LinearMipmapLinear,
                        });
                        
                        let mag_filter = sampler.mag_filter().map(|f| match f {
                            gltf::texture::MagFilter::Nearest => crate::core::texture::FilterType::Nearest,
                            gltf::texture::MagFilter::Linear => crate::core::texture::FilterType::Linear,
                        });
                        
                        (Some(wrapping_mode), min_filter, mag_filter)
                    };
                    
                    texture_map.set_properties(
                        crate::core::texture::Type::MetallicRoughness,
                        wrapping_mode,
                        Some(mr_texture.tex_coord() as isize),
                        min_filter,
                        mag_filter
                    );
                    
                    if let Some(texture) = scene.material_library().get_texture_library().get(texture_index) {
                        texture_map.set_texture(texture.clone());
                        material.set_texture_map(crate::core::texture::Type::MetallicRoughness, texture_map);
                    }
                }
                
                // Add material to scene
                scene.material_library_mut().add_material(material);
            }
        }
        Ok(())
    }

    /// Process all scene nodes to build the scene graph.
    fn decode_scene_nodes(&mut self, scene: &mut Scene) -> Result<(), Err> {
        // Collect node information first to avoid borrow checker issues
        let mut root_node_indices = Vec::new();
        
        if let Some(ref gltf_model) = self.gltf_model {
            // Process each scene in the glTF model
            for (_scene_index, gltf_scene) in gltf_model.scenes().enumerate() {
                // Collect root nodes from this scene
                for node in gltf_scene.nodes() {
                    root_node_indices.push(node.index());
                }
            }
        }
        
        // Clone extension attributes to avoid borrow checker issues
        let extension_attributes = self.extension_attributes.clone();
        
        // Process each root node. We'll temporarily take ownership of gltf_model and buffers to avoid borrow conflicts
        let gltf_model = self.gltf_model.take();
        let buffers = self.buffers.take();
        if let (Some(gltf_model), Some(buffers)) = (gltf_model, buffers) {
            for node_index in root_node_indices {
                if let Some(node) = gltf_model.nodes().nth(node_index) {
                    let parent_index = usize::MAX; // Use MAX to indicate this is a root node
                    Self::decode_node_for_scene_with_gltf_node_static(&node, parent_index, scene, &buffers, &extension_attributes, &gltf_model)?;
                }
            }
            // Restore the gltf_model and buffers
            self.gltf_model = Some(gltf_model);
            self.buffers = Some(buffers);
        }
        
        Ok(())
    }

    /// Static version of decode_node_for_scene_with_gltf_node to avoid borrow checker issues
    fn decode_node_for_scene_with_gltf_node_static(
        node: &gltf::Node, 
        parent_index: usize, 
        scene: &mut Scene, 
        buffers: &[gltf::buffer::Data], 
        extension_attributes: &[Vec<ExtensionAttributes>],
        gltf_model: &gltf::Document
    ) -> Result<(), Err> {
        // Get node transformation
        let trsm = Self::get_node_transformation_matrix(node);
        
        // Create a scene node with the transformation
        let mut scene_node = crate::core::scene::SceneNode::new();
        scene_node.set_trs_matrix(trsm);
        
        // Set parent if this is not a root node
        if parent_index != usize::MAX && parent_index < scene.num_nodes() {
            scene_node.add_parent_index(parent_index);
        }
        
        // Add the node to the scene
        let actual_node_index = scene.add_node(scene_node);
        
        // If this is a root node, add to root indices
        if parent_index == usize::MAX {
            scene.add_root_node_index(actual_node_index);
        } else {
            // Set parent-child relationship
            if parent_index < scene.num_nodes() {
                if let Some(parent_node) = scene.get_node_mut(parent_index) {
                    parent_node.add_child_index(actual_node_index);
                }
            }
        }
        
        // Process node's mesh if it has one
        if let Some(mesh) = node.mesh() {
            
            // Create a mesh group for this node
            let mesh_group_index = scene.add_mesh_group();
            
            // Set the mesh group index on the node
            if let Some(node_ref) = scene.get_node_mut(actual_node_index) {
                node_ref.set_mesh_group_index(Some(mesh_group_index));
            }
            
            // Process each primitive in the mesh
            for (primitive_index, primitive) in mesh.primitives().enumerate() {
                // Get extension attributes for this mesh and primitive
                let ext_attrs = if mesh.index() < extension_attributes.len() 
                    && primitive_index < extension_attributes[mesh.index()].len() {
                    Some(&extension_attributes[mesh.index()][primitive_index])
                } else {
                    None
                };
                
                // Create a mesh from the primitive data using actual buffer data
                match Self::create_mesh_from_primitive_with_buffers_with_extensions(&primitive, buffers, ext_attrs, gltf_model) {
                    Ok(draco_mesh) => {
                        // Add the mesh to the scene
                        let mesh_index = scene.add_mesh(draco_mesh);
                        
                        // Store mesh features information in scene metadata if present
                        if let Some(ext_attrs) = ext_attrs {
                            if let Some(mesh_features_ext) = ext_attrs.extensions.get("EXT_mesh_features") {
                                if let Ok(mesh_features_json) = serde_json::to_string(mesh_features_ext) {
                                    scene.metadata_mut().add_entry("mesh_features_json".to_string(), mesh_features_json);
                                }
                            }
                        }
                        
                        // Get material index from primitive
                        let material_index = primitive.material().index().map(|i| i as i32).unwrap_or(-1);
                        
                        // Create a mesh instance referencing the actual mesh
                        let mesh_instance = crate::core::scene::MeshInstance::new(mesh_index, material_index);
                        
                        // Add the mesh instance to the mesh group
                        if let Some(mesh_group) = scene.get_mesh_group_mut(mesh_group_index) {
                            mesh_group.add_mesh_instance(mesh_instance);
                        }
                        
                    }
                    Err(_) => {
                        // Create a placeholder mesh instance to maintain structure
                        let mesh_instance = crate::core::scene::MeshInstance::new(0, -1);
                        if let Some(mesh_group) = scene.get_mesh_group_mut(mesh_group_index) {
                            mesh_group.add_mesh_instance(mesh_instance);
                        }
                    }
                }
            }
        }
        
        // Process child nodes recursively
        for child in node.children() {
            Self::decode_node_for_scene_with_gltf_node_static(&child, actual_node_index, scene, buffers, extension_attributes, gltf_model)?;
        }
        
        Ok(())
    }

    /// Extract node transformation matrix from glTF node
    fn get_node_transformation_matrix(node: &gltf::Node) -> TrsMatrix {
        let mut trsm = TrsMatrix::new();
        
        // Extract transformation from glTF node
        match node.transform() {
            gltf::scene::Transform::Matrix { matrix } => {
                // Convert f32 matrix to f64 Matrix4d
                // glTF matrix is column-major, 16-element array [f32; 16]
                let mut data = [[0.0f64; 4]; 4];
                for col in 0..4 {
                    for row in 0..4 {
                        data[row][col] = matrix[col][row] as f64;
                    }
                }
                trsm.set_matrix(crate::core::scene::Matrix4d::new(data));
            }
            gltf::scene::Transform::Decomposed { translation, rotation, scale } => {
                // Set translation
                trsm.set_translation(crate::core::scene::Vector3d::new(
                    translation[0] as f64,
                    translation[1] as f64,
                    translation[2] as f64,
                ));
                
                // Set rotation (quaternion)
                trsm.set_rotation(crate::core::scene::Quaterniond::new(
                    rotation[3] as f64, // w
                    rotation[0] as f64, // x
                    rotation[1] as f64, // y
                    rotation[2] as f64, // z
                ));
                
                // Set scale
                trsm.set_scale(crate::core::scene::Vector3d::new(
                    scale[0] as f64,
                    scale[1] as f64,
                    scale[2] as f64,
                ));
            }
        }
        
        trsm
    }

    /// Create a Draco Mesh from a GLTF primitive (requires buffers for actual data extraction)
    
    /// Extract Vec3 data from GLTF buffer
    fn extract_vec3_from_buffer(accessor: &gltf::Accessor, view: &gltf::buffer::View, buffer: &gltf::buffer::Data) -> Result<Vec<crate::core::shared::NdVector<3, f32>>, Err> {
        let start = view.offset() + accessor.offset();
        let stride = view.stride().unwrap_or(12); // Default to 3 * 4 bytes for Vec3<f32>
        
        let mut result = Vec::new();
        for i in 0..accessor.count() {
            let offset = start + i * stride;
            if offset + 12 <= buffer.len() {
                let x = f32::from_le_bytes([buffer[offset], buffer[offset+1], buffer[offset+2], buffer[offset+3]]);
                let y = f32::from_le_bytes([buffer[offset+4], buffer[offset+5], buffer[offset+6], buffer[offset+7]]);
                let z = f32::from_le_bytes([buffer[offset+8], buffer[offset+9], buffer[offset+10], buffer[offset+11]]);
                result.push(crate::core::shared::NdVector::from([x, y, z]));
            }
        }
        Ok(result)
    }

    /// Extract Vec2 data from GLTF buffer
    fn extract_vec2_from_buffer(accessor: &gltf::Accessor, view: &gltf::buffer::View, buffer: &gltf::buffer::Data) -> Result<Vec<crate::core::shared::NdVector<2, f32>>, Err> {
        let start = view.offset() + accessor.offset();
        let stride = view.stride().unwrap_or(8); // Default to 2 * 4 bytes for Vec2<f32>
        
        let mut result = Vec::new();
        for i in 0..accessor.count() {
            let offset = start + i * stride;
            if offset + 8 <= buffer.len() {
                let x = f32::from_le_bytes([buffer[offset], buffer[offset+1], buffer[offset+2], buffer[offset+3]]);
                let y = f32::from_le_bytes([buffer[offset+4], buffer[offset+5], buffer[offset+6], buffer[offset+7]]);
                result.push(crate::core::shared::NdVector::from([x, y]));
            }
        }
        Ok(result)
    }

    /// Read index value from buffer based on component type
    fn read_index_from_buffer(buffer: &gltf::buffer::Data, offset: usize, component_size: usize) -> Result<usize, Err> {
        if offset + component_size > buffer.len() {
            return Err(Err::LoadError(format!("Index buffer overflow: offset {} + size {} > buffer length {}", offset, component_size, buffer.len())));
        }
        
        let result = match component_size {
            1 => buffer[offset] as usize, // UNSIGNED_BYTE
            2 => u16::from_le_bytes([buffer[offset], buffer[offset+1]]) as usize, // UNSIGNED_SHORT
            4 => u32::from_le_bytes([buffer[offset], buffer[offset+1], buffer[offset+2], buffer[offset+3]]) as usize, // UNSIGNED_INT
            _ => return Err(Err::LoadError(format!("Unsupported index component size: {}", component_size))),
        };
        
        Ok(result)
    }

    /// Create a Draco Mesh from a GLTF primitive with extension attributes support
    fn create_mesh_from_primitive_with_buffers_with_extensions(
        primitive: &gltf::Primitive, 
        buffers: &[gltf::buffer::Data], 
        extension_attributes: Option<&ExtensionAttributes>,
        _gltf_model: &gltf::Document
    ) -> Result<Mesh, Err> {
        use gltf::mesh::Mode;
        use gltf::Semantic;
        
        // Check primitive mode
        if primitive.mode() != Mode::Triangles {
            return Err(Err::LoadError(
                format!("Unsupported primitive mode: {:?}. Only triangles are supported.", primitive.mode())
            ));
        }

        // Create a mesh builder
        let mut mesh_builder = MeshBuilder::new();
        
        // Process indices if available
        let mut face_indices = Vec::new();
        if let Some(indices_accessor) = primitive.indices() {
            // Extract actual indices from the buffer
            if let Some(view) = indices_accessor.view() {
                let buffer = &buffers[view.buffer().index()];
                let start = view.offset() + indices_accessor.offset();
                let stride = indices_accessor.size();
                
                for i in 0..(indices_accessor.count() / 3) {
                    let offset0 = start + i * 3 * stride;
                    let offset1 = start + (i * 3 + 1) * stride;
                    let offset2 = start + (i * 3 + 2) * stride;
                    
                    let idx0 = Self::read_index_from_buffer(buffer, offset0, stride)?;
                    let idx1 = Self::read_index_from_buffer(buffer, offset1, stride)?;
                    let idx2 = Self::read_index_from_buffer(buffer, offset2, stride)?;
                    
                    face_indices.push([idx0, idx1, idx2]);
                }
            }
        }
        
        // Set connectivity
        if !face_indices.is_empty() {
            mesh_builder.set_connectivity_attribute(face_indices);
        }
        
        // Process standard attributes
        // We need to handle attributes carefully because primitive.attributes() may panic
        // on custom attributes when loaded without validation
        let mut attributes: Vec<(Semantic, Accessor<'_>)> = Vec::new();
        
        // Try to get standard attributes by known semantics
        if let Some(positions) = primitive.get(&Semantic::Positions) {
            attributes.push((Semantic::Positions, positions));
        }
        if let Some(normals) = primitive.get(&Semantic::Normals) {
            attributes.push((Semantic::Normals, normals));
        }
        for i in 0..8 {  // Support up to 8 texture coordinate sets
            if let Some(texcoords) = primitive.get(&Semantic::TexCoords(i)) {
                attributes.push((Semantic::TexCoords(i), texcoords));
            }
        }
        for i in 0..8 {  // Support up to 8 color sets
            if let Some(colors) = primitive.get(&Semantic::Colors(i)) {
                attributes.push((Semantic::Colors(i), colors));
            }
        }
        for i in 0..8 {  // Support up to 8 joint/weight sets  
            if let Some(joints) = primitive.get(&Semantic::Joints(i)) {
                attributes.push((Semantic::Joints(i), joints));
            }
            if let Some(weights) = primitive.get(&Semantic::Weights(i)) {
                attributes.push((Semantic::Weights(i), weights));
            }
        }
        if let Some(tangents) = primitive.get(&Semantic::Tangents) {
            attributes.push((Semantic::Tangents, tangents));
        }
        
        // sort attributes by the name
        attributes.sort_by_key(|(semantic, _)| semantic.to_string());

        // compute parents beforehand
        // For now we just let normals and texture coordinates depend on positions
        let mut parents = (0..attributes.len()).map(|_|Vec::new()).collect::<Vec<_>>();
        for (i, semantic) in attributes.iter().map(|x|&x.0).enumerate() {
            if semantic == &Semantic::Positions {
                for (j,semantic) in attributes.iter().map(|x|&x.0).enumerate() {
                    if matches!(semantic, Semantic::Normals | Semantic::TexCoords(_)) {
                        // Find the position attribute index
                        parents[j].push(AttributeId::new(i));
                    }
                }
            }
        }
        let mut parents = parents.into_iter();
        
        for (semantic, accessor) in attributes {
            let parent_deps = parents.next().unwrap_or_default();
            match semantic {
                Semantic::Positions => {
                    if let Some(view) = accessor.view() {
                        let buffer = &buffers[view.buffer().index()];
                        if let Ok(positions) = Self::extract_vec3_from_buffer(&accessor, &view, buffer) {
                            mesh_builder.add_attribute(
                                positions,
                                crate::prelude::AttributeType::Position,
                                crate::core::attribute::AttributeDomain::Position,
                                parent_deps,
                            );
                        }
                    }
                }
                Semantic::Normals => {
                    if let Some(view) = accessor.view() {
                        let buffer = &buffers[view.buffer().index()];
                        if let Ok(normals) = Self::extract_vec3_from_buffer(&accessor, &view, buffer) {
                            mesh_builder.add_attribute(
                                normals,
                                crate::prelude::AttributeType::Normal,
                                crate::core::attribute::AttributeDomain::Corner,
                                parent_deps,
                            );
                        }
                    }
                }
                Semantic::TexCoords(0) => {
                    if let Some(view) = accessor.view() {
                        let buffer = &buffers[view.buffer().index()];
                        if let Ok(texcoords) = Self::extract_vec2_from_buffer(&accessor, &view, buffer) {
                            mesh_builder.add_attribute(
                                texcoords,
                                crate::prelude::AttributeType::TextureCoordinate,
                                crate::core::attribute::AttributeDomain::Corner,
                                parent_deps,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        
        // Process extension attributes if provided
        if let Some(ext_attrs) = extension_attributes {
            // Check if this primitive uses Draco compression
            if ext_attrs.extensions.contains_key("KHR_draco_mesh_compression") {
                return Err(Err::LoadError(
                    "KHR_draco_mesh_compression is not yet supported. \
                    This file contains Draco-compressed mesh data that needs to be decoded first.".to_string()
                ));
            }
            
            // Process EXT_mesh_features extension
            // Note: The actual mesh features JSON is stored in scene metadata by the caller
            if let Some(_mesh_features_ext) = ext_attrs.extensions.get("EXT_mesh_features") {
                // EXT_mesh_features is processed by the caller and stored in scene metadata
                // This ensures the original feature count is preserved for the encoder
            }
            
            for (attr_name, accessor_info) in &ext_attrs.attributes {
                // Use the stored accessor information to extract the data directly from buffers
                if attr_name.starts_with("_FEATURE_ID_") {
                    // Extract feature ID data as u32 values using stored accessor info
                    if let Ok(feature_data) = Self::extract_uint32_from_accessor_info(accessor_info, buffers) {
                        // Convert to NdVector<1> for scalar values
                        let feature_data_vectors: Vec<_> = feature_data
                            .into_iter()
                            .map(|val| NdVector::from([val]))
                            .collect();
                        
                        // Add as Custom attribute to the mesh builder
                        // Custom attributes don't need dependencies for now
                        mesh_builder.add_attribute(
                            feature_data_vectors,
                            AttributeType::Custom,
                            AttributeDomain::Corner,
                            vec![],
                        );
                        
                        // Set the attribute name so it can be used during encoding
                        if let Some(attribute) = mesh_builder.attributes.last_mut() {
                            attribute.set_name(attr_name.to_string());
                        }
                        
                    }
                }
            }
        }
        
        // Build the mesh
        let mesh = mesh_builder.build().map_err(|e| Err::LoadError(format!("Failed to build mesh: {:?}", e)))?;
        
        Ok(mesh)
    }

    /// Extract uint32 data using stored accessor information
    fn extract_uint32_from_accessor_info(accessor_info: &AccessorInfo, buffers: &[gltf::buffer::Data]) -> Result<Vec<u32>, Err> {
        let buffer_data = &buffers[accessor_info.buffer_view_info.buffer];
        let data_start_idx = accessor_info.buffer_view_info.byte_offset + accessor_info.byte_offset;
        let component_size = match accessor_info.component_type {
            5120 => 1, // BYTE
            5121 => 1, // UNSIGNED_BYTE
            5122 => 2, // SHORT
            5123 => 2, // UNSIGNED_SHORT
            5125 => 4, // UNSIGNED_INT
            5126 => 4, // FLOAT
            _ => return Err(Err::LoadError(format!("Unsupported component type: {}", accessor_info.component_type))),
        };
        
        let byte_stride = accessor_info.buffer_view_info.byte_stride.unwrap_or(component_size);
        let num_elements = accessor_info.count;
        
        let mut out = Vec::with_capacity(num_elements);
        
        for i in 0..num_elements {
            let element_offset = data_start_idx + i * byte_stride;
            
            if element_offset + component_size > buffer_data.len() {
                return Err(Err::LoadError("Buffer overflow while reading extension attribute data".to_string()));
            }
            
            let value = match accessor_info.component_type {
                5121 => buffer_data[element_offset] as u32, // UNSIGNED_BYTE
                5123 => { // UNSIGNED_SHORT
                    let bytes = [buffer_data[element_offset], buffer_data[element_offset + 1]];
                    u16::from_le_bytes(bytes) as u32
                }
                5125 => { // UNSIGNED_INT
                    let bytes = [
                        buffer_data[element_offset],
                        buffer_data[element_offset + 1],
                        buffer_data[element_offset + 2],
                        buffer_data[element_offset + 3]
                    ];
                    u32::from_le_bytes(bytes)
                }
                5126 => { // FLOAT
                    let bytes = [
                        buffer_data[element_offset],
                        buffer_data[element_offset + 1],
                        buffer_data[element_offset + 2],
                        buffer_data[element_offset + 3]
                    ];
                    f32::from_le_bytes(bytes) as u32
                }
                _ => return Err(Err::LoadError(format!("Unsupported component type for uint32 conversion: {}", accessor_info.component_type))),
            };
            
            out.push(value);
        }
        Ok(out)
    }

    /// Decodes glTF mesh feature ID sets from extension and adds them to the mesh_features vector.
    fn decode_mesh_features_from_extension(&mut self, extension: &serde_json::Value, _texture_library: &mut crate::core::texture::TextureLibrary, mesh_features: &mut Vec<MeshFeatures>) -> Result<(), Err> {
        // Decode all mesh feature ID sets from JSON like this:
        //   "EXT_mesh_features": {
        //     "featureIds": [
        //       {
        //         "label": "water",
        //         "featureCount": 2,
        //         "propertyTable": 0,
        //         "attribute": 0
        //       },
        //       {
        //         "featureCount": 12,
        //         "nullFeatureId": 100,
        //         "texture" : {
        //           "index": 0,
        //           "texCoord": 0,
        //           "channels": [0, 1, 2, 3]
        //         }
        //       }
        //     ]
        //   }
        
        let feature_ids_array = extension
            .get("featureIds")
            .ok_or_else(|| Err::InvalidInput("Mesh features extension is malformed.".to_string()))?;
        
        let feature_ids = feature_ids_array
            .as_array()
            .ok_or_else(|| Err::InvalidInput("Mesh features array is malformed.".to_string()))?;
        
        for feature_id_obj in feature_ids {
            let obj = feature_id_obj
                .as_object()
                .ok_or_else(|| Err::InvalidInput("Mesh features array entry is malformed.".to_string()))?;
            
            // Create a new feature ID set object
            let mut features = MeshFeatures::new();
            
            // The "featureCount" property is required
            let feature_count = obj
                .get("featureCount")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| Err::InvalidInput("Feature count property is malformed.".to_string()))?;
            features.set_feature_count(feature_count as i32);
            
            // All other properties are optional
            if let Some(null_feature_id) = obj.get("nullFeatureId").and_then(|v| v.as_i64()) {
                features.set_null_feature_id(null_feature_id as i32);
            }
            
            if let Some(label) = obj.get("label").and_then(|v| v.as_str()) {
                features.set_label(label);
            }
            
            if let Some(attribute_index) = obj.get("attribute").and_then(|v| v.as_i64()) {
                // Convert index in feature ID vertex attribute name like _FEATURE_ID_5
                // to attribute index in draco::Mesh.
                if let Some(&att_index) = self.feature_id_attribute_indices.get(&(attribute_index as i32)) {
                    features.set_attribute_index(att_index);
                }
            }
            
            if let Some(texture_obj) = obj.get("texture").and_then(|v| v.as_object()) {
                // Decode texture containing mesh feature IDs
                let texture_map = TextureMap::new();
                
                // Decode the texture itself (implementation would depend on decode_texture method)
                // For now, this is a placeholder - the actual implementation would need to:
                // 1. Extract texture index from texture_obj
                // 2. Create or reference the appropriate texture
                // 3. Set up the texture map properly
                
                // Decode array of texture channel indices
                let channels = if let Some(channels_array) = texture_obj.get("channels").and_then(|v| v.as_array()) {
                    channels_array
                        .iter()
                        .filter_map(|v| v.as_i64().map(|i| i as i32))
                        .collect()
                } else {
                    vec![0] // Default to channel 0
                };
                
                features.set_texture_channels(&channels);
                features.set_texture_map(&texture_map);
            }
            
            if let Some(property_table) = obj.get("propertyTable").and_then(|v| v.as_i64()) {
                features.set_property_table_index(property_table as i32);
            }
            
            mesh_features.push(features);
        }
        
        Ok(())
    }

    /// Decodes glTF structural metadata from extension of a glTF primitive.
    fn decode_structural_metadata_from_extension(&mut self, extension: &GltfValue, property_attributes: &mut Vec<i32>) -> Result<(), Err> {
        // Decode all structural metadata from JSON like this in glTF primitive:
        //   "EXT_structural_metadata": {
        //     "propertyAttributes": [0]
        //   }
        
        let property_attributes_obj = extension
            .get("propertyAttributes")
            .ok_or_else(|| {
                // Extension might contain property textures, support that later
                // For now, just return OK if no propertyAttributes
                Err::InvalidInput("No propertyAttributes in structural metadata extension".to_string())
            });
        
        // If there are no property attributes, that's OK - return success
        if property_attributes_obj.is_err() {
            return Ok(());
        }
        
        let property_attributes_array = property_attributes_obj?
            .as_array()
            .ok_or_else(|| Err::InvalidInput("Property attributes array is malformed.".to_string()))?;
        
        for value in property_attributes_array {
            let index = value
                .as_i64()
                .ok_or_else(|| Err::InvalidInput("Property attributes array entry is malformed.".to_string()))?;
            property_attributes.push(index as i32);
        }
        
        Ok(())
    }

    /// Decodes glTF mesh features from extension attributes and adds them to mesh.
    fn decode_mesh_features_from_extension_attrs(&mut self, extension: &serde_json::Value, _mesh: &mut Mesh) -> Result<(), Err> {
        let mut mesh_features = Vec::new();
        let mut texture_library = crate::core::texture::TextureLibrary::new();
        
        // Decode mesh features from the extension
        self.decode_mesh_features_from_extension(extension, &mut texture_library, &mut mesh_features)?;
        
        // Store the mesh features JSON in temp storage for later use by the encoder
        // This preserves the original feature count and other properties
        if let Ok(mesh_features_json) = serde_json::to_string(extension) {
            self.temp_mesh_features_json = Some(mesh_features_json);
        }
        
        Ok(())
    }

    /// Decodes glTF structural metadata from extension attributes and adds them to mesh.
    fn decode_structural_metadata_from_extension_attrs(&mut self, extension: &serde_json::Value, _mesh: &mut Mesh) -> Result<(), Err> {
        let mut property_attributes = Vec::new();
        
        // Decode structural metadata from the extension
        self.decode_structural_metadata_from_extension(extension, &mut property_attributes)?;
        
        Ok(())
    }

    /// Adds mesh features to the scene.
    fn add_mesh_features_to_scene(&mut self, scene: &mut Scene) -> Result<(), Err> {
        // Store the mesh features JSON in the scene metadata for the encoder to access
        if let Some(ref mesh_features_json) = self.temp_mesh_features_json {
            scene.metadata_mut().add_entry("mesh_features_json".to_string(), mesh_features_json.clone());
        }
        Ok(())
    }

    /// Adds the materials to the scene.
    fn add_materials_to_scene(&mut self, _scene: &mut Scene) -> Result<(), Err> {
        let num_materials = if let Some(ref gltf_model) = self.gltf_model {
            gltf_model.materials().len()
        } else {
            return Ok(());
        };

        for _material_index in 0..num_materials {
            // For now, basic material processing - in full implementation would
            // create scene materials with all properties
        }
        Ok(())
    }

    /// Adds various asset metadata to the scene.
    fn add_asset_metadata_to_scene(&self, scene: &mut Scene) -> Result<(), Err> {
        if let Some(ref _gltf_model) = self.gltf_model {
            // Add glTF asset metadata like generator, version, etc.
            // Note: gltf crate doesn't expose asset() method directly
            // In a full implementation, would extract asset info and add to scene metadata
            let _ = scene;
        }
        Ok(())
    }

    /// Adds various asset metadata to the mesh.
    fn add_asset_metadata_to_mesh(&self, _mesh: &mut Mesh) -> Result<(), Err> {
        // Basic implementation - would add glTF asset metadata to the mesh
        if let Some(ref _gltf_model) = self.gltf_model {
            // let asset = gltf_model.asset();
            // Could add generator, version, etc. as metadata
            // let _ = (asset, mesh);
        }
        Ok(())
    }

    /// Moves non-material textures from material texture library to non-material texture library.
    fn move_non_material_textures_from_mesh(mesh: &mut Mesh) {
        // Basic implementation - would reorganize textures
        let _ = mesh;
    }

    /// Moves non-material textures from material texture library to non-material texture library.
    fn move_non_material_textures_from_scene(scene: &mut Scene) {
        // Basic implementation - would reorganize textures in scene
        // This is needed for textures that aren't part of materials (e.g., feature ID textures)
        let _ = scene;
    }
}


fn copy_data_as_uint32(accessor: &gltf::Accessor, buffers: &[gltf::buffer::Data]) -> Result<Vec<u32>, Err> {
    let view = accessor.view().ok_or_else(|| {
        Err::ConversionError("Error CopyDataAsUint32() accessor has no buffer view.".to_string())
    })?;

    let buffer_data = &buffers[view.buffer().index()];
    let data_start_idx = view.offset() + accessor.offset();
    let byte_stride = view.stride().unwrap_or(accessor.size());
    let num_elements = accessor.count();

    let mut out = Vec::with_capacity(num_elements);

    match accessor.data_type() {
        gltf::accessor::DataType::U8 => {
            for i in 0..num_elements {
                let offset = data_start_idx + i * byte_stride;
                let value = buffer_data[offset] as u32;
                out.push(value);
            }
        }
        gltf::accessor::DataType::U16 => {
            for i in 0..num_elements {
                let offset = data_start_idx + i * byte_stride;
                let bytes = &buffer_data[offset..offset + 2];
                let value = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
                out.push(value);
            }
        }
        gltf::accessor::DataType::U32 => {
            for i in 0..num_elements {
                let offset = data_start_idx + i * byte_stride;
                let bytes = &buffer_data[offset..offset + 4];
                let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                out.push(value);
            }
        }
        _ => {
            return Err(Err::ConversionError(format!(
                "Unsupported data type for indices: {:?}",
                accessor.data_type()
            )));
        }
    }

    Ok(out)
}
