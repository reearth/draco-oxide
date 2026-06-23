//! Minimal deterministic CPU rasterizer for the `Ssim` comparison method.
//!
//! It exists so the harness can compare two meshes *visually* without dragging
//! a GPU or browser toolchain into CI. Rendering is orthographic into an RGB
//! buffer — no PBR, no antialiasing, fully reproducible across machines.
//!
//! Fragments are colored according to [`ColorBy`]: either by geometry (flat
//! two-sided Lambert shading under a headlight) or by a non-position attribute
//! (vertex normal / texture coordinate / vertex color), so the same harness can
//! catch regressions in those attributes, not just in shape.
//!
//! The camera framing is computed once from the *reference* mesh and reused for
//! the test mesh (see [`Framing`]), so renders line up and only genuine
//! differences register in the SSIM score.

use std::path::Path;

use image::{Rgb, RgbImage};
use serde::Deserialize;

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        a
    }
}

/// 2D edge function: twice the signed area of triangle (p0, p1, p2). Used both
/// for the triangle's own orientation and for barycentric coverage tests.
fn edge(x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0)
}

/// What to map onto fragment color. `Geometry` shades by surface orientation
/// (so the score reflects shape); the others paint a non-position attribute
/// directly (unlit) so the score reflects that attribute.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub enum ColorBy {
    /// Flat two-sided Lambert shading from the geometric face normal.
    #[default]
    Geometry,
    /// Per-vertex normal mapped to RGB via `n * 0.5 + 0.5`.
    Normal,
    /// Texture coordinate mapped to `(u, v, 0)`, clamped to `[0, 1]`.
    Uv,
    /// Per-vertex color used directly.
    VertexColor,
}

impl ColorBy {
    /// Short tag for filenames / messages.
    pub fn tag(&self) -> &'static str {
        match self {
            ColorBy::Geometry => "geometry",
            ColorBy::Normal => "normal",
            ColorBy::Uv => "uv",
            ColorBy::VertexColor => "vertex_color",
        }
    }

    /// Per-vertex RGB colors in `0..1` for the attribute modes; `None` for
    /// `Geometry`, which is shaded per-face at render time. Errors when the mesh
    /// lacks the requested attribute.
    fn vertex_colors(&self, mesh: &MeshData) -> Result<Option<Vec<V3>>, String> {
        match self {
            ColorBy::Geometry => Ok(None),
            ColorBy::Normal => {
                let n = mesh
                    .normals
                    .as_ref()
                    .ok_or("color_by = Normal but the mesh has no normals")?;
                Ok(Some(
                    n.iter()
                        .map(|v| [v[0] * 0.5 + 0.5, v[1] * 0.5 + 0.5, v[2] * 0.5 + 0.5])
                        .collect(),
                ))
            }
            ColorBy::Uv => {
                let uv = mesh
                    .uvs
                    .as_ref()
                    .ok_or("color_by = Uv but the mesh has no texture coordinates")?;
                Ok(Some(
                    uv.iter()
                        .map(|t| [t[0].clamp(0.0, 1.0), t[1].clamp(0.0, 1.0), 0.0])
                        .collect(),
                ))
            }
            ColorBy::VertexColor => {
                let c = mesh
                    .colors
                    .as_ref()
                    .ok_or("color_by = VertexColor but the mesh has no vertex colors")?;
                Ok(Some(c.clone()))
            }
        }
    }
}

/// A loaded mesh: positions + triangles, plus whatever per-vertex attributes the
/// OBJ carried. Attribute vectors, when `Some`, are index-aligned with `verts`.
pub struct MeshData {
    pub verts: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    pub normals: Option<Vec<V3>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub colors: Option<Vec<V3>>,
}

