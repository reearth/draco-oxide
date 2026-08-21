//! Declarative test harness for draco-oxide.
//!
//! A test profile is a TOML file describing an ordered series of operations
//! (encode / decode / validate / compare) over named artifacts. The harness
//! interprets the operations at test time. Each producing operation writes
//! its result to a per-profile output directory under a name the user picks;
//! later operations refer to artifacts by that name. Names that don't match
//! a prior output are resolved against the shared `data/` directory.
//!
//! Operations that need Google Draco's reference binaries (`DracoEncode`,
//! `DracoDecode`) cause the whole profile to **skip with a warning** when
//! those binaries aren't available — the same auto-skip behavior used by the
//! existing `draco_decode` smoke test, so `cargo test` keeps passing on
//! machines without Google Draco built.
//!
//! `DRACO_OXIDE_WASM=<path to wasi-codec.wasm>` reroutes the draco-oxide
//! operations through that module under `wasmtime`, so the same profiles
//! exercise the codec on a 32-bit WASM target (see [`WasmCodec`]).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{self as oxide_encode, encode_mesh as oxide_encode_fn};
use draco_oxide::io::obj::load_obj;

mod render;

// ---------------------------------------------------------------------------
// Profile schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub operations: Vec<Operation>,
}

/// `DracoOxideEncode::cfg`: the encoder [`Config`](oxide_encode::Config)
/// together with the TOML table it was parsed from, so the WASM codec path
/// can hand the same configuration to the `wasi-codec` subprocess.
#[derive(Debug, Clone)]
pub struct OxideCfg {
    pub table: toml::Table,
    pub parsed: oxide_encode::Config,
}

impl Default for OxideCfg {
    fn default() -> Self {
        Self {
            table: toml::Table::new(),
            parsed: <oxide_encode::Config as ConfigType>::default(),
        }
    }
}

impl<'de> Deserialize<'de> for OxideCfg {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let table = toml::Table::deserialize(d)?;
        let parsed = table.clone().try_into().map_err(serde::de::Error::custom)?;
        Ok(Self { table, parsed })
    }
}

/// One step in a test profile.
///
/// Tag field is `op = "..."` so the TOML is flat and idiomatic:
///
/// ```toml
/// [[operations]]
/// op = "DracoOxideEncode"
/// input = "bunny.obj"
/// output = "oxide.drc"
/// ```
#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum Operation {
    /// Encode a mesh with draco-oxide.
    ///
    /// `cfg` (optional) is the encoder [`Config`](oxide_encode::Config),
    /// deserialized inline from TOML (an omitted table means
    /// `encode::Config::default()`). For example, to compress normals with the
    /// zero-CPU "trust the prediction" path:
    ///
    /// ```toml
    /// [[operations]]
    /// op = "DracoOxideEncode"
    /// input = "cube.obj"
    /// output = "oxide.drc"
    /// cfg = { normal = "PredictedOnly" }
    /// ```
    ///
    /// `timeout_secs` (optional) fails the operation if the encode runs longer
    /// than that many seconds. draco-oxide encodes meshes that are pathological
    /// for other encoders in well under a second, so this is a cheap regression
    /// guard against reintroducing a blow-up.
    DracoOxideEncode {
        input: String,
        output: String,
        #[serde(default)]
        timeout_secs: Option<f64>,
        #[serde(default)]
        cfg: OxideCfg,
    },
    /// Decode a `.drc` with draco-oxide's own decoder, writing Wavefront OBJ.
    DracoOxideDecode { input: String, output: String },
    /// Encode with Google Draco's `draco_encoder` CLI.
    DracoEncode {
        input: String,
        output: String,
        #[serde(default)]
        cfg: GoogleEncodeConfig,
    },
    /// Decode with Google Draco's `draco_decoder` CLI.
    DracoDecode {
        input: String,
        output: String,
        #[serde(default)]
        cfg: GoogleDecodeConfig,
    },
    /// Confirm an artifact parses as the named format. `.drc` cannot be
    /// validated here; it is validated by being decoded, via
    /// [`Operation::DracoDecode`] or [`Operation::DracoOxideDecode`].
    Validation { input: String, fmt: FormatName },
    /// Assert an artifact's exact byte length and FNV-1a hash (`fnv1a` is a
    /// decimal or 0x-hex string; the value exceeds TOML's integer range).
    /// `DUMP_ENCODE_FINGERPRINTS` prints computed fingerprints instead of
    /// asserting, for rebaselining.
    Fingerprint {
        input: String,
        len: usize,
        fnv1a: String,
    },
    /// Compare two artifacts under one or more comparison methods. Each
    /// method asserts its own pass/fail predicate (e.g. a distance threshold).
    Comparison {
        input1: String,
        input2: String,
        methods: Vec<ComparisonMethod>,
    },
}

