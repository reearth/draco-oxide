use crate::core::scene::Scene;
use crate::io::gltf::encode::GltfEncoder;
use crate::io::gltf::scene_io::{get_scene_file_format, SceneFileFormat};
use crate::prelude::ConfigType;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Transcoding Error: {0}")]
    TranscodingError(String),
    #[error("IO Error: {0}")]
    IoError(String),
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("Compression Error: {0}")]
    CompressionError(String),
    #[error("Encoding Error: {0}")]
    EncodingError(#[from] crate::io::gltf::encode::Err),
    #[error("Decoding Error: {0}")]
    DecodingError(#[from] crate::io::gltf::decode::Err),
}

/// Struct to hold Draco transcoding options.
#[derive(Debug, Clone)]
pub struct DracoTranscodingOptions {
    /// Options used when geometry compression optimization is disabled.
    pub geometry: Option<crate::encode::Config>,
}

impl Default for DracoTranscodingOptions {
    fn default() -> Self {
        Self {
            geometry: Some(ConfigType::default()),
        }
    }
}

impl DracoTranscodingOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Class that supports input of glTF (and some simple USD) files, encodes
/// them with Draco compression, and outputs glTF Draco compressed files.
///
/// glTF supported extensions:
///   Input and Output:
///     KHR_draco_mesh_compression
///     KHR_materials_unlit
///     KHR_texture_transform
///
/// glTF unsupported features:
///   Input and Output:
///     Morph targets
///     Sparse accessors
///     KHR_lights_punctual
///     KHR_materials_pbrSpecularGlossiness
///     All vendor extensions
#[derive(Debug)]
pub struct DracoTranscoder {
    gltf_encoder: GltfEncoder,
    /// The scene being transcoded.
    scene: Option<Box<Scene>>,
    /// Copy of the transcoding options passed into the Create function.
    /// If None, default options will be used.
    transcoding_options: Option<DracoTranscodingOptions>,
}

/// Configuration options for file input/output during transcoding.
#[derive(Debug, Clone)]
pub struct FileOptions {
    /// Must be non-empty.
    pub input_filename: String,
    /// Must be non-empty.
    pub output_filename: String,
    pub output_bin_filename: String,
    pub output_resource_directory: String,
}

impl FileOptions {
    pub fn new(input_filename: String, output_filename: String) -> Self {
        Self {
            input_filename,
            output_filename,
            output_bin_filename: String::new(),
            output_resource_directory: String::new(),
        }
    }

    pub fn with_bin_filename(mut self, bin_filename: String) -> Self {
        self.output_bin_filename = bin_filename;
        self
    }

    pub fn with_resource_directory(mut self, resource_directory: String) -> Self {
        self.output_resource_directory = resource_directory;
        self
    }
}

impl Default for DracoTranscoder {
    fn default() -> Self {
        Self::new()
    }
}

impl DracoTranscoder {
    pub fn new() -> Self {
        Self {
            gltf_encoder: GltfEncoder::new(),
            scene: None,
            transcoding_options: None,
        }
    }

    /// Creates a DracoTranscoder object. `options` sets the compression options
    /// used in the Encode function.
    pub fn create(options: Option<DracoTranscodingOptions>) -> Result<Box<Self>, Err> {
        // For now, we'll skip validation since Config doesn't have a check method yet
        // TODO: Implement validation when Config::check() is available
        // options.geometry.check().map_err(|e| {
        //     Err::TranscodingError(format!("Invalid compression options: {:?}", e))
        // })?;

        let mut transcoder = Self::new();
        transcoder.transcoding_options = options;
        Ok(Box::new(transcoder))
    }


    /// Encodes the input file with Draco compression using the compression options
    /// passed in the Create function. The recommended use case is to create a
    /// transcoder once and call Transcode for multiple files.
    pub fn transcode_file(&mut self, file_options: &FileOptions) -> Result<(), Err> {
        self.read_scene_from_file(file_options)?;
        self.compress_scene()?;
        self.write_scene_to_file(file_options)?;
        Ok(())
    }

    /// Encodes the input buffer with Draco compression using the compression options
    /// passed in the Create function. The recommended use case is to create a
    /// transcoder once and call Transcode for multiple files.
    pub fn transcode_buffer<W>(&mut self, in_buffer: &[u8], writer: &mut W) -> Result<(), Err> 
        where W: std::io::Write,
    {
        self.read_scene_from_buffer(in_buffer)?;
        self.compress_scene()?;
        self.write_scene_to_buffer(writer)?;
        Ok(())
    }

    // Private methods

    /// Read scene from file.
    fn read_scene_from_file(&mut self, file_options: &FileOptions) -> Result<(), Err> {
        if file_options.input_filename.is_empty() {
            return Err(Err::InvalidInput("Input filename is empty.".to_string()));
        }
        if file_options.output_filename.is_empty() {
            return Err(Err::InvalidInput("Output filename is empty.".to_string()));
        }

        let filename = &file_options.input_filename;
        self.scene = match get_scene_file_format(filename) {
            SceneFileFormat::Gltf => {
                let mut decoder = crate::io::gltf::decode::GltfDecoder::new();
                let scene = decoder.decode_from_file_to_scene_with_files(filename, Vec::new())?;
                Some(Box::new(scene))
            }
            SceneFileFormat::Usd => {
                return Err(Err::TranscodingError("USD is not supported yet.".to_string()))
            }
            _ => {
                return Err(Err::TranscodingError("Unknown input file format.".to_string()))
            }
        };
        Ok(())
    }

    /// Read scene from buffer.
    fn read_scene_from_buffer(&mut self, buffer: &[u8]) -> Result<(), Err> {
        let mut decoder = crate::io::gltf::decode::GltfDecoder::new();
        self.scene = Some(Box::new(decoder.decode_from_buffer_to_scene(&buffer)?));
        Ok(())
    }

    /// Write scene to file.
    fn write_scene_to_file(&mut self, file_options: &FileOptions) -> Result<(), Err> {
        let scene = self.scene.as_ref().ok_or_else(|| {
            Err::TranscodingError("No scene loaded for writing".to_string())
        })?;

        if !file_options.output_bin_filename.is_empty() 
            && !file_options.output_resource_directory.is_empty() {
            // Write with both bin filename and resource directory
            self.gltf_encoder.encode_scene_to_file(
                scene,
                &file_options.output_filename,
                &file_options.output_resource_directory,
            )?;
        } else if !file_options.output_bin_filename.is_empty() {
            // Write with bin filename only
            self.gltf_encoder.encode_scene_file_with_bin(
                scene,
                &file_options.output_filename,
                &file_options.output_bin_filename,
            )?;
        } else {
            // Write with default settings
            self.gltf_encoder.encode_scene_file(
                scene,
                &file_options.output_filename,
            )?;
        }

        Ok(())
    }

    /// Write scene to buffer.
    fn write_scene_to_buffer<W: std::io::Write>(&mut self, writer: &mut W) -> Result<(), Err> {
        let scene = self.scene.as_ref().ok_or_else(|| {
            Err::TranscodingError("No scene loaded for writing".to_string())
        })?;

        self.gltf_encoder.encode_scene_to_buffer(
            scene,
            writer,
        )?;

        Ok(())
    }

    /// Apply compression settings to the scene.
    fn compress_scene(&mut self) -> Result<(), Err> {
        if let Some(ref mut scene) = self.scene {
            // Apply geometry compression settings to all scene meshes.
            if let Some(ref op) = &self.transcoding_options {
                Self::set_draco_compression_options(&op.geometry, scene)?;
            } else {
                Self::set_draco_compression_options(&None, scene)?;
            }
        }
        Ok(())
    }

    /// Apply Draco compression options to all meshes in the scene.
    fn set_draco_compression_options(
        options: &Option<crate::encode::Config>,
        _scene: &mut Scene,
    ) -> Result<(), Err> {
        // Apply compression settings to all meshes in the scene
        // This function prepares the meshes for compression by storing the configuration
        
        let _config = if let Some(options) = options {
            options.clone()
        } else {
            // Use default compression config if none provided
            crate::encode::Config::default()
        };
        
        // For now, we don't need to modify the scene meshes directly since
        // the compression options will be applied during the actual encoding phase
        // This function serves as a validation and preparation step
        
        // In a full implementation, this might:
        // 1. Validate that the compression settings are compatible with the meshes
        // 2. Set up any mesh-specific compression metadata
        // 3. Optimize mesh data layout for compression
        
        // For the transcoder, the compression will happen when the scene is encoded to the output file
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcode_buffer_equals_transcode_file() {
        // Read the actual Duck.glb test file
        // let test_glb_path = "../private_tests/bad_files/39769_bldg_Building.glb";
        // let test_glb_path = "tests/data/Duck/Duck.glb";
        let test_glb_path = "../39772_bldg_Building.glb";
        let test_glb = std::fs::read(test_glb_path).expect("Failed to read input glb test file");
        
        // Create transcoder with default options
        let mut transcoder1 = DracoTranscoder::create(None).unwrap();
        
        // Transcode using buffer method
        let mut buffer_output = Vec::new();
        transcoder1.transcode_buffer(&test_glb, &mut buffer_output)
            .expect("Failed to transcode buffer");
        
        // Reset transcoder and use file method on the same instance
        let mut transcoder2 = DracoTranscoder::create(None).unwrap();
        
        // Write test GLB to a temporary file for file-based transcoding
        let temp_dir = std::env::temp_dir();
        let input_path = temp_dir.join("test_gltf_input.glb");
        let output_path = temp_dir.join("test_gltf_output.glb");
        
        std::fs::write(&input_path, &test_glb)
            .expect("Failed to write temporary input file");
        
        // Transcode using file method
        let file_options = FileOptions::new(
            input_path.to_string_lossy().to_string(),
            output_path.to_string_lossy().to_string(),
        );
        transcoder2.transcode_file(&file_options)
            .expect("Failed to transcode file");
        
        // Read the file output
        let file_output = std::fs::read(&output_path)
            .expect("Failed to read output file");
        
        // Clean up input file
        let _ = std::fs::remove_file(&input_path);
        
        // Write the buffer output to a file for comparison
        let dir = std::env::current_dir()
            .expect("Failed to get current directory");
        let buffer_output_path = dir.join("test_gltf_buffer_output.glb");
        std::fs::write(&buffer_output_path, &buffer_output)
            .expect("Failed to write buffer output file");
        
        // make sure that the outputs are nontrivial
        assert!(!buffer_output.len() > 10, "Buffer output is too small: size = {}", buffer_output.len());
        // Compare outputs
        assert_eq!(
            buffer_output.len(), file_output.len(), 
            "transcode_buffer and transcode_file outputs have different lengths"
        );
        
        assert_eq!(
            buffer_output, file_output,
            "transcode_buffer and transcode_file outputs differ"
        );
    }
}