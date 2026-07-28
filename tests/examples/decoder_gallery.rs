//! Manual decoder gallery: for every OBJ in `tests/data/`, encode it with both
//! draco-oxide and Google Draco, decode both streams with the draco-oxide
//! decoder, and render the original plus the two decoded meshes from a shared
//! camera. Emits one JPEG per cell and a `summary.json` describing the run.
//!
//! Usage: `cargo run -p tests --example decoder_gallery -- <out_dir>`

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[path = "../src/render.rs"]
mod render;

use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{self, encode};
use draco_oxide::io::obj::{load_obj, write_obj};
use render::{load_obj_mesh, render_views, ColorBy, Framing};

const RESOLUTION: u32 = 512;
const JPEG_QUALITY: u8 = 85;

/// Result of one pipeline stage: either a value or the failure message.
type Stage<T> = Result<T, String>;

fn guard<T>(what: &str, f: impl FnOnce() -> Stage<T>) -> Stage<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".into());
            Err(format!("{what} panicked: {msg}"))
        }
    }
}

fn draco_encoder() -> PathBuf {
    std::env::var("DRACO_ENCODER")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/usr/bin/draco_encoder"))
}

/// Encode `obj` with draco-oxide, returning the bitstream and elapsed millis.
fn oxide_encode(obj: &Path) -> Stage<(Vec<u8>, f64)> {
    guard("oxide encode", || {
        let mesh = load_obj(obj).map_err(|e| format!("load_obj: {e:?}"))?;
        let t = Instant::now();
        let mut buf = Vec::new();
        encode(mesh, &mut buf, encode::Config::default()).map_err(|e| format!("encode: {e:?}"))?;
        Ok((buf, t.elapsed().as_secs_f64() * 1e3))
    })
}