/// Wrapper for `draco_encoder` CLI flags (see `draco_encoder --help`). All
/// fields are optional; omitted ones fall back to draco_encoder's defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GoogleEncodeConfig {
    /// `-cl` — compression level, 0..=10 (default 7 in draco_encoder).
    pub compression_level: Option<u8>,
    /// `-qp` — position quantization bits (default 11).
    pub position_quantization: Option<i32>,
    /// `-qt` — texture-coordinate quantization bits (default 10).
    pub uv_quantization: Option<i32>,
    /// `-qn` — normal quantization bits (default 8).
    pub normal_quantization: Option<i32>,
    /// `-qg` — generic-attribute quantization bits (default 8).
    pub generic_quantization: Option<i32>,
    /// `--skip ATTRIBUTE` — repeatable; values are NORMAL, TEX_COORD, GENERIC.
    pub skip: Vec<String>,
    /// `--metadata`.
    pub metadata: bool,
    /// `-preserve_polygons`.
    pub preserve_polygons: bool,
}

impl GoogleEncodeConfig {
    fn apply(&self, cmd: &mut Command) {
        if let Some(v) = self.compression_level {
            cmd.arg("-cl").arg(v.to_string());
        }
        if let Some(v) = self.position_quantization {
            cmd.arg("-qp").arg(v.to_string());
        }
        if let Some(v) = self.uv_quantization {
            cmd.arg("-qt").arg(v.to_string());
        }
        if let Some(v) = self.normal_quantization {
            cmd.arg("-qn").arg(v.to_string());
        }
        if let Some(v) = self.generic_quantization {
            cmd.arg("-qg").arg(v.to_string());
        }
        for attr in &self.skip {
            cmd.arg("--skip").arg(attr);
        }
        if self.metadata {
            cmd.arg("--metadata");
        }
        if self.preserve_polygons {
            cmd.arg("-preserve_polygons");
        }
    }
}

/// `draco_decoder` has no config knobs today; the type exists for symmetry
/// and as a place to grow into.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GoogleDecodeConfig {}

#[derive(Debug, Deserialize)]
pub enum FormatName {
    Obj,
    Gltf,
}

/// Each variant carries its own parameters (typically a tolerance).
#[derive(Debug, Deserialize)]
#[serde(tag = "method")]
pub enum ComparisonMethod {
    /// Symmetric area-weighted surface L2 (RMS) distance between the two
    /// meshes. For each triangle the squared distance from its centroid to the
    /// closest point on the *other* mesh's surface is weighted by the
    /// triangle's area; summed and divided by total area gives a one-sided
    /// mean-squared distance. Both directions are averaged and the square root
    /// taken, yielding a true distance-unit norm. Robust to vertex reordering
    /// and remeshing. Asserts the value is `<= max`. Both inputs must currently
    /// be OBJ files.
    L2Norm { max: f64 },
    /// Rendered-view structural similarity. Renders both inputs from several
    /// viewpoints with a small CPU rasterizer (see [`render`]) and scores RGB
    /// SSIM per view. Asserts the *worst* view's score is `>= min` (1.0 =
    /// identical). Both inputs must be OBJ files. The rendered PNGs are written
    /// to the profile's output dir for debugging.
    Ssim {
        /// Minimum acceptable SSIM in `0.0..=1.0` for every view.
        min: f64,
        /// Square render size in pixels (default 512).
        #[serde(default)]
        resolution: Option<u32>,
        /// Number of viewpoints rotated around the model's up axis; the worst
        /// (minimum) score across them gates the test (default 4).
        #[serde(default)]
        views: Option<usize>,
        /// What to color fragments by: `"Geometry"` (default — shape via flat
        /// shading), `"Normal"`, `"Uv"`, or `"VertexColor"`. The attribute modes
        /// let the test catch regressions in non-position attributes; they
        /// require the corresponding attribute to be present in both OBJs.
        #[serde(default)]
        color_by: render::ColorBy,
    },
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Run a profile end-to-end. Intended to be called from generated `#[test]`
/// stubs emitted by `build.rs`.
///
/// `name` is the sanitized profile name (also the per-profile output subdir).
/// `data_dir` and `outputs_dir` resolve input/output bindings.
///
/// Panics on any failure so the surrounding `#[test]` reports a real failure;
/// returns silently (with an `eprintln!` SKIP message) when a needed external
/// tool is missing.
pub fn run_profile(name: &str, profile_path: &str, data_dir: &str, outputs_dir: &str) {
    let profile_path = Path::new(profile_path);
    let data_dir = Path::new(data_dir);
    let out_dir = Path::new(outputs_dir).join(name);

    let toml_text = std::fs::read_to_string(profile_path).unwrap_or_else(|e| {
        panic!(
            "[{name}] failed to read profile {}: {e}",
            profile_path.display()
        )
    });
    let profile: Profile = toml::from_str(&toml_text)
        .unwrap_or_else(|e| panic!("[{name}] failed to parse {}: {e}", profile_path.display()));

    // Pre-scan: if any op needs an external tool that isn't available, skip
    // the whole profile rather than failing partway through.
    let needs_google_encoder = profile
        .operations
        .iter()
        .any(|op| matches!(op, Operation::DracoEncode { .. }));
    let needs_google_decoder = profile
        .operations
        .iter()
        .any(|op| matches!(op, Operation::DracoDecode { .. }));

    let google_encoder = needs_google_encoder
        .then(find_google_draco_encoder)
        .flatten();
    let google_decoder = needs_google_decoder
        .then(find_google_draco_decoder)
        .flatten();

    if needs_google_encoder && google_encoder.is_none() {
        skip(name, "Google Draco `draco_encoder` not found (run scripts/build-draco.sh, or set DRACO_ENCODER=<path>)");
        return;
    }
    if needs_google_decoder && google_decoder.is_none() {
        skip(name, "Google Draco `draco_decoder` not found (run scripts/build-draco.sh, or set DRACO_DECODER=<path>)");
        return;
    }

    let wasm_codec = WasmCodec::from_env(name, &out_dir, data_dir);

    std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| {
        panic!(
            "[{name}] failed to create output dir {}: {e}",
            out_dir.display()
        )
    });

