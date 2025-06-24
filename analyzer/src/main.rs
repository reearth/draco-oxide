use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::io::Write;

use draco::core::attribute::AttributeDomain;
use draco::prelude::*;
use draco::eval;

use clap::Parser;
use serde::Deserialize;
use std::io;
use std::fs::{create_dir_all, read_to_string, write, copy};
use chrono::Local;



/// Decode a .drc file using Google's C++ Draco decoder and parse the result into a Mesh
fn decode_with_cpp_draco(drc_path: &Path, ply_output_path: &Path) -> io::Result<Mesh> {
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
    
    // Decode the .drc file to a PLY file
    let output = Command::new("draco_decoder")
        .arg("-i")
        .arg(drc_path)
        .arg("-o")
        .arg(ply_output_path)
        .output()?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("draco_decoder failed: {}", stderr)
        ));
    }
    
    // Parse the PLY file back into a Mesh
    parse_ply_to_mesh(ply_output_path)
}

/// Parse a PLY file and convert it to our internal Mesh representation
fn parse_ply_to_mesh(ply_path: &Path) -> io::Result<Mesh> {
    let ply_content = std::fs::read_to_string(ply_path)?;
    
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut faces = Vec::new();
    
    let lines: Vec<&str> = ply_content.lines().collect();
    let mut i = 0;
    let mut vertex_count = 0;
    let mut face_count = 0;
    let mut in_vertex_data = false;
    let mut in_face_data = false;
    let mut vertex_idx = 0;
    let mut face_idx = 0;
    
    // Parse header
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("element vertex") {
            vertex_count = line.split_whitespace().last()
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap_or(0);
        } else if line.starts_with("element face") {
            face_count = line.split_whitespace().last()
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap_or(0);
        } else if line == "end_header" {
            in_vertex_data = true;
            i += 1;
            break;
        }
        i += 1;
    }
    
    // Parse vertex data
    while i < lines.len() && vertex_idx < vertex_count {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 6 {
            // Assuming format: x y z nx ny nz
            let x: f64 = parts[0].parse().unwrap_or(0.0);
            let y: f64 = parts[1].parse().unwrap_or(0.0);
            let z: f64 = parts[2].parse().unwrap_or(0.0);
            vertices.push(NdVector::from([x, y, z]));
            
            let nx: f32 = parts[3].parse().unwrap_or(0.0);
            let ny: f32 = parts[4].parse().unwrap_or(0.0);
            let nz: f32 = parts[5].parse().unwrap_or(0.0);
            normals.push(NdVector::from([nx, ny, nz]));
        }
        vertex_idx += 1;
        i += 1;
    }
    
    // Parse face data
    while i < lines.len() && face_idx < face_count {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 && parts[0] == "3" {
            // Triangle face: 3 v1 v2 v3
            let v1: usize = parts[1].parse().unwrap_or(0);
            let v2: usize = parts[2].parse().unwrap_or(0);
            let v3: usize = parts[3].parse().unwrap_or(0);
            faces.push([v1, v2, v3]);
        }
        face_idx += 1;
        i += 1;
    }
    
    // Build the mesh
    let mut builder = MeshBuilder::new();
    builder.set_connectivity_attribute(faces);
    let pos_id = builder.add_attribute(vertices, AttributeType::Position, AttributeDomain::Position, vec![]);
    builder.add_attribute(normals, AttributeType::Normal, AttributeDomain::Position, vec![pos_id]);
    
    builder.build().map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Failed to build mesh: {}", e))
    })
}

/// Mesh Analyzer CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Path to the original mesh file
    #[arg(long)]
    original: PathBuf,
}

fn main() {
    let args = Args::parse();

    // Check file extension
    let extension = args.original.extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase());
    
    match extension.as_deref() {
        Some("obj") => {
            if let Err(e) = generate_html_report(&args.original) {
                eprintln!("Failed to generate report: {}", e);
                process::exit(1);
            }
        }
        Some("glb") => {
            if let Err(e) = process_glb_file(&args.original) {
                eprintln!("Failed to process GLB file: {}", e);
                process::exit(1);
            }
        }
        _ => {
            eprintln!("Unsupported file format. Only .obj and .glb files are supported.");
            process::exit(1);
        }
    }
}

