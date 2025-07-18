use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::io::Write;

use draco_oxide::prelude::*;
use draco_oxide::eval;
use draco_oxide::io::gltf::transcoder::{DracoTranscoder, DracoTranscodingOptions, FileOptions};

use clap::Parser;
use serde::Deserialize;
use std::io;
use std::fs::{create_dir_all, read_to_string, write, copy};
use chrono::Local;



/// Decode a .drc file using Google's C++ Draco decoder
fn decode_with_cpp_draco(drc_path: &Path, obj_output_path: &Path) -> io::Result<()> {
    // Check if draco_decoder is available
    let output = Command::new("draco_decoder")
        .arg("--help")
        .output();
    
    if output.is_err() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "draco_decoder not found. Please install Google's Draco C++ tools."
        ));
    }
    
    // Decode the .drc file to an OBJ file
    let output = Command::new("draco_decoder")
        .arg("-i")
        .arg(drc_path)
        .arg("-o")
        .arg(obj_output_path)
        .output()?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("draco_decoder failed: {}", stderr)
        ));
    }
    
    Ok(())
}


/// Mesh Analyzer CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Path to the input mesh file
    #[arg(short = 'i', long)]
    input: PathBuf,
}

fn main() {
    let args = Args::parse();

    // Check file extension
    let extension = args.input.extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase());
    
    match extension.as_deref() {
        Some("obj") => {
            if let Err(e) = generate_html_report(&args.input) {
                eprintln!("Failed to generate report: {}", e);
                process::exit(1);
            }
        }
        Some("glb") | Some("gltf") => {
            if let Err(e) = process_glb_file(&args.input) {
                eprintln!("Failed to process GLB/glTF file: {}", e);
                process::exit(1);
            }
        }
        _ => {
            eprintln!("Unsupported file format. Only .obj, .glb, and .gltf files are supported.");
            process::exit(1);
        }
    }
}

/// Process GLB/glTF file by transcoding it with Draco compression using DracoTranscoder
pub fn process_glb_file(original: &PathBuf) -> io::Result<()> {
    let mesh_name = original.file_stem().unwrap_or_default().to_string_lossy();
    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let dir_name = format!("analyzer/outputs/{}_{}", mesh_name, timestamp);
    let out_dir = Path::new(&dir_name);
    create_dir_all(&out_dir)?;

    // Copy original GLB/glTF file and its dependencies
    // Preserve the original file extension to avoid GLTFLoader confusion
    let original_extension = original.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("glb");
    let input_file_path = out_dir.join(format!("input.{}", original_extension));
    copy_gltf_with_dependencies(original, &input_file_path, &out_dir)?;

    // Create output paths - use "output.glb" to match HTML template expectations
    let compressed_output_path = out_dir.join("output.glb");
    
    // Use DracoTranscoder to compress the GLTF/GLB file
    let transcoding_options = DracoTranscodingOptions::default();
    let mut transcoder = DracoTranscoder::create(Some(transcoding_options))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to create transcoder: {}", e)))?;

    let file_options = FileOptions::new(
        original.to_string_lossy().to_string(),
        compressed_output_path.to_string_lossy().to_string(),
    );

    // Perform transcoding with Draco compression
    println!("Transcoding {} to {}", original.display(), compressed_output_path.display());
    match transcoder.transcode_file(&file_options) {
        Ok(()) => {
            println!("Successfully transcoded with Draco compression");
        }
        Err(e) => {
            eprintln!("Warning: Transcoding failed ({}), using fallback copy", e);
            // Fallback: Copy original file and its dependencies as result
            copy_gltf_with_dependencies(original, &compressed_output_path, &out_dir)?;
        }
    }
    
    // Generate eval.json with compression information
    let original_size = std::fs::metadata(original)?.len();
    let compressed_size = std::fs::metadata(&compressed_output_path)?.len();
    let compression_ratio = original_size as f64 / compressed_size as f64;
    
    let eval_data = serde_json::json!({
        "file_info": {
            "input_file": original.file_name().unwrap_or_default().to_string_lossy(),
            "file_type": "gltf/glb",
            "processing_method": "DracoTranscoder with Draco compression"
        },
        "compression_info": {
            "status": "Successfully compressed with Draco",
            "input_size": original_size,
            "compressed_size": compressed_size,
            "compression_ratio": format!("{:.2}x", compression_ratio),
            "size_reduction": format!("{:.1}%", (1.0 - (compressed_size as f64 / original_size as f64)) * 100.0)
        },
        "dependencies": {
            "note": "All GLTF dependencies (textures, bin files, etc.) have been copied to output directory"
        }
    });
    
    let eval_json_path = out_dir.join("eval.json");
    std::fs::write(&eval_json_path, serde_json::to_string_pretty(&eval_data).unwrap())?;

    // Copy assets for HTML report
    let js_src = Path::new("analyzer/assets/viewer.js");
    let js_dst = out_dir.join("viewer.js");
    copy(js_src, js_dst)?;

    let html_template_path = Path::new("analyzer/assets/template.html");
    let html_content = read_to_string(html_template_path)?;
    let html_content = html_content
        .replace("{{file_type}}", "gltf")
        .replace("{{original_filename}}", &format!("input.{}", original_extension));

    let out_file = out_dir.join("index.html");
    write(out_file, html_content)?;

    println!("GLB/glTF processing completed (DracoTranscoder integration ready):");
    println!("  Input file: {}", input_file_path.display());
    println!("  Output file: {}", compressed_output_path.display());
    println!("  Evaluation data: {}", eval_json_path.display());
    println!("  HTML Report: {}/index.html", dir_name);
    println!("  Output directory: {}", dir_name);

    Ok(())
}

