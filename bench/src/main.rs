//! Benchmark of draco-oxide against Google Draco over the meshes in
//! `tests/data`, measuring encode/decode speed and compression ratio. Renders
//! SVG charts into `bench/assets/` and splices a markdown report into
//! `bench/README.md`.
//!
//! Both codecs run in-process and are timed with the same harness: Google Draco
//! is linked as `libdraco` through the C shim in `shim.cc` (built by
//! `scripts/build-draco.sh`; override with `DRACO_SRC_DIR`/`DRACO_BUILD_DIR`).
//! Run with `cargo run -p bench --release`. Pass `--local` to also bench the
//! OBJ files in the git-ignored `tests/data/local/` directory.

// Without libdraco there is nothing to compare against, so `main` exits before
// reaching the harness and everything below it is unreferenced in that build.
#![cfg_attr(not(have_libdraco), allow(dead_code, unused_imports))]

mod alloc_track;
mod chart;

use draco_oxide::core::attribute::AttributeType;
use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode, Config};
use draco_oxide::io::obj::load_obj;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[global_allocator]
static ALLOC: alloc_track::TrackingAlloc = alloc_track::TrackingAlloc;

/// Per-codec measurements for one mesh, in milliseconds.
struct CodecResult {
    encode_ms: f64,
    decode_ms: f64,
    compressed_bytes: usize,
    /// Exact heap-byte stats of one encode (tracking allocator); oxide only,
    /// the C++ side does not allocate through the Rust allocator. Relative to
    /// the input mesh, which is held before the window opens.
    encode_heap: Option<alloc_track::HeapStats>,
    /// Peak additional RSS of one encode in a fresh subprocess, in bytes.
    encode_peak_rss: Option<usize>,
    /// Exact heap-byte stats of one decode (tracking allocator); oxide only.
    decode_heap: Option<alloc_track::HeapStats>,
    /// Peak additional RSS of one decode in a fresh subprocess, in bytes.
    decode_peak_rss: Option<usize>,
}

struct MeshBench {
    name: String,
    /// Comma-separated attribute codes carried by the mesh, e.g. "P, N, T".
    attrs: String,
    faces: usize,
    raw_bytes: usize,
    oxide: CodecResult,
    draco: CodecResult,
}

impl MeshBench {
    /// Display label combining the mesh name with its attributes, e.g.
    /// `bunny (P, N)`.
    fn label(&self) -> String {
        format!("{} ({})", self.name, self.attrs)
    }
}

/// One-letter code for an attribute, following the report legend
/// (P position, N normal, C color, T texture coordinate).
fn attr_code(ty: AttributeType) -> &'static str {
    match ty {
        AttributeType::Position => "P",
        AttributeType::Normal => "N",
        AttributeType::Color => "C",
        AttributeType::TextureCoordinate => "T",
        AttributeType::Tangent => "Tan",
        AttributeType::Material => "M",
        AttributeType::Joint => "J",
        AttributeType::Weight => "W",
        AttributeType::Custom => "X",
        AttributeType::Invalid => "?",
    }
}

/// The mesh's attributes as a stable "P, N, T"-style summary.
fn attr_summary(mesh: &Mesh) -> String {
    let mut codes: Vec<&str> = mesh
        .attributes
        .iter()
        .map(|a| attr_code(a.get_attribute_type()))
        .collect();
    codes.dedup();
    codes.join(", ")
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
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 && args[1] == "--mem-child" {
        mem_child(&args[2], Path::new(&args[3]));
        return;
    }
    let include_local = args.iter().skip(1).any(|a| a == "--local");

    #[cfg(not(have_libdraco))]
    {
        let _ = include_local;
        eprintln!(
            "bench was built without libdraco. Run scripts/build-draco.sh (or set \
             DRACO_SRC_DIR / DRACO_BUILD_DIR) and rebuild."
        );
        std::process::exit(1);
    }

    #[cfg(have_libdraco)]
    run(include_local);
}

