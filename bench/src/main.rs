//! Benchmark of draco-oxide against Google Draco over the meshes in
//! `tests/data`, measuring encode/decode speed and compression ratio. Writes a
//! single markdown report (`bench/report.md`) with SVG charts (`bench/assets/`).
//!
//! Both codecs run in-process and are timed with the same harness: Google Draco
//! is linked as `libdraco` through the C shim in `shim.cc` (built by
//! `scripts/build-draco.sh`; override with `DRACO_SRC_DIR`/`DRACO_BUILD_DIR`).
//! Run with `cargo run -p bench --release`.

mod chart;

use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode, Config};
use draco_oxide::io::obj::load_obj;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Per-codec measurements for one mesh, in milliseconds.
struct CodecResult {
    encode_ms: f64,
    decode_ms: f64,
    compressed_bytes: usize,
}

struct MeshBench {
    name: String,
    faces: usize,
    raw_bytes: usize,
    oxide: CodecResult,
    draco: CodecResult,
}

#[cfg(have_libdraco)]
mod draco_ffi {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};
    use std::path::Path;

    extern "C" {
        fn draco_bench_load_obj(path: *const c_char) -> *mut c_void;
        fn draco_bench_free_mesh(mesh: *mut c_void);
        fn draco_bench_encode(mesh: *mut c_void, out: *mut *mut u8, out_len: *mut usize) -> c_int;
        fn draco_bench_free_buffer(buffer: *mut u8);
        fn draco_bench_decode(data: *const u8, len: usize) -> c_int;
    }

    /// An owned `draco::Mesh` behind the shim.
    pub struct DracoMesh(*mut c_void);

    impl Drop for DracoMesh {
        fn drop(&mut self) {
            unsafe { draco_bench_free_mesh(self.0) };
        }
    }

    pub fn load_obj(path: &Path) -> Option<DracoMesh> {
        let c_path = CString::new(path.to_str()?).ok()?;
        let mesh = unsafe { draco_bench_load_obj(c_path.as_ptr()) };
        (!mesh.is_null()).then_some(DracoMesh(mesh))
    }

    /// Encodes and returns the compressed bytes.
    pub fn encode(mesh: &DracoMesh) -> Option<Vec<u8>> {
        let mut out: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;
        let rc = unsafe { draco_bench_encode(mesh.0, &mut out, &mut out_len) };
        if rc != 0 || out.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(out, out_len) }.to_vec();
        unsafe { draco_bench_free_buffer(out) };
        Some(bytes)
    }

    /// Encodes and discards the output (the timed path).
    pub fn encode_discard(mesh: &DracoMesh) -> bool {
        unsafe { draco_bench_encode(mesh.0, std::ptr::null_mut(), std::ptr::null_mut()) == 0 }
    }

    /// Decodes an in-memory stream to a mesh and discards it.
    pub fn decode(data: &[u8]) -> bool {
        unsafe { draco_bench_decode(data.as_ptr(), data.len()) == 0 }
    }
}

fn main() {
    #[cfg(not(have_libdraco))]
    {
        eprintln!(
            "bench was built without libdraco. Run scripts/build-draco.sh (or set \
             DRACO_SRC_DIR / DRACO_BUILD_DIR) and rebuild."
        );
        std::process::exit(1);
    }

    #[cfg(have_libdraco)]
    run();
}

#[cfg(have_libdraco)]
fn run() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let data_dir = root.join("tests/data");
    let bench_dir = root.join("bench");
    let assets_dir = bench_dir.join("assets");
    std::fs::create_dir_all(&assets_dir).expect("create bench/assets");

    let mut objs: Vec<PathBuf> = std::fs::read_dir(&data_dir)
        .expect("read tests/data")
        .filter_map(|e| {
            let p = e.ok()?.path();
            if p.extension()? != "obj" {
                return None;
            }
            // The pathological_* meshes are timeout regression fixtures, not
            // representative inputs.
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            (!stem.starts_with("pathological")).then_some(p)
        })
        .collect();
    objs.sort();

    let mut results: Vec<MeshBench> = Vec::new();
    for obj in &objs {
        let name = obj.file_stem().unwrap().to_string_lossy().to_string();
        eprintln!("benchmarking {name} ...");
        let mesh = match load_obj(obj.to_str().unwrap()) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  skipping {name}: failed to load ({e:?})");
                continue;
            }
        };
        let faces = mesh.faces.len();
        let raw_bytes = raw_geometry_bytes(&mesh);

        let oxide = bench_oxide(&mesh);
        let draco = match bench_draco(obj) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  skipping {name}: libdraco failed ({e})");
                continue;
            }
        };

        results.push(MeshBench {
            name,
            faces,
            raw_bytes,
            oxide,
            draco,
        });
    }

    // Largest meshes first, in the table and the charts alike.
    results.sort_by_key(|r| std::cmp::Reverse(r.faces));

    write_charts(&assets_dir, &results);
    let report = render_report(&results);
    let report_path = bench_dir.join("report.md");
    std::fs::write(&report_path, report).expect("write report");
    eprintln!("report written to {}", report_path.display());
}