/// Process GLB file by transcoding it with Draco compression
pub fn process_glb_file(original: &PathBuf) -> io::Result<()> {
    let mesh_name = original.file_stem().unwrap_or_default().to_string_lossy();
    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let dir_name = format!("analyzer/outputs/{}_{}", mesh_name, timestamp);
    let out_dir = Path::new(&dir_name);
    create_dir_all(&out_dir)?;

    // Copy original GLB file
    let original_glb_path = out_dir.join("original.glb");
    copy(original, &original_glb_path)?;

    // Load GLB file using draco-rs gltf loader
    let mesh = draco::io::gltf::load_gltf(original.to_str().unwrap())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to load GLB: {}", e)))?;

    // Create a new glTF asset and add the compressed mesh
    let mut asset = draco::io::gltf::GltfAsset::new();
    let mesh_index = asset.add_draco_mesh(&mesh)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to add mesh to glTF asset: {}", e)))?;
    
    // Create a simple scene
    asset.create_simple_scene(mesh_index);
    
    // Generate glTF JSON and binary data
    let (json, binary) = asset.to_gltf()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to generate glTF: {}", e)))?;
    
    // Create GLB format
    let result_glb_path = out_dir.join("result.glb");
    create_glb_file(&result_glb_path, &json, &binary)?;

    println!("GLB processing completed:");
    println!("  Original GLB: {}", original_glb_path.display());
    println!("  Result GLB: {}", result_glb_path.display());
    println!("  Output directory: {}", dir_name);

    Ok(())
}

/// Create a GLB (binary glTF) file from JSON and binary data
fn create_glb_file(path: &Path, json: &str, binary: &[u8]) -> io::Result<()> {
    use std::io::{Cursor, Write};
    
    let mut glb_data = Vec::new();
    
    // GLB header
    glb_data.extend_from_slice(b"glTF"); // magic
    glb_data.extend_from_slice(&2u32.to_le_bytes()); // version
    
    // Calculate chunk sizes
    let json_bytes = json.as_bytes();
    let json_length = json_bytes.len();
    let json_padded_length = (json_length + 3) & !3; // Pad to 4-byte boundary
    
    let binary_length = binary.len();
    let binary_padded_length = (binary_length + 3) & !3; // Pad to 4-byte boundary
    
    let total_length = 12 + // header
                      8 + json_padded_length + // JSON chunk header + data
                      if binary.is_empty() { 0 } else { 8 + binary_padded_length }; // BIN chunk header + data
    
    glb_data.extend_from_slice(&(total_length as u32).to_le_bytes()); // total length
    
    // JSON chunk
    glb_data.extend_from_slice(&(json_padded_length as u32).to_le_bytes()); // chunk length
    glb_data.extend_from_slice(b"JSON"); // chunk type
    glb_data.extend_from_slice(json_bytes);
    
    // Pad JSON to 4-byte boundary with spaces
    for _ in json_length..json_padded_length {
        glb_data.push(b' ');
    }
    
    // Binary chunk (if present)
    if !binary.is_empty() {
        glb_data.extend_from_slice(&(binary_padded_length as u32).to_le_bytes()); // chunk length
        glb_data.extend_from_slice(b"BIN\0"); // chunk type
        glb_data.extend_from_slice(binary);
        
        // Pad binary to 4-byte boundary with zeros
        for _ in binary_length..binary_padded_length {
            glb_data.push(0);
        }
    }
    
    std::fs::write(path, glb_data)?;
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
    // html_content = html_content.replace("{{original_obj}}", &obj_filename.to_string_lossy());

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
    pub original_size: usize,
    pub compressed_size: usize,
    pub encode_time_ms: f32,
    pub decode_time_ms: f32,
}

