//! Round-trip gate for draco-oxide's own decoder.
//!
//! Encodes with draco-oxide and decodes with draco-oxide, then checks the
//! decoded geometry against the input. The other suites decode with Google's
//! reference `draco_decoder`, so they cover the bitstream but not the decoder;
//! this is the suite that exercises `decode`.
//!
//! Meshes are chosen to cover all three point-assignment paths: seamless (no
//! attribute carries an interior seam), one seamed attribute, and the general
//! multi-seam case.

use std::collections::HashMap;

use draco_oxide::core::attribute::AttributeType;
use draco_oxide::core::mesh::Mesh;
use draco_oxide::core::types::{ConfigType, NdVector, PointIdx, Vector};
use draco_oxide::{
    encode::{self, encode},
    io::obj::load_obj,
};

/// Meshes covering the three `fan_vertices` paths, with the per-vertex position
/// tolerance each may drift by under default quantization.
const CASES: &[(&str, f32)] = &[
    ("data/tetrahedron.obj", 1e-3),
    ("data/cube_quads.obj", 1e-3),
    ("data/open_box.obj", 1e-3),
    ("data/groove_fan.obj", 1e-3),
    ("data/torus.obj", 1e-2),
    ("data/sphere.obj", 1e-2),
    ("data/punctured_sphere.obj", 1e-2),
    ("data/bunny.obj", 1e-2),
];

fn positions(mesh: &Mesh) -> Vec<NdVector<3, f32>> {
    let att = mesh
        .attributes
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .expect("decoded mesh has a position attribute");
    (0..att.len())
        .map(|i| att.get::<NdVector<3, f32>, 3>(PointIdx::from(i)))
        .collect()
}

/// The axis-aligned extent of a point set, used to normalize the tolerance.
fn bbox_diagonal(pts: &[NdVector<3, f32>]) -> f32 {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in pts {
        for c in 0..3 {
            lo[c] = lo[c].min(*p.get(c));
            hi[c] = hi[c].max(*p.get(c));
        }
    }
    ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt()
}

#[test]
fn oxide_decodes_its_own_output() {
    for (obj, tol) in CASES {
        let original = load_obj(obj).unwrap();
        let orig_faces = original.faces.len();

        let mut buf = Vec::new();
        encode(original.clone(), &mut buf, encode::Config::default())
            .unwrap_or_else(|e| panic!("{obj}: encode failed: {e:?}"));

        let decoded = draco_oxide::decode::decode(&buf)
            .unwrap_or_else(|e| panic!("{obj}: decode failed: {e:?}"));

        assert_eq!(
            decoded.faces.len(),
            orig_faces,
            "{obj}: face count changed on round trip"
        );

        // Every decoded position must coincide with some input position. This
        // catches a corrupted point-to-vertex map, which reshuffles values
        // without changing their count.
        let orig = positions(&original);
        let got = positions(&decoded);
        let scale = bbox_diagonal(&orig).max(1e-6);
        let eps = tol * scale;

        assert!(
            !got.is_empty(),
            "{obj}: decoded mesh has no position values"
        );

        // Bucket the input positions on an `eps` grid so each decoded position
        // only has to search the 27 cells around it, keeping the check linear
        // and therefore usable in debug builds.
        let cell = |p: &NdVector<3, f32>| {
            [
                (*p.get(0) / eps).floor() as i64,
                (*p.get(1) / eps).floor() as i64,
                (*p.get(2) / eps).floor() as i64,
            ]
        };
        let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, o) in orig.iter().enumerate() {
            grid.entry(cell(o)).or_default().push(i);
        }

        for (i, g) in got.iter().enumerate() {
            let [cx, cy, cz] = cell(g);
            let mut nearest = f32::MAX;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for &j in grid.get(&[cx + dx, cy + dy, cz + dz]).into_iter().flatten() {
                            let d = (0..3)
                                .map(|c| (*g.get(c) - *orig[j].get(c)).powi(2))
                                .sum::<f32>()
                                .sqrt();
                            nearest = nearest.min(d);
                        }
                    }
                }
            }
            assert!(
                nearest <= eps,
                "{obj}: decoded position {i} is {nearest} from the nearest input \
                 position (tolerance {eps})"
            );
        }
    }
}