    let resolve_input = |bind: &str| -> PathBuf {
        let from_outputs = out_dir.join(bind);
        if from_outputs.is_file() {
            from_outputs
        } else {
            data_dir.join(bind)
        }
    };
    let resolve_output = |bind: &str| -> PathBuf { out_dir.join(bind) };

    for (idx, op) in profile.operations.iter().enumerate() {
        let label = format!("[{name}] op#{idx} {}", op_kind(op));
        eprintln!("{label} starting");
        match op {
            Operation::DracoOxideEncode {
                input,
                output,
                timeout_secs,
                cfg,
            } => {
                let in_path = resolve_input(input);
                let out_path = resolve_output(output);
                let timeout = timeout_secs.map(Duration::from_secs_f64);
                if let Some(wasm) = &wasm_codec {
                    let cfg_path = (!cfg.table.is_empty()).then(|| {
                        let path = out_dir.join(format!("op{idx}.cfg.toml"));
                        std::fs::write(&path, cfg.table.to_string()).unwrap_or_else(|e| {
                            panic!("{label}: writing {} failed: {e}", path.display())
                        });
                        path
                    });
                    wasm.encode(&label, &in_path, &out_path, cfg_path.as_deref(), timeout);
                    continue;
                }
                let buf = match timeout {
                    Some(timeout) => encode_oxide_with_timeout(
                        &label,
                        in_path.clone(),
                        timeout,
                        cfg.parsed.clone(),
                    ),
                    None => {
                        let mesh = load_mesh_for_oxide(&in_path).unwrap_or_else(|e| {
                            panic!("{label}: failed to load {}: {e}", in_path.display())
                        });
                        let mut buf = Vec::new();
                        oxide_encode_fn(mesh, &mut buf, cfg.parsed.clone()).unwrap_or_else(|e| {
                            panic!("{label}: draco-oxide encode failed: {e:?}")
                        });
                        buf
                    }
                };
                std::fs::write(&out_path, &buf).unwrap_or_else(|e| {
                    panic!("{label}: writing {} failed: {e}", out_path.display())
                });
            }
            Operation::DracoOxideDecode { input, output } => {
                let in_path = resolve_input(input);
                let out_path = resolve_output(output);
                if let Some(wasm) = &wasm_codec {
                    wasm.decode(&label, &in_path, &out_path);
                    continue;
                }
                draco_oxide::io::obj::decode_drc_to_obj(&in_path, &out_path).unwrap_or_else(|e| {
                    panic!(
                        "{label}: draco-oxide decode of {} failed: {e}",
                        in_path.display()
                    )
                });
            }
            Operation::DracoEncode { input, output, cfg } => {
                let bin = google_encoder.as_ref().expect("pre-scan guarantees this");
                let in_path = resolve_input(input);
                let out_path = resolve_output(output);
                let mut cmd = Command::new(bin);
                cmd.arg("-i").arg(&in_path).arg("-o").arg(&out_path);
                cfg.apply(&mut cmd);
                run_subprocess(&label, "draco_encoder", cmd);
            }
            Operation::DracoDecode {
                input,
                output,
                cfg: _,
            } => {
                let bin = google_decoder.as_ref().expect("pre-scan guarantees this");
                let in_path = resolve_input(input);
                let out_path = resolve_output(output);
                let mut cmd = Command::new(bin);
                cmd.arg("-i").arg(&in_path).arg("-o").arg(&out_path);
                run_subprocess(&label, "draco_decoder", cmd);
            }
            Operation::Validation { input, fmt } => {
                let path = resolve_input(input);
                validate(&label, &path, fmt);
            }
            Operation::Fingerprint { input, len, fnv1a } => {
                let path = resolve_input(input);
                let bytes = std::fs::read(&path)
                    .unwrap_or_else(|e| panic!("{label} failed to read {}: {e}", path.display()));
                let hash = fnv1a_hash(&bytes);
                if std::env::var("DUMP_ENCODE_FINGERPRINTS").is_ok() {
                    eprintln!(
                        "{label} {}: len = {}, fnv1a = \"{hash:#x}\"",
                        path.display(),
                        bytes.len()
                    );
                    continue;
                }
                let expected = parse_u64(fnv1a).unwrap_or_else(|| {
                    panic!("{label} fnv1a {fnv1a:?} is not a decimal or 0x-hex u64")
                });
                assert_eq!(
                    (bytes.len(), hash),
                    (*len, expected),
                    "{label} encoded output changed for {} (got len = {}, fnv1a = \"{hash:#x}\")",
                    path.display(),
                    bytes.len(),
                );
            }
            Operation::Comparison {
                input1,
                input2,
                methods,
            } => {
                let p1 = resolve_input(input1);
                let p2 = resolve_input(input2);
                for method in methods {
                    match method {
                        ComparisonMethod::L2Norm { max } => {
                            let m1 = render::load_obj_mesh(&p1).unwrap_or_else(|e| {
                                panic!("{label} L2Norm: failed to load {}: {e}", p1.display())
                            });
                            let m2 = render::load_obj_mesh(&p2).unwrap_or_else(|e| {
                                panic!("{label} L2Norm: failed to load {}: {e}", p2.display())
                            });
                            let dist = symmetric_surface_l2(&m1, &m2);
                            eprintln!("{label} L2Norm: dist = {dist} (max {max})");
                            assert!(
                                dist <= *max,
                                "{label} L2Norm: distance {dist} > max {max}\n    inputs: {} vs {}",
                                p1.display(),
                                p2.display(),
                            );
                        }
                        ComparisonMethod::Ssim {
                            min,
                            resolution,
                            views,
                            color_by,
                        } => {
                            let res = resolution.unwrap_or(512);
                            let n_views = views.unwrap_or(4).max(1);
                            let m1 = render::load_obj_mesh(&p1).unwrap_or_else(|e| {
                                panic!("{label} Ssim: failed to load {}: {e}", p1.display())
                            });
                            let m2 = render::load_obj_mesh(&p2).unwrap_or_else(|e| {
                                panic!("{label} Ssim: failed to load {}: {e}", p2.display())
                            });
                            // Frame both meshes with input1's framing so only
                            // genuine differences register.
                            let cam = render::Framing::fit(&m1.verts);
                            let imgs1 = render::render_views(&m1, &cam, res, n_views, *color_by)
                                .unwrap_or_else(|e| {
                                    panic!("{label} Ssim: rendering {} failed: {e}", p1.display())
                                });
                            let imgs2 = render::render_views(&m2, &cam, res, n_views, *color_by)
                                .unwrap_or_else(|e| {
                                    panic!("{label} Ssim: rendering {} failed: {e}", p2.display())
                                });

                            let s1 = p1.file_stem().and_then(|s| s.to_str()).unwrap_or("ref");
                            let s2 = p2.file_stem().and_then(|s| s.to_str()).unwrap_or("test");
                            let tag = color_by.tag();

                            let mut worst = f64::INFINITY;
                            let mut worst_view = 0;
                            for (i, (a, b)) in imgs1.iter().zip(&imgs2).enumerate() {
                                // True RGB SSIM: per-channel MSSIM (SSIM over 8x8
                                // windows) with the worst channel taken as the
                                // score. Not `rgb_hybrid_compare`, which blends in
                                // a color-distance term and so isn't SSIM.
                                let sim = image_compare::rgb_similarity_structure(
                                    &image_compare::Algorithm::MSSIMSimple,
                                    a,
                                    b,
                                )
                                .unwrap_or_else(|e| {
                                    panic!("{label} Ssim: comparison failed at view {i}: {e}")
                                });
                                if sim.score < worst {
                                    worst = sim.score;
                                    worst_view = i;
                                }
                            }
                            // 0 = never, 1 = on failure (so a failing run is
                            // debuggable without a rerun), 2+ = always.
                            let should_save = match render_save_level() {
                                0 => false,
                                1 => worst < *min,
                                _ => true,
                            };
                            if should_save {
                                for (i, (a, b)) in imgs1.iter().zip(&imgs2).enumerate() {
                                    let pa = out_dir.join(format!("ssim_{s1}_{tag}_view{i}.png"));
                                    let pb = out_dir.join(format!("ssim_{s2}_{tag}_view{i}.png"));
                                    a.save(&pa).unwrap_or_else(|e| {
                                        panic!("{label} Ssim: writing {} failed: {e}", pa.display())
                                    });
                                    b.save(&pb).unwrap_or_else(|e| {
                                        panic!("{label} Ssim: writing {} failed: {e}", pb.display())
                                    });
                                }
                                eprintln!(
                                    "{label} Ssim: wrote {} renders to {}",
                                    imgs1.len() * 2,
                                    out_dir.display()
                                );
                            }
                            eprintln!(
                                "{label} Ssim[{tag}]: worst score {worst} at view {worst_view} \
                                 (min {min}, {n_views} views, {res}px)"
                            );
                            assert!(
                                worst >= *min,
                                "{label} Ssim[{tag}]: worst score {worst} < min {min} (view {worst_view})\n    \
                                 inputs: {} vs {}\n    renders in: {}",
                                p1.display(),
                                p2.display(),
                                out_dir.display(),
                            );
                        }
                    }
                }
            }
        }
        eprintln!("{label} ok");
    }
}