fn compress_and_decompress(original: &PathBuf, out_dir: &Path) -> io::Result<()> {
    let (original,_) = tobj::load_obj(
        original.to_str().unwrap(), 
        &tobj::GPU_LOAD_OPTIONS
    ).unwrap();
    let mesh = &original[0].mesh;

    let faces = mesh.indices.chunks(3)
        .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
        .collect::<Vec<_>>();

    let points = mesh.positions.chunks(3)
        .map(|x| NdVector::from([x[0] as f64, x[1] as f64, x[2] as f64]))
        .collect::<Vec<_>>();

    let normal = mesh.normals.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();

    let original_mesh = {
        let mut builder = MeshBuilder::new();
        builder.set_connectivity_attribute(faces);
        let ref_pos_att = builder.add_attribute(points, AttributeType::Position, AttributeDomain::Position, Vec::new());
        builder.add_attribute(normal, AttributeType::Normal, AttributeDomain::Position, vec![ref_pos_att]);
        builder.build().unwrap()
    };
    
    let mut buffer = Vec::new();
    let mut writer = eval::EvalWriter::new(&mut buffer);
    println!("Encoding...");
    encode(original_mesh.clone(), &mut writer, encode::Config::default()).unwrap();
    println!("Encoding done.");

    // write the mesh to a file
    let mut obj_file = std::fs::File::create(
        out_dir.join("original.obj")
    ).unwrap();
    let mut file_writer = std::io::BufWriter::new(&mut obj_file);
    for (point, normal) in 
        original_mesh.get_attributes()[1].as_slice::<[f64; 3]>().iter().zip(
            original_mesh.get_attributes()[2].as_slice::<[f32; 3]>().iter()
        ) 
    {
        writeln!(file_writer, "vn {} {} {}", normal[0], normal[1], normal[2]).unwrap();
        writeln!(file_writer, "v {} {} {}", point[0], point[1], point[2]).unwrap();
    }
    let face_data = original_mesh.get_attributes()[0].as_slice::<[usize; 3]>();
    for face in face_data.iter() {
        writeln!(file_writer, "f {0}//{0} {1}//{1} {2}//{2}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
    }

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
    let decoded_ply_path = out_dir.join("decoded.ply");
    let mesh = decode_with_cpp_draco(&compressed_file_path, &decoded_ply_path)?;
    println!("Decoding done.");

    let mut obj_file = std::fs::File::create(
        out_dir.join("result.obj")
    ).unwrap();
    let mut file_writer = std::io::BufWriter::new(&mut obj_file);

    // create the mtl file containing coloring for the clers symbol.
    let eval_json: serde_json::Value = serde_json::from_str(&data).unwrap();
    let clers_symbols = eval_json
        .get("compression info")
        .and_then(|ci| ci.get("connectivity info"))
        .and_then(|conn| conn.get("edgebreaker"))
        .and_then(|eb| eb.get("clers_string"))
        .and_then(|cs| cs.as_str())
        .unwrap_or("")
        .to_string();
    // Remove any substring enclosed in parentheses, e.g., (*), (abc), etc.
    let clers_symbols = regex::Regex::new(r"\([^)]*\)")
        .unwrap()
        .replace_all(&clers_symbols, "")
        .to_string();


    // Write the MTL file with a comment containing the clers symbols
    let mut mtl_file = std::fs::File::create(out_dir.join("result.mtl")).unwrap();
    writeln!(mtl_file, "# clers_symbols").unwrap();
    writeln!(mtl_file, "newmtl C\nKd 1.0 0.0 0.0").unwrap();
    writeln!(mtl_file, "newmtl R\nKd 0.0 1.0 0.0").unwrap();
    writeln!(mtl_file, "newmtl L\nKd 0.0 0.0 1.0").unwrap();
    writeln!(mtl_file, "newmtl E\nKd 1.0 1.0 0.0").unwrap();
    writeln!(mtl_file, "newmtl S\nKd 1.0 0.0 1.0").unwrap();
    writeln!(mtl_file, "newmtl M\nKd 0.0 1.0 1.0").unwrap();
    writeln!(mtl_file, "newmtl H\nKd 1.0 1.0 1.0").unwrap();

    writeln!(file_writer, "mtllib result.mtl").unwrap();
    for (point, normal) in 
        mesh.get_attributes()[1].as_slice::<[f64; 3]>().iter().zip(
            mesh.get_attributes()[2].as_slice::<[f32; 3]>().iter()
        ) 
    {
        writeln!(file_writer, "vn {} {} {}", normal[0], normal[1], normal[2]).unwrap();
        writeln!(file_writer, "v {} {} {}", point[0], point[1], point[2]).unwrap();
    }

    let face_data = mesh.get_attributes()[0].as_slice::<[usize; 3]>();
    assert!(
        clers_symbols.len() == face_data.len(), 
        "clers_symbols.len() = {} != face_data.len() = {}", 
        clers_symbols.len(), face_data.len()
    );
    for (face, symbol) in face_data.iter().zip(clers_symbols.chars()) {
        writeln!(file_writer, "usemtl {}", symbol).unwrap();
        writeln!(file_writer, "f {0}//{0} {1}//{1} {2}//{2}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
    }
    
    Ok(())
}