/// The uncompressed size of the geometry both codecs start from: every
/// attribute's unique values plus 32-bit face indices. Used as the numerator of
/// both compression ratios so they share a baseline.
fn raw_geometry_bytes(mesh: &Mesh) -> usize {
    let atts: usize = mesh
        .attributes
        .iter()
        .map(|a| a.num_unique_values() * a.get_num_components() * a.get_component_type().size())
        .sum();
    atts + mesh.faces.len() * 3 * 4
}

/// Runs `f` repeatedly (after one warmup) until enough samples are collected,
/// and returns the median wall time in milliseconds.
fn time_median_ms<F: FnMut()>(mut f: F) -> f64 {
    f();
    let mut samples = Vec::new();
    let start = Instant::now();
    while samples.len() < 5 || (start.elapsed().as_secs_f64() < 0.3 && samples.len() < 30) {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_secs_f64() * 1e3);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn bench_oxide(mesh: &Mesh) -> CodecResult {
    let mut buffer = Vec::new();
    encode(mesh.clone(), &mut buffer, Config::default()).expect("draco-oxide encode");
    let compressed_bytes = buffer.len();

    // The mesh clone inside the timed closure is a plain buffer copy, negligible
    // next to the encode itself.
    let encode_ms = time_median_ms(|| {
        let mut out = Vec::with_capacity(compressed_bytes);
        encode(mesh.clone(), &mut out, Config::default()).expect("draco-oxide encode");
    });

    let decode_ms = time_median_ms(|| {
        let buf = buffer.clone();
        draco_oxide::decode::decode(buf.into_iter()).expect("draco-oxide decode");
    });

    CodecResult {
        encode_ms,
        decode_ms,
        compressed_bytes,
    }
}

/// Benchmarks Google Draco through the libdraco shim, with the same timing
/// harness as draco-oxide. Draco parses the OBJ itself, so each codec encodes
/// its own natural in-memory form of the same file.
#[cfg(have_libdraco)]
fn bench_draco(obj: &Path) -> Result<CodecResult, String> {
    let mesh = draco_ffi::load_obj(obj).ok_or("obj load failed")?;
    let encoded = draco_ffi::encode(&mesh).ok_or("encode failed")?;
    let compressed_bytes = encoded.len();

    let encode_ms = time_median_ms(|| {
        assert!(draco_ffi::encode_discard(&mesh), "draco encode failed");
    });
    let decode_ms = time_median_ms(|| {
        assert!(draco_ffi::decode(&encoded), "draco decode failed");
    });

    Ok(CodecResult {
        encode_ms,
        decode_ms,
        compressed_bytes,
    })
}

const OXIDE_COLOR: &str = "#2a78d6";
const DRACO_COLOR: &str = "#008300";

fn write_charts(assets_dir: &Path, results: &[MeshBench]) {
    let categories: Vec<String> = results.iter().map(|r| r.name.clone()).collect();

    let two_series = |oxide: Vec<Option<f64>>, draco: Vec<Option<f64>>| {
        [
            chart::Series {
                name: "draco-oxide",
                color: OXIDE_COLOR,
                values: oxide,
                na_label: "n/a",
            },
            chart::Series {
                name: "Draco",
                color: DRACO_COLOR,
                values: draco,
                na_label: "n/a",
            },
        ]
    };

    let ratio_chart = chart::grouped_bar_svg(
        "Compression ratio",
        "raw geometry bytes / compressed bytes — higher is better",
        &categories,
        &two_series(
            results
                .iter()
                .map(|r| Some(r.raw_bytes as f64 / r.oxide.compressed_bytes as f64))
                .collect(),
            results
                .iter()
                .map(|r| Some(r.raw_bytes as f64 / r.draco.compressed_bytes as f64))
                .collect(),
        ),
    );
    std::fs::write(assets_dir.join("compression-ratio.svg"), ratio_chart).unwrap();

    // Speed charts show throughput in MB/s of raw geometry so meshes of
    // different sizes share an axis: the encoder consumes raw bytes, the
    // decoder produces them.
    let throughput = |raw_bytes: usize, ms: f64| Some(raw_bytes as f64 / ms * 1e3 / 1e6);

    let encode_chart = chart::grouped_bar_svg(
        "Encode speed",
        "input MB/s (raw geometry consumed per second) — higher is better",
        &categories,
        &two_series(
            results
                .iter()
                .map(|r| throughput(r.raw_bytes, r.oxide.encode_ms))
                .collect(),
            results
                .iter()
                .map(|r| throughput(r.raw_bytes, r.draco.encode_ms))
                .collect(),
        ),
    );
    std::fs::write(assets_dir.join("encode-speed.svg"), encode_chart).unwrap();

    let decode_chart = chart::grouped_bar_svg(
        "Decode speed",
        "output MB/s (raw geometry produced per second) — higher is better",
        &categories,
        &two_series(
            results
                .iter()
                .map(|r| throughput(r.raw_bytes, r.oxide.decode_ms))
                .collect(),
            results
                .iter()
                .map(|r| throughput(r.raw_bytes, r.draco.decode_ms))
                .collect(),
        ),
    );
    std::fs::write(assets_dir.join("decode-speed.svg"), decode_chart).unwrap();
}

fn fmt_ms(ms: f64) -> String {
    if ms >= 100.0 {
        format!("{ms:.0}")
    } else if ms >= 10.0 {
        format!("{ms:.1}")
    } else if ms >= 1.0 {
        format!("{ms:.2}")
    } else {
        format!("{ms:.3}")
    }
}

fn fmt_kb(bytes: usize) -> String {
    format!("{:.1}", bytes as f64 / 1024.0)
}

fn render_report(results: &[MeshBench]) -> String {
    let mut md = String::new();
    md.push_str("# draco-oxide vs Google Draco — benchmark\n\n");
    md.push_str(&format!(
        "Generated by `cargo run -p bench --release` on {}.\n\n",
        hostname_summary()
    ));
    md.push_str(
        "Both codecs run in-process with the same timing harness and matching \
         settings: edgebreaker connectivity, 11-bit positions, 10-bit texture \
         coordinates, 8-bit octahedral normals (Draco at compression level 7, its \
         CLI default).\n\n",
    );

    md.push_str("## Compression\n\n");
    md.push_str("![Compression ratio](assets/compression-ratio.svg)\n\n");

    md.push_str("## Speed\n\n");
    md.push_str("![Encode speed](assets/encode-speed.svg)\n\n");
    md.push_str("![Decode speed](assets/decode-speed.svg)\n\n");

    md.push_str("## Results\n\n");
    md.push_str(
        "| mesh | faces | raw KB | oxide KB | Draco KB | oxide ratio | Draco ratio | \
         oxide enc ms | Draco enc ms | oxide dec ms | Draco dec ms |\n",
    );
    md.push_str("|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|\n");
    for r in results {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2} | {:.2} | {} | {} | {} | {} |\n",
            r.name,
            r.faces,
            fmt_kb(r.raw_bytes),
            fmt_kb(r.oxide.compressed_bytes),
            fmt_kb(r.draco.compressed_bytes),
            r.raw_bytes as f64 / r.oxide.compressed_bytes as f64,
            r.raw_bytes as f64 / r.draco.compressed_bytes as f64,
            fmt_ms(r.oxide.encode_ms),
            fmt_ms(r.draco.encode_ms),
            fmt_ms(r.oxide.decode_ms),
            fmt_ms(r.draco.decode_ms),
        ));
    }
    md.push('\n');

    md.push_str("## Method\n\n");
    md.push_str(
        "- Input meshes are the OBJ files in `tests/data/`; \"raw\" is the \
         uncompressed geometry both codecs start from (unique attribute values plus \
         32-bit face indices).\n\
         - Both codecs are timed in-process on in-memory data: median wall time over \
         repeated runs after a warmup, same harness for both.\n\
         - draco-oxide: `encode()` with `Config::default()`; decode is `decode()` \
         back to original-format floats.\n\
         - Google Draco: `libdraco` (the checkout built by `scripts/build-draco.sh`) \
         called through a C shim; encode is `Encoder::EncodeMeshToBuffer` with the \
         CLI-default options, decode is `Decoder::DecodeMeshFromBuffer`. Each codec \
         encodes its own parse of the same OBJ.\n\
         - Speed is throughput over the raw geometry size: MB/s consumed by the \
         encoder and MB/s produced by the decoder (1 MB = 10^6 bytes).\n\
         - Compression ratio = raw bytes / compressed bytes, same raw baseline for \
         both codecs.\n",
    );
    md
}

fn hostname_summary() -> String {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|m| m.trim().to_string())
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());
    format!("{} ({})", cpu, std::env::consts::OS)
}
