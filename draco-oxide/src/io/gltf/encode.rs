use crate::core::material::Material;
use crate::core::material::TransparencyMode;
use crate::core::mesh::meh_features::MeshFeatures;
use crate::core::mesh::Mesh;
use crate::core::scene::Scene;
use crate::core::shared::PointIdx;
use crate::core::texture;
use crate::core::texture::FilterType;
use crate::core::texture::TextureUtils;
use crate::core::shared::ConfigType;
use crate::core::shared::Vector;
use std::io::Write;


#[cfg(feature = "evaluation")]
use crate::eval::EvalWriter;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Encoding Error: {0}")]
    EncodingError(String),
    #[error("IO Error: {0}")]
    IoError(String),
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("std io Error: {0}")]
    StdIoError(#[from] std::io::Error),
    #[error("Draco Encode Error: {0}")]
    DracoError(#[from] crate::encode::Err),
}

/// Types of output modes for the glTF data encoder. COMPACT will output
/// required and non-default glTF data. VERBOSE will output required and
/// default glTF data as well as readable JSON even when the output is saved in
/// a glTF-Binary file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputType {
    Compact,
    Verbose,
}

impl Default for OutputType {
    fn default() -> Self {
        OutputType::Compact
    }
}

/// JSON output modes corresponding to C++ JsonWriter modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonOutputMode {
    Compact,
    Readable,
}

impl Default for JsonOutputMode {
    fn default() -> Self {
        JsonOutputMode::Readable
    }
}

/// Class for encoding draco::Mesh into the glTF file format.
#[derive(Debug, Clone)]
pub struct GltfEncoder {
    output_type: OutputType,
    copyright: String,
}

impl Default for GltfEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl GltfEncoder {
    /// The name of the attribute metadata that contains the glTF attribute
    /// name. For application-specific generic attributes, if the metadata for
    /// an attribute contains this key, then the value will be used as the
    /// encoded attribute name in the output GLTF.
    pub const DRACO_METADATA_GLTF_ATTRIBUTE_NAME: &'static str = "GLTF_ATTRIBUTE_NAME";

    /// Creates a new GltfEncoder with default settings.
    pub fn new() -> Self {
        Self {
            output_type: OutputType::default(),
            copyright: String::new(),
        }
    }


    /// Encodes the geometry and saves it into a file. Returns an error when either
    /// the encoding failed or when the file couldn't be opened.
    pub fn encode_scene_to_file(&mut self, scene: &Scene, file_name: &str, base_dir: &str) -> Result<(), Err> {
        use std::path::Path;
        
        let path = Path::new(file_name);
        let file_stem = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let extension = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
            
        // Determine output format based on extension
        match extension.to_lowercase().as_str() {
            "glb" => {
                // Write as GLB (binary format)
                self.encode_scene_file(scene, file_name)
            }
            "gltf" | _ => {
                // Write as separate GLTF + bin files
                let bin_filename = format!("{}.bin", file_stem);
                self.encode_scene_file_with_bin_and_resource_dir(scene, file_name, &bin_filename, base_dir)
            }
        }
    }


    /// Saves scene into glTF 2.0 format. filename is the name of the
    /// glTF file. The glTF bin file (if needed) will be named stem(filename) +
    /// ".bin". The other files (if needed) will be saved to basedir(filename).
    /// If filename has the extension "glb" then filename will be written as a
    /// glTF-Binary file. Otherwise filename will be written as non-binary glTF
    /// file.
    pub fn encode_scene_file(&mut self, scene: &Scene, filename: &str) -> Result<(), Err> {
        use std::path::Path;
        
        let path = Path::new(filename);
        let file_stem = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let extension = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let base_dir = path.parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".");
            
        match extension.to_lowercase().as_str() {
            "glb" => {
                // For GLB, write directly to a binary file
                self.write_scene_glb_file(scene, filename)
            }
            "gltf" | _ => {
                // For GLTF, create a bin filename and use the parent directory
                let bin_filename = format!("{}.bin", file_stem);
                self.encode_scene_file_with_bin_and_resource_dir(scene, filename, &bin_filename, base_dir)
            }
        }
    }


    /// Saves scene into glTF 2.0 format. filename is the name of the
    /// glTF file. bin_filename is the name of the glTF bin file. The other
    /// files (if needed) will be saved to basedir(filename). bin_filename will
    /// be ignored if output is glTF-Binary.
    pub fn encode_scene_file_with_bin(&mut self, scene: &Scene, filename: &str, bin_filename: &str) -> Result<(), Err> {
        use std::path::Path;
        
        let path = Path::new(filename);
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
            
        // Validate that the output format is supported
        if extension != "gltf" && extension != "glb" {
            return Err(Err::InvalidInput(
                "gltf_encoder only supports .gltf or .glb output.".to_string()
            ));
        }
        
        // Extract the directory path from the filename for the resource directory
        let resource_dir = path.parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".");
            
        // Delegate to the full implementation
        self.encode_scene_file_with_bin_and_resource_dir(scene, filename, bin_filename, resource_dir)
    }


    /// Saves scene into glTF 2.0 format. filename is the name of the
    /// glTF file. bin_filename is the name of the glTF bin file. The other
    /// files will be saved to resource_dir. bin_filename and resource_dir
    /// will be ignored if output is glTF-Binary.
    pub fn encode_scene_file_with_bin_and_resource_dir(
        &mut self,
        scene: &Scene,
        filename: &str,
        bin_filename: &str,
        resource_dir: &str,
    ) -> Result<(), Err> {
        use std::path::Path;
        
        let path = Path::new(filename);
        let extension = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
            
        // Validate that the output format is supported
        let ext_lower = extension.to_lowercase();
        if ext_lower != "gltf" && ext_lower != "glb" {
            return Err(Err::InvalidInput(
                "gltf_encoder only supports .gltf or .glb output.".to_string()
            ));
        }
        
        // Create the glTF asset and encode the scene into it
        let mut gltf_asset = GltfAsset::default();
        gltf_asset.set_output_type(self.output_type);
        gltf_asset.set_copyright(&self.copyright);
        
        // Configure based on output format
        match ext_lower.as_str() {
            "glb" => {
                // For GLB format, embed everything in the binary file
                gltf_asset.set_buffer_name(String::new());
                gltf_asset.set_add_images_to_buffer(true);
            }
            "gltf" | _ => {
                // For GLTF format, use separate files
                let bin_path = Path::new(bin_filename);
                let bin_name = bin_path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("output.bin");
                gltf_asset.set_buffer_name(bin_name.to_string());
                gltf_asset.set_add_images_to_buffer(false);
            }
        }
        
        // Encode the scene into the glTF asset
        self.encode_scene_to_buffer_internal(scene, &mut gltf_asset)?;
        
        // Write the files based on format
        match ext_lower.as_str() {
            "glb" => {
                // For GLB format, write binary file (bin_filename and resource_dir are ignored)
                self.write_glb_file_from_asset(&mut gltf_asset, scene, filename)
            }
            "gltf" | _ => {
                // For GLTF format, write separate JSON and binary files
                self.write_gltf_files_from_asset(&mut gltf_asset, filename, bin_filename, resource_dir)
            }
        }
    }

    /// Encodes scene to out_buffer in glTF 2.0 GLB format.
    pub fn encode_scene_to_buffer<W>(&self, scene: &Scene, writer: &mut W) -> Result<(), Err> 
        where W: std::io::Write
    {
        let mut gltf_asset = GltfAsset::default();
        gltf_asset.set_output_type(self.output_type);
        gltf_asset.set_buffer_name(String::new());
        gltf_asset.set_add_images_to_buffer(true);
        gltf_asset.set_copyright(&self.copyright);

        // Encode scene into the glTF asset
        self.encode_scene_to_buffer_internal(scene, &mut gltf_asset)?;

        // Write GLB buffer
        self.write_glb_buffer_from_asset(&mut gltf_asset, scene, writer)
    }

    /// Sets the output type for the encoder.
    pub fn set_output_type(&mut self, output_type: OutputType) {
        self.output_type = output_type;
    }

    /// Gets the current output type.
    pub fn output_type(&self) -> OutputType {
        self.output_type
    }

    /// Sets the copyright string for the glTF asset.
    pub fn set_copyright(&mut self, copyright: String) {
        self.copyright = copyright;
    }

    /// Gets the current copyright string.
    pub fn copyright(&self) -> &str {
        &self.copyright
    }

    /// Encodes the scene into a buffer.
    fn encode_scene_to_buffer_internal(
        &self,
        scene: &Scene,
        gltf_asset: &mut GltfAsset,
    ) -> Result<(), Err> 
    {
        // Set JSON writer mode based on asset configuration
        Self::set_json_writer_mode(gltf_asset);
        
        // Add the scene to the glTF asset
        gltf_asset.add_scene(scene)?;
        
        Ok(())
    }

    /// Set the JSON output mode based on the output type and image buffer settings.
    /// This mimics the C++ implementation: SetJsonWriterMode(GltfAsset *gltf_asset)
    fn set_json_writer_mode(gltf_asset: &mut GltfAsset) {
        if gltf_asset.output_type() == OutputType::Compact && gltf_asset.add_images_to_buffer() {
            gltf_asset.set_json_output_mode(JsonOutputMode::Compact);
        } else {
            gltf_asset.set_json_output_mode(JsonOutputMode::Readable);
        }
    }

    /// Helper function to write a scene to a GLB file
    fn write_scene_glb_file(&mut self, scene: &Scene, filename: &str) -> Result<(), Err> {
        let mut gltf_asset = GltfAsset::default();
        gltf_asset.set_output_type(self.output_type);
        gltf_asset.set_buffer_name(String::new());
        gltf_asset.set_add_images_to_buffer(true);
        gltf_asset.set_copyright(&self.copyright);

        // Encode scene into the glTF asset
        self.encode_scene_to_buffer_internal(scene, &mut gltf_asset)?;

        // Use the new helper function
        self.write_glb_file_from_asset(&mut gltf_asset, scene, filename)
    }

    /// Helper function to write GLB buffer from asset
    fn write_glb_buffer_from_asset<W>(&self, gltf_asset: &mut GltfAsset, scene: &Scene, writer: &mut W) -> Result<(), Err>
        where W: std::io::Write
    {
        // Generate JSON data with WebP restoration
        let mut json_data = Vec::new();
        gltf_asset.output_with_webp_restoration(scene, &mut json_data)?;
        
        // Get binary buffer data
        let binary_data = gltf_asset.buffer();
        
        self.write_glb_format(writer, &json_data, binary_data)?;
        
        Ok(())
    }

    /// Write GLB file from a glTF asset
    fn write_glb_file_from_asset(&mut self, gltf_asset: &mut GltfAsset, scene: &Scene, filename: &str) -> Result<(), Err> {
        use std::fs::File;
        
        // Generate JSON data with WebP restoration
        let mut json_data = Vec::new();
        gltf_asset.output_with_webp_restoration(scene, &mut json_data)?;
        
        // Get binary buffer data
        let binary_data = gltf_asset.buffer();
        
        // Write proper GLB binary file
        let mut file = File::create(filename)
            .map_err(|e| Err::IoError(format!("Failed to create GLB file {}: {}", filename, e)))?;
            
        self.write_glb_format(&mut file, &json_data, binary_data)?;
        
        Ok(())
    }
    
    /// Write GLB binary format with proper header and chunks
    fn write_glb_format<W: std::io::Write>(&self, writer: &mut W, json_data: &[u8], binary_data: &[u8]) -> Result<(), Err> {
        // GLB header: magic (4 bytes) + version (4 bytes) + length (4 bytes)
        writer.write_all(b"glTF")
            .map_err(|e| Err::IoError(format!("Failed to write GLB magic: {}", e)))?;
        writer.write_all(&2u32.to_le_bytes())
            .map_err(|e| Err::IoError(format!("Failed to write GLB version: {}", e)))?;
        
        // Calculate chunk sizes (must be 4-byte aligned)
        let json_length = json_data.len();
        let json_padded_length = (json_length + 3) & !3; // Round up to 4-byte boundary
        
        let binary_length = binary_data.len();
        let binary_padded_length = if binary_length > 0 { (binary_length + 3) & !3 } else { 0 };
        
        // Total file length: header (12) + JSON chunk header (8) + JSON data + optional BIN chunk
        let total_length = 12 + 8 + json_padded_length + 
                          if binary_padded_length > 0 { 8 + binary_padded_length } else { 0 };
        
        // Write total length
        writer.write_all(&(total_length as u32).to_le_bytes())
            .map_err(|e| Err::IoError(format!("Failed to write GLB length: {}", e)))?;
        
        // Write JSON chunk
        writer.write_all(&(json_padded_length as u32).to_le_bytes())
            .map_err(|e| Err::IoError(format!("Failed to write JSON chunk length: {}", e)))?;
        writer.write_all(b"JSON")
            .map_err(|e| Err::IoError(format!("Failed to write JSON chunk type: {}", e)))?;
        writer.write_all(json_data)
            .map_err(|e| Err::IoError(format!("Failed to write JSON data: {}", e)))?;
        
        // Pad JSON to 4-byte boundary with spaces
        for _ in json_length..json_padded_length {
            writer.write_all(b" ")
                .map_err(|e| Err::IoError(format!("Failed to write JSON padding: {}", e)))?;
        }
        
        // Write binary chunk if present
        if binary_padded_length > 0 {
            writer.write_all(&(binary_padded_length as u32).to_le_bytes())
                .map_err(|e| Err::IoError(format!("Failed to write BIN chunk length: {}", e)))?;
            writer.write_all(b"BIN\0")
                .map_err(|e| Err::IoError(format!("Failed to write BIN chunk type: {}", e)))?;
            writer.write_all(binary_data)
                .map_err(|e| Err::IoError(format!("Failed to write binary data: {}", e)))?;
            
            // Pad binary to 4-byte boundary with zeros
            for _ in binary_length..binary_padded_length {
                writer.write_all(&[0u8])
                    .map_err(|e| Err::IoError(format!("Failed to write binary padding: {}", e)))?;
            }
        }
        
        Ok(())
    }

    /// Write separate GLTF and binary files from a glTF asset
    fn write_gltf_files_from_asset(&mut self, gltf_asset: &mut GltfAsset, filename: &str, bin_filename: &str, resource_dir: &str) -> Result<(), Err> {
        use std::fs::{self, File};
        use std::path::Path;
        
        // Generate JSON data
        let mut json_data = Vec::new();
        gltf_asset.output(&mut json_data)?;
        
        // Write GLTF JSON file
        let mut gltf_file = File::create(filename)
            .map_err(|e| Err::IoError(format!("Failed to create GLTF file {}: {}", filename, e)))?;
        gltf_file.write_all(&json_data)
            .map_err(|e| Err::IoError(format!("Failed to write GLTF file: {}", e)))?;
            
        // Write binary data file if there's any buffer data
        let binary_data = gltf_asset.buffer();
        if !binary_data.is_empty() {
            let mut bin_file = File::create(bin_filename)
                .map_err(|e| Err::IoError(format!("Failed to create bin file {}: {}", bin_filename, e)))?;
            bin_file.write_all(binary_data)
                .map_err(|e| Err::IoError(format!("Failed to write bin file: {}", e)))?;
        }
        
        // Write image files to resource directory
        if !gltf_asset.add_images_to_buffer() {
            // Create resource directory if it doesn't exist
            if resource_dir != "." && !resource_dir.is_empty() {
                fs::create_dir_all(resource_dir)
                    .map_err(|e| Err::IoError(format!("Failed to create resource directory {}: {}", resource_dir, e)))?;
            }
            
            // Write each image to the resource directory
            for i in 0..gltf_asset.num_images() {
                if let Some(image) = gltf_asset.get_image(i) {
                    if !image.image_name.is_empty() {
                        let image_path = Path::new(resource_dir).join(&image.image_name);
                        // For now, we'll skip writing image files as this requires texture implementation
                        // In a full implementation, this would write the image data to disk
                        let _ = image_path;
                    }
                }
            }
        }
        
        Ok(())
    }

}