/// Subprocess mode for the peak-RSS measurement: runs one decode (input is a
/// compressed stream) or one encode (input is an OBJ) in this fresh process and
/// prints its additional peak RSS in bytes. The input is loaded before the
/// high-water mark is reset, so only what the measured operation adds counts.
fn mem_child(codec: &str, input: &Path) {
    let run: Box<dyn FnOnce()> = match codec {
        "oxide" => {
            let bytes = std::fs::read(input).expect("read drc");
            Box::new(move || {
                let decoded = draco_oxide::decode::decode(&bytes).expect("oxide decode");
                std::hint::black_box(&decoded);
            })
        }
        "oxide-enc" => {
            let mesh = load_obj(input.to_str().expect("obj path")).expect("obj load");
            Box::new(move || {
                let mut out = Vec::new();
                encode(mesh, &mut out, Config::default()).expect("oxide encode");
                std::hint::black_box(&out);
            })
        }
        #[cfg(have_libdraco)]
        "draco" => {
            let bytes = std::fs::read(input).expect("read drc");
            Box::new(move || assert!(draco_ffi::decode(&bytes), "draco decode failed"))
        }
        #[cfg(have_libdraco)]
        "draco-enc" => {
            let mesh = draco_ffi::load_obj(input).expect("obj load");
            Box::new(move || assert!(draco_ffi::encode_discard(&mesh), "draco encode failed"))
        }
        other => panic!("unknown mem-child codec: {other}"),
    };
    // Resetting the kernel's RSS high-water mark makes VmHWM track only what
    // the operation below adds.
    std::fs::write("/proc/self/clear_refs", "5").expect("reset VmHWM");
    let baseline = read_vm_hwm_bytes().expect("read VmHWM");
    run();
    let peak = read_vm_hwm_bytes().expect("read VmHWM");
    println!("{}", peak.saturating_sub(baseline));
}

/// The process's RSS high-water mark (`VmHWM`) in bytes.
fn read_vm_hwm_bytes() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: usize = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Runs a `--mem-child` subprocess over `input` and returns the printed peak
/// additional RSS in bytes.
fn mem_child_rss(codec: &str, input: &Path) -> Option<usize> {
    let exe = std::env::current_exe().ok()?;
    let out = std::process::Command::new(exe)
        .arg("--mem-child")
        .arg(codec)
        .arg(input)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `mem_child_rss` over an in-memory compressed stream (the decode modes).
fn peak_rss_via_child(codec: &str, drc_bytes: &[u8]) -> Option<usize> {
    let path = std::env::temp_dir().join(format!("draco-bench-mem-{}.drc", std::process::id()));
    std::fs::write(&path, drc_bytes).ok()?;
    let rss = mem_child_rss(codec, &path);
    let _ = std::fs::remove_file(&path);
    rss
}

#[cfg(have_libdraco)]
fn run(include_local: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let data_dir = root.join("tests/data");
    let bench_dir = root.join("bench");
    let assets_dir = bench_dir.join("assets");
    std::fs::create_dir_all(&assets_dir).expect("create bench/assets");

    // The benched subset of tests/data. The rest of the directory is
    // regression fixtures (pathological_*, tiny synthetic shapes) or meshes
    // Draco cannot encode for a fair comparison (mobius, non-orientable).
    let included = [
        "DragonAttenuation",
        "Duck",
        "bldg_894e93d9",
        "bunny",
        "cube_quads",
        "sphere",
        "torus",
    ];
    let mut objs: Vec<PathBuf> = included
        .iter()
        .filter_map(|name| {
            let p = data_dir.join(format!("{name}.obj"));
            if !p.exists() {
                eprintln!("skipping {name}: {} not found", p.display());
                return None;
            }
            Some(p)
        })
        .collect();
    if include_local {
        let local_dir = data_dir.join("local");
        if let Ok(entries) = std::fs::read_dir(&local_dir) {
            let stems: Vec<_> = objs
                .iter()
                .filter_map(|p| Some(p.file_stem()?.to_os_string()))
                .collect();
            objs.extend(entries.filter_map(|e| {
                let p = e.ok()?.path();
                if p.extension()? != "obj" {
                    return None;
                }
                let stem = p.file_stem()?.to_os_string();
                (!stems.contains(&stem)).then_some(p)
            }));
        } else {
            eprintln!("--local: no {} directory, skipping", local_dir.display());
        }
    }
    objs.sort_by_key(|p| p.file_stem().map(|s| s.to_os_string()));

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
        let attrs = attr_summary(&mesh);

        let oxide = bench_oxide(&mesh, obj);
        let draco = match bench_draco(obj) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  skipping {name}: libdraco failed ({e})");
                continue;
            }
        };

        results.push(MeshBench {
            name,
            attrs,
            faces,
            raw_bytes,
            oxide,
            draco,
        });
    }

    // Largest meshes first, in the table and the charts alike.
    results.sort_by_key(|r| std::cmp::Reverse(r.faces));

    write_charts(&assets_dir, &results);
    let report = render_report(&results, include_local);
    splice_report_into_readme(&bench_dir, &report);
}