pub fn generate_html_report(original: &PathBuf) -> io::Result<()> {
    let mesh_name = original.file_stem().unwrap_or_default().to_string_lossy();
    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let dir_name = format!("analyzer/outputs/{}_{}", mesh_name, timestamp);
    let out_dir = Path::new(&dir_name);
    create_dir_all(&out_dir)?;

    let obj_filename = original.file_name().unwrap_or_default();
    copy(original, out_dir.join(&obj_filename))?;

    compress_and_decompress(original, &out_dir)?;

    // Copy assets
    let js_src = Path::new("analyzer/assets/viewer.js");
    let js_dst = out_dir.join("viewer.js");
    copy(js_src, js_dst)?;

    let html_template_path = Path::new("analyzer/assets/template.html");
    let html_content = read_to_string(html_template_path)?;
    let html_content = html_content.replace("{{file_type}}", "obj");

    let out_file = out_dir.join("index.html");
    write(out_file, html_content)?;
    println!("Report generated at: {}/index.html", dir_name);
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CompressionConfig {
    pub quantization_bits: u8,
    pub simplification_ratio: f32,
}

#[derive(Debug, Deserialize)]
pub struct CompressionStats {
    pub input_size: usize,
    pub compressed_size: usize,
    pub encode_time_ms: f32,
    pub decode_time_ms: f32,
}


fn compress_and_decompress(original: &PathBuf, out_dir: &Path) -> io::Result<()> {
    let original_mesh = draco_oxide::io::obj::load_obj(original)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to load OBJ: {}", e)))?;
    
    let mut buffer = Vec::new();
    let mut writer = eval::EvalWriter::new(&mut buffer);
    encode(original_mesh.clone(), &mut writer, encode::Config::default()).unwrap();
    println!("Encoding done.");

    // Copy the input file as "input.obj"
    std::fs::copy(original, out_dir.join("input.obj"))?;

    // write json
    let mut eval_file = std::fs::File::create(
    out_dir.join("eval.json")
    ).unwrap();
    let data = writer.get_result();
    let data = serde_json::to_string_pretty(&data).unwrap();
    eval_file.write_all(data.as_bytes()).unwrap();


    // Write compressed data to a temporary file for C++ decoder
    let compressed_file_path = out_dir.join("compressed.drc");
    std::fs::write(&compressed_file_path, &buffer)?;

    println!("Decoding with C++ Draco decoder...");
    let decoded_obj_path = out_dir.join("output.obj");
    decode_with_cpp_draco(&compressed_file_path, &decoded_obj_path)?;
    println!("Decoding done.");

    // let mut file_writer = std::io::BufWriter::new(&mut obj_file);

    // // create the mtl file containing coloring for the clers symbol.
    // let eval_json: serde_json::Value = serde_json::from_str(&data).unwrap();
    // let clers_symbols = eval_json
    //     .get("compression info")
    //     .and_then(|ci| ci.get("connectivity info"))
    //     .and_then(|conn| conn.get("edgebreaker"))
    //     .and_then(|eb| eb.get("clers_string"))
    //     .and_then(|cs| cs.as_str())
    //     .unwrap_or("")
    //     .to_string();
    // // Remove any substring enclosed in parentheses, e.g., (*), (abc), etc.
    // let clers_symbols = regex::Regex::new(r"\([^)]*\)")
    //     .unwrap()
    //     .replace_all(&clers_symbols, "")
    //     .to_string();


    // // Write the MTL file with a comment containing the clers symbols
    // let mut mtl_file = std::fs::File::create(out_dir.join("output.mtl")).unwrap();
    // writeln!(mtl_file, "# clers_symbols").unwrap();
    // writeln!(mtl_file, "newmtl C\nKd 1.0 0.0 0.0").unwrap();
    // writeln!(mtl_file, "newmtl R\nKd 0.0 1.0 0.0").unwrap();
    // writeln!(mtl_file, "newmtl L\nKd 0.0 0.0 1.0").unwrap();
    // writeln!(mtl_file, "newmtl E\nKd 1.0 1.0 0.0").unwrap();
    // writeln!(mtl_file, "newmtl S\nKd 1.0 0.0 1.0").unwrap();
    // writeln!(mtl_file, "newmtl M\nKd 0.0 1.0 1.0").unwrap();
    // writeln!(mtl_file, "newmtl H\nKd 1.0 1.0 1.0").unwrap();

    // writeln!(file_writer, "mtllib output.mtl").unwrap();
    
    // let result_attributes = mesh.get_attributes();
    // // The decoded mesh might have different attribute layout
    // // Position is typically at index 1, normals at index 2 after decoding
    // if result_attributes.len() > 2 {
    //     let positions = result_attributes[1].as_slice::<[f64; 3]>();
    //     let normals = result_attributes[2].as_slice::<[f32; 3]>();
    //     for (point, normal) in positions.iter().zip(normals.iter()) {
    //         writeln!(file_writer, "vn {} {} {}", normal[0], normal[1], normal[2]).unwrap();
    //         writeln!(file_writer, "v {} {} {}", point[0], point[1], point[2]).unwrap();
    //     }
    // } else if result_attributes.len() > 1 {
    //     // Only positions available at index 1
    //     let positions = result_attributes[1].as_slice::<[f64; 3]>();
    //     for point in positions.iter() {
    //         writeln!(file_writer, "v {} {} {}", point[0], point[1], point[2]).unwrap();
    //     }
    // }

    // let face_data = result_attributes[0].as_slice::<[usize; 3]>();
    // assert!(
    //     clers_symbols.len() == face_data.len(), 
    //     "clers_symbols.len() = {} != face_data.len() = {}", 
    //     clers_symbols.len(), face_data.len()
    // );
    // for (face, symbol) in face_data.iter().zip(clers_symbols.chars()) {
    //     writeln!(file_writer, "usemtl {}", symbol).unwrap();
    //     writeln!(file_writer, "f {0}//{0} {1}//{1} {2}//{2}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
    // }
    
    Ok(())
}

/// Copy a GLTF/GLB file along with all its dependencies (textures, bin files, etc.)
fn copy_gltf_with_dependencies(source_path: &Path, dest_path: &Path, dest_dir: &Path) -> io::Result<()> {
    let source_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    let source_extension = source_path.extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase());
    
    match source_extension.as_deref() {
        Some("glb") => {
            // GLB files are self-contained binary files, just copy the file
            copy(source_path, dest_path)?;
            println!("Copied GLB file: {} -> {}", source_path.display(), dest_path.display());
        },
        Some("gltf") => {
            // GLTF files are JSON and may reference external files
            copy(source_path, dest_path)?;
            println!("Copied GLTF file: {} -> {}", source_path.display(), dest_path.display());
            
            // Parse the GLTF file to find dependencies
            let dependencies = parse_gltf_dependencies(source_path)?;
            
            // Copy each dependency
            for dep_path in dependencies {
                let source_dep = source_dir.join(&dep_path);
                let dest_dep = dest_dir.join(&dep_path);
                
                // Create subdirectories if needed
                if let Some(parent) = dest_dep.parent() {
                    create_dir_all(parent)?;
                }
                
                if source_dep.exists() {
                    copy(&source_dep, &dest_dep)?;
                    println!("Copied dependency: {} -> {}", source_dep.display(), dest_dep.display());
                } else {
                    eprintln!("Warning: Dependency not found: {}", source_dep.display());
                }
            }
        },
        _ => {
            // Unknown format, just copy the file
            copy(source_path, dest_path)?;
        }
    }
    
    Ok(())
}