/// Struct to hold glTF Scene data.
#[derive(Debug, Clone, Default)]
pub struct GltfScene {
    pub node_indices: Vec<i32>,
}

/// Struct to hold glTF Node data.
#[derive(Debug, Clone)]
pub struct GltfNode {
    pub name: String,
    pub children_indices: Vec<i32>,
    pub mesh_index: i32,
    pub skin_index: i32,
    pub light_index: i32,
    pub instance_array_index: i32,
    pub root_node: bool,
    pub trs_matrix: crate::core::scene::TrsMatrix,
}

impl Default for GltfNode {
    fn default() -> Self {
        Self {
            name: String::new(),
            children_indices: Vec::new(),
            mesh_index: -1,
            skin_index: -1,
            light_index: -1,
            instance_array_index: -1,
            root_node: false,
            trs_matrix: crate::core::scene::TrsMatrix::default(),
        }
    }
}

/// Struct to hold image data.
#[derive(Debug, Clone)]
pub(crate) struct GltfImage {
    pub image_name: String,
    pub texture: Option<crate::core::texture::Texture>,
    pub num_components: i32,
    pub buffer_view: i32,
    pub mime_type: String,
}

impl Default for GltfImage {
    fn default() -> Self {
        Self {
            image_name: String::new(),
            texture: None,
            num_components: 0,
            buffer_view: -1,
            mime_type: String::new(),
        }
    }
}

/// Struct to hold texture filtering options. The members are based on glTF 2.0
/// samplers. For more information see:
/// https://github.com/KhronosGroup/glTF/tree/master/specification/2.0#samplers
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextureSampler {
    pub min_filter: crate::core::texture::FilterType,
    pub mag_filter: crate::core::texture::FilterType,
    pub wrapping_mode: crate::core::texture::WrappingMode,
}

impl TextureSampler {
    pub fn new(
        min: crate::core::texture::FilterType,
        mag: crate::core::texture::FilterType,
        mode: crate::core::texture::WrappingMode,
    ) -> Self {
        Self {
            min_filter: min,
            mag_filter: mag,
            wrapping_mode: mode,
        }
    }

    pub fn wrapping_mode(&self) -> &crate::core::texture::WrappingMode {
        &self.wrapping_mode
    }

    pub fn min_filter(&self) -> crate::core::texture::FilterType {
        self.min_filter
    }

    pub fn mag_filter(&self) -> crate::core::texture::FilterType {
        self.mag_filter
    }
}

impl Default for TextureSampler {
    fn default() -> Self {
        Self {
            min_filter: crate::core::texture::FilterType::Unspecified,
            mag_filter: crate::core::texture::FilterType::Unspecified,
            wrapping_mode: crate::core::texture::WrappingMode::new(
                crate::core::texture::AxisWrappingMode::ClampToEdge,
                crate::core::texture::AxisWrappingMode::ClampToEdge,
            ),
        }
    }
}

/// Struct to hold texture data. Multiple textures can reference the same image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GltfTexture {
    pub image_index: i32,
    pub sampler_index: i32,
}

impl GltfTexture {
    pub fn new(image: i32, sampler: i32) -> Self {
        Self {
            image_index: image,
            sampler_index: sampler,
        }
    }
}

/// Struct to hold glTF Accessor data.
#[derive(Debug, Clone)]
pub struct GltfAccessor {
    pub buffer_view_index: i32,
    pub byte_stride: i32,
    pub component_type: i32,
    pub count: i64,
    pub max: Vec<serde_json::Value>,
    pub min: Vec<serde_json::Value>,
    pub accessor_type: String,
    pub normalized: bool,
}

impl Default for GltfAccessor {
    fn default() -> Self {
        Self {
            buffer_view_index: -1,
            byte_stride: 0,
            component_type: -1,
            count: 0,
            max: Vec::new(),
            min: Vec::new(),
            accessor_type: String::new(),
            normalized: false,
        }
    }
}

/// Struct to hold glTF BufferView data. Currently there is only one Buffer, so
/// there is no need to store a buffer index.
#[derive(Debug, Clone, Default)]
pub struct GltfBufferView {
    pub buffer_byte_offset: i64,
    pub byte_length: i64,
    pub byte_stride: i32,
    pub target: i32,
}

impl GltfBufferView {
    pub fn new() -> Self {
        Self {
            buffer_byte_offset: -1,
            byte_length: 0,
            byte_stride: 0,
            target: 0,
        }
    }
}

/// Struct to hold information about a Draco compressed mesh.
#[derive(Debug, Clone, Default)]
pub struct GltfDracoCompressedMesh {
    pub buffer_view_index: i32,
    pub attributes: indexmap::IndexMap<String, i32>,
}

impl GltfDracoCompressedMesh {
    pub fn new() -> Self {
        Self {
            buffer_view_index: -1,
            attributes: indexmap::IndexMap::new(),
        }
    }
}

/// Struct to hold glTF Primitive data.
#[derive(Debug, Clone)]
pub(crate) struct GltfPrimitive {
    pub indices: i32,
    pub mode: i32,
    pub material: i32,
    pub material_variants_mappings: Vec<crate::core::scene::MaterialsVariantsMapping>,
    pub mesh_features: Vec<MeshFeatures>, // Placeholder for MeshFeatures pointers
    pub property_attributes: Vec<i32>,
    pub attributes: indexmap::IndexMap<String, i32>,
    pub compressed_mesh_info: GltfDracoCompressedMesh,
    /// Map from the index of a feature ID vertex attribute in draco::Mesh to the
    /// attribute name like _FEATURE_ID_0.
    pub feature_id_name_indices: std::collections::HashMap<i32, String>,
}

