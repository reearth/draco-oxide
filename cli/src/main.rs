use clap::Parser;
use anyhow::Result;
use std::path::Path;
use draco_oxide::prelude::ConfigType;

#[derive(Parser)]
#[command(name = "draco-cli")]
#[command(about = "A CLI tool for Draco mesh compression")]
struct Cli {
    /// Input file path
    #[arg(short, long)]
    input: String,

    /// Output file path
    #[arg(short, long)]
    output: String,

    /// Transcode mode for glTF/GLB files (compress with Draco)
    #[arg(long)]
    transcode: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.transcode {
        transcode_gltf(&cli.input, &cli.output)
    } else {
        convert_obj_to_drc(&cli.input, &cli.output)
    }
}

fn convert_obj_to_drc(input_path: &str, output_path: &str) -> Result<()> {
    
    // Check input file extension
    let input_ext = Path::new(input_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    if input_ext != "obj" {
        anyhow::bail!("Input file must be a .obj file for conversion mode");
    }

    // Check output file extension
    let output_ext = Path::new(output_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    if output_ext != "drc" {
        anyhow::bail!("Output file must be a .drc file for conversion mode");
    }

    // Load OBJ file using draco-oxide's OBJ loader
    let mesh = draco_oxide::io::obj::load_obj(input_path)
        .map_err(|e| anyhow::anyhow!("Failed to load OBJ file: {:?}", e))?;


    // Configure compression settings
    let config = draco_oxide::encode::Config::default();

    // Encode the mesh to a buffer
    let mut buffer = Vec::new();
    draco_oxide::encode::encode(mesh, &mut buffer, config)
        .map_err(|e| anyhow::anyhow!("Failed to encode mesh: {:?}", e))?;

    // Write to output file
    std::fs::write(output_path, buffer)
        .map_err(|e| anyhow::anyhow!("Failed to write output file: {}", e))?;

    Ok(())
}

fn transcode_gltf(input_path: &str, output_path: &str) -> Result<()> {
    
    // Check input file extension
    let input_ext = Path::new(input_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    if !matches!(input_ext, "gltf" | "glb") {
        anyhow::bail!("Input file must be a .gltf or .glb file for transcode mode");
    }

    // Check output file extension
    let output_ext = Path::new(output_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    if !matches!(output_ext, "gltf" | "glb") {
        anyhow::bail!("Output file must be a .gltf or .glb file for transcode mode");
    }

    // Create transcoder with default options
    let mut transcoder = draco_oxide::io::gltf::transcoder::DracoTranscoder::create(None)
        .map_err(|e| anyhow::anyhow!("Failed to create transcoder: {:?}", e))?;

    // Set up file options
    let file_options = draco_oxide::io::gltf::transcoder::FileOptions::new(
        input_path.to_string(),
        output_path.to_string(),
    );

    // Perform transcoding
    transcoder.transcode_file(&file_options)
        .map_err(|e| anyhow::anyhow!("Failed to transcode: {:?}", e))?;

    Ok(())
}