/// Replaces the region between the report markers in `bench/README.md` with
/// the freshly generated report, so the README always shows the latest run.
fn splice_report_into_readme(bench_dir: &Path, report: &str) {
    const START: &str = "<!-- report:start -->";
    const END: &str = "<!-- report:end -->";
    let path = bench_dir.join("README.md");
    let Ok(readme) = std::fs::read_to_string(&path) else {
        eprintln!("{} not found; skipping README update", path.display());
        return;
    };
    let (Some(start), Some(end)) = (readme.find(START), readme.find(END)) else {
        eprintln!("report markers missing in {}; skipping", path.display());
        return;
    };
    if end < start {
        eprintln!(
            "report markers out of order in {}; skipping",
            path.display()
        );
        return;
    }
    let updated = format!(
        "{}\n{}{}",
        &readme[..start + START.len()],
        report,
        &readme[end..]
    );
    std::fs::write(&path, updated).expect("write bench/README.md");
    eprintln!("report spliced into {}", path.display());
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

fn bench_oxide(mesh: &Mesh, obj: &Path) -> CodecResult {
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
        draco_oxide::decode::decode(&buffer).expect("draco-oxide decode");
    });

    // The input clone happens before the window opens, so the stats are memory
    // on top of the held input mesh, matching the subprocess RSS delta.
    let mesh_for_heap = mesh.clone();
    alloc_track::start_window();
    let mut encoded = Vec::new();
    encode(mesh_for_heap, &mut encoded, Config::default()).expect("draco-oxide encode");
    let encode_heap = alloc_track::end_window();
    drop(encoded);

    alloc_track::start_window();
    let decoded = draco_oxide::decode::decode(&buffer).expect("draco-oxide decode");
    let decode_heap = alloc_track::end_window();
    drop(decoded);

    CodecResult {
        encode_ms,
        decode_ms,
        compressed_bytes,
        encode_heap: Some(encode_heap),
        encode_peak_rss: mem_child_rss("oxide-enc", obj),
        decode_heap: Some(decode_heap),
        decode_peak_rss: peak_rss_via_child("oxide", &buffer),
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
        encode_heap: None,
        encode_peak_rss: mem_child_rss("draco-enc", obj),
        decode_heap: None,
        decode_peak_rss: peak_rss_via_child("draco", &encoded),
    })
}

const OXIDE_COLOR: &str = "#58a6ff";
const DRACO_COLOR: &str = "#3fb950";

fn write_charts(assets_dir: &Path, results: &[MeshBench]) {
    let categories: Vec<String> = results.iter().map(|r| r.label()).collect();

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

    // Peak-RSS is the one memory metric measured identically for both codecs
    // (fresh subprocess per decode), so it is the comparable chart. Normalized
    // by the decoded output size so meshes share an axis. Peaks under 5 MB are
    // dropped from the chart: there the ~1 MB process-baseline RSS noise
    // dominates the ratio (the table still lists them).
    let ratio = |bytes: Option<usize>, raw: usize| {
        bytes
            .filter(|&b| b >= 5_000_000)
            .map(|b| b as f64 / raw as f64)
    };
    let memory_chart = chart::grouped_bar_svg(
        "Decode memory",
        "peak additional RSS of one decode / decoded geometry bytes — lower is better",
        &categories,
        &two_series(
            results
                .iter()
                .map(|r| ratio(r.oxide.decode_peak_rss, r.raw_bytes))
                .collect(),
            results
                .iter()
                .map(|r| ratio(r.draco.decode_peak_rss, r.raw_bytes))
                .collect(),
        ),
    );
    std::fs::write(assets_dir.join("decode-memory.svg"), memory_chart).unwrap();

    let encode_memory_chart = chart::grouped_bar_svg(
        "Encode memory",
        "peak additional RSS of one encode / raw geometry bytes — lower is better",
        &categories,
        &two_series(
            results
                .iter()
                .map(|r| ratio(r.oxide.encode_peak_rss, r.raw_bytes))
                .collect(),
            results
                .iter()
                .map(|r| ratio(r.draco.encode_peak_rss, r.raw_bytes))
                .collect(),
        ),
    );
    std::fs::write(assets_dir.join("encode-memory.svg"), encode_memory_chart).unwrap();
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

/// Appends one memory table (heap stats for oxide, peak RSS for both codecs),
/// with every value normalized by the mesh's raw geometry size. `row` selects
/// the encode or decode fields.
fn push_memory_table<F>(md: &mut String, results: &[MeshBench], row: F)
where
    F: Fn(
        &MeshBench,
    ) -> (
        Option<&alloc_track::HeapStats>,
        Option<usize>,
        Option<usize>,
    ),
{
    md.push_str(
        "| mesh | oxide heap peak | oxide heap avg | oxide heap RMS | \
         oxide peak RSS | Draco peak RSS |\n",
    );
    md.push_str("|---|--:|--:|--:|--:|--:|\n");
    for r in results {
        let (heap, oxide_rss, draco_rss) = row(r);
        let norm = |bytes: f64| format!("{:.2}", bytes / r.raw_bytes as f64);
        let heap_cols = heap.map_or("n/a | n/a | n/a".into(), |h| {
            format!(
                "{} | {} | {}",
                norm(h.peak as f64),
                norm(h.avg),
                norm(h.rms)
            )
        });
        let rss_col = |rss: Option<usize>| rss.map_or("n/a".into(), |b| norm(b as f64));
        md.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.label(),
            heap_cols,
            rss_col(oxide_rss),
            rss_col(draco_rss),
        ));
    }
    md.push('\n');
}