/// Encode `obj` with Google Draco's `draco_encoder`.
fn google_encode(obj: &Path, drc: &Path) -> Stage<(Vec<u8>, f64)> {
    let t = Instant::now();
    let out = Command::new(draco_encoder())
        .arg("-i")
        .arg(obj)
        .arg("-o")
        .arg(drc)
        .output()
        .map_err(|e| format!("spawn draco_encoder: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "draco_encoder {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let bytes = std::fs::read(drc).map_err(|e| format!("read drc: {e}"))?;
    Ok((bytes, t.elapsed().as_secs_f64() * 1e3))
}

/// Decode `bytes` with the draco-oxide decoder and write the result as OBJ.
fn oxide_decode_to_obj(bytes: &[u8], obj: &Path) -> Stage<f64> {
    guard("oxide decode", || {
        let t = Instant::now();
        let mesh = draco_oxide::decode::decode(bytes).map_err(|e| format!("decode: {e:?}"))?;
        let ms = t.elapsed().as_secs_f64() * 1e3;
        write_obj(&mesh, obj).map_err(|e| format!("write_obj: {e:?}"))?;
        Ok(ms)
    })
}

/// Render one mesh file with the shared framing and write it as JPEG.
fn render_to(src: &Path, cam: &Framing, dst: &Path) -> Stage<(usize, usize)> {
    guard("render", || {
        let mesh = load_obj_mesh(src)?;
        let counts = (mesh.verts.len(), mesh.tris.len());
        let img = render_views(&mesh, cam, RESOLUTION, 1, ColorBy::Geometry)?
            .pop()
            .ok_or("no view rendered")?;
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(dst).map_err(|e| format!("create jpeg: {e}"))?,
        );
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, JPEG_QUALITY)
            .encode_image(&img)
            .map_err(|e| format!("encode jpeg: {e}"))?;
        Ok(counts)
    })
}

/// A `field: value` line for the JSON summary, escaping only what OBJ names and
/// error strings can contain.
fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt_num(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}"))
        .unwrap_or_else(|| "null".into())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out_dir = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "gallery".into()));
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let mut objs: Vec<PathBuf> = std::fs::read_dir(&data_dir)
        .expect("read data dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("obj"))
        .collect();
    objs.sort();

    let mut rows = Vec::new();
    for obj in &objs {
        let name = obj.file_stem().unwrap().to_string_lossy().to_string();
        println!("=== {name}");

        let enc_oxide = oxide_encode(obj);
        let drc_google = out_dir.join(format!("{name}.google.drc"));
        let enc_google = google_encode(obj, &drc_google);

        // Decode each stream that was produced, writing an OBJ per column.
        let mut dec = |bytes: &Stage<(Vec<u8>, f64)>, tag: &str| -> (Stage<f64>, PathBuf) {
            let path = out_dir.join(format!("{name}.{tag}.obj"));
            let r = match bytes {
                Ok((b, _)) => oxide_decode_to_obj(b, &path),
                Err(e) => Err(format!("no stream to decode ({e})")),
            };
            (r, path)
        };
        let (dec_oxide, obj_oxide) = dec(&enc_oxide, "oxide");
        let (dec_google, obj_google) = dec(&enc_google, "google");

        if let Ok((b, _)) = &enc_oxide {
            let _ = std::fs::write(out_dir.join(format!("{name}.oxide.drc")), b);
        }

        // One camera for all three columns, fit to the original mesh.
        let orig = load_obj_mesh(obj);
        let cam = orig.as_ref().ok().map(|m| Framing::fit(&m.verts));

        let mut cell = |src: &Path, tag: &str| -> (Stage<(usize, usize)>, String) {
            let file = format!("{name}.{tag}.jpg");
            let r = match (&cam, src.exists()) {
                (Some(c), true) => render_to(src, c, &out_dir.join(&file)),
                (None, _) => Err("no camera: original failed to load".into()),
                (_, false) => Err("no mesh to render".into()),
            };
            (r, file)
        };
        let (r_orig, img_orig) = cell(obj, "original");
        let (r_oxide, img_oxide) = cell(&obj_oxide, "oxide");
        let (r_google, img_google) = cell(&obj_google, "google");

        for (label, e) in [
            ("oxide encode", enc_oxide.as_ref().err()),
            ("draco encode", enc_google.as_ref().err()),
            ("oxide decode of oxide", dec_oxide.as_ref().err()),
            ("oxide decode of draco", dec_google.as_ref().err()),
            ("render original", r_orig.as_ref().err()),
            ("render oxide", r_oxide.as_ref().err()),
            ("render draco", r_google.as_ref().err()),
        ] {
            if let Some(e) = e {
                println!("    {label}: FAILED: {e}");
            }
        }

        let counts = |r: &Stage<(usize, usize)>| match r {
            Ok((v, t)) => format!("{{\"verts\":{v},\"tris\":{t}}}"),
            Err(_) => "null".into(),
        };
        fn err<T>(r: &Result<T, String>) -> String {
            match r {
                Ok(_) => "null".into(),
                Err(e) => json_str(e),
            }
        }
        rows.push(format!(
            "{{\"name\":{n},\"src_bytes\":{src},\
             \"oxide\":{{\"drc_bytes\":{ob},\"encode_ms\":{oe},\"decode_ms\":{od},\
             \"encode_err\":{oee},\"decode_err\":{ode},\"img\":{oi},\"mesh\":{om}}},\
             \"google\":{{\"drc_bytes\":{gb},\"encode_ms\":{ge},\"decode_ms\":{gd},\
             \"encode_err\":{gee},\"decode_err\":{gde},\"img\":{gi},\"mesh\":{gm}}},\
             \"original\":{{\"img\":{pi},\"mesh\":{pm},\"err\":{pe}}}}}",
            n = json_str(&name),
            src = std::fs::metadata(obj).map(|m| m.len()).unwrap_or(0),
            ob = enc_oxide
                .as_ref()
                .map(|(b, _)| b.len().to_string())
                .unwrap_or_else(|_| "null".into()),
            oe = opt_num(enc_oxide.as_ref().ok().map(|(_, t)| *t)),
            od = opt_num(dec_oxide.as_ref().ok().copied()),
            oee = err(&enc_oxide),
            ode = err(&dec_oxide),
            oi = json_str(&img_oxide),
            om = counts(&r_oxide),
            gb = enc_google
                .as_ref()
                .map(|(b, _)| b.len().to_string())
                .unwrap_or_else(|_| "null".into()),
            ge = opt_num(enc_google.as_ref().ok().map(|(_, t)| *t)),
            gd = opt_num(dec_google.as_ref().ok().copied()),
            gee = err(&enc_google),
            gde = err(&dec_google),
            gi = json_str(&img_google),
            gm = counts(&r_google),
            pi = json_str(&img_orig),
            pm = counts(&r_orig),
            pe = err(&r_orig),
        ));
    }

    let summary = format!("[\n  {}\n]\n", rows.join(",\n  "));
    std::fs::write(out_dir.join("summary.json"), summary).expect("write summary");
    println!("\nwrote {}/summary.json", out_dir.display());
}