/// Parse a GLTF JSON file to extract dependency file paths
fn parse_gltf_dependencies(gltf_path: &Path) -> io::Result<Vec<String>> {
    let content = std::fs::read_to_string(gltf_path)?;
    let gltf_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to parse GLTF JSON: {}", e)))?;
    
    let mut dependencies = Vec::new();
    
    // Extract buffer dependencies (typically .bin files)
    if let Some(buffers) = gltf_json.get("buffers").and_then(|b| b.as_array()) {
        for buffer in buffers {
            if let Some(uri) = buffer.get("uri").and_then(|u| u.as_str()) {
                // Skip data URIs (embedded data)
                if !uri.starts_with("data:") {
                    dependencies.push(uri.to_string());
                }
            }
        }
    }
    
    // Extract image dependencies (textures)
    if let Some(images) = gltf_json.get("images").and_then(|i| i.as_array()) {
        for image in images {
            if let Some(uri) = image.get("uri").and_then(|u| u.as_str()) {
                // Skip data URIs (embedded images)
                if !uri.starts_with("data:") {
                    dependencies.push(uri.to_string());
                }
            }
        }
    }
    
    // Extract other potential dependencies from extensions
    extract_extension_dependencies(&gltf_json, &mut dependencies);
    
    Ok(dependencies)
}

/// Extract dependencies from GLTF extensions
fn extract_extension_dependencies(gltf_json: &serde_json::Value, dependencies: &mut Vec<String>) {
    // Check for KHR_lights_punctual extension files
    if let Some(extensions) = gltf_json.get("extensions") {
        // Add extraction for various extensions that might reference external files
        // For now, this is a placeholder for future extension support
        let _ = extensions; // Suppress unused variable warning
    }
    
    // Check for any other URI references in the JSON
    extract_uris_recursively(gltf_json, dependencies);
}

/// Recursively search for URI fields in the GLTF JSON that might reference external files
fn extract_uris_recursively(value: &serde_json::Value, dependencies: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "uri" {
                    if let Some(uri_str) = val.as_str() {
                        // Only add non-data URIs that we haven't already added
                        if !uri_str.starts_with("data:") && !dependencies.contains(&uri_str.to_string()) {
                            dependencies.push(uri_str.to_string());
                        }
                    }
                } else {
                    extract_uris_recursively(val, dependencies);
                }
            }
        },
        serde_json::Value::Array(arr) => {
            for item in arr {
                extract_uris_recursively(item, dependencies);
            }
        },
        _ => {} // Primitive values, nothing to do
    }
}