fn render_report(results: &[MeshBench], include_local: bool) -> String {
    let mut md = String::new();
    if include_local {
        md.push_str(
            "This run was generated with `--local`, so it also includes the OBJ \
             files in the git-ignored `tests/data/local/` directory.\n\n",
        );
    }

    md.push_str("## Compression\n\n");
    md.push_str("![Compression ratio](assets/compression-ratio.svg)\n\n");

    md.push_str("## Speed\n\n");
    md.push_str("![Encode speed](assets/encode-speed.svg)\n\n");
    md.push_str("![Decode speed](assets/decode-speed.svg)\n\n");

    md.push_str("## Encode memory\n\n");
    md.push_str("![Encode memory](assets/encode-memory.svg)\n\n");
    md.push_str(
        "All values are normalized by the raw geometry size (the raw KB column of \
         the results table) and measure memory on top of the already-loaded input \
         mesh: memory bytes per input byte. Peak RSS is measured the same way for \
         both codecs (one encode in a fresh subprocess that has loaded the OBJ, \
         `VmHWM` delta) and is directly comparable. The heap columns are exact \
         allocation-event byte counts from a tracking allocator (oxide only): peak, \
         time-weighted average, and RMS of live encode-window bytes; they exclude \
         allocator overhead, so they read below RSS.\n\n",
    );
    push_memory_table(&mut md, results, |r| {
        (
            r.oxide.encode_heap.as_ref(),
            r.oxide.encode_peak_rss,
            r.draco.encode_peak_rss,
        )
    });

    md.push_str("## Decode memory\n\n");
    md.push_str("![Decode memory](assets/decode-memory.svg)\n\n");
    md.push_str(
        "All values are normalized by the decoded geometry size (the raw KB column \
         of the results table): memory bytes per output byte. Peak RSS is measured \
         the same way for both codecs (one decode in a fresh subprocess, `VmHWM` \
         delta) and is directly comparable. The heap columns are exact \
         allocation-event byte counts from a tracking allocator (oxide only): peak, \
         time-weighted average, and RMS of live decode-window bytes; they exclude \
         allocator overhead, so they read below RSS.\n\n",
    );
    push_memory_table(&mut md, results, |r| {
        (
            r.oxide.decode_heap.as_ref(),
            r.oxide.decode_peak_rss,
            r.draco.decode_peak_rss,
        )
    });

    md.push_str("## Results\n\n");
    md.push_str(
        "Each mesh name is annotated with the attributes it carries: \
         P (position), N (normal), T (texture coordinate), C (color).\n\n",
    );
    md.push_str(
        "| mesh | faces | raw KB | oxide KB | Draco KB | oxide ratio | Draco ratio | \
         oxide enc ms | Draco enc ms | oxide dec ms | Draco dec ms |\n",
    );
    md.push_str("|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|\n");
    for r in results {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2} | {:.2} | {} | {} | {} | {} |\n",
            r.label(),
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

    md.push_str("Measurement details are in the [Method](#method) section above.\n");
    md
}