/// Load an OBJ as a flat vertex list, triangle indices, and per-vertex
/// attributes. Triangulated and single-indexed so the result is independent of
/// how the decoder ordered or duplicated vertices, and so every attribute is
/// aligned to the position index.
pub fn load_obj_mesh(path: &Path) -> Result<MeshData, String> {
    let (models, _materials) = tobj::load_obj(
        path,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("tobj failed: {e}"))?;

    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for m in &models {
        let base = verts.len() as u32;
        for c in m.mesh.positions.chunks_exact(3) {
            verts.push([c[0], c[1], c[2]]);
        }
        for idx in m.mesh.indices.chunks_exact(3) {
            tris.push([base + idx[0], base + idx[1], base + idx[2]]);
        }
    }
    if verts.is_empty() {
        return Err("no vertices found".into());
    }
    let n = verts.len();

    Ok(MeshData {
        verts,
        tris,
        normals: gather_vec3(&models, n, |m| &m.normals),
        uvs: gather_vec2(&models, n, |m| &m.texcoords),
        colors: gather_vec3(&models, n, |m| &m.vertex_color),
    })
}

/// Concatenate a flat `[x,y,z,...]` per-vertex attribute across models into
/// `[[x,y,z]; n]`. Returns `None` if any model is missing the attribute (so we
/// never silently misalign), or if the total count doesn't match `n_verts`.
fn gather_vec3(
    models: &[tobj::Model],
    n_verts: usize,
    sel: impl Fn(&tobj::Mesh) -> &Vec<f32>,
) -> Option<Vec<V3>> {
    let mut out = Vec::with_capacity(n_verts);
    for m in models {
        let a = sel(&m.mesh);
        if a.len() != m.mesh.positions.len() {
            return None;
        }
        for c in a.chunks_exact(3) {
            out.push([c[0], c[1], c[2]]);
        }
    }
    (out.len() == n_verts).then_some(out)
}

/// Like [`gather_vec3`] but for 2-component attributes (texture coordinates).
fn gather_vec2(
    models: &[tobj::Model],
    n_verts: usize,
    sel: impl Fn(&tobj::Mesh) -> &Vec<f32>,
) -> Option<Vec<[f32; 2]>> {
    let mut out = Vec::with_capacity(n_verts);
    for m in models {
        let a = sel(&m.mesh);
        // 2 components per vertex vs 3 for positions.
        if a.len() * 3 != m.mesh.positions.len() * 2 {
            return None;
        }
        for c in a.chunks_exact(2) {
            out.push([c[0], c[1]]);
        }
    }
    (out.len() == n_verts).then_some(out)
}

/// Shared camera framing: the bounding-sphere center and radius of the mesh the
/// camera was fit to. Reusing one framing for both meshes keeps their renders
/// pixel-aligned.
pub struct Framing {
    center: V3,
    radius: f32,
}

impl Framing {
    /// Fit framing to a point set: bounding-box center and the farthest-vertex
    /// radius. A degenerate (zero-radius) mesh falls back to radius 1.
    pub fn fit(verts: &[V3]) -> Framing {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in verts {
            for i in 0..3 {
                min[i] = min[i].min(v[i]);
                max[i] = max[i].max(v[i]);
            }
        }
        let center = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];
        let mut radius = 0.0f32;
        for v in verts {
            radius = radius.max(dot(sub(*v, center), sub(*v, center)).sqrt());
        }
        Framing {
            center,
            radius: if radius > 0.0 { radius } else { 1.0 },
        }
    }
}

/// Render `num_views` images, rotating the camera around the model's up axis at
/// a fixed elevation. Returns one image per view, in azimuth order. Errors if
/// `color_by` needs an attribute the mesh doesn't have.
pub fn render_views(
    mesh: &MeshData,
    cam: &Framing,
    resolution: u32,
    num_views: usize,
    color_by: ColorBy,
) -> Result<Vec<RgbImage>, String> {
    const ELEVATION: f32 = 0.5; // ~28 degrees above the equator.
    let vcolors = color_by.vertex_colors(mesh)?;
    Ok((0..num_views)
        .map(|i| {
            let azimuth = (i as f32) * std::f32::consts::TAU / (num_views as f32);
            render(
                mesh,
                cam,
                resolution,
                azimuth,
                ELEVATION,
                vcolors.as_deref(),
            )
        })
        .collect())
}