fn op_kind(op: &Operation) -> &'static str {
    match op {
        Operation::DracoOxideEncode { .. } => "DracoOxideEncode",
        Operation::DracoOxideDecode { .. } => "DracoOxideDecode",
        Operation::DracoEncode { .. } => "DracoEncode",
        Operation::DracoDecode { .. } => "DracoDecode",
        Operation::Validation { .. } => "Validation",
        Operation::Fingerprint { .. } => "Fingerprint",
        Operation::Comparison { .. } => "Comparison",
    }
}

/// FNV-1a over an artifact's bytes. Deterministic, dependency-free.
fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Parses a u64 from a decimal or `0x`-prefixed hex string.
fn parse_u64(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn skip(name: &str, reason: &str) {
    eprintln!("SKIP [{name}]: {reason}");
}

/// Verbosity level for writing `Ssim` renders, from the `DRACO_SSIM_SAVE_RENDERS`
/// env var:
///   * `0` → never write,
///   * `1` → write only on failure (the default when unset),
///   * `2` (or higher) → always write, pass or fail.
///
/// e.g. `DRACO_SSIM_SAVE_RENDERS=2 cargo test -p tests`. A value that isn't a
/// non-negative integer falls back to the default.
fn render_save_level() -> u32 {
    std::env::var("DRACO_SSIM_SAVE_RENDERS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(1)
}

fn run_subprocess(label: &str, tool: &str, mut cmd: Command) {
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("{label}: failed to spawn {tool}: {e}"));
    if !output.status.success() {
        panic!(
            "{label}: {tool} failed with {}\n    stdout: {}\n    stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        );
    }
}

fn validate(label: &str, path: &Path, fmt: &FormatName) {
    match fmt {
        FormatName::Obj => {
            // Only checking that the file parses; the materials Result is ignored.
            if let Err(e) = tobj::load_obj(
                path,
                &tobj::LoadOptions {
                    triangulate: true,
                    single_index: true,
                    ..Default::default()
                },
            ) {
                panic!("{label}: OBJ validation failed for {}: {e}", path.display());
            }
        }
        FormatName::Gltf => {
            gltf::import(path).unwrap_or_else(|e| {
                panic!(
                    "{label}: glTF validation failed for {}: {e}",
                    path.display()
                )
            });
        }
    }
}

/// Load and encode `in_path` with draco-oxide on a worker thread, panicking if
/// the **encode** doesn't finish within `timeout`.
///
/// The work runs on the worker because `Mesh` is `!Send` (it owns a raw buffer
/// pointer), so it can't be built on one thread and handed to another — only the
/// `PathBuf` in and the encoded `Vec<u8>` out cross the boundary. The worker
/// signals `Loaded` once the OBJ is parsed; the main thread waits for that
/// unbounded (OBJ parsing is slow in debug and isn't what we're guarding), then
/// bounds only the encode with `timeout`. The encode is CPU-bound and can't be
/// interrupted, so on timeout the worker is left to run to completion (or until
/// the process exits) while the operation fails — fine for a guard whose whole
/// point is that the encode *shouldn't* take that long.
fn encode_oxide_with_timeout(
    label: &str,
    in_path: PathBuf,
    timeout: Duration,
    cfg: oxide_encode::Config,
) -> Vec<u8> {
    enum WorkerMsg {
        Loaded,
        Done(Result<Vec<u8>, String>),
    }

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mesh = match load_mesh_for_oxide(&in_path) {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(WorkerMsg::Done(Err(format!(
                    "failed to load {}: {e}",
                    in_path.display()
                ))));
                return;
            }
        };
        if tx.send(WorkerMsg::Loaded).is_err() {
            return; // receiver gone
        }
        let mut buf = Vec::new();
        let result = oxide_encode_fn(mesh, &mut buf, cfg)
            .map(|()| buf)
            .map_err(|e| format!("encode failed: {e:?}"));
        let _ = tx.send(WorkerMsg::Done(result));
    });

    // Wait (unbounded) for the load to finish — only the encode is timed.
    match rx.recv() {
        Ok(WorkerMsg::Loaded) => {}
        Ok(WorkerMsg::Done(Err(e))) => panic!("{label}: draco-oxide {e}"),
        Ok(WorkerMsg::Done(Ok(_))) => unreachable!("worker sends Loaded before Done"),
        Err(_) => panic!("{label}: draco-oxide worker died during load"),
    }

    match rx.recv_timeout(timeout) {
        Ok(WorkerMsg::Done(Ok(buf))) => buf,
        Ok(WorkerMsg::Done(Err(e))) => panic!("{label}: draco-oxide {e}"),
        Ok(WorkerMsg::Loaded) => unreachable!("Loaded already consumed"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label}: draco-oxide encode exceeded timeout of {timeout:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label}: draco-oxide worker panicked during encode")
        }
    }
}

