//! File-in/file-out front end over `draco-oxide` for the profile-test harness.
//!
//! ```text
//! wasi-codec encode <input.obj> <output.drc> [config.toml]
//! wasi-codec decode <input.drc> <output.obj>
//! ```
//!
//! `encode` prints `encode_secs=<f64>` (the encode alone, excluding the OBJ
//! load and the output write) to stdout so the harness can enforce
//! `timeout_secs` on the same region it times natively.

use std::process::ExitCode;
use std::time::Instant;

use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode_mesh, Config};
use draco_oxide::io::obj::{decode_drc_to_obj, load_obj};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [_, "encode", input, output] => encode(input, output, None),
        [_, "encode", input, output, config] => encode(input, output, Some(config)),
        [_, "decode", input, output] => decode(input, output),
        _ => Err(
            "usage: wasi-codec encode <input.obj> <output.drc> [config.toml]\n       \
             wasi-codec decode <input.drc> <output.obj>"
                .to_string(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wasi-codec: {e}");
            ExitCode::FAILURE
        }
    }
}

fn encode(input: &str, output: &str, config: Option<&str>) -> Result<(), String> {
    let cfg = match config {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("failed to read config {path}: {e}"))?;
            toml::from_str::<Config>(&text)
                .map_err(|e| format!("failed to parse config {path}: {e}"))?
        }
        None => <Config as ConfigType>::default(),
    };
    let mesh = load_obj(input).map_err(|e| format!("failed to load {input}: {e}"))?;

    let start = Instant::now();
    let mut buf = Vec::new();
    encode_mesh(mesh, &mut buf, cfg).map_err(|e| format!("encode failed: {e:?}"))?;
    println!("encode_secs={}", start.elapsed().as_secs_f64());

    std::fs::write(output, &buf).map_err(|e| format!("failed to write {output}: {e}"))
}

fn decode(input: &str, output: &str) -> Result<(), String> {
    decode_drc_to_obj(input, output).map_err(|e| format!("decode of {input} failed: {e}"))
}
