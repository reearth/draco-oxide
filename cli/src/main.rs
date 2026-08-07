use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use draco_oxide::encode::{
    Config, ConnectivityConfig, EdgebreakerConfig, NormalEncoding, Quantization, SequentialConfig,
};
use draco_oxide::{AttributeType, ConfigType};
use std::path::Path;

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

    /// Path to a TOML encoder-config file (see `encode::Config`). Provides the
    /// full configuration surface; any flags below override values from it.
    #[arg(long, value_name = "FILE")]
    config: Option<String>,

    /// Override position quantization bits (1..=30).
    #[arg(long, value_name = "BITS")]
    position_bits: Option<u8>,

    /// Override how normals are compressed.
    #[arg(long, value_enum, value_name = "MODE")]
    normal: Option<NormalArg>,

    /// Override the connectivity compression method.
    #[arg(long, value_enum, value_name = "METHOD")]
    connectivity: Option<ConnectivityArg>,

    /// Enable metadata encoding.
    #[arg(long)]
    metadata: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum NormalArg {
    Quantized,
    PredictedOnly,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConnectivityArg {
    Edgebreaker,
    Sequential,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.transcode {
        transcode_gltf(&cli.input, &cli.output)
    } else {
        convert_obj_to_drc(&cli)
    }
}

/// Builds the encoder config: start from `--config <file>` (or the default),
/// then layer any explicitly-provided flags on top (flags win over the file).
fn build_config(cli: &Cli) -> Result<Config> {
    let mut config = match &cli.config {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file: {path}"))?;
            toml::from_str::<Config>(&text)
                .with_context(|| format!("Failed to parse TOML config: {path}"))?
        }
        None => <Config as ConfigType>::default(),
    };

    if let Some(bits) = cli.position_bits {
        // Read-modify-write so we don't clobber other Position knobs from the file.
        let mut ac = config.attribute_config(AttributeType::Position);
        ac.quantization = Some(Quantization::Bits(bits));
        config = config.with_attribute(AttributeType::Position, ac);
    }

    if let Some(mode) = cli.normal {
        let mut ac = config.attribute_config(AttributeType::Normal);
        ac.normal_encoding = Some(match mode {
            NormalArg::Quantized => NormalEncoding::Quantized,
            NormalArg::PredictedOnly => NormalEncoding::PredictedOnly,
        });
        config = config.with_attribute(AttributeType::Normal, ac);
    }

    if let Some(method) = cli.connectivity {
        let already_edgebreaker =
            matches!(config.connectivity(), ConnectivityConfig::Edgebreaker(_));
        config = match method {
            // Keep any file-provided edgebreaker sub-config if already selected.
            ConnectivityArg::Edgebreaker if already_edgebreaker => config,
            ConnectivityArg::Edgebreaker => {
                config.with_edgebreaker(<EdgebreakerConfig as ConfigType>::default())
            }
            ConnectivityArg::Sequential => {
                config.with_sequential(<SequentialConfig as ConfigType>::default())
            }
        };
    }

    if cli.metadata {
        config = config.with_metadata(true);
    }

    // Surface config problems with a clear message before encoding.
    config.validate().context("Invalid encoder configuration")?;

    Ok(config)
}

fn convert_obj_to_drc(cli: &Cli) -> Result<()> {
    let input_path = &cli.input;
    let output_path = &cli.output;

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

    // Configure compression settings from --config file and/or flags.
    let config = build_config(cli)?;

    // Encode the mesh to a buffer
    let mut buffer = Vec::new();
    draco_oxide::encode::encode_mesh(mesh, &mut buffer, config)
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

    // Read input file
    let input = std::fs::read(input_path)
        .map_err(|e| anyhow::anyhow!("Failed to read input file: {}", e))?;

    // Create transcoder and transcode
    let transcoder = draco_oxide::io::gltf::GltfTranscoder::default();
    let warnings = transcoder
        .transcode_to_file(&input, Path::new(output_path))
        .map_err(|e| anyhow::anyhow!("Failed to transcode: {}", e))?;

    // Print any warnings
    for warning in warnings {
        eprintln!("Warning: {}", warning);
    }

    Ok(())
}