/// Load a mesh for draco-oxide to encode. Today only OBJ — glTF input would
/// route through a different loader and is out of scope for the first pass.
fn load_mesh_for_oxide(path: &Path) -> Result<Mesh, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext {
        "obj" => load_obj(path).map_err(|e| format!("{e}")),
        other => Err(format!(
            "unsupported input extension for DracoOxideEncode: .{other}"
        )),
    }
}

/// Read raw vertex positions from an OBJ file, with no draco-oxide-specific
/// processing. The comparison metric below works on point sets, so we don't
/// need the full `Mesh` structure here.
/// Symmetric area-weighted surface L2 (RMS) distance between two triangle
/// meshes — the standard Metro/MeshLab-style surface-to-surface metric.
///
/// For one direction A->B we approximate the area integral
/// `(1/area(A)) * ∫_A d(x, B)² dx`, where `d(x, B)` is the exact distance from
/// point `x` to the closest point on B's *surface*. Each triangle contributes
/// `d(centroid, B)² * area` (midpoint quadrature); the sum divided by A's total
/// area is the one-sided mean-squared distance. The two directions are averaged
/// and the square root taken so the result is a true distance-unit norm.
///
/// Each mesh is indexed once with parry3d's `TriMesh` (an internal QBVH), so
/// every closest-point query is O(log n) — versus the old nearest-*vertex*
/// kd-tree, which both ignored triangle areas and overestimated surface
/// distance (the closest point on a surface is almost never a vertex).
fn symmetric_surface_l2(a: &render::MeshData, b: &render::MeshData) -> f64 {
    let tri_a = build_trimesh(a);
    let tri_b = build_trimesh(b);
    let a_to_b = one_sided_area_weighted_msd(a, &tri_b);
    let b_to_a = one_sided_area_weighted_msd(b, &tri_a);
    (0.5 * (a_to_b + b_to_a)).sqrt()
}

