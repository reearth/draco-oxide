use crate::core::scene::Scene;
use crate::io::gltf::decode::GltfDecoder;
use crate::io::gltf::encode::GltfEncoder;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Error: {0}")]
    Error(String),
    #[error("GLTF Encoder Error: {0}")]
    GltfEncoderError(#[from] crate::io::gltf::encode::Err),
    #[error("GLTF Decoder Error: {0}")]
    GltfDecoderError(#[from] crate::io::gltf::decode::Err),
}

/// Supported scene file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneFileFormat {
    Unknown,
    Gltf,
    Usd,
    Ply,
    Obj,
}

/// Configuration options for scene writing
#[derive(Debug, Default, Clone)]
pub struct SceneWriteOptions {
    /// Forces implicit vertex interpolation while exporting to USD
    pub force_usd_vertex_interpolation: bool,
}

/// Reads a scene from a file. Currently only GLTF 2.0 scene files are supported.
pub fn read_scene_from_file(file_name: &str) -> Result<Scene, Err> {
    read_scene_from_file_with_files(file_name, Vec::new())
}

/// Reads a scene from a file and optionally returns the files associated with the scene.
pub fn read_scene_from_file_with_files(
    file_name: &str,
    scene_files: Vec<String>,
) -> Result<Scene, Err> {
    match get_scene_file_format(file_name) {
        SceneFileFormat::Gltf => {
            let mut decoder = GltfDecoder::new();
            Ok(decoder.decode_from_file_to_scene_with_files(file_name, scene_files)?)
        }
        SceneFileFormat::Usd => {
            Err(Err::Error(format!("USD is not supported yet.")))
        }
        _ => {
            Err(Err::Error(format!("Unknown input file format.")))
        }
    }
}


/// Writes a scene into a file with configurable options.
///
/// Supported options:
/// - `force_usd_vertex_interpolation`: forces implicit vertex interpolation 
///   while exporting to USD (default = false)
pub fn write_scene_to_file_with_options(
    file_name: &str,
    scene: &Scene,
    _options: &SceneWriteOptions,
) -> Result<(), Err> {
    let (folder_path, _out_file_name) = {
        let path = std::path::Path::new(file_name);
        let folder_path = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let out_file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("output");
        (folder_path.to_string_lossy().to_string(), out_file_name.to_string())
    };
    let format = get_scene_file_format(file_name);
    
    match format {
        SceneFileFormat::Gltf => {
            let mut encoder = GltfEncoder::new();
            encoder.encode_scene_to_file(scene, file_name, &folder_path)?;
            Ok(())
        }
        SceneFileFormat::Usd => {
            Err(Err::Error(format!("USD is not supported yet.")))
        }
        SceneFileFormat::Ply | SceneFileFormat::Obj => {
            unimplemented!("Ply and Obj formats are not supported yet.");
            // // Convert the scene to mesh and save the scene as a mesh. For now we do
            // // that by converting the scene to GLB and decoding the GLB into a mesh.
            // let gltf_encoder = GltfEncoder::new();
            // let mut buffer = Vec::<u8>::new();
            // gltf_encoder.encode_scene_to_buffer(scene, &mut buffer)?;
            
            // let gltf_decoder = GltfDecoder::new();
            // let mesh = gltf_decoder.decode_from_buffer(&buffer)?;
            
            // match format {
            //     SceneFileFormat::Ply => {
            //         let ply_encoder = PlyEncoder::new();
            //         if !ply_encoder.encode_to_file(&mesh, file_name) {
            //             return Err(Err::Error(format!("Failed to encode the scene as PLY.")));
            //         }
            //     }
            //     SceneFileFormat::Obj => {
            //         let obj_encoder = ObjEncoder::new();
            //         if !obj_encoder.encode_to_file(&mesh, file_name) {
            //             return Err(Err::Error(format!("Failed to encode the scene as OBJ.")));
            //         }
            //     }
            //     _ => unreachable!(),
            // }
            // Ok(())
        }
        SceneFileFormat::Unknown => {
            Err(Err::Error(format!("Unknown output file format.")))
        }
    }
}

/// Determines the scene file format based on the file extension.
pub fn get_scene_file_format(file_name: &str) -> SceneFileFormat {
    //get the file extension
    let extension = match file_name.rfind('.') {
        Some(pos) => &file_name[pos + 1..],
        None => return SceneFileFormat::Unknown,
    };
    
    match extension {
        "gltf" | "glb" => SceneFileFormat::Gltf,
        "usd" | "usda" | "usdc" | "usdz" => SceneFileFormat::Usd,
        "obj" => SceneFileFormat::Obj,
        "ply" => SceneFileFormat::Ply,
        _ => SceneFileFormat::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_scene_file_format() {
        assert_eq!(get_scene_file_format("model.gltf"), SceneFileFormat::Gltf);
        assert_eq!(get_scene_file_format("model.glb"), SceneFileFormat::Gltf);
        assert_eq!(get_scene_file_format("model.usd"), SceneFileFormat::Usd);
        assert_eq!(get_scene_file_format("model.usda"), SceneFileFormat::Usd);
        assert_eq!(get_scene_file_format("model.usdc"), SceneFileFormat::Usd);
        assert_eq!(get_scene_file_format("model.usdz"), SceneFileFormat::Usd);
        assert_eq!(get_scene_file_format("model.obj"), SceneFileFormat::Obj);
        assert_eq!(get_scene_file_format("model.ply"), SceneFileFormat::Ply);
        assert_eq!(get_scene_file_format("model.xyz"), SceneFileFormat::Unknown);
    }

    #[test]
    fn test_scene_write_options_default() {
        let options = SceneWriteOptions::default();
        assert!(!options.force_usd_vertex_interpolation);
    }
}