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

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{self as oxide_encode, encode as oxide_encode_fn};
use draco_oxide::io::obj::load_obj;

mod render;

// ---------------------------------------------------------------------------
// Profile schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub operations: Vec<Operation>,
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
    /// Encode a mesh with draco-oxide. No config field today — the encoder
    /// doesn't expose tunable knobs yet; default `encode::Config` is used.
    DracoOxideEncode { input: String, output: String },
    /// Decode a `.drc` with draco-oxide. Currently stubbed in the library
    /// itself, so this op deliberately errors with a clear message rather
    /// than silently producing garbage.
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
    /// validated here — its only validation is "Google's decoder accepts it",
    /// which is covered by [`Operation::DracoDecode`].
    Validation { input: String, fmt: FormatName },
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
    /// Symmetric nearest-neighbor RMS between the two point sets (robust to
    /// vertex reordering). Asserts the value is `<= max`. Both inputs must
    /// currently be OBJ files.
    L2Norm { max: f64 },
    /// Rendered-view structural similarity. Renders both inputs from several
    /// viewpoints with a small CPU rasterizer (see [`render`]) and scores SSIM
    /// per view. Asserts the *worst* view's score is `>= min` (1.0 = identical).
    /// Both inputs must be OBJ files. The rendered PNGs are written to the
    /// profile's output dir for debugging.
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
            Operation::DracoOxideEncode { input, output } => {
                let in_path = resolve_input(input);
                let mesh = load_mesh_for_oxide(&in_path).unwrap_or_else(|e| {
                    panic!("{label}: failed to load {}: {e}", in_path.display())
                });
                let mut buf = Vec::new();
                oxide_encode_fn(mesh, &mut buf, oxide_encode::Config::default())
                    .unwrap_or_else(|e| panic!("{label}: draco-oxide encode failed: {e:?}"));
                let out_path = resolve_output(output);
                std::fs::write(&out_path, &buf).unwrap_or_else(|e| {
                    panic!("{label}: writing {} failed: {e}", out_path.display())
                });
            }
            Operation::DracoOxideDecode { .. } => {
                panic!(
                    "{label}: DracoOxideDecode is not implemented yet — draco-oxide's decoder \
                     is still stubbed (see draco-oxide/src/decode/mod.rs)."
                );
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
                            let pos1 = load_obj_positions(&p1).unwrap_or_else(|e| {
                                panic!("{label} L2Norm: failed to load {}: {e}", p1.display())
                            });
                            let pos2 = load_obj_positions(&p2).unwrap_or_else(|e| {
                                panic!("{label} L2Norm: failed to load {}: {e}", p2.display())
                            });
                            let dist = symmetric_nearest_neighbor_rms(&pos1, &pos2);
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
                        } => {
                            let res = resolution.unwrap_or(512);
                            let n_views = views.unwrap_or(4).max(1);
                            let (v1, t1) = render::load_obj_mesh(&p1).unwrap_or_else(|e| {
                                panic!("{label} Ssim: failed to load {}: {e}", p1.display())
                            });
                            let (v2, t2) = render::load_obj_mesh(&p2).unwrap_or_else(|e| {
                                panic!("{label} Ssim: failed to load {}: {e}", p2.display())
                            });
                            // Frame both meshes with input1's framing so only
                            // genuine shape differences register.
                            let cam = render::Framing::fit(&v1);
                            let imgs1 = render::render_views(&v1, &t1, &cam, res, n_views);
                            let imgs2 = render::render_views(&v2, &t2, &cam, res, n_views);

                            let s1 = p1.file_stem().and_then(|s| s.to_str()).unwrap_or("ref");
                            let s2 = p2.file_stem().and_then(|s| s.to_str()).unwrap_or("test");

                            let mut worst = f64::INFINITY;
                            let mut worst_view = 0;
                            for (i, (a, b)) in imgs1.iter().zip(&imgs2).enumerate() {
                                let sim = image_compare::gray_similarity_structure(
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
                                    let pa = out_dir.join(format!("ssim_{s1}_view{i}.png"));
                                    let pb = out_dir.join(format!("ssim_{s2}_view{i}.png"));
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
                                "{label} Ssim: worst score {worst} at view {worst_view} \
                                 (min {min}, {n_views} views, {res}px)"
                            );
                            assert!(
                                worst >= *min,
                                "{label} Ssim: worst score {worst} < min {min} (view {worst_view})\n    \
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
        Operation::Comparison { .. } => "Comparison",
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
fn load_obj_positions(path: &Path) -> Result<Vec<[f32; 3]>, String> {
    let (models, _materials) = tobj::load_obj(
        path,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("tobj failed: {e}"))?;
    let mut out = Vec::new();
    for m in &models {
        for chunk in m.mesh.positions.chunks_exact(3) {
            out.push([chunk[0], chunk[1], chunk[2]]);
        }
    }
    if out.is_empty() {
        return Err("no vertex positions found".into());
    }
    Ok(out)
}

/// Symmetric nearest-neighbor RMS between two point sets, computed with a
/// kd-tree on each side. This is robust to vertex reordering by the decoder
/// (Google Draco may permute vertices), unlike index-aligned comparisons.
fn symmetric_nearest_neighbor_rms(a: &[[f32; 3]], b: &[[f32; 3]]) -> f64 {
    use kiddo::float::{distance::SquaredEuclidean, kdtree::KdTree};

    fn build(points: &[[f32; 3]]) -> KdTree<f32, u64, 3, 32, u32> {
        let mut tree = KdTree::new();
        for (i, p) in points.iter().enumerate() {
            tree.add(p, i as u64);
        }
        tree
    }

    fn one_sided_rms(query: &[[f32; 3]], tree: &KdTree<f32, u64, 3, 32, u32>) -> f64 {
        let n = query.len() as f64;
        let mut sum_sq = 0.0_f64;
        for p in query {
            let nn = tree.nearest_one::<SquaredEuclidean>(p);
            sum_sq += nn.distance as f64;
        }
        (sum_sq / n).sqrt()
    }

    let tree_a = build(a);
    let tree_b = build(b);
    one_sided_rms(a, &tree_b).max(one_sided_rms(b, &tree_a))
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