/// Build a parry3d `TriMesh` (with its internal QBVH acceleration structure)
/// from a loaded mesh. Degenerate/duplicate triangles are tolerated.
fn build_trimesh(m: &render::MeshData) -> parry3d::shape::TriMesh {
    let verts: Vec<parry3d::math::Vector> = m.verts.iter().map(|v| vert(*v)).collect();
    parry3d::shape::TriMesh::new(verts, m.tris.clone())
        .expect("TriMesh construction failed (empty or malformed mesh)")
}

/// Convert a loaded `[x, y, z]` position into the glam `Vec3` (`parry3d`'s
/// `Vector`) used for all the geometry math, so the rest of the code reads
/// `.x/.y/.z` and uses `cross`/`length` instead of raw `[n]` indexing.
fn vert(p: [f32; 3]) -> parry3d::math::Vector {
    parry3d::math::Vector::from_array(p)
}

/// One-sided area-weighted mean-squared distance from the surface of `from` to
/// the surface of `target`: `(Σ_T d(centroid_T, target)² · area_T) / Σ_T area_T`.
fn one_sided_area_weighted_msd(from: &render::MeshData, target: &parry3d::shape::TriMesh) -> f64 {
    use parry3d::query::PointQuery;

    let mut weighted_sq = 0.0_f64;
    let mut total_area = 0.0_f64;
    for t in &from.tris {
        let v0 = vert(from.verts[t[0] as usize]);
        let v1 = vert(from.verts[t[1] as usize]);
        let v2 = vert(from.verts[t[2] as usize]);

        let area = triangle_area(v0, v1, v2);
        if area == 0.0 {
            continue; // degenerate triangle: no surface, no contribution
        }

        let centroid = (v0 + v1 + v2) / 3.0;
        // Exact closest point on `target`'s surface (`solid = false`: we always
        // want the boundary distance, never zero for points "inside").
        let proj = target.project_local_point(centroid, false);
        let d_sq = (proj.point - centroid).length_squared() as f64;

        weighted_sq += d_sq * area;
        total_area += area;
    }

    if total_area == 0.0 {
        return 0.0;
    }
    weighted_sq / total_area
}