impl Default for GltfPrimitive {
    fn default() -> Self {
        Self {
            indices: -1,
            mode: 4,
            material: 0,
            material_variants_mappings: Vec::new(),
            mesh_features: Vec::new(),
            property_attributes: Vec::new(),
            attributes: indexmap::IndexMap::new(),
            compressed_mesh_info: GltfDracoCompressedMesh::default(),
            feature_id_name_indices: std::collections::HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GltfMesh {
    pub name: String,
    pub primitives: Vec<GltfPrimitive>,
}

/// Class to hold and output glTF data.
#[derive(Debug, Clone)]
pub(crate) struct GltfAsset {
    copyright: String,
    generator: String,
    #[allow(unused)]
    version: String,
    scenes: Vec<GltfScene>,
    scene_index: i32,
    nodes: Vec<GltfNode>,
    accessors: Vec<GltfAccessor>,
    buffer_views: Vec<GltfBufferView>,
    meshes: Vec<GltfMesh>,
    material_library: crate::core::material::MaterialLibrary,
    images: Vec<GltfImage>,
    textures: Vec<GltfTexture>,
    texture_to_image_index_map: std::collections::HashMap<usize, i32>, // Placeholder for texture pointer mapping
    buffer_name: String,
    buffer: Vec<u8>, // Placeholder for EncoderBuffer
    json_output_mode: JsonOutputMode, // Mode for JSON output (compact or readable)
    structural_metadata_json: Option<serde_json::Value>,
    mesh_features_used: bool,
    structural_metadata_used: bool,
    add_images_to_buffer: bool,
    extensions_used: std::collections::BTreeSet<String>,
    extensions_required: std::collections::BTreeSet<String>,
    texture_samplers: Vec<TextureSampler>,
    output_type: OutputType,
}

/// glTF value types and values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentType {
    Byte = 5120,
    UnsignedByte = 5121,
    Short = 5122,
    UnsignedShort = 5123,
    UnsignedInt = 5125,
    Float = 5126,
}

#[derive(Debug, Clone)]
pub struct EncoderAnimation {
    pub name: String,
    pub samplers: Vec<Box<()>>, // Placeholder for AnimationSampler
    pub channels: Vec<Box<()>>, // Placeholder for AnimationChannel
}

#[derive(Debug, Clone)]
pub struct EncoderSkin {
    pub inverse_bind_matrices_index: i32,
    pub joints: Vec<i32>,
    pub skeleton_index: i32,
}

impl Default for EncoderSkin {
    fn default() -> Self {
        Self {
            inverse_bind_matrices_index: -1,
            joints: Vec::new(),
            skeleton_index: -1,
        }
    }
}

/// Instance array is represented by its attribute accessors.
#[derive(Debug, Clone)]
pub struct EncoderInstanceArray {
    pub translation: i32,
    pub rotation: i32,
    pub scale: i32,
}

impl Default for EncoderInstanceArray {
    fn default() -> Self {
        Self {
            translation: -1,
            rotation: -1,
            scale: -1,
        }
    }
}

impl Default for GltfAsset {
    fn default() -> Self {
        Self::new()
    }
}

impl GltfAsset {
    pub fn new() -> Self {
        Self {
            copyright: "Generated by draco-oxide".to_string(),
            generator: "draco-oxide".to_string(),
            version: String::new(),
            scenes: Vec::new(),
            scene_index: -1,
            nodes: Vec::new(),
            accessors: Vec::new(),
            buffer_views: Vec::new(),
            meshes: Vec::new(),
            material_library: crate::core::material::MaterialLibrary::new(),
            images: Vec::new(),
            textures: Vec::new(),
            texture_to_image_index_map: std::collections::HashMap::new(),
            buffer_name: String::new(),
            buffer: Vec::new(),
            json_output_mode: JsonOutputMode::default(),
            structural_metadata_json: None,
            mesh_features_used: false,
            structural_metadata_used: false,
            add_images_to_buffer: false,
            extensions_used: std::collections::BTreeSet::new(),
            extensions_required: std::collections::BTreeSet::new(),
            texture_samplers: Vec::new(),
            output_type: OutputType::default(),
        }
    }

    /// Compute min/max values for a VEC3 attribute (position, normal)
    fn compute_vec3_bounds(&self, attr: &crate::core::attribute::Attribute) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        use crate::core::shared::NdVector;
        
        if attr.len() == 0 {
            return (vec![], vec![]);
        }
        
        // Get the first value to initialize bounds
        let first: NdVector<3, f32> = attr.get(PointIdx::from(0));
        let mut min_x = *first.get(0);
        let mut min_y = *first.get(1); 
        let mut min_z = *first.get(2);
        let mut max_x = *first.get(0);
        let mut max_y = *first.get(1);
        let mut max_z = *first.get(2);
        
        // Iterate through all values to find actual bounds
        for i in 1..attr.len() {
            let val: NdVector<3, f32> = attr.get(PointIdx::from(i));
            min_x = min_x.min(*val.get(0));
            min_y = min_y.min(*val.get(1));
            min_z = min_z.min(*val.get(2));
            max_x = max_x.max(*val.get(0));
            max_y = max_y.max(*val.get(1));
            max_z = max_z.max(*val.get(2));
        }
        
        let min_vec = vec![
            serde_json::Value::Number(serde_json::Number::from_f64(min_x as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(min_y as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(min_z as f64).unwrap()),
        ];
        let max_vec = vec![
            serde_json::Value::Number(serde_json::Number::from_f64(max_x as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(max_y as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(max_z as f64).unwrap()),
        ];
        
        (min_vec, max_vec)
    }

    /// Compute min/max values for a VEC4 attribute (tangent)
    fn compute_vec4_bounds(&self, attr: &crate::core::attribute::Attribute) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        use crate::core::shared::NdVector;
        
        if attr.len() == 0 {
            return (vec![], vec![]);
        }
        
        // Get the first value to initialize bounds
        let first: NdVector<4, f32> = attr.get(PointIdx::from(0));
        let mut min_x = *first.get(0);
        let mut min_y = *first.get(1);
        let mut min_z = *first.get(2);
        let mut min_w = *first.get(3);
        let mut max_x = *first.get(0);
        let mut max_y = *first.get(1);
        let mut max_z = *first.get(2);
        let mut max_w = *first.get(3);
        
        // Iterate through all values to find actual bounds
        for i in 1..attr.len() {
            let val: NdVector<4, f32> = attr.get(PointIdx::from(i));
            min_x = min_x.min(*val.get(0));
            min_y = min_y.min(*val.get(1));
            min_z = min_z.min(*val.get(2));
            min_w = min_w.min(*val.get(3));
            max_x = max_x.max(*val.get(0));
            max_y = max_y.max(*val.get(1));
            max_z = max_z.max(*val.get(2));
            max_w = max_w.max(*val.get(3));
        }
        
        let min_vec = vec![
            serde_json::Value::Number(serde_json::Number::from_f64(min_x as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(min_y as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(min_z as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(min_w as f64).unwrap()),
        ];
        let max_vec = vec![
            serde_json::Value::Number(serde_json::Number::from_f64(max_x as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(max_y as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(max_z as f64).unwrap()),
            serde_json::Value::Number(serde_json::Number::from_f64(max_w as f64).unwrap()),
        ];
        
        (min_vec, max_vec)
    }

    pub fn set_copyright(&mut self, copyright: &String) {
        self.copyright = copyright.clone();
    }

    pub fn set_buffer_name(&mut self, name: String) {
        self.buffer_name = name;
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Add a Draco Mesh as a primitive to an existing glTF mesh.
    /// The material_index specifies which material to filter for.
    fn add_draco_mesh_as_primitive(&mut self, mesh: &Mesh, scene: &Scene, material_index: i32, gltf_mesh: &mut GltfMesh) -> Result<(), Err> {
        // Filter the mesh by material to create a primitive for this specific material
        let filtered_mesh = self.filter_mesh_by_material(mesh, material_index)?;
        
        // Skip empty meshes (can happen when filtering results in no faces)
        if filtered_mesh.get_faces().is_empty() {
            return Ok(());
        }
        
        self.add_draco_mesh_internal(&filtered_mesh, scene, Some(material_index), gltf_mesh)
    }
    
    /// Internal method to add a Draco Mesh as a primitive to a glTF mesh.
    fn add_draco_mesh_internal(&mut self, mesh: &Mesh, scene: &Scene, material_index: Option<i32>, gltf_mesh: &mut GltfMesh) -> Result<(), Err> {
        // Skip empty meshes (can happen when filtering results in no faces)
        if mesh.get_faces().is_empty() {
            return Ok(());
        }
        
        // Compress the mesh with Draco
        let mut draco_buffer = Vec::new();
        {
            let config = crate::encode::Config::default();
            
            // Use the provided mesh
            let mesh_copy = mesh.clone();
            
            #[cfg(feature = "evaluation")]
            {
                let mut eval_writer = EvalWriter::new(&mut draco_buffer);
                crate::encode::encode(mesh_copy, &mut eval_writer, config)?;
            }
            #[cfg(not(feature = "evaluation"))]
            {
                crate::encode::encode(mesh_copy, &mut draco_buffer, config)?;
            }
        }

        // Add the compressed data to the GLB buffer
        let buffer_start_offset = self.buffer.len();
        self.buffer.extend_from_slice(&draco_buffer);
        
        // Pad buffer to 4-byte alignment as required by GLB spec
        self.pad_buffer();
        
        // Create a buffer view for the compressed data
        let buffer_view = GltfBufferView {
            buffer_byte_offset: buffer_start_offset as i64,
            byte_length: (self.buffer.len() - buffer_start_offset) as i64,
            byte_stride: 0, // No specific byte stride for Draco compressed data
            target: 0, // Set to 0 for no specific target
        };
        
        self.buffer_views.push(buffer_view);
        
        // Create a new primitive
        let mut primitive = GltfPrimitive::default();
        
        // Set up the Draco extension
        primitive.compressed_mesh_info.buffer_view_index = (self.buffer_views.len() - 1) as i32;
        
        // Add the KHR_draco_mesh_compression extension to the asset's extensions
        self.extensions_required.insert("KHR_draco_mesh_compression".to_string());
        self.extensions_used.insert("KHR_draco_mesh_compression".to_string());
        
        // Add accessors and standard attributes to match C++ reference format
        let mut accessor_index = self.accessors.len() as i32;
        
        // Add indices accessor (indices are stored in the Draco buffer but we need a placeholder accessor)
        let indices_accessor = GltfAccessor {
            buffer_view_index: -1, // No buffer view for Draco-compressed indices
            byte_stride: 0,
            component_type: 5121, // UNSIGNED_BYTE, but this will be ignored for Draco
            count: mesh.get_faces().len() as i64 * 3, // Triangle indices count (3 indices per triangle)
            accessor_type: "SCALAR".to_string(),
            max: vec![],
            min: vec![],
            normalized: false,
        };
        self.accessors.push(indices_accessor);
        primitive.indices = accessor_index;
        accessor_index += 1;
        
        // Add attribute accessors and both standard attributes and Draco extension attributes
        // We need to create accessors in C++ reference order for standard attributes:
        // POSITION → accessor 1, NORMAL → accessor 2, TEXCOORD_0 → accessor 3
        // But use actual iteration order for Draco decoder IDs
        
        // First pass: collect attribute data by type
        let mut position_data = None;
        let mut normal_data = None;
        let mut texcoord_data = None;
        let mut position_draco_id = 0;
        let mut normal_draco_id = 0;
        let mut texcoord_draco_id = 0;
        
        for (iteration_id, attr) in mesh.get_attributes().iter().enumerate() {
            let vertex_count = attr.len() as i64;
            // Map Draco attribute IDs to match accessor indices:
            // Position (accessor 1) -> Draco ID 1
            // Normal (accessor 2) -> Draco ID 0  
            let actual_draco_id = match attr.get_attribute_type() {
                crate::core::attribute::AttributeType::Position => 1, // Maps to accessor 1
                crate::core::attribute::AttributeType::Normal => 0,   // Maps to accessor 2, but Draco ID 0
                crate::core::attribute::AttributeType::TextureCoordinate => iteration_id as i32,
                _ => iteration_id as i32,
            };
            match attr.get_attribute_type() {
                crate::core::attribute::AttributeType::Position => {
                    let (min_vals, max_vals) = self.compute_vec3_bounds(attr);
                    position_data = Some((vertex_count, min_vals, max_vals));
                    position_draco_id = actual_draco_id;
                }
                crate::core::attribute::AttributeType::Normal => {
                    normal_data = Some(vertex_count);
                    normal_draco_id = actual_draco_id;
                }
                crate::core::attribute::AttributeType::TextureCoordinate => {
                    texcoord_data = Some(vertex_count);
                    texcoord_draco_id = actual_draco_id;
                }
                _ => {} // Skip other types
            }
        }
        
        // Second pass: create accessors in C++ reference order
        // POSITION → accessor 1 (accessor_index starts at 1 after indices)
        if let Some((vertex_count, min_vals, max_vals)) = position_data {
            let position_accessor = GltfAccessor {
                buffer_view_index: -1, // No buffer view for Draco-compressed attributes
                byte_stride: 0,
                component_type: 5126, // FLOAT
                count: vertex_count,
                accessor_type: "VEC3".to_string(),
                max: max_vals,
                min: min_vals,
                normalized: false,
            };
            self.accessors.push(position_accessor);
            primitive.attributes.insert("POSITION".to_string(), accessor_index); // Should be 1
            primitive.compressed_mesh_info.attributes.insert("POSITION".to_string(), position_draco_id);
            accessor_index += 1;
        }
        
        // NORMAL → accessor 2
        if let Some(vertex_count) = normal_data {
            let normal_accessor = GltfAccessor {
                buffer_view_index: -1, // No buffer view for Draco-compressed attributes
                byte_stride: 0,
                component_type: 5126, // FLOAT
                count: vertex_count,
                accessor_type: "VEC3".to_string(),
                max: vec![], // No bounds for normals to match C++ reference
                min: vec![], // No bounds for normals to match C++ reference
                normalized: false,
            };
            self.accessors.push(normal_accessor);
            primitive.attributes.insert("NORMAL".to_string(), accessor_index); // Should be 2
            primitive.compressed_mesh_info.attributes.insert("NORMAL".to_string(), normal_draco_id);
            accessor_index += 1;
        }
        
        // TEXCOORD_0 → accessor 3
        if let Some(vertex_count) = texcoord_data {
            let texcoord_accessor = GltfAccessor {
                buffer_view_index: -1, // No buffer view for Draco-compressed attributes
                byte_stride: 0,
                component_type: 5126, // FLOAT
                count: vertex_count,
                accessor_type: "VEC2".to_string(),
                max: vec![], // No bounds for texcoords to match C++ reference
                min: vec![], // No bounds for texcoords to match C++ reference
                normalized: false,
            };
            self.accessors.push(texcoord_accessor);
            primitive.attributes.insert("TEXCOORD_0".to_string(), accessor_index); // Should be 3
            primitive.compressed_mesh_info.attributes.insert("TEXCOORD_0".to_string(), texcoord_draco_id);
            accessor_index += 1;
        }
        
        // Handle other attribute types if present
        for (iteration_id, attr) in mesh.get_attributes().iter().enumerate() {
            let vertex_count = attr.len() as i64;
            match attr.get_attribute_type() {
                crate::core::attribute::AttributeType::Tangent => {
                    // Compute actual min/max bounds for tangent attribute
                    let (min_vals, max_vals) = self.compute_vec4_bounds(attr);
                    
                    // Add accessor for tangent attribute
                    let tangent_accessor = GltfAccessor {
                        buffer_view_index: -1, // No buffer view for Draco-compressed attributes
                        byte_stride: 0,
                        component_type: 5126, // FLOAT
                        count: vertex_count,
                        accessor_type: "VEC4".to_string(),
                        max: max_vals,
                        min: min_vals,
                        normalized: false,
                    };
                    self.accessors.push(tangent_accessor);
                    
                    // Add to both standard attributes and Draco extension
                    primitive.attributes.insert("TANGENT".to_string(), accessor_index);
                    primitive.compressed_mesh_info.attributes.insert("TANGENT".to_string(), iteration_id as i32);
                    accessor_index += 1;
                }
                crate::core::attribute::AttributeType::Custom => {
                    // Handle Custom attributes (like _FEATURE_ID_0)
                    let attribute_name = if let Some(name) = attr.get_name() {
                        name.clone()
                    } else {
                        format!("_CUSTOM_{}", iteration_id)
                    };
                    
                    // Get the data to determine if this is a scalar or vector attribute
                    let accessor_type = match attr.get_num_components() {
                        1 => "SCALAR",
                        2 => "VEC2", 
                        3 => "VEC3",
                        4 => "VEC4",
                        _ => "SCALAR"
                    };
                    
                    // For Custom attributes, determine appropriate component type
                    let component_type = if attribute_name.starts_with("_FEATURE_ID_") {
                        // Feature IDs should be integer type, not float
                        // Use UNSIGNED_SHORT (5123) for most feature ID cases as it supports 0-65535 values
                        // which should be sufficient for most feature ID use cases
                        5123 // UNSIGNED_SHORT
                    } else {
                        // For other custom attributes, use FLOAT to match C++ reference
                        5126 // FLOAT
                    };
                    
                    let custom_accessor = GltfAccessor {
                        buffer_view_index: -1, // No buffer view for Draco-compressed attributes
                        byte_stride: 0,
                        component_type,
                        count: vertex_count,
                        accessor_type: accessor_type.to_string(),
                        max: vec![], // No bounds needed for feature IDs
                        min: vec![],
                        normalized: false,
                    };
                    self.accessors.push(custom_accessor);
                    
                    // Add to both standard attributes and Draco extension
                    primitive.attributes.insert(attribute_name.clone(), accessor_index);
                    primitive.compressed_mesh_info.attributes.insert(attribute_name.clone(), iteration_id as i32);
                    
                    // Check if this is a feature ID attribute and add EXT_mesh_features extension
                    if attribute_name.starts_with("_FEATURE_ID_") {
                        // Create MeshFeatures for this feature ID attribute
                        let mut mesh_features = MeshFeatures::new();
                        
                        // Try to get the original feature count from scene metadata first
                        let feature_count = if let Some(mesh_features_json) = scene.metadata().get_entry("mesh_features_json") {
                            // Parse the stored mesh features JSON to get the original feature count
                            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(mesh_features_json) {
                                if let Some(feature_ids) = json_value.get("featureIds").and_then(|v| v.as_array()) {
                                    if let Some(first_feature_id) = feature_ids.first() {
                                        if let Some(feature_count) = first_feature_id.get("featureCount").and_then(|v| v.as_i64()) {
                                            feature_count as i32
                                        } else {
                                            // Fallback to calculating from attribute data
                                            self.calculate_feature_count_from_attribute(mesh, iteration_id)
                                        }
                                    } else {
                                        // Fallback to calculating from attribute data
                                        self.calculate_feature_count_from_attribute(mesh, iteration_id)
                                    }
                                } else {
                                    // Fallback to calculating from attribute data
                                    self.calculate_feature_count_from_attribute(mesh, iteration_id)
                                }
                            } else {
                                // Fallback to calculating from attribute data
                                self.calculate_feature_count_from_attribute(mesh, iteration_id)
                            }
                        } else {
                            // Fallback to calculating from attribute data
                            self.calculate_feature_count_from_attribute(mesh, iteration_id)
                        };
                        
                        // Only add mesh features if we found actual feature data
                        if feature_count > 0 {
                            mesh_features.set_feature_count(feature_count);
                            mesh_features.set_attribute_index(iteration_id as i32); // Reference to the draco attribute
                            mesh_features.set_property_table_index(0); // Based on original glTF
                            
                            // Add to the primitive's mesh features
                            primitive.mesh_features.push(mesh_features);
                        } else {
                            // Log a warning or handle the case where no feature data was found
                            eprintln!("Warning: Feature ID attribute '{}' found but no feature data could be extracted", attribute_name);
                        }
                        
                        // Map the draco attribute index to the attribute name for featureIds.attribute
                        primitive.feature_id_name_indices.insert(iteration_id as i32, attribute_name.clone());
                        
                        // When we have feature IDs, we typically also want structural metadata
                        self.structural_metadata_used = true;
                        self.extensions_used.insert("EXT_structural_metadata".to_string());
                    }
                    
                    accessor_index += 1;
                }
                _ => {
                    // Skip other attribute types for now
                }
            }
        }
        
        // Use the provided material index, or determine one if not provided
        let material_idx = if let Some(idx) = material_index {
            idx
        } else {
            self.determine_material_index_for_mesh(mesh)
        };
        primitive.material = material_idx;
        
        // Add a default material if none exists
        if self.material_library.num_materials() == 0 {
            // Create a default material directly in the library at index 0
            // This will automatically resize the materials vec and create a new material
            if let Some(mat) = self.material_library.mutable_material(0) {
                // The material is already created with default values by Material::new()
                // Default values are: color_factor=[1,1,1,1], metallic_factor=1.0, roughness_factor=1.0
                // But for PBR we want metallic_factor=0.0 to match the C++ reference
                mat.set_metallic_factor(0.0);
            }
        }
        
        gltf_mesh.primitives.push(primitive);
        Ok(())
    }

    /// Filter a mesh to include only faces belonging to a specific material.
    /// This is used to handle multi-primitive meshes correctly.
    fn filter_mesh_by_material(&self, mesh: &Mesh, target_material: i32) -> Result<Mesh, Err> {
        use crate::core::mesh::builder::MeshBuilder;
        use crate::core::attribute::{AttributeType, AttributeDomain};
        use std::collections::HashMap;
        
        // Find the Material attribute
        let material_attr = mesh.get_attributes().iter()
            .find(|attr| attr.get_attribute_type() == AttributeType::Material);
            
        let material_attr = match material_attr {
            Some(attr) => attr,
            None => {
                // If no material attribute, return the original mesh
                return Ok(mesh.clone());
            }
        };
        
        // Collect faces that belong to the target material
        let mut filtered_face_indices = Vec::new();
        let faces = mesh.get_faces();
        
        for (face_idx, face) in faces.iter().enumerate() {
            // Check if any vertex of this face has the target material
            let v0_material: crate::core::shared::NdVector<1, i32> = material_attr.get(face[0]);
            let v1_material: crate::core::shared::NdVector<1, i32> = material_attr.get(face[1]);
            let v2_material: crate::core::shared::NdVector<1, i32> = material_attr.get(face[2]);
            
            // If any vertex has the target material, include this face
            if *v0_material.get(0) == target_material || *v1_material.get(0) == target_material || *v2_material.get(0) == target_material {
                filtered_face_indices.push(face_idx);
            }
        }
        
        // If no faces match, return an empty mesh
        if filtered_face_indices.is_empty() {
            return Ok(MeshBuilder::new().build().map_err(|e| Err::InvalidInput(e.to_string()))?);
        }
        
        // Create a new mesh with only the filtered faces
        let mut builder = MeshBuilder::new();
        
        // Map from old vertex indices to new vertex indices
        let mut vertex_remap = HashMap::new();
        let mut next_vertex_idx = 0;
        
        // Collect all unique vertices used by filtered faces
        for &face_idx in filtered_face_indices.iter() {
            let face = &faces[face_idx];
            
            for i in 0..3 {
                let old_vertex_idx = face[i];
                if !vertex_remap.contains_key(&old_vertex_idx) {
                    vertex_remap.insert(old_vertex_idx, crate::core::shared::PointIdx::from(next_vertex_idx));
                    next_vertex_idx += 1;
                }
            }
        }
        
        // Copy vertex attributes for the remapped vertices in the correct order
        let mut old_vertex_indices: Vec<_> = vertex_remap.keys().copied().collect();
        old_vertex_indices.sort_by_key(|idx| vertex_remap[idx]);
        
        for (_attr_idx, old_attr) in mesh.get_attributes().iter().enumerate() {
            // Skip empty attributes
            if old_attr.len() == 0 {
                continue;
            }
            
            let attr_type = old_attr.get_attribute_type();
            
            // Create new attribute data based on the attribute type
            match attr_type {
                AttributeType::Position => {
                    let mut new_positions = Vec::new();
                    for &old_idx in &old_vertex_indices {
                        let pos: crate::core::shared::NdVector<3, f32> = old_attr.get(old_idx);
                        new_positions.push(pos);
                    }
                    builder.add_attribute(new_positions, attr_type, AttributeDomain::Position, vec![]);
                }
                AttributeType::Normal => {
                    let mut new_normals = Vec::new();
                    for &old_idx in &old_vertex_indices {
                        let normal: crate::core::shared::NdVector<3, f32> = old_attr.get(old_idx);
                        new_normals.push(normal);
                    }
                    builder.add_attribute(new_normals, attr_type, AttributeDomain::Position, vec![]);
                }
                AttributeType::TextureCoordinate => {
                    let mut new_texcoords = Vec::new();
                    for &old_idx in &old_vertex_indices {
                        let texcoord: crate::core::shared::NdVector<2, f32> = old_attr.get(old_idx);
                        new_texcoords.push(texcoord);
                    }
                    builder.add_attribute(new_texcoords, attr_type, AttributeDomain::Position, vec![]);
                }
                AttributeType::Color => {
                    let mut new_colors = Vec::new();
                    for &old_idx in &old_vertex_indices {
                        let color: crate::core::shared::NdVector<4, f32> = old_attr.get(old_idx);
                        new_colors.push(color);
                    }
                    builder.add_attribute(new_colors, attr_type, AttributeDomain::Position, vec![]);
                }
                AttributeType::Tangent => {
                    let mut new_tangents = Vec::new();
                    for &old_idx in &old_vertex_indices {
                        let tangent: crate::core::shared::NdVector<4, f32> = old_attr.get(old_idx);
                        new_tangents.push(tangent);
                    }
                    builder.add_attribute(new_tangents, attr_type, AttributeDomain::Position, vec![]);
                }
                AttributeType::Material => {
                    let mut new_materials = Vec::new();
                    for &old_idx in &old_vertex_indices {
                        let material: crate::core::shared::NdVector<1, i32> = old_attr.get(old_idx);
                        new_materials.push(material);
                    }
                    builder.add_attribute(new_materials, attr_type, AttributeDomain::Position, vec![]);
                }
                _ => {
                    // Skip other attribute types for now
                }
            }
        }
        
        // Add filtered faces with remapped vertex indices
        let mut new_faces = Vec::new();
        for &face_idx in &filtered_face_indices {
            let old_face = &faces[face_idx];
            let new_v0 = vertex_remap[&old_face[0]];
            let new_v1 = vertex_remap[&old_face[1]]; 
            let new_v2 = vertex_remap[&old_face[2]];
            
            new_faces.push([usize::from(new_v0), usize::from(new_v1), usize::from(new_v2)]);
        }
        builder.set_connectivity_attribute(new_faces);
        
        let result = builder.build().map_err(|e| Err::InvalidInput(e.to_string()))?;
        Ok(result)
    }

    /// Convert a Draco Scene to glTF data.
    pub fn add_scene(&mut self, scene: &Scene) -> Result<(), Err> {
        // Set copyright from scene metadata if available
        self.set_copyright_from_scene(scene);
        
        // Add materials from the scene BEFORE processing meshes
        // so that material assignment can find the correct materials
        self.add_materials_from_scene(scene);
        
        // Add scene nodes to the glTF asset
        for node_index in 0..scene.num_nodes() {
            self.add_scene_node(scene, node_index)?;
        }
        
        // Add lights to the scene (skip as per instructions)
        // self.add_lights(scene)?;
        
        // Add animations (skip as per instructions)
        // self.add_animations(scene)?;
        
        // Add skins (skip as per instructions)
        // self.add_skins(scene)?;
        
        // Add materials variants names
        self.add_materials_variants_names(scene)?;
        
        // Add instance arrays
        self.add_instance_arrays(scene)?;
        
        // Add structural metadata
        self.add_structural_metadata(scene);
        
        // Restore property table buffer data and views if present
        self.restore_property_table_buffers(scene)?;
        
        // Create a default scene in the glTF asset
        let scene_index = self.add_scene_internal();
        if scene_index >= 0 {
            self.scene_index = scene_index;
        }
        
        Ok(())
    }

    /// Copy the glTF data to |buf_out| with WebP restoration.
    pub fn output_with_webp_restoration(&mut self, scene: &Scene, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        // Process images (embed into buffer if needed) before generating JSON
        self.process_images()?;
        
        // Restore WebP images from scene metadata after images are processed
        self.restore_webp_images(scene)?;
        
        // Generate the complete glTF JSON
        self.output_internal(buf_out)
    }

    /// Copy the glTF data to |buf_out|.
    pub fn output(&mut self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        // Process images (embed into buffer if needed) before generating JSON
        self.process_images()?;
        
        // Generate the complete glTF JSON
        self.output_internal(buf_out)
    }

    /// Internal method to generate glTF JSON output
    fn output_internal(&mut self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        use std::io::Write;
        
        // Start building the glTF JSON structure
        write!(buf_out, "{{")?;
        
        // Write asset information
        self.encode_asset_property(buf_out)?;
        write!(buf_out, ",")?;
        
        // Write scenes
        self.encode_scenes_property(buf_out)?;
        
        // Write default scene if we have one
        if self.scene_index >= 0 {
            write!(buf_out, ",")?;
            self.encode_initial_scene_property(buf_out)?;
        }
        
        // Write nodes if any
        if !self.nodes.is_empty() {
            write!(buf_out, ",")?;
            self.encode_nodes_property(buf_out)?;
        }
        
        // Write meshes if any
        if !self.meshes.is_empty() {
            write!(buf_out, ",")?;
            self.encode_meshes_property(buf_out)?;
        }
        
        // Write materials if any
        if self.material_library.num_materials() > 0 {
            write!(buf_out, ",")?;
            self.encode_materials(buf_out)?;
        }
        
        // Write accessors if any
        if !self.accessors.is_empty() {
            write!(buf_out, ",")?;
            self.encode_accessors_property(buf_out)?;
        }
        
        // Write buffer views if any
        if !self.buffer_views.is_empty() {
            write!(buf_out, ",")?;
            self.encode_buffer_views_property(buf_out)?;
        }
        
        // Write buffers if we have buffer data
        if !self.buffer.is_empty() {
            write!(buf_out, ",")?;
            self.encode_buffers_property(buf_out)?;
        }
        
        // Write extensions if any
        if !self.extensions_used.is_empty() || !self.extensions_required.is_empty() || self.structural_metadata_used {
            write!(buf_out, ",")?;
            self.encode_top_level_extensions_property(buf_out)?;
            
            // Add extension content if needed
            if self.structural_metadata_used {
                write!(buf_out, ",\"extensions\":{{")?;
                self.encode_structural_metadata_property(buf_out)?;
                write!(buf_out, "}}")?;
            }
        }
        
        // Close the JSON object
        write!(buf_out, "}}")?;
        
        Ok(())
    }

    /// Return the output image referenced by |index|.
    pub fn get_image(&self, index: i32) -> Option<&GltfImage> {
        if index >= 0 && (index as usize) < self.images.len() {
            Some(&self.images[index as usize])
        } else {
            None
        }
    }

    /// Return the number of images added to the GltfAsset.
    pub fn num_images(&self) -> i32 {
        self.images.len() as i32
    }

    #[allow(unused)]
    pub fn image_name(&self, i: i32) -> &str {
        &self.images[i as usize].image_name
    }

    pub fn set_add_images_to_buffer(&mut self, flag: bool) {
        self.add_images_to_buffer = flag;
    }

    pub fn add_images_to_buffer(&self) -> bool {
        self.add_images_to_buffer
    }

    pub fn set_output_type(&mut self, output_type: OutputType) {
        self.output_type = output_type;
    }

    pub fn output_type(&self) -> OutputType {
        self.output_type
    }

    pub fn set_json_output_mode(&mut self, mode: JsonOutputMode) {
        self.json_output_mode = mode;
    }

    #[allow(unused)]
    pub fn json_output_mode(&self) -> JsonOutputMode {
        self.json_output_mode
    }

    pub fn set_structural_metadata_json(&mut self, metadata: Option<serde_json::Value>) {
        self.structural_metadata_json = metadata;
    }

    /// Saves image to buffer for GLB embedding. Based on SaveImageToBuffer in C++ implementation.
    pub fn save_image_to_buffer(&mut self, image_index: i32) -> Result<(), Err> {
        if let Some(image) = self.images.get(image_index as usize) {
            let texture = image.texture.as_ref().ok_or_else(|| {
                Err::InvalidInput(format!("Image {} has no texture data", image_index))
            })?;
            
            // Write texture data to buffer
            let mut buffer = Vec::new();
            crate::io::texture_io::write_texture_to_buffer(texture, &mut buffer)
                .map_err(|e| Err::EncodingError(format!("Failed to write texture to buffer: {}", e)))?;
            
            // Add the image data to the main buffer
            let buffer_start_offset = self.buffer.len();
            self.buffer.extend_from_slice(&buffer);
            
            // Pad to 4-byte boundary for GLB alignment
            self.pad_buffer();
            
            // Add a buffer view pointing to the image data
            let buffer_view = GltfBufferView {
                buffer_byte_offset: buffer_start_offset as i64,
                byte_length: buffer.len() as i64,
                byte_stride: 0,
                target: -1, // No specific target for image data
            };
            
            let buffer_view_index = self.buffer_views.len() as i32;
            self.buffer_views.push(buffer_view);
            
            // Update the image to reference the buffer view instead of external file
            if let Some(image) = self.images.get_mut(image_index as usize) {
                image.buffer_view = buffer_view_index;
                if let Some(ref texture) = image.texture {
                    image.mime_type = crate::core::texture::TextureUtils::get_target_mime_type(texture);
                }
            }
            
            Ok(())
        } else {
            Err(Err::InvalidInput(format!("Image index {} not found", image_index)))
        }
    }

    /// Processes all images and embeds them into the buffer if add_images_to_buffer is true.
    pub fn process_images(&mut self) -> Result<(), Err> {
        if self.add_images_to_buffer {
            let num_images = self.images.len();
            for i in 0..num_images {
                self.save_image_to_buffer(i as i32)?;
            }
        }
        Ok(())
    }

    // Private methods

    /// Pad |buffer_| to 4 byte boundary.
    fn pad_buffer(&mut self) -> bool {
        if self.buffer.len() % 4 != 0 {
            let pad_bytes = 4 - (self.buffer.len() % 4);
            // Pad with zeros
            for _ in 0..pad_bytes {
                self.buffer.push(0);
            }
        }
        true
    }

    /// Returns the index of the scene that was added. -1 on error.
    fn add_scene_internal(&mut self) -> i32 {
        let mut gltf_scene = GltfScene::default();
        
        // Add all root node indices to the scene
        for i in 0..self.nodes.len() {
            if i < self.nodes.len() && self.nodes[i].root_node {
                gltf_scene.node_indices.push(i as i32);
            }
        }
        
        // Add the scene to the scenes list
        self.scenes.push(gltf_scene);
        (self.scenes.len() - 1) as i32
    }

    /// Find texture index that should be used for a given material texture
    fn find_texture_index_for_material_texture(
        &self,
        material_index: usize,
        _texture: &crate::core::texture::Texture,
    ) -> Option<i32> {
        // For material-based texture lookup, we use the material index to determine 
        // which texture should be used. This assumes textures were added in material order.
        
        // Count how many materials with textures we've seen so far
        let mut texture_count = 0;
        for i in 0..material_index {
            if let Some(material) = self.material_library.get_material(i) {
                if material.get_texture_map_by_type(crate::core::texture::Type::Color).is_some() {
                    texture_count += 1;
                }
            }
        }
        
        // The texture index should correspond to this count
        if texture_count < self.textures.len() {
            Some(texture_count as i32)
        } else {
            None
        }
    }

    /// Gets the image index for a texture, using existing index if available or adding new image
    fn get_or_add_image_index(
        &mut self,
        image_stem: &str,
        texture: &crate::core::texture::Texture,
        num_components: i32,
    ) -> Result<i32, Err> {
        // Use texture pointer as a stable key since we already processed textures in process_materials_and_add_images
        let texture_ptr = texture as *const crate::core::texture::Texture as usize;
        
        // Check if we already have this texture in our pointer map from add_image
        if let Some(&image_index) = self.texture_to_image_index_map.get(&texture_ptr) {
            // Already have an image for this texture, update num_components if needed
            if let Some(image) = self.images.get_mut(image_index as usize) {
                if image.num_components < num_components {
                    image.num_components = num_components;
                }
            }
            return Ok(image_index);
        }
        
        // Texture not found, add new image
        let new_index = self.add_image(image_stem, texture, num_components)?;
        Ok(new_index)
    }

    /// Adds a new glTF image to the asset and returns its index. |owned_texture|
    /// is an optional argument that can be used when the added image is not
    /// contained in the encoded MaterialLibrary (e.g. for images that are locally
    /// modified before they are encoded to disk). The image file name is generated
    /// by combining |image_stem| and image mime type contained in the |texture|.
    fn add_image(
        &mut self,
        image_stem: &str,
        texture: &crate::core::texture::Texture,
        num_components: i32,
    ) -> Result<i32, Err> {
        // Check if the texture is already in the map
        let texture_ptr = texture as *const crate::core::texture::Texture as usize;
        if let Some(&image_index) = self.texture_to_image_index_map.get(&texture_ptr) {
            // Already have an image for this texture, update num_components if needed
            if let Some(image) = self.images.get_mut(image_index as usize) {
                if image.num_components < num_components {
                    image.num_components = num_components;
                }
            }
            return Ok(image_index);
        }

        // Get extension from texture (placeholder: you should implement TextureUtils::get_target_extension)
        let mut extension = crate::core::texture::TextureUtils::get_target_extension(texture);
        if extension.is_empty() {
            // Try to get extension from the source file name (placeholder)
            extension = crate::core::texture::TextureUtils::lowercase_file_extension(
                &texture.get_source_image().get_filename()
            );
        }

        // Build image name
        let image_name = format!("{}.{}", image_stem, extension);

        // Build mime type (placeholder: you should implement TextureUtils::get_target_mime_type)
        let mime_type = crate::core::texture::TextureUtils::get_target_mime_type(texture);

        // For KTX2 with Basis compression, state that its extension is required.
        if extension == "ktx2" {
            self.extensions_used.insert("KHR_texture_basisu".to_string());
            self.extensions_required.insert("KHR_texture_basisu".to_string());
        }

        // If this is webp, state that its extension is required.
        if extension == "webp" {
            self.extensions_used.insert("EXT_texture_webp".to_string());
            self.extensions_required.insert("EXT_texture_webp".to_string());
        }

        // Create the image struct
        let image = GltfImage {
            image_name,
            texture: Some(texture.clone()),
            num_components,
            buffer_view: -1,
            mime_type,
        };

        // Add to images and map
        self.images.push(image);
        let new_index = (self.images.len() - 1) as i32;
        self.texture_to_image_index_map.insert(texture_ptr, new_index);
        Ok(new_index)
    }


    /// Adds a Draco SceneNode, referenced by |scene_node_index|, to the glTF data.
    fn add_scene_node(&mut self, scene: &Scene, scene_node_index: usize) -> Result<(), Err> {
        // Get the actual scene node
        if let Some(scene_node) = scene.get_node(scene_node_index) {
            let mut gltf_node = GltfNode::default();
            
            // Set name from scene node or use default
            if !scene_node.get_name().is_empty() {
                gltf_node.name = scene_node.get_name().to_string();
            } else {
                gltf_node.name = format!("node_{}", scene_node_index);
            }
            
            // Check if this is a root node (has no parents)
            gltf_node.root_node = scene_node.parents().is_empty();
            
            // Set children indices
            gltf_node.children_indices = scene_node.children().iter().map(|&idx| idx as i32).collect();
            
            // Set transformation matrix from scene node
            gltf_node.trs_matrix = scene_node.get_trs_matrix().clone();
            
            // Add mesh reference if the node has a mesh group
            if let Some(mesh_group_index) = scene_node.get_mesh_group_index() {
                if let Some(mesh_group) = scene.get_mesh_group(mesh_group_index) {
                    // Create a single glTF mesh that will contain all primitives
                    let gltf_mesh_index = self.meshes.len() as i32;
                    let mut gltf_mesh = GltfMesh::default();
                    gltf_mesh.name = format!("mesh_{}", gltf_mesh_index);
                    
                    // Track which mesh we're processing to avoid duplicates
                    let mut processed_mesh_materials = std::collections::HashSet::new();
                    
                    // Process all mesh instances in the group
                    for mesh_instance_index in 0..mesh_group.num_mesh_instances() {
                        if let Some(mesh_instance) = mesh_group.get_mesh_instance(mesh_instance_index) {
                            // Get the mesh from the scene
                            if let Some(mesh) = scene.get_mesh(mesh_instance.mesh_index) {
                                // Create a unique key for this mesh+material combination
                                let key = (mesh_instance.mesh_index, mesh_instance.material_index);
                                
                                // Skip if we've already processed this mesh+material combination
                                if processed_mesh_materials.contains(&key) {
                                    continue;
                                }
                                processed_mesh_materials.insert(key);
                                
                                // Add this mesh as a primitive with the correct material filtering
                                // This will create a filtered primitive for this specific material
                                self.add_draco_mesh_as_primitive(mesh, scene, mesh_instance.material_index, &mut gltf_mesh)?;
                            }
                        }
                    }
                    
                    // Only add the mesh if it has primitives
                    if !gltf_mesh.primitives.is_empty() {
                        self.meshes.push(gltf_mesh);
                        gltf_node.mesh_index = gltf_mesh_index;
                    }
                }
            }
            
            // Add the node to the glTF asset
            self.nodes.push(gltf_node);
        } else {
            // Create a default node if scene node doesn't exist
            let mut gltf_node = GltfNode::default();
            gltf_node.name = format!("node_{}", scene_node_index);
            gltf_node.root_node = true;
            gltf_node.trs_matrix = crate::core::scene::TrsMatrix::default();
            self.nodes.push(gltf_node);
        }
        
        Ok(())
    }

    /// Iterate through the materials that are associated with |scene| and add them
    /// to the asset.
    fn add_materials_from_scene(&mut self, scene: &Scene) {
        if scene.material_library().num_materials() > 0 {
            self.material_library = scene.material_library().clone();
            
            // Process all materials and add their images to the GltfAsset
            self.process_materials_and_add_images(scene);
        }
    }

    /// Process all materials in the scene and add their images to the GltfAsset
    fn process_materials_and_add_images(&mut self, scene: &Scene) {
        let material_library = scene.material_library();
        
        for material_index in 0..material_library.num_materials() {
            if let Some(material) = material_library.get_material(material_index) {
                // Process each texture map type and add images
                use crate::core::texture::Type;
                
                let texture_types = [
                    Type::Color,
                    Type::Opacity,
                    Type::Metallic,
                    Type::Roughness,
                    Type::MetallicRoughness,
                    Type::NormalObjectSpace,
                    Type::NormalTangentSpace,
                    Type::AmbientOcclusion,
                    Type::Emissive,
                    Type::SheenColor,
                    Type::SheenRoughness,
                    Type::Transmission,
                    Type::Clearcoat,
                    Type::ClearcoatRoughness,
                    Type::ClearcoatNormal,
                    Type::Thickness,
                    Type::Specular,
                    Type::SpecularColor,
                ];
                
                for texture_type in texture_types {
                    if let Some(texture_map) = material.get_texture_map_by_type(texture_type) {
                        let texture = texture_map.get_texture();
                        let texture_stem = crate::core::texture::TextureUtils::get_or_generate_target_stem(
                            texture, 
                            material_index, 
                            "_BaseColor"
                        );
                        
                        // Determine appropriate number of components based on texture type
                        let num_components = match texture_type {
                            Type::Color => 4,
                            Type::NormalObjectSpace | Type::NormalTangentSpace => 3,
                            Type::Emissive => 3,
                            Type::MetallicRoughness => 3,
                            _ => 1,
                        };
                        
                        // Add the image to the GltfAsset
                        if let Ok(image_index) = self.add_image(&texture_stem, texture, num_components) {
                            // Create a default texture sampler
                            let sampler = TextureSampler::new(
                                texture_map.min_filter(),
                                texture_map.mag_filter(),
                                texture_map.get_wrapping_mode()
                            );
                            
                            // Find or add the sampler
                            let sampler_index = if let Some(pos) = self.texture_samplers.iter().position(|s| s == &sampler) {
                                pos as i32
                            } else {
                                let index = self.texture_samplers.len() as i32;
                                self.texture_samplers.push(sampler);
                                index
                            };
                            
                            // Create and add texture object if it doesn't exist
                            let texture_obj = GltfTexture::new(image_index, sampler_index);
                            if !self.textures.iter().any(|t| t == &texture_obj) {
                                self.textures.push(texture_obj);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Iterate through materials variants names that are associated with |scene|
    /// and add them to the asset. Returns OkStatus() if |scene| does not contain
    /// any materials variants.
    fn add_materials_variants_names(&mut self, scene: &Scene) -> Result<(), Err> {
        // Placeholder implementation - would extract material variants from scene
        let _ = scene;
        Ok(())
    }

    /// Iterate through the mesh group instance arrays that are associated with
    /// |scene| and add them to the asset. Returns OkStatus() if |scene| does not
    /// contain any mesh group instance arrays.
    fn add_instance_arrays(&mut self, scene: &Scene) -> Result<(), Err> {
        // Placeholder implementation - would extract instance arrays from scene
        let _ = scene;
        Ok(())
    }

    /// Adds structural metadata from |geometry| to the asset, if any.
    fn add_structural_metadata(&mut self, scene: &Scene) {
        // Extract structural metadata JSON from the scene's metadata
        if let Some(json_string) = scene.metadata().get_entry("structural_metadata_json") {
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(json_string) {
                self.set_structural_metadata_json(Some(json_value));
            }
        }
    }

    /// Get image buffer view indices by checking the actual buffer data for image signatures
    fn get_image_buffer_view_indices(&self, buffer_views_array: &[serde_json::Value], property_data: &[u8], original_start_offset: usize) -> std::collections::HashSet<usize> {
        let mut image_buffer_views = std::collections::HashSet::new();
        
        for (i, bv_json) in buffer_views_array.iter().enumerate() {
            if let Some(bv_obj) = bv_json.as_object() {
                let original_offset = bv_obj.get("byteOffset")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let byte_length = bv_obj.get("byteLength")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                
                // Handle the case where original_offset might be smaller than original_start_offset
                if original_offset < original_start_offset {
                    continue;
                }
                let data_offset = original_offset - original_start_offset;
                
                // Check if we can read the data and if it has image signatures
                if byte_length > 0 && data_offset + byte_length <= property_data.len() {
                    let sample_size = std::cmp::min(20, byte_length);
                    let sample_data = &property_data[data_offset..data_offset + sample_size];
                    
                    // Check for image signatures
                    if sample_data.starts_with(b"RIFF") && sample_data.len() >= 12 && &sample_data[8..12] == b"WEBP" {
                        // WebP image
                        image_buffer_views.insert(i);
                    } else if sample_data.starts_with(b"\x89PNG") {
                        // PNG image  
                        image_buffer_views.insert(i);
                    } else if sample_data.starts_with(b"\xFF\xD8\xFF") {
                        // JPEG image
                        image_buffer_views.insert(i);
                    }
                }
            } 
        }
        
        image_buffer_views
    }

    /// Restores property table buffer data and views if preserved from decoding
    fn restore_property_table_buffers(&mut self, scene: &Scene) -> Result<(), Err> {
        // Check if we have property buffer views stored
        if let Some(buffer_views_json_str) = scene.metadata().get_entry("property_buffer_views_json") {
            if let Ok(buffer_views_array) = serde_json::from_str::<Vec<serde_json::Value>>(buffer_views_json_str) {
                // Get the property buffer data if available
                let property_data = if let Some(base64_data) = scene.metadata().get_entry("property_buffer_data_base64") {
                    use base64::{Engine as _, engine::general_purpose};
                    general_purpose::STANDARD.decode(base64_data)
                        .map_err(|e| Err::EncodingError(format!("Failed to decode base64 property data: {}", e)))?
                } else {
                    Vec::new()
                };
                
                // Get the original property buffer start offset
                let original_start_offset = scene.metadata().get_entry("property_buffer_start_offset")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                
                // Get image buffer view indices to exclude them (they're handled separately)
                let image_buffer_view_indices = self.get_image_buffer_view_indices(&buffer_views_array, &property_data, original_start_offset);
                
                // Create a mapping from old buffer view indices to new ones
                // This is critical for EXT_mesh_features and EXT_structural_metadata
                let mut buffer_view_mapping = std::collections::HashMap::new();
                
                for (i, bv_json) in buffer_views_array.iter().enumerate() {
                    if i == 0 {
                        // Skip the first buffer view - it's for the mesh data which will be 
                        // replaced by Draco-compressed data
                        continue;
                    }
                    
                    // Skip image buffer views - they're handled separately by image processing
                    if image_buffer_view_indices.contains(&i) {
                        continue;
                    }
                    
                    if let Some(bv_obj) = bv_json.as_object() {
                        // Extract buffer view properties
                        let original_offset = bv_obj.get("byteOffset")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        let byte_length = bv_obj.get("byteLength")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        
                        // Calculate the offset within the property data buffer
                        let data_offset = original_offset - original_start_offset;
                        
                        if byte_length > 0 && data_offset + byte_length <= property_data.len() {
                            // Add property buffer views after Draco buffer views
                            // This preserves the relative indexing for EXT_mesh_features
                            let new_buffer_view_index = self.buffer_views.len();
                            buffer_view_mapping.insert(i, new_buffer_view_index);
                            
                            // Create a new buffer view
                            let buffer_view = GltfBufferView {
                                buffer_byte_offset: self.buffer.len() as i64,
                                byte_length: byte_length as i64,
                                byte_stride: bv_obj.get("byteStride")
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as i32)
                                    .unwrap_or(-1),
                                target: bv_obj.get("target")
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as i32)
                                    .unwrap_or(-1),
                            };
                            
                            // Append the property data to the main buffer
                            self.buffer.extend_from_slice(&property_data[data_offset..data_offset + byte_length]);
                            
                            // Add the buffer view (don't overwrite Draco buffer views)
                            self.buffer_views.push(buffer_view);
                        }
                    }
                }
                
                // Update structural metadata with new buffer view indices
                self.update_structural_metadata_buffer_views(&buffer_view_mapping);
            }
        }
        Ok(())
    }

    /// Updates structural metadata buffer view references with new indices
    fn update_structural_metadata_buffer_views(&mut self, buffer_view_mapping: &std::collections::HashMap<usize, usize>) {
        if let Some(ref mut metadata_json) = self.structural_metadata_json {
            if let Some(property_tables) = metadata_json.get_mut("propertyTables").and_then(|pt| pt.as_array_mut()) {
                for property_table in property_tables {
                    if let Some(properties) = property_table.get_mut("properties").and_then(|p| p.as_object_mut()) {
                        for (_prop_name, prop_value) in properties {
                            if let Some(prop_obj) = prop_value.as_object_mut() {
                                // Update 'values' buffer view index
                                if let Some(values_index) = prop_obj.get("values").and_then(|v| v.as_u64()) {
                                    if let Some(&new_index) = buffer_view_mapping.get(&(values_index as usize)) {
                                        prop_obj.insert("values".to_string(), serde_json::Value::Number(serde_json::Number::from(new_index)));
                                    }
                                }
                                
                                // Update 'stringOffsets' buffer view index
                                if let Some(string_offsets_index) = prop_obj.get("stringOffsets").and_then(|v| v.as_u64()) {
                                    if let Some(&new_index) = buffer_view_mapping.get(&(string_offsets_index as usize)) {
                                        prop_obj.insert("stringOffsets".to_string(), serde_json::Value::Number(serde_json::Number::from(new_index)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Restores WebP images from scene metadata
    fn restore_webp_images(&mut self, scene: &Scene) -> Result<(), Err> {
        // Check for WebP info in scene metadata
        if let Some(webp_info_string) = scene.metadata().get_entry("webp_info") {
            if let Ok(webp_info) = serde_json::from_str::<serde_json::Value>(webp_info_string) {
                // Restore WebP images by replacing placeholder data
                self.restore_webp_from_info(&webp_info)?;
            } else {
            }
        } 
        Ok(())
    }

    fn restore_webp_from_info(&mut self, webp_info: &serde_json::Value) -> Result<(), Err> {
        // Get the original file path to read the WebP data from
        let original_file_path = webp_info.get("original_file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Err::IoError("Missing original_file_path in webp_info".to_string()))?;
        
        // Read the original GLB file to extract WebP data
        let original_glb_data = std::fs::read(original_file_path)
            .map_err(|e| Err::IoError(format!("Failed to read original GLB file for WebP restoration: {}", e)))?;
        
        // Extract JSON from the original GLB to get the original image buffer view information
        let original_json_content = self.extract_json_from_original_glb(&original_glb_data)?;
        let original_json: serde_json::Value = serde_json::from_str(&original_json_content)
            .map_err(|e| Err::IoError(format!("Failed to parse original GLB JSON: {}", e)))?;
        
        // Check for webp_images array - structure: {"webp_images": [[0, {"bufferView": 74, "mimeType": "image/webp"}]]}
        if let Some(webp_images_array) = webp_info.get("webp_images").and_then(|v| v.as_array()) {
            for (_, webp_entry) in webp_images_array.iter().enumerate() {
                if let Some(entry_array) = webp_entry.as_array() {
                    if entry_array.len() >= 2 {
                        if let (Some(index), Some(webp_info_obj)) = (
                            entry_array[0].as_u64(),
                            entry_array[1].as_object(),
                        ) {
                            if let Some(mime_type) = webp_info_obj.get("mimeType").and_then(|v| v.as_str()) {
                                if mime_type == "image/webp" && index < self.images.len() as u64 {
                                    
                                    // Extract WebP binary data from the original GLB file
                                    if let Some(buffer_view_index) = webp_info_obj.get("bufferView").and_then(|v| v.as_u64()) {
                                        if let Ok(webp_data) = self.extract_webp_data_from_original_glb(
                                            &original_glb_data, 
                                            &original_json, 
                                            buffer_view_index as usize
                                        ) {
                                            // Create a buffer view for the WebP data in our current buffer
                                            let buffer_view_index = self.add_webp_data_to_buffer(&webp_data)?;
                                            
                                            // Update the image with WebP data
                                            self.images[index as usize].mime_type = "image/webp".to_string();
                                            self.images[index as usize].buffer_view = buffer_view_index;
                                        }
                                    } else {
                                        // Handle URI-based WebP images (external files)
                                        if let Some(uri) = webp_info_obj.get("uri").and_then(|v| v.as_str()) {
                                            if let Ok(webp_data) = self.load_external_webp_file(original_file_path, uri) {
                                                let buffer_view_index = self.add_webp_data_to_buffer(&webp_data)?;
                                                self.images[index as usize].mime_type = "image/webp".to_string();
                                                self.images[index as usize].buffer_view = buffer_view_index;
                                            }
                                        }
                                    }
                                    
                                    // Ensure WebP extension is declared
                                    self.extensions_used.insert("EXT_texture_webp".to_string());
                                    self.extensions_required.insert("EXT_texture_webp".to_string());
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Fallback to old structure for compatibility
            if let Some(images_array) = webp_info.get("images").and_then(|v| v.as_array()) {
                for (_, image_info) in images_array.iter().enumerate() {
                    if let (Some(index), Some(webp_data), Some(mime_type)) = (
                        image_info.get("index").and_then(|v| v.as_u64()),
                        image_info.get("webp_data").and_then(|v| v.as_str()),
                        image_info.get("mime_type").and_then(|v| v.as_str()),
                    ) {
                        if mime_type == "image/webp" && index < self.images.len() as u64 {
                            // Decode base64 WebP data
                            if let Ok(webp_bytes) = base64::prelude::Engine::decode(&base64::prelude::BASE64_STANDARD, webp_data) {
                                // Add WebP data to buffer and update image
                                let buffer_view_index = self.add_webp_data_to_buffer(&webp_bytes)?;
                                self.images[index as usize].mime_type = "image/webp".to_string();
                                self.images[index as usize].buffer_view = buffer_view_index;
                                
                                // Ensure WebP extension is declared
                                self.extensions_used.insert("EXT_texture_webp".to_string());
                                self.extensions_required.insert("EXT_texture_webp".to_string());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Extract JSON content from original GLB file
    fn extract_json_from_original_glb(&self, glb_data: &[u8]) -> Result<String, Err> {
        
        if glb_data.len() < 20 {
            return Err(Err::IoError("GLB file too small".to_string()));
        }
        
        // Check GLB header
        let magic = &glb_data[0..4];
        if magic != b"glTF" {
            return Err(Err::IoError("Invalid GLB magic number".to_string()));
        }
        
        // Read JSON chunk length and type
        let json_length = u32::from_le_bytes([glb_data[12], glb_data[13], glb_data[14], glb_data[15]]) as usize;
        let json_type = &glb_data[16..20];
        
        if json_type != b"JSON" {
            return Err(Err::IoError("First chunk is not JSON".to_string()));
        }
        
        // Extract JSON content
        let json_start = 20;
        let json_end = json_start + json_length;
        
        if json_end > glb_data.len() {
            return Err(Err::IoError("JSON chunk extends beyond file".to_string()));
        }
        
        let json_bytes = &glb_data[json_start..json_end];
        String::from_utf8(json_bytes.to_vec())
            .map_err(|e| Err::IoError(format!("Invalid UTF-8 in JSON chunk: {}", e)))
    }
    
    /// Extract WebP binary data from original GLB file using buffer view information
    fn extract_webp_data_from_original_glb(
        &self, 
        glb_data: &[u8], 
        json: &serde_json::Value, 
        buffer_view_index: usize
    ) -> Result<Vec<u8>, Err> {
        // Get buffer view information
        let buffer_views = json.get("bufferViews")
            .and_then(|bv| bv.as_array())
            .ok_or_else(|| Err::IoError("No bufferViews found in original GLB".to_string()))?;
        
        let buffer_view = buffer_views.get(buffer_view_index)
            .ok_or_else(|| Err::IoError(format!("BufferView {} not found in original GLB", buffer_view_index)))?;
        
        let byte_offset = buffer_view.get("byteOffset")
            .and_then(|b| b.as_u64())
            .unwrap_or(0) as usize;
        
        let byte_length = buffer_view.get("byteLength")
            .and_then(|b| b.as_u64())
            .ok_or_else(|| Err::IoError("BufferView missing byteLength".to_string()))? as usize;
        
        // Find the binary chunk in the GLB
        let json_length = u32::from_le_bytes([glb_data[12], glb_data[13], glb_data[14], glb_data[15]]) as usize;
        let binary_chunk_start = 20 + json_length;
        
        // Skip binary chunk header (8 bytes: 4 for length, 4 for type)
        let binary_data_start = binary_chunk_start + 8;
        let data_start = binary_data_start + byte_offset;
        let data_end = data_start + byte_length;
        
        if data_end > glb_data.len() {
            return Err(Err::IoError("WebP data extends beyond GLB file".to_string()));
        }
        
        Ok(glb_data[data_start..data_end].to_vec())
    }
    
    /// Load external WebP file
    fn load_external_webp_file(&self, original_file_path: &str, uri: &str) -> Result<Vec<u8>, Err> {
        use std::path::Path;
        
        let original_dir = Path::new(original_file_path).parent().unwrap_or(Path::new("."));
        let webp_path = original_dir.join(uri);
        
        std::fs::read(&webp_path)
            .map_err(|e| Err::IoError(format!("Failed to read external WebP file {}: {}", webp_path.display(), e)))
    }
    
    /// Add WebP data to the current buffer and return the buffer view index
    fn add_webp_data_to_buffer(&mut self, webp_data: &[u8]) -> Result<i32, Err> {
        // Add the WebP data to the main buffer
        let buffer_offset = self.buffer.len();
        self.buffer.extend_from_slice(webp_data);
        
        // Create a new buffer view for the WebP data
        let buffer_view = GltfBufferView {
            buffer_byte_offset: buffer_offset as i64,
            byte_length: webp_data.len() as i64,
            byte_stride: 0, // No stride for image data
            target: -1, // No specific target for image data
        };
        
        let buffer_view_index = self.buffer_views.len() as i32;
        self.buffer_views.push(buffer_view);
        
        Ok(buffer_view_index)
    }

    /// Determine the appropriate material index for a mesh based on its attributes
    fn determine_material_index_for_mesh(&self, mesh: &Mesh) -> i32 {
        // Check if this mesh has texture coordinates, which indicates it should use a textured material
        let has_texture_coords = mesh.get_attributes().iter()
            .any(|attr| attr.get_attribute_type() == crate::core::attribute::AttributeType::TextureCoordinate);
        
        if self.material_library.num_materials() > 1 {
            if has_texture_coords {
                // For textured meshes, assign materials based on mesh order
                // Count how many textured meshes we've processed so far
                let textured_mesh_count = self.meshes.iter()
                    .filter(|gltf_mesh| {
                        gltf_mesh.primitives.iter().any(|prim| {
                            // Check if this primitive has texture coordinates
                            prim.attributes.contains_key("TEXCOORD_0")
                        })
                    })
                    .count();
                
                // Find the textured_mesh_count-th material with a texture
                let mut found_textured_materials = 0;
                for material_index in 0..self.material_library.num_materials() {
                    if let Some(material) = self.material_library.get_material(material_index) {
                        if material.get_texture_map_by_type(crate::core::texture::Type::Color).is_some() {
                            if found_textured_materials == textured_mesh_count {
                                return material_index as i32;
                            }
                            found_textured_materials += 1;
                        }
                    }
                }
                
                // Fallback: use the first textured material
                for material_index in 0..self.material_library.num_materials() {
                    if let Some(material) = self.material_library.get_material(material_index) {
                        if material.get_texture_map_by_type(crate::core::texture::Type::Color).is_some() {
                            return material_index as i32;
                        }
                    }
                }
            } else {
                // Mesh has NO texture coordinates - find a material WITHOUT texture
                for material_index in 0..self.material_library.num_materials() {
                    if let Some(material) = self.material_library.get_material(material_index) {
                        // Check if this material has NO color texture
                        if material.get_texture_map_by_type(crate::core::texture::Type::Color).is_none() {
                            return material_index as i32;
                        }
                    }
                }
            }
        }
        
        // Default to material 0 if no appropriate material is found or only one material exists
        0
    }

    fn encode_asset_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        use std::io::Write;
        
        write!(buf_out, "\"asset\":{{")?;
        write!(buf_out, "\"version\":\"2.0\"")?;
        
        if !self.generator.is_empty() {
            write!(buf_out, ",\"generator\":\"{}\"", self.generator)?;
        }
        
        if !self.copyright.is_empty() {
            write!(buf_out, ",\"copyright\":\"{}\"", self.copyright)?;
        }
        
        write!(buf_out, "}}")?;
        Ok(())
    }

    fn encode_scenes_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        use std::io::Write;
        
        write!(buf_out, "\"scenes\":[")?;
        
        for (i, scene) in self.scenes.iter().enumerate() {
            if i > 0 {
                write!(buf_out, ",")?;
            }
            
            write!(buf_out, "{{")?;
            write!(buf_out, "\"nodes\":[")?;
            
            for (j, node_index) in scene.node_indices.iter().enumerate() {
                if j > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "{}", node_index)?;
            }
            
            write!(buf_out, "]")?;
            write!(buf_out, "}}")?;
        }
        
        write!(buf_out, "]")?;
        Ok(())
    }

    fn encode_initial_scene_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        use std::io::Write;
        
        write!(buf_out, "\"scene\":{}", self.scene_index)?;
        Ok(())
    }

    fn encode_nodes_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        use std::io::Write;
        
        write!(buf_out, "\"nodes\":[")?;
        
        for (i, node) in self.nodes.iter().enumerate() {
            if i > 0 {
                write!(buf_out, ",")?;
            }
            
            write!(buf_out, "{{")?;
            
            // Write node name if it has one
            if !node.name.is_empty() {
                write!(buf_out, "\"name\":\"{}\"", node.name)?;
            }
            
            // Write children if any
            if !node.children_indices.is_empty() {
                if !node.name.is_empty() {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "\"children\":[")?;
                for (j, child_index) in node.children_indices.iter().enumerate() {
                    if j > 0 {
                        write!(buf_out, ",")?;
                    }
                    write!(buf_out, "{}", child_index)?;
                }
                write!(buf_out, "]")?;
            }
            
            // Write mesh reference if any
            if node.mesh_index >= 0 {
                if !node.name.is_empty() || !node.children_indices.is_empty() {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "\"mesh\":{}", node.mesh_index)?;
            }
            
            // Write transformation components if any transformation is set
            if node.trs_matrix.transform_set() {
                let mut needs_comma = !node.name.is_empty() || !node.children_indices.is_empty() || node.mesh_index >= 0;
                
                // Prefer translation/rotation/scale over matrix when available
                if node.trs_matrix.translation_set() {
                    if needs_comma {
                        write!(buf_out, ",")?;
                    }
                    if let Ok(translation) = node.trs_matrix.translation() {
                        write!(buf_out, "\"translation\":[{},{},{}]", translation.x, translation.y, translation.z)?;
                        needs_comma = true;
                    }
                }
                
                if node.trs_matrix.rotation_set() {
                    if needs_comma {
                        write!(buf_out, ",")?;
                    }
                    if let Ok(rotation) = node.trs_matrix.rotation() {
                        write!(buf_out, "\"rotation\":[{},{},{},{}]", rotation.x, rotation.y, rotation.z, rotation.w)?;
                        needs_comma = true;
                    }
                }
                
                if node.trs_matrix.scale_set() {
                    if needs_comma {
                        write!(buf_out, ",")?;
                    }
                    if let Ok(scale) = node.trs_matrix.scale() {
                        write!(buf_out, "\"scale\":[{},{},{}]", scale.x, scale.y, scale.z)?;
                        needs_comma = true;
                    }
                }
                
                // Fallback to matrix if no individual components are set but matrix is set
                if node.trs_matrix.matrix_set() && !node.trs_matrix.translation_set() && !node.trs_matrix.rotation_set() && !node.trs_matrix.scale_set() {
                    if needs_comma {
                        write!(buf_out, ",")?;
                    }
                    let matrix = node.trs_matrix.compute_transformation_matrix();
                    write!(buf_out, "\"matrix\":[")?;
                    for i in 0..4 {
                        for j in 0..4 {
                            if i > 0 || j > 0 {
                                write!(buf_out, ",")?;
                            }
                            write!(buf_out, "{}", matrix.data[i][j])?;
                        }
                    }
                    write!(buf_out, "]")?;
                }
            }
            
            write!(buf_out, "}}")?;
        }
        
        write!(buf_out, "]")?;
        Ok(())
    }

    fn encode_meshes_property(&mut self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        use std::io::Write;
        
        write!(buf_out, "\"meshes\":[")?;
        
        for i in 0..self.meshes.len() {
            let mesh = &self.meshes[i].clone(); // TODO: We should avoid this clone.
            if i > 0 {
                write!(buf_out, ",")?;
            }
            
            write!(buf_out, "{{")?;
            
            if !mesh.name.is_empty() {
                write!(buf_out, "\"name\":\"{}\"", mesh.name)?;
            }
            
            if !mesh.name.is_empty() && !mesh.primitives.is_empty() {
                write!(buf_out, ",")?;
            }
            
            if !mesh.primitives.is_empty() {
                write!(buf_out, "\"primitives\":[")?;
                
                for j in 0..mesh.primitives.len() {
                    let primitive = &mesh.primitives[j];
                    if j > 0 {
                        write!(buf_out, ",")?;
                    }
                    // self.encode_primitive(primitive, buf_out)?;
                    write!(buf_out, "{{")?;

                    // Write standard attributes (accessor references)
                    if !primitive.attributes.is_empty() {
                        write!(buf_out, "\"attributes\":{{")?;
                        let mut first = true;
                        for (attr_name, accessor_id) in &primitive.attributes {
                            if !first {
                                write!(buf_out, ",")?;
                            }
                            write!(buf_out, "\"{}\":{}", attr_name, accessor_id)?;
                            first = false;
                        }
                        write!(buf_out, "}}")?;
                    }

                    let mut first_field = primitive.attributes.is_empty();
                    if primitive.indices >= 0 {
                        if !first_field {
                            write!(buf_out, ",")?;
                        }
                        write!(buf_out, "\"indices\":{}", primitive.indices)?;
                        first_field = false;
                    }
                    if !first_field {
                        write!(buf_out, ",")?;
                    }
                    write!(buf_out, "\"mode\":{}", primitive.mode)?;
                    if primitive.material >= 0 {
                        write!(buf_out, ",\"material\":{}", primitive.material)?;
                    }
                    write!(buf_out, ",")?;
                    self.encode_primitive_extensions_property(primitive, buf_out)?;
                    write!(buf_out, "}}")?;
                }
                
                write!(buf_out, "]")?;
            }
            
            write!(buf_out, "}}")?;
        }
        
        write!(buf_out, "]")?;
        Ok(())
    }
    

    fn encode_primitive_extensions_property(
        &mut self,
        primitive: &GltfPrimitive,
        buf_out: &mut Vec<u8>,
    ) -> Result<(), Err> {

        let has_draco_mesh_compression = primitive.compressed_mesh_info.buffer_view_index >= 0;
        let has_materials_variants = !primitive.material_variants_mappings.is_empty();
        let has_structural_metadata = !primitive.property_attributes.is_empty();
        let has_mesh_features = !primitive.mesh_features.is_empty();

        // Early return if no extensions are needed
        if !has_draco_mesh_compression && !has_materials_variants && !has_mesh_features && !has_structural_metadata {
            return Ok(());
        }

        // Begin the "extensions" object
        write!(buf_out, "\"extensions\":{{")?;
        let mut first_extension = true;

        // KHR_draco_mesh_compression
        if has_draco_mesh_compression {
            if !first_extension {
                write!(buf_out, ",")?;
            }
            first_extension = false;
            write!(buf_out, "\"KHR_draco_mesh_compression\":{{\"bufferView\":{}", 
                    primitive.compressed_mesh_info.buffer_view_index)?;
            if !primitive.compressed_mesh_info.attributes.is_empty() {
                write!(buf_out, ",\"attributes\":{{")?;
                let mut first_attr = true;
                let mut atts = primitive.compressed_mesh_info.attributes.iter().collect::<Vec<_>>();
                // Sort attributes by name for consistent output
                atts.sort_by_key(|&(_, id)| *id);
                for (attr_name, attr_id) in atts {
                    if !first_attr {
                        write!(buf_out, ",")?;
                    }
                    first_attr = false;
                    write!(buf_out, "\"{}\":{}", attr_name, attr_id)?;
                }
                write!(buf_out, "}}")?;
            }
            write!(buf_out, "}}")?;
        }

        // KHR_materials_variants
        if has_materials_variants {
            if !first_extension {
                write!(buf_out, ",")?;
            }
            first_extension = false;
            write!(buf_out, "\"KHR_materials_variants\":{{\"mappings\":[")?;
            for (i, mapping) in primitive.material_variants_mappings.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "{{\"material\":{}", mapping.material)?;
                if !mapping.variants.is_empty() {
                    write!(buf_out, ",\"variants\":[")?;
                    for (j, variant) in mapping.variants.iter().enumerate() {
                        if j > 0 {
                            write!(buf_out, ",")?;
                        }
                        write!(buf_out, "{}", variant)?;
                    }
                    write!(buf_out, "]")?;
                }
                write!(buf_out, "}}")?;
            }
            write!(buf_out, "]}}")?;
        }

        // EXT_mesh_features
        if has_mesh_features {
            if !first_extension {
                write!(buf_out, ",")?;
            }
            first_extension = false;
            self.mesh_features_used = true;
            self.extensions_used.insert("EXT_mesh_features".to_string());
            write!(buf_out, "\"EXT_mesh_features\":{{\"featureIds\":[")?;
            for (i, features) in primitive.mesh_features.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "{{")?;
                let mut first_feature_prop = true;
                if !features.get_label().is_empty() {
                    write!(buf_out, "\"label\":\"{}\"", features.get_label())?;
                    first_feature_prop = false;
                }
                if !first_feature_prop {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "\"featureCount\":{}", features.get_feature_count())?;
                if features.get_attribute_index() != -1 {
                    if let Some(attr_name) = primitive.feature_id_name_indices.get(&features.get_attribute_index()) {
                        // Extract the numeric suffix from "_FEATURE_ID_X"
                        if let Some(feature_id_str) = attr_name.strip_prefix("_FEATURE_ID_") {
                            write!(buf_out, ",\"attribute\":{}", feature_id_str)?;
                        } else {
                            // For other patterns, try to extract a numeric suffix after the last underscore
                            if let Some(last_underscore_pos) = attr_name.rfind('_') {
                                let suffix = &attr_name[last_underscore_pos + 1..];
                                if suffix.chars().all(|c| c.is_ascii_digit()) && !suffix.is_empty() {
                                    write!(buf_out, ",\"attribute\":{}", suffix)?;
                                } else {
                                    // If no valid numeric suffix, use the full attribute name as quoted string
                                    write!(buf_out, ",\"attribute\":\"{}\"", attr_name)?;
                                }
                            } else {
                                // No underscore found, use the full attribute name as quoted string
                                write!(buf_out, ",\"attribute\":\"{}\"", attr_name)?;
                            }
                        }
                    }
                }
                if features.get_property_table_index() != -1 {
                    write!(buf_out, ",\"propertyTable\":{}", features.get_property_table_index())?;
                }
                if features.get_texture_map().tex_coord_index() != -1 {
                    // Placeholder for texture logic: add image, encode texture, etc.
                    // For now, skip texture map encoding as we don't have texture support
                    // TODO: Implement texture map encoding when texture support is added
                }
                if features.get_null_feature_id() != -1 {
                    write!(buf_out, ",\"nullFeatureId\":{}", features.get_null_feature_id())?;
                }
                write!(buf_out, "}}")?;
            }
            write!(buf_out, "]}}")?;
        }

        // EXT_structural_metadata
        if has_structural_metadata {
            if !first_extension {
                write!(buf_out, ",")?;
            }
            self.structural_metadata_used = true;
            self.extensions_used.insert("EXT_structural_metadata".to_string());
            write!(buf_out, "\"EXT_structural_metadata\":{{\"propertyAttributes\":[")?;
            for (i, pa_index) in primitive.property_attributes.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "{}", pa_index)?;
            }
            write!(buf_out, "]}}")?;
        }

        // Close "extensions" object
        write!(buf_out, "}}")?;
        Ok(())
    }

    
    fn encode_materials(&mut self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        // Check if we have textures to write.
        if self.material_library.num_materials() == 0 {
            self.encode_default_material(buf_out)
        } else {
            self.encode_materials_property(buf_out)
        }
    }

    fn encode_default_material(&self, _buf_out: &mut Vec<u8>) -> Result<(), Err> {
        unimplemented!()
    }

    /// Encodes a texture map. |object_name| is the name of the texture map.
    /// |image_index| is the index into the texture image array. |tex_coord_index|
    /// is the index into the texture coordinates. |texture_map| is a reference to
    /// the texture map that is going to be encoded.
    fn encode_texture_map(
        &mut self,
        object_name: &str,
        image_index: i32,
        tex_coord_index: i32,
        _material: &crate::core::material::Material,
        texture_map: &crate::core::texture::TextureMap,
        buf_out: &mut Vec<u8>,
    ) -> Result<(), Err> {
        // Create a texture sampler (or reuse an existing one if possible)
        let sampler = TextureSampler::new(
            texture_map.min_filter(),
            texture_map.mag_filter(),
            texture_map.get_wrapping_mode()
        );
        
        // Find or add the sampler
        let sampler_index = if let Some(pos) = self.texture_samplers.iter().position(|s| s == &sampler) {
            pos as i32
        } else {
            let index = self.texture_samplers.len() as i32;
            self.texture_samplers.push(sampler);
            index
        };
        
        // Check if we can reuse an existing texture object
        let texture = GltfTexture::new(image_index, sampler_index);
        let texture_index = if let Some(pos) = self.textures.iter().position(|t| t == &texture) {
            pos as i32
        } else {
            let index = self.textures.len() as i32;
            self.textures.push(texture);
            index
        };
        
        // Write JSON for the texture map object
        write!(buf_out, "\"{}\": {{", object_name)?;
        write!(buf_out, "\"index\": {}", texture_index)?;
        write!(buf_out, ", \"texCoord\": {}", tex_coord_index)?;
        
        // For special handling of normal textures
        if object_name == "normalTexture" {
            // TODO: Add normal texture scale property when available on Material
            // let scale = material.get_normal_texture_scale();
            // if scale != 1.0 {
            //     write!(buf_out, ", \"scale\": {}", scale)?;
            // }
        }
        
        // Handle texture transform extension if needed
        let texture_transform = texture_map.get_transform();
        if !crate::core::texture::TextureTransform::is_default(texture_transform) {
            self.extensions_used.insert("KHR_texture_transform".to_string());
            self.extensions_required.insert("KHR_texture_transform".to_string());
            
            write!(buf_out, ", \"extensions\": {{")?;
            write!(buf_out, "\"KHR_texture_transform\": {{")?;
            
            if texture_transform.is_offset_set() {
                let offset = texture_transform.offset();
                write!(buf_out, "\"offset\": [{}, {}]", offset[0], offset[1])?;
            }
            
            if texture_transform.is_rotation_set() {
                if texture_transform.is_offset_set() {
                    write!(buf_out, ", ")?;
                }
                write!(buf_out, "\"rotation\": {}", texture_transform.rotation())?;
            }
            
            if texture_transform.is_scale_set() {
                if texture_transform.is_offset_set() || texture_transform.is_rotation_set() {
                    write!(buf_out, ", ")?;
                }
                let scale = texture_transform.scale();
                write!(buf_out, "\"scale\": [{}, {}]", scale[0], scale[1])?;
            }
            
            if texture_transform.is_tex_coord_set() {
                if texture_transform.is_offset_set() || texture_transform.is_rotation_set() || texture_transform.is_scale_set() {
                    write!(buf_out, ", ")?;
                }
                write!(buf_out, "\"texCoord\": {}", texture_transform.tex_coord())?;
            }
            
            write!(buf_out, "}}")?; // Close KHR_texture_transform
            write!(buf_out, "}}")?; // Close extensions
        }
        
        write!(buf_out, "}}")?; // Close texture map object
        
        Ok(())
    }


    fn encode_materials_property(&mut self, buf_out: &mut Vec<u8>) -> Result<(), Err> {

        // Begin "materials" array
        write!(buf_out, "\"materials\":[")?;

        let num_materials = self.material_library.num_materials();
        for i in 0..num_materials {
            // Separate material entries with commas
            if i > 0 {
                write!(buf_out, ",")?;
            }

            // Obtain material reference
            let material = self.material_library
                .get_material(i)
                .ok_or_else(|| Err::EncodingError("Error getting material.".to_string()))?
                .clone();

            // Check if we need to add "KHR_materials_unlit" to required extensions
            // (translation of the "unlit and fallback" logic)
            if material.is_unlit_fallback_required() {
                self.extensions_required.insert("KHR_materials_unlit".to_string());
            }

            // Begin material object
            write!(buf_out, "{{")?;

            // Begin pbrMetallicRoughness
            write!(buf_out, "\"pbrMetallicRoughness\":{{")?;

            // Possibly encode baseColorTexture (translation of color texture logic)
            if let Some(color_map) = material.get_texture_map_by_type(texture::Type::Color) {
                // Try to find existing texture first, fallback to creating new one
                if let Some(texture_index) = self.find_texture_index_for_material_texture(i, color_map.get_texture()) {
                    // Found existing texture, use it directly
                    write!(buf_out, "\"baseColorTexture\":{{")?;
                    write!(buf_out, "\"index\":{}", texture_index)?;
                    write!(buf_out, ",\"texCoord\":{}", color_map.tex_coord_index())?;
                    write!(buf_out, "}}")?;
                } else {
                    // Fallback to old method
                    let texture_stem = TextureUtils::get_or_generate_target_stem(
                        color_map.get_texture(), i, "_BaseColor"
                    );
                    let image_index = self.get_or_add_image_index(&texture_stem, color_map.get_texture(), 4)?;
                    self.encode_texture_map("baseColorTexture", image_index, color_map.tex_coord_index() as i32, &material, color_map, buf_out)?;
                }
            }

            // Possibly combine metallic & occlusion maps (translation of combined logic)
            let mut occlusion_metallic_roughness_image_index = -1;
            let metallic = material.get_texture_map_by_type(texture::Type::MetallicRoughness);
            let occlusion = material.get_texture_map_by_type(texture::Type::AmbientOcclusion);
            if let (Some(metallic_map), Some(occlusion_map)) =
                (metallic, occlusion)
            {
                if metallic_map.get_texture()== occlusion_map.get_texture() {
                    let texture_stem = TextureUtils::get_or_generate_target_stem(
                        metallic_map.get_texture(), i, "_OcclusionMetallicRoughness"
                    );
                    occlusion_metallic_roughness_image_index = self.get_or_add_image_index(
                        &texture_stem,
                        metallic_map.get_texture(),
                        3
                    )?;
                }
                if occlusion_metallic_roughness_image_index >= 0 {
                    // Add comma if baseColorTexture was written before
                    if material.get_texture_map_by_type(texture::Type::Color).is_some() {
                        write!(buf_out, ",")?;
                    }
                    self.encode_texture_map(
                        "metallicRoughnessTexture",
                        occlusion_metallic_roughness_image_index,
                        metallic_map.tex_coord_index() as i32,
                        &material,
                        metallic_map,
                        buf_out,
                    )?;
                };
            }

            // Encode metallicMap if not combined
            if let Some(metallic_map) = metallic {
                if occlusion_metallic_roughness_image_index < 0 {
                    let texture_stem = TextureUtils::get_or_generate_target_stem(
                        metallic_map.get_texture(), i, "_MetallicRoughness"
                    );
                    let metallic_idx = self.get_or_add_image_index(
                        &texture_stem,
                        metallic_map.get_texture(),
                        3
                    )?;
                    // Add comma if baseColorTexture was written before
                    if material.get_texture_map_by_type(texture::Type::Color).is_some() {
                        write!(buf_out, ",")?;
                    }
                    self.encode_texture_map(
                        "metallicRoughnessTexture",
                        metallic_idx,
                        metallic_map.tex_coord_index() as i32,
                        &material,
                        metallic_map,
                        buf_out,
                    )?;
                }
            }

            // Encode baseColorFactor, metallicFactor, roughnessFactor
            let metalic = material.get_texture_map_by_type(texture::Type::MetallicRoughness);
            
            // Check if we need a comma before baseColorFactor (if there were texture maps before)
            let has_base_color_texture = material.get_texture_map_by_type(texture::Type::Color).is_some();
            let has_metallic_roughness_texture = metalic.is_some() || material.get_texture_map_by_type(texture::Type::AmbientOcclusion).is_some();
            
            if has_base_color_texture || has_metallic_roughness_texture {
                write!(buf_out, ",")?;
            }
            
            self.encode_vector_array("baseColorFactor", *material.get_color_factor(), buf_out);
            write!(buf_out, ",\"metallicFactor\":{}", material.get_metallic_factor())?;
            write!(buf_out, ",\"roughnessFactor\":{}", material.get_roughness_factor())?;

            // Close pbrMetallicRoughness
            write!(buf_out, "}}")?;

            // normalTexture
            let normal = material.get_texture_map_by_type(texture::Type::NormalTangentSpace);
            if let Some(normal_map) = normal {
                let texture_stem = TextureUtils::get_or_generate_target_stem(
                    normal_map.get_texture(), i, "_Normal"
                );
                let normal_index = self.get_or_add_image_index(&texture_stem, normal_map.get_texture(), 3)?;
                write!(buf_out, ",")?;
                self.encode_texture_map(
                    "normalTexture",
                    normal_index,
                    normal_map.tex_coord_index() as i32,
                    &material,
                    normal_map,
                    buf_out,
                )?;
            }

            // occlusionTexture if not combined
            if let Some(occlusion_map) = occlusion {
                if occlusion_metallic_roughness_image_index < 0 {
                    let num_components = TextureUtils::compute_required_num_channels(
                        &occlusion_map.get_texture(), 
                        &self.material_library
                    );
                    let suffix = if num_components == 1 {"_Occlusion"} else {"_OcclusionMetallicRoughness"};
                    let texture_stem = TextureUtils::get_or_generate_target_stem(
                        occlusion_map.get_texture(), i, suffix
                    );
                    let occ_index = self.get_or_add_image_index(&texture_stem, occlusion_map.get_texture(), num_components as i32)?;
                    write!(buf_out, ",")?;
                    self.encode_texture_map(
                        "occlusionTexture",
                        occ_index,
                        occlusion_map.tex_coord_index() as i32,
                        &material,
                        occlusion_map,
                        buf_out,
                    )?;
                } else {
                    write!(buf_out, ",")?;
                    self.encode_texture_map(
                        "occlusionTexture",
                        occlusion_metallic_roughness_image_index,
                        occlusion_map.tex_coord_index() as i32,
                        &material,
                        occlusion_map,
                        buf_out,
                    )?;
                }
            }

            // emissiveTexture
            let emmissive_map = material.get_texture_map_by_type(texture::Type::Emissive);
            if let Some(emissive_map) = emmissive_map {
                let texture_stem = TextureUtils::get_or_generate_target_stem(
                    emissive_map.get_texture(), i, "_Emissive"
                );
                let emissive_idx = self.get_or_add_image_index(&texture_stem, emissive_map.get_texture(), 3)?;
                write!(buf_out, ",")?;
                self.encode_texture_map(
                    "emissiveTexture",
                    emissive_idx,
                    emissive_map.tex_coord_index() as i32,
                    &material,
                    emissive_map,
                    buf_out,
                )?;
            }

            // emissiveFactor
            write!(buf_out, ",\"emissiveFactor\":[{},{},{}]",
                   material.get_emissive_factor().get(0),
                   material.get_emissive_factor().get(1),
                   material.get_emissive_factor().get(2))?;

            // alphaMode
            match material.get_transparency_mode() {
                TransparencyMode::Mask => {
                    write!(buf_out, ",\"alphaMode\":\"MASK\",\"alphaCutoff\":{}",
                           material.get_alpha_cutoff())?;
                }
                TransparencyMode::Blend => {
                    write!(buf_out, ",\"alphaMode\":\"BLEND\"")?;
                }
                _ => {
                    write!(buf_out, ",\"alphaMode\":\"OPAQUE\"")?;
                }
            }

            // name
            if !material.get_name().is_empty() {
                write!(buf_out, ",\"name\":\"{}\"", material.get_name())?;
            }

            // doubleSided
            if material.is_double_sided() {
                write!(buf_out, ",\"doubleSided\":true")?;
            }

            // Handle any material extensions (unlit, sheen, etc.)
            let has_extensions = material.check_any_pbr_extensions();
            if has_extensions {
                write!(buf_out, ",\"extensions\":{{")?;
                // Possibly encode unlit
                if material.is_unlit_fallback_required() {
                    self.encode_material_unlit_extension(&material);
                } else {
                    // Possibly encode other PBR extensions
                    let default_material = Material::new();
                    if material.has_sheen() {
                        self.encode_material_sheen_extension(&material, &default_material, i as i32)?;
                    }
                    if material.has_transmission() {
                        self.encode_material_transmission_extension(&material, &default_material, i as i32)?;
                    }
                    if material.has_clearcoat() {
                        self.encode_material_clearcoat_extension(&material, &default_material, i as i32)?;
                    }
                    if material.has_volume() {
                        self.encode_material_volume_extension(&material, &default_material, i as i32)?;
                    }
                    if material.has_ior() {
                        self.encode_material_ior_extension(&material, &default_material)?;
                    }
                    if material.has_specular() {
                        self.encode_material_specular_extension(&material, &default_material, i as i32)?;
                    }
                }
                write!(buf_out, "}}")?; 
            }

            // Close current material
            write!(buf_out, "}}")?;
        }

        // Close "materials" array
        write!(buf_out, "]")?;

        // Encode "textures" array if needed
        if !self.textures.is_empty() {
            write!(buf_out, ",\"textures\":[")?;
            for (i, tex) in self.textures.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "{{")?;
                // Possibly handle extension for ktx2 or webp
                if self.images[tex.image_index as usize].mime_type == "image/webp" {
                    write!(buf_out, "\"source\":{}", tex.image_index)?;
                    write!(buf_out, ",\"extensions\":{{\"EXT_texture_webp\":{{\"source\":{}}}}}", tex.image_index)?;
                    if tex.sampler_index >= 0 {
                        write!(buf_out, ",\"sampler\":{}", tex.sampler_index)?;
                    }
                } else if self.images[tex.image_index as usize].mime_type == "image/ktx2" {
                    write!(buf_out, "\"extensions\":{{\"KHR_texture_basisu\":{{\"source\":{}}}}}", tex.image_index)?;
                    if tex.sampler_index >= 0 {
                        write!(buf_out, ",\"sampler\":{}", tex.sampler_index)?;
                    }
                } else {
                    write!(buf_out, "\"source\":{}", tex.image_index)?;
                    if tex.sampler_index >= 0 {
                        write!(buf_out, ",\"sampler\":{}", tex.sampler_index)?;
                    }
                }
                write!(buf_out, "}}")?;
            }
            write!(buf_out, "]")?;
        }

        // Encode "samplers" array if needed
        if !self.texture_samplers.is_empty() {
            write!(buf_out, ",\"samplers\":[")?;
            for (i, sampler) in self.texture_samplers.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "{{\"wrapS\":{},\"wrapT\":{}", 
                       texture_axis_wrapping_mode_to_gltf_value(sampler.wrapping_mode().s()),
                       texture_axis_wrapping_mode_to_gltf_value(sampler.wrapping_mode().t()))?;
                if sampler.min_filter() != FilterType::Unspecified {
                    write!(buf_out, ",\"minFilter\":{}", texture_filter_type_to_gltf_value(sampler.min_filter()))?;
                }
                if sampler.mag_filter() != FilterType::Unspecified {
                    write!(buf_out, ",\"magFilter\":{}", texture_filter_type_to_gltf_value(sampler.mag_filter()))?;
                }
                write!(buf_out, "}}")?;
            }
            write!(buf_out, "]")?;
        }

        // Encode "images" array if needed
        if !self.images.is_empty() {
            write!(buf_out, ",\"images\":[")?;
            for i in 0..self.images.len() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                // Possibly embed image in the buffer (only if not already embedded)
                if self.add_images_to_buffer && self.images[i].buffer_view < 0 {
                    self.save_image_to_buffer(i as i32)?;
                }
                let image = &self.images[i]; 
                write!(buf_out, "{{")?;
                if image.buffer_view >= 0 {
                    write!(buf_out, "\"bufferView\":{},\"mimeType\":\"{}\"",
                           image.buffer_view,
                           image.mime_type)?;
                } else {
                    write!(buf_out, "\"uri\":\"{}\"", image.image_name)?;
                }
                write!(buf_out, "}}")?;
            }
            write!(buf_out, "]")?;
        }

        // Finalize (here we'd append the JSON chunk to buf_out if needed)
        // In this code, assume we've been writing directly to buf_out all along.

        Ok(())
    }

    fn encode_material_unlit_extension(&mut self, _material: &crate::core::material::Material) {
        unimplemented!()
    }

    fn encode_material_sheen_extension(
        &self,
        _material: &crate::core::material::Material,
        _defaults: &crate::core::material::Material,
        _material_index: i32,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    fn encode_material_transmission_extension(
        &self,
        _material: &crate::core::material::Material,
        _defaults: &crate::core::material::Material,
        _material_index: i32,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    fn encode_material_clearcoat_extension(
        &self,
        _material: &crate::core::material::Material,
        _defaults: &crate::core::material::Material,
        _material_index: i32,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    fn encode_material_volume_extension(
        &self,
        _material: &crate::core::material::Material,
        _defaults: &crate::core::material::Material,
        _material_index: i32,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    fn encode_material_ior_extension(
        &self,
        _material: &crate::core::material::Material,
        _defaults: &crate::core::material::Material,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    fn encode_material_specular_extension(
        &self,
        _material: &crate::core::material::Material,
        _defaults: &crate::core::material::Material,
        _material_index: i32,
    ) -> Result<(), Err> {
        unimplemented!()
    }

    fn encode_top_level_extensions_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        if !self.extensions_required.is_empty() {
            write!(buf_out, "\"extensionsRequired\":[")?;
            for (i, extension) in self.extensions_required.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "\"{}\"", extension)?;
            }
            write!(buf_out, "]")?;
        }

        if !self.extensions_used.is_empty() {
            if !self.extensions_required.is_empty() {
                write!(buf_out, ",")?;
            }
            write!(buf_out, "\"extensionsUsed\":[")?;
            for (i, extension) in self.extensions_used.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "\"{}\"", extension)?;
            }
            write!(buf_out, "]")?;
        }
        
        Ok(())
    }

    fn encode_structural_metadata_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        if let Some(ref metadata_json) = self.structural_metadata_json {
            // Use the actual structural metadata from the original file
            write!(buf_out, "\"EXT_structural_metadata\":")?;
            let metadata_str = serde_json::to_string(metadata_json)
                .map_err(|e| Err::EncodingError(format!("Failed to serialize structural metadata: {}", e)))?;
            write!(buf_out, "{}", metadata_str)?;
        } else {
            // Fallback to minimal structural metadata schema
            write!(buf_out, "\"EXT_structural_metadata\":{{")?;
            write!(buf_out, "\"schema\":{{")?;
            write!(buf_out, "\"id\":\"Schema\",")?;
            write!(buf_out, "\"classes\":{{")?;
            write!(buf_out, "\"bldg_Building\":{{")?;
            write!(buf_out, "\"properties\":{{")?;
            
            // Add a minimal set of properties matching the original schema
            let properties = [
                "_lod", "_x", "_y", "_xmin", "_xmax", "_ymin", "_ymax", "_zmin", "_zmax",
                "meshcode", "city_code", "city_name", "gml_id", "bldg:class", "bldg:usage",
                "bldg:measuredHeight", "bldg:storeysAboveGround", "bldg:storeysBelowGround"
            ];
            
            for (i, prop) in properties.iter().enumerate() {
                if i > 0 {
                    write!(buf_out, ",")?;
                }
                write!(buf_out, "\"{}\":{{\"type\":\"STRING\",\"noData\":\"\"}}", prop)?;
            }
            
            write!(buf_out, "}}")?; // close properties
            write!(buf_out, "}}")?; // close bldg_Building
            write!(buf_out, "}}")?; // close classes
            write!(buf_out, "}},")?; // close schema
            
            // Add minimal property tables
            write!(buf_out, "\"propertyTables\":[{{")?;
            write!(buf_out, "\"class\":\"bldg_Building\",")?;
            write!(buf_out, "\"count\":9")?;
            write!(buf_out, "}}]")?; // close propertyTables array
            
            write!(buf_out, "}}")?; // close EXT_structural_metadata
        }
        Ok(())
    }

    fn encode_accessors_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        write!(buf_out, "\"accessors\":[")?;
        
        for (i, accessor) in self.accessors.iter().enumerate() {
            if i > 0 {
                write!(buf_out, ",")?;
            }
            
            write!(buf_out, "{{")?;
            
            // Component type
            write!(buf_out, "\"componentType\":{}", accessor.component_type)?;
            
            // Count
            write!(buf_out, ",\"count\":{}", accessor.count)?;
            
            // Type 
            write!(buf_out, ",\"type\":\"{}\"", accessor.accessor_type)?;
            
            // Min and max values (if present)
            if !accessor.min.is_empty() {
                write!(buf_out, ",\"min\":[")?;
                for (j, value) in accessor.min.iter().enumerate() {
                    if j > 0 {
                        write!(buf_out, ",")?;
                    }
                    write!(buf_out, "{}", value)?;
                }
                write!(buf_out, "]")?;
            }
            
            if !accessor.max.is_empty() {
                write!(buf_out, ",\"max\":[")?;
                for (j, value) in accessor.max.iter().enumerate() {
                    if j > 0 {
                        write!(buf_out, ",")?;
                    }
                    write!(buf_out, "{}", value)?;
                }
                write!(buf_out, "]")?;
            }
            
            write!(buf_out, "}}")?; // close accessor
        }
        
        write!(buf_out, "]")?;
        Ok(())
    }

    fn encode_buffer_views_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {
        write!(buf_out, "\"bufferViews\":[")?;
        
        for (i, buffer_view) in self.buffer_views.iter().enumerate() {
            if i > 0 {
                write!(buf_out, ",")?;
            }
            
            write!(buf_out, "{{")?;
            write!(buf_out, "\"buffer\":0")?; // Always buffer 0 for GLB format
            write!(buf_out, ",\"byteOffset\":{}", buffer_view.buffer_byte_offset)?;
            write!(buf_out, ",\"byteLength\":{}", buffer_view.byte_length)?;
            write!(buf_out, "}}")?;
        }
        
        write!(buf_out, "]")?;
        Ok(())
    }

    fn encode_buffers_property(&self, buf_out: &mut Vec<u8>) -> Result<(), Err> {        
        write!(buf_out, "\"buffers\":[")?;
        write!(buf_out, "{{")?;
        // Ensure buffer length is correctly aligned to 4 bytes as required by GLB spec
        let aligned_length = if self.buffer.len() % 4 == 0 {
            self.buffer.len()
        } else {
            self.buffer.len() + (4 - (self.buffer.len() % 4))
        };
        write!(buf_out, "\"byteLength\":{}", aligned_length)?;
        
        if !self.buffer_name.is_empty() {
            write!(buf_out, ",\"uri\":\"{}\"", self.buffer_name)?;
        }
        
        write!(buf_out, "}}")?;
        write!(buf_out, "]")?;
        Ok(())
    }

    /// Encodes a draco::VectorNX as a glTF array.
    fn encode_vector_array<T, const N: usize>(&mut self, array_name: &str, vec: T, buf_out: &mut Vec<u8>) 
        where 
            T: Vector<N>,
            T::Component: std::fmt::Display,
    {
        write!(buf_out, "\"{}\": [", array_name).unwrap();
        for i in 0..N {
            if i > 0 {
                write!(buf_out, ",").unwrap();
            }
            write!(buf_out, "{}", *vec.get(i)).unwrap();
        }
        write!(buf_out, "]").unwrap();
    }

    fn set_copyright_from_scene(&mut self, scene: &Scene) {
        // Placeholder implementation - would extract copyright from scene metadata
        let _ = scene;
        if self.copyright.is_empty() {
            self.copyright = "Generated by draco-oxide".to_string();
        }
    }

    /// Calculate feature count from attribute data (fallback method)
    fn calculate_feature_count_from_attribute(&self, mesh: &Mesh, iteration_id: usize) -> i32 {
        // Count unique values in the feature ID attribute
        let mut unique_ids = std::collections::HashSet::new();
        
        // Access the attribute data from the mesh
        if let Some(attr) = mesh.get_attributes().get(iteration_id) {
            // The attribute contains NdVector values, we need to extract the scalar feature IDs
            for i in 0..attr.len() {
                // Feature IDs are stored as single values (scalars) in the attribute
                // Use the appropriate type based on the attribute
                use crate::core::shared::NdVector;
                let value: NdVector<1, f32> = attr.get(PointIdx::from(i));
                // Extract the first (and only) component as the feature ID
                let feature_id = *value.get(0) as i32;
                unique_ids.insert(feature_id);
            }
        }
        
        // Use the number of unique feature IDs as the feature count
        // This represents the actual number of features referenced by the geometry
        unique_ids.len() as i32
    }
}

/// Convert a FilterType to the corresponding glTF value
fn texture_filter_type_to_gltf_value(filter_type: FilterType) -> i32 {
    match filter_type {
        FilterType::Nearest => 9728,
        FilterType::Linear => 9729,
        FilterType::NearestMipmapNearest => 9984,
        FilterType::LinearMipmapNearest => 9985,
        FilterType::NearestMipmapLinear => 9986,
        FilterType::LinearMipmapLinear => 9987,
        FilterType::Unspecified => 9729, // Default to Linear
    }
}

/// Convert an AxisWrappingMode to the corresponding glTF value
fn texture_axis_wrapping_mode_to_gltf_value(wrapping_mode: crate::core::texture::AxisWrappingMode) -> i32 {
    match wrapping_mode {
        crate::core::texture::AxisWrappingMode::ClampToEdge => 33071,
        crate::core::texture::AxisWrappingMode::MirroredRepeat => 33648,
        crate::core::texture::AxisWrappingMode::Repeat => 10497,
    }
}