/// Render a single orthographic view into a square RGB image. When `vcolors` is
/// `Some`, fragments are the barycentric-interpolated per-vertex color (unlit);
/// when `None`, triangles are flat-shaded by their geometric normal.
fn render(
    mesh: &MeshData,
    cam: &Framing,
    resolution: u32,
    azimuth: f32,
    elevation: f32,
    vcolors: Option<&[V3]>,
) -> RgbImage {
    const BACKGROUND: Rgb<u8> = Rgb([30, 30, 30]);
    const AMBIENT: f32 = 0.2;
    const DIFFUSE: f32 = 0.8;

    let res = resolution;
    let mut img = RgbImage::from_pixel(res, res, BACKGROUND);
    let mut zbuf = vec![f32::NEG_INFINITY; (res * res) as usize];

    // Camera basis. `w` points from the model center toward the eye; the camera
    // also acts as a headlight along `w`, so every geometry view is lit.
    let w = norm([
        elevation.cos() * azimuth.sin(),
        elevation.sin(),
        elevation.cos() * azimuth.cos(),
    ]);
    let up = if w[1].abs() > 0.999 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let right = norm(cross(up, w));
    let true_up = cross(w, right);

    // Fit the bounding sphere into 90% of the frame.
    let scale = (res as f32 * 0.5 * 0.9) / cam.radius;
    let half = res as f32 * 0.5;
    let project = |v: V3| -> (f32, f32, f32) {
        let rel = sub(v, cam.center);
        (
            half + dot(rel, right) * scale,
            half - dot(rel, true_up) * scale,
            dot(rel, w), // depth: larger is closer to the eye.
        )
    };

    for t in &mesh.tris {
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let v0 = mesh.verts[i0];
        let v1 = mesh.verts[i1];
        let v2 = mesh.verts[i2];

        // Geometric face normal. We cull back faces (`n·w <= 0`, since `w`
        // points toward the eye) so that a *missing* front face reveals the
        // background instead of being filled in by the far side of the model —
        // otherwise a hole, exactly the kind of regression this test exists to
        // catch, would be hidden behind the model's interior and leave the SSIM
        // score nearly unchanged. The encode/decode round-trip preserves face
        // winding, so the reference and decoded meshes cull consistently.
        let n = norm(cross(sub(v1, v0), sub(v2, v0)));
        let facing = dot(n, w);
        if facing <= 0.0 {
            continue; // back-facing or degenerate.
        }

        // Per-vertex colors in 0..1: either the chosen attribute, or a flat
        // Lambert shade from the (front-facing) normal, replicated across the
        // vertices.
        let (c0, c1, c2) = match vcolors {
            Some(vc) => (vc[i0], vc[i1], vc[i2]),
            None => {
                let s = (AMBIENT + DIFFUSE * facing).clamp(0.0, 1.0);
                let g = [s, s, s];
                (g, g, g)
            }
        };

        let (ax, ay, az) = project(v0);
        let (bx, by, bz) = project(v1);
        let (cx, cy, cz) = project(v2);

        let area = edge(ax, ay, bx, by, cx, cy);
        if area.abs() < 1e-6 {
            continue; // degenerate / edge-on triangle.
        }

        let min_x = ax.min(bx).min(cx).floor().max(0.0) as i32;
        let max_x = ax.max(bx).max(cx).ceil().min((res - 1) as f32) as i32;
        let min_y = ay.min(by).min(cy).floor().max(0.0) as i32;
        let max_y = ay.max(by).max(cy).ceil().min((res - 1) as f32) as i32;

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let fy = py as f32 + 0.5;
                // Barycentric weights (l0 for v0, etc.); inside iff all >= 0.
                let l0 = edge(bx, by, cx, cy, fx, fy) / area;
                let l1 = edge(cx, cy, ax, ay, fx, fy) / area;
                let l2 = edge(ax, ay, bx, by, fx, fy) / area;
                if l0 < 0.0 || l1 < 0.0 || l2 < 0.0 {
                    continue;
                }
                // Depth is linear in screen space under orthographic projection.
                let depth = l0 * az + l1 * bz + l2 * cz;
                let i = (py as u32 * res + px as u32) as usize;
                if depth > zbuf[i] {
                    zbuf[i] = depth;
                    let chan = |k: usize| {
                        ((l0 * c0[k] + l1 * c1[k] + l2 * c2[k]).clamp(0.0, 1.0) * 255.0).round()
                            as u8
                    };
                    img.put_pixel(px as u32, py as u32, Rgb([chan(0), chan(1), chan(2)]));
                }
            }
        }
    }
    img
}