/// Area of the triangle `(v0, v1, v2)` via half the cross-product magnitude.
fn triangle_area(
    v0: parry3d::math::Vector,
    v1: parry3d::math::Vector,
    v2: parry3d::math::Vector,
) -> f64 {
    0.5 * (v1 - v0).cross(v2 - v0).length() as f64
}

// ---------------------------------------------------------------------------
// draco-oxide under a WASM runtime
// ---------------------------------------------------------------------------

/// Runs `DracoOxideEncode` / `DracoOxideDecode` through the `wasi-codec`
/// module under `wasmtime` instead of in-process, so every profile exercises
/// the codec on a 32-bit target. Enabled by `DRACO_OXIDE_WASM=<path to
/// wasi-codec.wasm>` (build it with `cargo build -p wasi-codec --target
/// wasm32-wasip1 --release`); `WASMTIME` overrides the runtime binary.
///
/// The guest sees the profile's output dir as `/out` and the data dir as
/// `/data`; host paths are translated on the way in.
struct WasmCodec {
    runtime: PathBuf,
    module: PathBuf,
    out_dir: PathBuf,
    data_dir: PathBuf,
}

impl WasmCodec {
    /// Guest mount point of the profile's output dir.
    const GUEST_OUT: &'static str = "/out";
    /// Guest mount point of the shared test data dir.
    const GUEST_DATA: &'static str = "/data";

    /// The opt-in is explicit, so a missing module or runtime is a failure,
    /// never a silent fallback to the native codec.
    fn from_env(name: &str, out_dir: &Path, data_dir: &Path) -> Option<Self> {
        let module = PathBuf::from(std::env::var_os("DRACO_OXIDE_WASM")?);
        assert!(
            module.is_file(),
            "[{name}] DRACO_OXIDE_WASM={} is not a file (build it with \
             `cargo build -p wasi-codec --target wasm32-wasip1 --release`)",
            module.display()
        );
        let runtime = PathBuf::from(std::env::var_os("WASMTIME").unwrap_or("wasmtime".into()));
        Some(Self {
            runtime,
            module,
            out_dir: out_dir.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
        })
    }

    fn encode(
        &self,
        label: &str,
        input: &Path,
        output: &Path,
        cfg: Option<&Path>,
        timeout: Option<Duration>,
    ) {
        let mut cmd = self.command();
        cmd.arg("encode")
            .arg(self.guest_path(label, input))
            .arg(self.guest_path(label, output));
        if let Some(cfg) = cfg {
            cmd.arg(self.guest_path(label, cfg));
        }
        let stdout = self.run(label, cmd);
        let secs: f64 = stdout
            .lines()
            .find_map(|l| l.strip_prefix("encode_secs="))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or_else(|| panic!("{label}: wasi-codec did not report encode_secs"));
        if let Some(timeout) = timeout {
            assert!(
                secs <= timeout.as_secs_f64(),
                "{label}: draco-oxide (wasm) encode took {secs:.3}s, exceeding timeout of {timeout:?}"
            );
        }
    }

