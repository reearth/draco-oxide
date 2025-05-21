use std::path::{Path, PathBuf};
use std::process;
use std::io::Write;

use draco::prelude::*;
use draco::eval;

use clap::Parser;
use serde::Deserialize;
use std::io;
use std::fs::{create_dir_all, read_to_string, write, copy};
use chrono::Local;



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

    if let Err(e) = generate_html_report(&args.original) {
        eprintln!("Failed to generate report: {}", e);
        process::exit(1);
    }
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

    println!("normal= {:?}", mesh.normals);

    let faces = mesh.indices.chunks(3)
        .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
        .collect::<Vec<_>>();

    let points = mesh.positions.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();

    let normal = mesh.normals.chunks(3)
        .map(|x| NdVector::from([x[0] as f32, x[1] as f32, x[2] as f32]))
        .collect::<Vec<_>>();

    let original_mesh = {
        let mut builder = MeshBuilder::new();
        let ref_face_att = builder.add_connectivity_attribute(faces, Vec::new());
        let ref_pos_att = builder.add_attribute(points, AttributeType::Position, vec![ref_face_att]);
        builder.add_attribute(normal, AttributeType::Normal, vec![ref_face_att, ref_pos_att]);
        builder.build().unwrap()
    };
    
    let mut buff_writer = buffer::writer::Writer::new();
    let mut writer = |input: (u8, u64)| {
        buff_writer.next(input);
    };
    let mut eval_writer = eval::EvalWriter::new(&mut writer);
    let mut writer = |input: (u8, u64)| {
        eval_writer.write(input);
    };
    println!("Encoding...");
    let original_mesh = encode(original_mesh, &mut writer, encode::Config::default()).unwrap();
    println!("Encoding done.");

    // write the mesh to a file
    let mut obj_file = std::fs::File::create(
        out_dir.join("original.obj")
    ).unwrap();
    let mut file_writer = std::io::BufWriter::new(&mut obj_file);
    for (point, normal) in 
        original_mesh.get_attributes()[1].as_slice::<[f32; 3]>().iter().zip(
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
    let data = eval_writer.get_result();
    let data = serde_json::to_string_pretty(&data).unwrap();
    eval_file.write_all(data.as_bytes()).unwrap();


    let buffer: Buffer = buff_writer.into();

    let mut buff_reader = buffer.into_reader();
    let mut bit_counter: usize = 0;
    let mut reader = |size| {
        bit_counter += size as usize;
        // println!("bit_counter = {}  reading {} bits", bit_counter, size);
        buff_reader.next(size)
    };
    println!("Decoding...");
    let mesh = decode(&mut reader, decode::Config::default()).unwrap();
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
        mesh.get_attributes()[1].as_slice::<[f32; 3]>().iter().zip(
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