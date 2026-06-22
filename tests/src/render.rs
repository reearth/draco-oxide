//! Minimal deterministic CPU rasterizer for the `Ssim` comparison method.
//!
//! It exists so the harness can compare two meshes *visually* without dragging
//! a GPU or browser toolchain into CI. Rendering is orthographic with two-sided
//! Lambert shading under a headlight, into a grayscale buffer — no PBR, no
//! antialiasing, fully reproducible across machines.
//!
//! The camera framing is computed once from the *reference* mesh and reused for
//! the test mesh (see [`Framing`]), so only genuine shape differences show up in
//! the SSIM score — not an incidental reframing from a tiny bounding-box shift.

use std::path::Path;

use image::{GrayImage, Luma};

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

/// Load an OBJ as a flat vertex list plus triangle indices. Triangulated and
/// single-indexed so the result is independent of how the decoder ordered or
/// duplicated vertices.
pub fn load_obj_mesh(path: &Path) -> Result<(Vec<V3>, Vec<[u32; 3]>), String> {
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
    Ok((verts, tris))
}

/// Render `num_views` grayscale images, rotating the camera around the model's
/// up axis at a fixed elevation. Returns one image per view, in azimuth order.
pub fn render_views(
    verts: &[V3],
    tris: &[[u32; 3]],
    cam: &Framing,
    resolution: u32,
    num_views: usize,
) -> Vec<GrayImage> {
    const ELEVATION: f32 = 0.5; // ~28 degrees above the equator.
    (0..num_views)
        .map(|i| {
            let azimuth = (i as f32) * std::f32::consts::TAU / (num_views as f32);
            render(verts, tris, cam, resolution, azimuth, ELEVATION)
        })
        .collect()
}

/// Render a single orthographic view into a square grayscale image.
fn render(
    verts: &[V3],
    tris: &[[u32; 3]],
    cam: &Framing,
    resolution: u32,
    azimuth: f32,
    elevation: f32,
) -> GrayImage {
    const BACKGROUND: u8 = 30;
    const AMBIENT: f32 = 0.2;
    const DIFFUSE: f32 = 0.8;

    let res = resolution;
    let mut img = GrayImage::from_pixel(res, res, Luma([BACKGROUND]));
    let mut zbuf = vec![f32::NEG_INFINITY; (res * res) as usize];

    // Camera basis. `w` points from the model center toward the eye; the camera
    // also acts as a headlight along `w`, so every view is lit.
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

    for t in tris {
        let v0 = verts[t[0] as usize];
        let v1 = verts[t[1] as usize];
        let v2 = verts[t[2] as usize];

        // Two-sided Lambert via |n·w| so inconsistent winding between the two
        // meshes can't flip a face to black.
        let n = norm(cross(sub(v1, v0), sub(v2, v0)));
        let shade = (AMBIENT + DIFFUSE * dot(n, w).abs()).clamp(0.0, 1.0);
        let color = (shade * 255.0).round() as u8;

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
                    img.put_pixel(px as u32, py as u32, Luma([color]));
                }
            }
        }
    }
    img
}