    fn decode(&self, label: &str, input: &Path, output: &Path) {
        let mut cmd = self.command();
        cmd.arg("decode")
            .arg(self.guest_path(label, input))
            .arg(self.guest_path(label, output));
        self.run(label, cmd);
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.runtime);
        cmd.arg("run")
            .arg("--dir")
            .arg(format!("{}::{}", self.out_dir.display(), Self::GUEST_OUT))
            .arg("--dir")
            .arg(format!("{}::{}", self.data_dir.display(), Self::GUEST_DATA))
            .arg(&self.module);
        cmd
    }

    fn run(&self, label: &str, mut cmd: Command) -> String {
        let output = cmd.output().unwrap_or_else(|e| {
            panic!(
                "{label}: failed to spawn {} (set WASMTIME=<path> if it is not on PATH): {e}",
                self.runtime.display()
            )
        });
        if !output.status.success() {
            panic!(
                "{label}: wasi-codec failed with {}\n    stdout: {}\n    stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim(),
            );
        }
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn guest_path(&self, label: &str, host: &Path) -> String {
        let (root, guest) = if let Ok(rel) = host.strip_prefix(&self.out_dir) {
            (rel, Self::GUEST_OUT)
        } else if let Ok(rel) = host.strip_prefix(&self.data_dir) {
            (rel, Self::GUEST_DATA)
        } else {
            panic!(
                "{label}: {} is outside the output and data dirs",
                host.display()
            )
        };
        let mut path = guest.to_string();
        for component in root.components() {
            path.push('/');
            path.push_str(&component.as_os_str().to_string_lossy());
        }
        path
    }
}

// ---------------------------------------------------------------------------
// Locating Google Draco binaries
// ---------------------------------------------------------------------------

fn find_google_draco_decoder() -> Option<PathBuf> {
    find_binary("DRACO_DECODER", "draco_decoder")
}

fn find_google_draco_encoder() -> Option<PathBuf> {
    find_binary("DRACO_ENCODER", "draco_encoder")
}

/// Resolution order: explicit env var, then the default path produced by
/// `scripts/build-draco.sh` at the workspace root.
fn find_binary(env_var: &str, default_name: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_var) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    // This crate lives at <workspace>/tests, so the workspace root is the
    // parent of CARGO_MANIFEST_DIR.
    let default = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../third_party/draco/_build")
        .join(default_name);
    default.is_file().then_some(default)
}

#[cfg(test)]
mod surface_l2_tests {
    use super::*;

    /// A unit square in the `z = z0` plane, two triangles.
    fn square(z0: f32) -> render::MeshData {
        render::MeshData {
            verts: vec![
                [0.0, 0.0, z0],
                [1.0, 0.0, z0],
                [1.0, 1.0, z0],
                [0.0, 1.0, z0],
            ],
            tris: vec![[0, 1, 2], [0, 2, 3]],
            normals: None,
            uvs: None,
            colors: None,
        }
    }

    #[test]
    fn self_distance_is_zero() {
        let m = square(0.0);
        assert!(symmetric_surface_l2(&m, &m) < 1e-6);
    }

    #[test]
    fn parallel_planes_equal_offset() {
        // Two coincident-extent squares offset by `dz` along z. Every centroid
        // projects straight onto the other plane, so the surface distance is
        // exactly `dz` everywhere → the RMS norm equals `dz`.
        let dz = 0.25;
        let a = square(0.0);
        let b = square(dz);
        let d = symmetric_surface_l2(&a, &b);
        assert!((d - dz as f64).abs() < 1e-5, "got {d}, expected {dz}");
    }

    #[test]
    fn area_weighting_dominated_by_large_triangle() {
        // `from` has one tiny far triangle and one large near triangle; the
        // area weighting must pull the result toward the large (near) one.
        let target = square(0.0);
        let from = render::MeshData {
            verts: vec![
                // large triangle near the target plane (z = 0.1)
                [0.0, 0.0, 0.1],
                [1.0, 0.0, 0.1],
                [0.0, 1.0, 0.1],
                // tiny triangle far away (z = 10.0)
                [0.0, 0.0, 10.0],
                [0.01, 0.0, 10.0],
                [0.0, 0.01, 10.0],
            ],
            tris: vec![[0, 1, 2], [3, 4, 5]],
            normals: None,
            uvs: None,
            colors: None,
        };
        // Unweighted, the far triangle (dist 10) would dominate; area-weighted,
        // the near 0.1-distance triangle dwarfs it, so the result stays small.
        let d = symmetric_surface_l2(&from, &target);
        assert!(d < 1.0, "area weighting failed: got {d}");
    }
}
