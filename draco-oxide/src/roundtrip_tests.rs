//! Cross-crate round-trip / integration tests relocated here from `draco-oxide-core`
//! and `draco-oxide-decoder` during the crate split. They exercise core/decoder
//! functionality but need the encoder (and `io`/decoder), which only the
//! `draco-oxide` crate sees together.

mod attribute_ds {
    use crate::encode::ds::{build_attribute_ds, build_global_ds};
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::mesh::ds::GenericCornerTable;
    use draco_oxide_core::types::{CornerIdx, VertexIdx};

    #[test]
    fn test_no_att_seam() {
        let mesh = load_obj("../tests/data/sphere.obj").unwrap();
        let faces = mesh.faces;
        let mut attributes = mesh.attributes;

        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
        let adss = build_attribute_ds(&ds, &pos_corner_table, attributes);

        let pos_ds = adss
            .iter()
            .find(|a| a.att_data().get_attribute_type() == AttributeType::Position)
            .unwrap();
        let normal_ds = adss
            .iter()
            .find(|a| a.att_data().get_attribute_type() == AttributeType::Normal)
            .unwrap();

        // The sphere's normals carry no attribute seams, so the normal
        // connectivity must coincide with the position connectivity.
        assert_eq!(normal_ds.num_vertices(), pos_ds.num_vertices());

        for c in 0..ds.num_corners() {
            let c = CornerIdx::from(c);
            // No corner is opposite a seam edge.
            assert!(!normal_ds.corner_table().is_corner_opposite_to_seam_edge(c));
            // Opposite corners are identical to the position corner table.
            assert_eq!(
                normal_ds.corner_table().opposite(c),
                pos_corner_table.opposite(c)
            );
            // Vertices are identical to the position vertices.
            assert_eq!(normal_ds.vertex_idx(c), pos_ds.vertex_idx(c));
        }
    }

    #[test]
    fn test_att_seam() {
        let mesh = load_obj("../tests/data/tetrahedron.obj").unwrap();
        let faces = mesh.faces;
        let mut attributes = mesh.attributes;

        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
        let adss = build_attribute_ds(&ds, &pos_corner_table, attributes);

        let pos_ds = adss
            .iter()
            .find(|a| a.att_data().get_attribute_type() == AttributeType::Position)
            .unwrap();
        let tex_ds = adss
            .iter()
            .find(|a| a.att_data().get_attribute_type() == AttributeType::TextureCoordinate)
            .unwrap();

        // The texture seams split two of the position vertices, so the texture
        // connectivity has two additional vertices.
        assert_eq!(tex_ds.num_vertices(), pos_ds.num_vertices() + 2);

        // The corners opposite a texture seam edge, and only those.
        let seam_edge_corners = [3usize, 5, 6, 7, 9, 11];
        for c in 0..ds.num_corners() {
            let is_seam = seam_edge_corners.contains(&c);
            assert_eq!(
                tex_ds
                    .corner_table()
                    .is_corner_opposite_to_seam_edge(CornerIdx::from(c)),
                is_seam,
                "corner {c} seam-status mismatch",
            );
        }

        // Every vertex's left-most corner must map back to that vertex.
        for v in 0..tex_ds.num_vertices() {
            let v = VertexIdx::from(v);
            let left_most_corner = tex_ds.left_most_corner(v);
            assert_eq!(
                tex_ds.vertex_idx(left_most_corner),
                v,
                "left-most corner {left_most_corner:?} does not belong to vertex {v:?}",
            );
        }
    }
}

mod sequence {
    use crate::encode::connectivity::encode_connectivity;
    use crate::encode::ds::{build_attribute_ds, build_global_ds};
    use crate::encode::Config;
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::codec::attribute::sequence::Traverser;
    use draco_oxide_core::types::ConfigType;

    /// One captured traversal step: (attr_idx, len, fnv1a_digest).
    type AttrDigest = (usize, usize, u64);

    #[test]
    fn test_traverser() {
        let mesh = load_obj("../tests/data/tetrahedron.obj").unwrap();
        let faces = mesh.faces;
        let mut attributes = mesh.attributes;

        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
        let mut adss = build_attribute_ds(&ds, &pos_corner_table, attributes);

        let corners = encode_connectivity(&mut adss, &mut Vec::new(), &Config::default()).unwrap();

        // The point-index sequence produced by traversing a single attribute.
        let sequence_of = |ty: AttributeType| -> Vec<usize> {
            let ads = adss
                .iter()
                .find(|a| a.att_data().get_attribute_type() == ty)
                .unwrap();
            Traverser::new(ads, corners.clone())
                .compute_seqeunce()
                .iter()
                .map(|c| usize::from(ads.global_ds().point_idx(*c)))
                .collect()
        };

        assert_eq!(sequence_of(AttributeType::Position), vec![3, 1, 0, 2]);
        assert_eq!(sequence_of(AttributeType::Normal), vec![3, 1, 0, 2]);
        assert_eq!(
            sequence_of(AttributeType::TextureCoordinate),
            vec![3, 1, 0, 2, 5, 4]
        );
    }

    /// FNV-1a over the little-endian bytes of the point-index sequence.
    /// Deterministic and toolchain-independent, unlike DefaultHasher.
    fn digest(seq: &[usize]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &v in seq {
            for b in (v as u64).to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// Computes (attr_idx, sequence_len, digest) for every attribute data
    /// structure of `mesh`, indexed by position in the attribute list (0 is the
    /// position attribute). The digest captures the exact `Vec<CornerIdx>`
    /// traversal order via point indices — this is the shared encoder/decoder
    /// symmetry that must stay byte-identical.
    fn sequence_fingerprints(path: &str) -> Vec<(usize, usize, u64)> {
        let mesh = load_obj(path).unwrap();
        let faces = mesh.faces;
        let mut attributes = mesh.attributes;

        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
        let mut adss = build_attribute_ds(&ds, &pos_corner_table, attributes);

        let corners = encode_connectivity(&mut adss, &mut Vec::new(), &Config::default()).unwrap();

        adss.iter()
            .enumerate()
            .map(|(attr_idx, ads)| {
                let seq: Vec<usize> = Traverser::new(ads, corners.clone())
                    .compute_seqeunce()
                    .iter()
                    .map(|c| usize::from(ads.global_ds().point_idx(*c)))
                    .collect();
                (attr_idx, seq.len(), digest(&seq))
            })
            .collect()
    }

    /// Byte-identical oracle for `compute_sequence`. The expected fingerprints
    /// were captured from the pre-optimization implementation; any change that
    /// alters the traversal order on these meshes (boundaries, handles) trips
    /// this test. torus.obj carries topological handles, which is exactly the
    /// case the handle-detection scan-and-remove blocks exist to handle.
    #[test]
    fn oracle_compute_sequence() {
        let cases: &[(&str, &[AttrDigest])] = &[
            ("../tests/data/tetrahedron.obj", EXPECT_TETRAHEDRON),
            ("../tests/data/sphere.obj", EXPECT_SPHERE),
            (
                "../tests/data/punctured_sphere.obj",
                EXPECT_PUNCTURED_SPHERE,
            ),
            ("../tests/data/torus.obj", EXPECT_TORUS),
            ("../tests/data/bunny.obj", EXPECT_BUNNY),
        ];

        let dump = std::env::var("DUMP_FINGERPRINTS").is_ok();
        for (path, expected) in cases {
            let got = sequence_fingerprints(path);
            if dump {
                eprintln!("{path} => {got:?}");
                continue;
            }
            assert_eq!(
                &got[..],
                *expected,
                "compute_sequence output changed for {path}"
            );
        }
    }

    /// The lazy iterator must yield exactly the sequence `compute_seqeunce`
    /// materializes, for every attribute connectivity (boundaries, seams,
    /// handles included).
    #[test]
    fn iterator_matches_drive() {
        let paths = [
            "../tests/data/tetrahedron.obj",
            "../tests/data/sphere.obj",
            "../tests/data/punctured_sphere.obj",
            "../tests/data/torus.obj",
            "../tests/data/bunny.obj",
        ];
        for path in paths {
            let mesh = load_obj(path).unwrap();
            let faces = mesh.faces;
            let mut attributes = mesh.attributes;

            let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
            let mut adss = build_attribute_ds(&ds, &pos_corner_table, attributes);

            let corners =
                encode_connectivity(&mut adss, &mut Vec::new(), &Config::default()).unwrap();

            for (attr_idx, ads) in adss.iter().enumerate() {
                let lazy: Vec<_> = Traverser::new(ads, corners.clone()).collect();
                let driven = Traverser::new(ads, corners.clone()).compute_seqeunce();
                assert_eq!(
                    lazy, driven,
                    "iterator order diverged: {path} attr {attr_idx}"
                );
            }
        }
    }

    // Captured from the pre-optimization implementation. Format: (attr_idx, len, fnv1a_digest).
    const EXPECT_TETRAHEDRON: &[AttrDigest] = &[
        (0, 4, 18054049684469353541),
        (1, 4, 18054049684469353541),
        (2, 6, 3159456026337658052),
    ];
    const EXPECT_SPHERE: &[AttrDigest] = &[
        (0, 114, 17737425019064467876),
        (1, 114, 17737425019064467876),
    ];
    const EXPECT_PUNCTURED_SPHERE: &[AttrDigest] = &[
        (0, 114, 17132826066695074116),
        (1, 114, 17132826066695074116),
    ];
    const EXPECT_TORUS: &[AttrDigest] = &[(0, 2051, 930682351741064974)];
    const EXPECT_BUNNY: &[AttrDigest] = &[
        (0, 34834, 3080192193140594432),
        (1, 34834, 3080192193140594432),
    ];
}

// Full symbol-coding round trip: the encoder's `encode_symbols` writer against the
// decoder's `decode_symbols` reader. Only DirectCoded is covered; LengthCoded
// decode lands with Google interop (milestone B).
#[cfg(feature = "decoder")]
mod symbol_coding {
    use crate::encode::entropy::symbol_coding;
    use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
    use draco_oxide_decoder::entropy::decode_symbols;
    use draco_oxide_decoder::Err;

    fn round_trip(num_values: usize, num_components: usize) -> Result<(), Err> {
        let symbols = (0..num_values * num_components)
            .map(|x| ((x * x * x) % 23) as u64)
            .collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(),
            num_components,
            SymbolEncodingMethod::DirectCoded,
            &mut buffer,
        )
        .unwrap();

        let mut reader = draco_oxide_core::bit_coder::Reader::new(&buffer);
        let decoded = decode_symbols(&mut reader, num_values, num_components)?;
        assert!(
            reader.is_empty(),
            "reader should be empty after decoding all symbols"
        );
        assert_eq!(decoded, symbols);
        Ok(())
    }

    #[test]
    fn direct_coded_single_component() -> Result<(), Err> {
        round_trip(100, 1)
    }

    #[test]
    fn direct_coded_multi_component() -> Result<(), Err> {
        round_trip(100, 3)
    }
}

// Connectivity round trip: encode each `tests/data` mesh with the default config,
// decode the header + edgebreaker connectivity, and check the decoded position
// face lattice matches the encoder's, up to vertex relabeling. Small meshes use
// core's Laplacian spectrum check (O(E^2) memory, so only for small E); larger
// meshes use a cheaper structural summary (counts + degree sequence).
#[cfg(feature = "decoder")]
mod connectivity {
    use crate::encode::ds::{build_attribute_ds, build_global_ds};
    use crate::encode::{encode, Config};
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::codec::connectivity::eq::weak_eq_by_laplacian;
    use draco_oxide_core::types::{ConfigType, CornerIdx};
    use draco_oxide_decoder::connectivity::decode_connectivity;
    use draco_oxide_decoder::header::decode_header;
    use std::collections::HashMap;

    /// Relabels the vertices of `faces` densely so referenced ids occupy `0..k`.
    fn densify(mut faces: Vec<[usize; 3]>) -> Vec<[usize; 3]> {
        let mut remap: HashMap<usize, usize> = HashMap::new();
        for face in &mut faces {
            for v in face.iter_mut() {
                let next = remap.len();
                *v = *remap.entry(*v).or_insert(next);
            }
        }
        faces
    }

    /// The encoder's own position-vertex face lattice for `path`, built the same way
    /// `encode` builds it, then densified.
    fn encoder_position_faces(path: &str) -> Vec<[usize; 3]> {
        let mesh = load_obj(path).unwrap();
        let faces = mesh.faces;
        let mut attributes = mesh.attributes;
        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
        let adss = build_attribute_ds(&ds, &pos_corner_table, attributes);
        let pos = adss
            .iter()
            .find(|a| a.att_data().get_attribute_type() == AttributeType::Position)
            .unwrap();
        let out = (0..ds.num_faces())
            .map(|f| {
                [
                    usize::from(pos.vertex_idx(CornerIdx::from(3 * f))),
                    usize::from(pos.vertex_idx(CornerIdx::from(3 * f + 1))),
                    usize::from(pos.vertex_idx(CornerIdx::from(3 * f + 2))),
                ]
            })
            .collect();
        densify(out)
    }

    /// Decodes the header + connectivity for `path` and returns the decoded position
    /// faces (already densified).
    fn decoded_position_faces(path: &str) -> Vec<[usize; 3]> {
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode(mesh, &mut buffer, Config::default()).unwrap();
        let mut reader = draco_oxide_core::bit_coder::Reader::new(&buffer);
        let header = decode_header(&mut reader).unwrap();
        let conn = decode_connectivity(&mut reader, header.encoder_method).unwrap();
        conn.edgebreaker().unwrap().position_faces().0
    }

    /// (num_vertices, num_faces, num_edges, sorted vertex-degree sequence).
    fn summary(faces: &[[usize; 3]]) -> (usize, usize, usize, Vec<usize>) {
        let num_verts = faces
            .iter()
            .flatten()
            .copied()
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        let mut edges = std::collections::HashSet::new();
        for f in faces {
            for i in 0..3 {
                let (a, b) = (f[i], f[(i + 1) % 3]);
                edges.insert((a.min(b), a.max(b)));
            }
        }
        let mut degree = vec![0usize; num_verts];
        for &(a, b) in &edges {
            degree[a] += 1;
            degree[b] += 1;
        }
        degree.sort_unstable();
        (num_verts, faces.len(), edges.len(), degree)
    }

    fn assert_laplacian_equivalent(path: &str) {
        let decoded = decoded_position_faces(path);
        let truth = encoder_position_faces(path);
        assert_eq!(
            weak_eq_by_laplacian(&decoded, &truth),
            Some(true),
            "connectivity mismatch for {path}"
        );
    }

    fn assert_structurally_equivalent(path: &str) {
        let decoded = decoded_position_faces(path);
        let truth = encoder_position_faces(path);
        assert_eq!(
            summary(&decoded),
            summary(&truth),
            "connectivity mismatch for {path}"
        );
    }

    #[test]
    fn tetrahedron() {
        assert_laplacian_equivalent("../tests/data/tetrahedron.obj");
    }

    #[test]
    fn groove_fan() {
        assert_laplacian_equivalent("../tests/data/groove_fan.obj");
    }

    #[test]
    fn cube_flat() {
        assert_laplacian_equivalent("../tests/data/cube_flat.obj");
    }

    #[test]
    fn cube_quads() {
        assert_laplacian_equivalent("../tests/data/cube_quads.obj");
    }

    #[test]
    fn open_box() {
        assert_laplacian_equivalent("../tests/data/open_box.obj");
    }

    #[test]
    fn sphere() {
        assert_laplacian_equivalent("../tests/data/sphere.obj");
    }

    #[test]
    fn punctured_sphere() {
        assert_laplacian_equivalent("../tests/data/punctured_sphere.obj");
    }

    #[test]
    fn torus() {
        // 4095 faces: too large for the O(E^2) Laplacian, use the structural check.
        assert_structurally_equivalent("../tests/data/torus.obj");
    }

    #[test]
    fn bunny() {
        assert_structurally_equivalent("../tests/data/bunny.obj");
    }
}

// Shared helpers for the attribute round-trip tests: the encoder-side quantized
// values (the portabilization math replayed on the input mesh) and the
// relabeling-invariant face-multiset comparison.
#[cfg(feature = "decoder")]
mod attribute_roundtrip_util {
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::{Attribute, AttributeType};
    use draco_oxide_core::codec::attribute::geom::{float_vector_to_oct, oct_center};
    use draco_oxide_core::mesh::Mesh;
    use draco_oxide_core::types::{NdVector, PointIdx, Vector};

    /// Per-face, per-corner concatenation of all attributes' integer values.
    pub type CornerTuples = Vec<[Vec<i32>; 3]>;

    /// The default quantization bits per attribute type (mirrors
    /// `portabilization::Config::default_for`).
    pub fn default_bits(ty: AttributeType) -> u8 {
        match ty {
            AttributeType::Normal => 8,
            AttributeType::TextureCoordinate => 10,
            _ => 11,
        }
    }

    /// The encoder's quantization bounds for `att`: per-component minima and the
    /// largest per-component extent, with the min/max scan seeded at zero
    /// exactly like the encoder's.
    pub fn quant_bounds<const N: usize>(att: &Attribute) -> (Vec<f32>, f32)
    where
        NdVector<N, f32>: Vector<N, Component = f32>,
    {
        let mut min = [0f32; N];
        let mut max = [0f32; N];
        for i in 0..att.num_unique_values() {
            let v: NdVector<N, f32> = att.get_unique_val(i.into());
            for j in 0..N {
                min[j] = min[j].min(*v.get(j));
                max[j] = max[j].max(*v.get(j));
            }
        }
        let delta_max = (0..N).map(|j| max[j] - min[j]).fold(0.0f32, f32::max);
        (min.to_vec(), delta_max)
    }

    /// Replays the encoder's coordinate-wise quantization on all points of
    /// `att` (min/max seeded at zero, f32 math, truncating round-half-up).
    fn quantize_coordinate_wise<const N: usize>(att: &Attribute, bits: u8) -> Vec<Vec<i32>>
    where
        NdVector<N, f32>: Vector<N, Component = f32>,
    {
        let (min, delta_max) = quant_bounds::<N>(att);
        let scale = ((1u64 << bits) - 1) as f32;

        (0..att.len())
            .map(|p| {
                let v: NdVector<N, f32> = att.get(PointIdx::from(p));
                (0..N)
                    .map(|j| {
                        let diff = *v.get(j) - min[j];
                        let normalized = if delta_max == 0.0 {
                            diff
                        } else {
                            diff / delta_max
                        };
                        (normalized * scale + 0.5) as i64 as i32
                    })
                    .collect()
            })
            .collect()
    }

    /// Replays the encoder's octahedral quantization on all points of `att`.
    fn quantize_octahedral(att: &Attribute, bits: u8) -> Vec<Vec<i32>> {
        let center = oct_center(bits);
        (0..att.len())
            .map(|p| {
                let v: NdVector<3, f32> = att.get(PointIdx::from(p));
                let q = float_vector_to_oct(v, center);
                vec![*q.get(0), *q.get(1)]
            })
            .collect()
    }

    /// The expected quantized integers for every point of `att` under the
    /// default encoder configuration.
    pub fn expected_quantized(att: &Attribute) -> Vec<Vec<i32>> {
        let bits = default_bits(att.get_attribute_type());
        match att.get_attribute_type() {
            AttributeType::Normal => quantize_octahedral(att, bits),
            _ => match att.get_num_components() {
                2 => quantize_coordinate_wise::<2>(att, bits),
                3 => quantize_coordinate_wise::<3>(att, bits),
                n => panic!("unsupported component count {n} in test data"),
            },
        }
    }

    /// Octahedral coordinates on the octahedron-square boundary have several
    /// bit representations of the same normal (the four square corners all
    /// encode the -x pole, and the fold duplicates edge points); the diamond
    /// inversion canonicalizes them many-to-one, so decoded bits can be a
    /// different representative of the same normal. Normals are therefore
    /// compared through the exact inverse octahedral mapping: equal bits of the
    /// reconstructed unit vector, which is injective modulo exactly that
    /// boundary equivalence.
    fn canonical_normal(q: &[i32]) -> Vec<i32> {
        use draco_oxide_core::codec::attribute::geom::octahedral_inverse_transform;
        let scale = ((1u64 << (default_bits(AttributeType::Normal) - 1)) - 1) as f32;
        let oct = NdVector::<2, f32>::from([q[0] as f32 / scale - 1.0, q[1] as f32 / scale - 1.0]);
        // Safety: the output type is three dimensional.
        let n: NdVector<3, f32> = unsafe { octahedral_inverse_transform(oct) };
        let bits = |x: f32| if x == 0.0 { 0 } else { x.to_bits() as i32 };
        vec![bits(*n.get(0)), bits(*n.get(1)), bits(*n.get(2))]
    }

    /// The comparison form of one attribute value: quantized integers verbatim,
    /// except normals which go through `canonical_normal`.
    pub fn comparison_form(ty: AttributeType, ints: Vec<i32>) -> Vec<i32> {
        if ty == AttributeType::Normal {
            canonical_normal(&ints)
        } else {
            ints
        }
    }

    /// The integer values of `att` at point `p` (the decoded portable side).
    pub fn attribute_ints(att: &Attribute, p: PointIdx) -> Vec<i32> {
        match att.get_num_components() {
            1 => {
                let v: NdVector<1, i32> = att.get(p);
                vec![*v.get(0)]
            }
            2 => {
                let v: NdVector<2, i32> = att.get(p);
                vec![*v.get(0), *v.get(1)]
            }
            3 => {
                let v: NdVector<3, i32> = att.get(p);
                vec![*v.get(0), *v.get(1), *v.get(2)]
            }
            n => panic!("unsupported component count {n}"),
        }
    }

    /// The float values of `att` at point `p` (the dequantized side).
    pub fn attribute_floats(att: &Attribute, p: PointIdx) -> Vec<f32> {
        match att.get_num_components() {
            2 => {
                let v: NdVector<2, f32> = att.get(p);
                vec![*v.get(0), *v.get(1)]
            }
            3 => {
                let v: NdVector<3, f32> = att.get(p);
                vec![*v.get(0), *v.get(1), *v.get(2)]
            }
            n => panic!("unsupported component count {n}"),
        }
    }

    /// The expected corner tuples of the input mesh at `path`, quantized the way
    /// the default encoder configuration quantizes them.
    pub fn expected_corner_tuples(path: &str) -> CornerTuples {
        let mesh = load_obj(path).unwrap();
        let per_att: Vec<(AttributeType, Vec<Vec<i32>>)> = mesh
            .attributes
            .iter()
            .map(|att| {
                let ty = att.get_attribute_type();
                let vals = expected_quantized(att)
                    .into_iter()
                    .map(|v| comparison_form(ty, v))
                    .collect();
                (ty, vals)
            })
            .collect();
        mesh.faces
            .iter()
            .map(|face| {
                let corner = |p: PointIdx| -> Vec<i32> {
                    per_att
                        .iter()
                        .flat_map(|(_, vals)| vals[usize::from(p)].iter().copied())
                        .collect()
                };
                [corner(face[0]), corner(face[1]), corner(face[2])]
            })
            .collect()
    }

    /// The decoded corner tuples of `mesh` (portable integer attributes).
    pub fn decoded_corner_tuples(mesh: &Mesh) -> CornerTuples {
        mesh.faces
            .iter()
            .map(|face| {
                let corner = |p: PointIdx| -> Vec<i32> {
                    mesh.attributes
                        .iter()
                        .flat_map(|att| {
                            comparison_form(att.get_attribute_type(), attribute_ints(att, p))
                        })
                        .collect()
                };
                [corner(face[0]), corner(face[1]), corner(face[2])]
            })
            .collect()
    }

    /// Canonicalizes each face to its lexicographically smallest rotation
    /// (faces are oriented, so rotations are the only relabeling freedom) and
    /// sorts the face list, making the comparison face-order independent.
    pub fn canonicalize(mut tuples: CornerTuples) -> CornerTuples {
        for face in tuples.iter_mut() {
            let min_rot = (0..3)
                .min_by_key(|&r| [&face[r], &face[(r + 1) % 3], &face[(r + 2) % 3]])
                .unwrap();
            face.rotate_left(min_rot);
        }
        tuples.sort();
        tuples
    }
}

// Full-reconstruction round trip (milestone 4, `dequantize`): `decode()` returns
// original-format floats. The decoded values must equal the input values
// quantize-dequantized with the encoder's parameters, bit-for-bit (the decoder's
// dequantization is replicated here), compared as per-corner multisets; and the
// decoded positions must sit within quantization tolerance of the input surface.
#[cfg(feature = "decoder")]
mod dequantized {
    use super::attribute_roundtrip_util::{
        attribute_floats, default_bits, expected_quantized, quant_bounds,
    };
    use crate::encode::{encode, Config, NormalEncoding};
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::{Attribute, AttributeType};
    use draco_oxide_core::codec::attribute::geom::octahedral_inverse_transform;
    use draco_oxide_core::types::{ConfigType, NdVector, PointIdx, Vector};
    use draco_oxide_decoder::decode;

    /// Floats as comparable bits, with negative zero normalized.
    fn bit_form(vals: &[f32]) -> Vec<i32> {
        vals.iter()
            .map(|&x| if x == 0.0 { 0 } else { x.to_bits() as i32 })
            .collect()
    }

    /// The expected reconstructed floats for every point of `att`: the encoder's
    /// quantization replayed, then the decoder's dequantization replayed.
    fn expected_floats(att: &Attribute) -> Vec<Vec<f32>> {
        let bits = default_bits(att.get_attribute_type());
        let quantized = expected_quantized(att);
        match att.get_attribute_type() {
            AttributeType::Normal => {
                let scale = ((1u64 << (bits - 1)) - 1) as f32;
                quantized
                    .into_iter()
                    .map(|q| {
                        let oct = NdVector::<2, f32>::from([
                            q[0] as f32 / scale - 1.0,
                            q[1] as f32 / scale - 1.0,
                        ]);
                        // Safety: the output type is three dimensional.
                        let n: NdVector<3, f32> = unsafe { octahedral_inverse_transform(oct) };
                        vec![*n.get(0), *n.get(1), *n.get(2)]
                    })
                    .collect()
            }
            _ => {
                let (min, delta_max) = match att.get_num_components() {
                    2 => quant_bounds::<2>(att),
                    3 => quant_bounds::<3>(att),
                    n => panic!("unsupported component count {n} in test data"),
                };
                let max_quantized = ((1u64 << bits) - 1) as f32;
                let step = delta_max / max_quantized;
                quantized
                    .into_iter()
                    .map(|q| {
                        q.iter()
                            .enumerate()
                            .map(|(j, &qj)| min[j] + qj as f32 * step)
                            .collect()
                    })
                    .collect()
            }
        }
    }

    /// The sorted multiset of per-corner values of one attribute.
    fn corner_value_multiset(faces: &[[PointIdx; 3]], per_point: &[Vec<f32>]) -> Vec<Vec<i32>> {
        let mut out: Vec<Vec<i32>> = faces
            .iter()
            .flat_map(|f| f.iter().map(|&p| bit_form(&per_point[usize::from(p)])))
            .collect();
        out.sort();
        out
    }

    fn assert_dequantized_roundtrip(path: &str) {
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode(mesh, &mut buffer, Config::default()).unwrap();
        let decoded = decode(&buffer).unwrap();

        let input = load_obj(path).unwrap();
        assert_eq!(decoded.attributes.len(), input.attributes.len());
        for (i, att) in input.attributes.iter().enumerate() {
            let expected = corner_value_multiset(&input.faces, &expected_floats(att));
            let datt = &decoded.attributes[i];
            let decoded_per_point: Vec<Vec<f32>> = (0..datt.len())
                .map(|p| attribute_floats(datt, PointIdx::from(p)))
                .collect();
            let got = corner_value_multiset(&decoded.faces, &decoded_per_point);
            assert_eq!(
                got, expected,
                "dequantized values mismatch for attribute {i} of {path}"
            );
        }

        // The decoded positions stay within quantization tolerance of the input.
        // `diff_l2_norm` is O(points * faces), so this extra geometric check only
        // runs on small meshes; the bit-level comparison above covers all sizes.
        if input.faces.len() <= 1000 {
            assert_positions_within_quantization_step(&decoded, &input, path);
        }
    }

    fn assert_positions_within_quantization_step(
        decoded: &draco_oxide_core::mesh::Mesh,
        input: &draco_oxide_core::mesh::Mesh,
        path: &str,
    ) {
        let l2 = decoded.diff_l2_norm(input);
        let pos = input
            .attributes
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::Position)
            .unwrap();
        let (_, delta_max) = quant_bounds::<3>(pos);
        let step = delta_max / (((1u64 << default_bits(AttributeType::Position)) - 1) as f32);
        assert!(
            l2 <= step as f64,
            "decoded positions off the input surface for {path}: l2 = {l2}, step = {step}"
        );
    }

    #[test]
    fn tetrahedron() {
        assert_dequantized_roundtrip("../tests/data/tetrahedron.obj");
    }

    #[test]
    fn cube_flat() {
        assert_dequantized_roundtrip("../tests/data/cube_flat.obj");
    }

    #[test]
    fn sphere() {
        assert_dequantized_roundtrip("../tests/data/sphere.obj");
    }

    #[test]
    fn punctured_sphere() {
        assert_dequantized_roundtrip("../tests/data/punctured_sphere.obj");
    }

    #[test]
    fn torus() {
        assert_dequantized_roundtrip("../tests/data/torus.obj");
    }

    #[test]
    fn bunny() {
        assert_dequantized_roundtrip("../tests/data/bunny.obj");
    }

    /// The zero-CPU normal path: the encoder emits an all-zero correction
    /// stream, so the decoder reconstructs exactly the geometry-derived
    /// predictions. They must come back as finite unit vectors, and the other
    /// attributes must be untouched by the mode.
    #[test]
    fn predicted_only_normals() {
        let path = "../tests/data/sphere.obj";
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode(
            mesh,
            &mut buffer,
            Config::default().with_normals(NormalEncoding::PredictedOnly),
        )
        .unwrap();
        let decoded = decode(&buffer).unwrap();

        let normals = decoded
            .attributes
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::Normal)
            .unwrap();
        for p in 0..normals.len() {
            let n: NdVector<3, f32> = normals.get(PointIdx::from(p));
            let norm = (n.get(0) * n.get(0) + n.get(1) * n.get(1) + n.get(2) * n.get(2)).sqrt();
            assert!(
                norm.is_finite() && (norm - 1.0).abs() < 1e-3,
                "predicted-only normal at point {p} is not a unit vector: {n:?}"
            );
        }

        let input = load_obj(path).unwrap();
        assert_positions_within_quantization_step(&decoded, &input, path);
    }
}

// Portable-attribute round trip (milestone 3): encode each mesh with the default
// configuration, decode to the portable representation, and require the decoded
// quantized integers to equal the encoder's portabilized values bit-exactly,
// corner-wise up to face relabeling.
#[cfg(feature = "decoder")]
mod portable_attributes {
    use super::attribute_roundtrip_util::{
        canonicalize, decoded_corner_tuples, expected_corner_tuples,
    };
    use crate::encode::{encode, Config};
    use crate::io::obj::load_obj;
    use draco_oxide_core::types::ConfigType;
    use draco_oxide_decoder::decode_portable;

    fn assert_portable_roundtrip(path: &str) {
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode(mesh, &mut buffer, Config::default()).unwrap();
        let portable = decode_portable(&buffer).unwrap();

        let decoded = canonicalize(decoded_corner_tuples(&portable.mesh));
        let expected = canonicalize(expected_corner_tuples(path));
        assert_eq!(
            decoded.len(),
            expected.len(),
            "face count mismatch for {path}"
        );
        assert_eq!(decoded, expected, "portable attribute mismatch for {path}");
    }

    #[test]
    fn tetrahedron() {
        assert_portable_roundtrip("../tests/data/tetrahedron.obj");
    }

    #[test]
    fn cube_flat() {
        assert_portable_roundtrip("../tests/data/cube_flat.obj");
    }

    #[test]
    fn cube_quads() {
        assert_portable_roundtrip("../tests/data/cube_quads.obj");
    }

    #[test]
    fn open_box() {
        assert_portable_roundtrip("../tests/data/open_box.obj");
    }

    #[test]
    fn groove_fan() {
        assert_portable_roundtrip("../tests/data/groove_fan.obj");
    }

    #[test]
    fn sphere() {
        assert_portable_roundtrip("../tests/data/sphere.obj");
    }

    #[test]
    fn punctured_sphere() {
        assert_portable_roundtrip("../tests/data/punctured_sphere.obj");
    }

    #[test]
    fn torus() {
        assert_portable_roundtrip("../tests/data/torus.obj");
    }

    #[test]
    fn bunny() {
        assert_portable_roundtrip("../tests/data/bunny.obj");
    }
}

// Attribute-seam round trip: decode the per-attribute seam edges and check the
// resulting attribute connectivity has the same number of attribute vertices as
// the encoder's, per non-position attribute. The attribute-vertex count is a
// topological invariant, so it holds under the encoder/decoder corner relabeling.
#[cfg(feature = "decoder")]
mod attribute_seams {
    use crate::encode::ds::{build_attribute_ds, build_global_ds};
    use crate::encode::{encode, Config};
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::mesh::ds::{AttributeCornerTable, GenericCornerTable};
    use draco_oxide_core::types::{ConfigType, CornerIdx};

    /// Number of attribute vertices: the count of seam-separated corner fans.
    fn count_attribute_vertices(act: &AttributeCornerTable, num_corners: usize) -> usize {
        let mut visited = vec![false; num_corners];
        let mut count = 0;
        for start in 0..num_corners {
            if visited[start] {
                continue;
            }
            count += 1;
            let start_c = CornerIdx::from(start);
            // Walk to the left-most corner of this fan.
            let mut c = start_c;
            while let Some(l) = act.swing_left(c) {
                if l == start_c {
                    break;
                }
                c = l;
            }
            // Swing right across the whole fan, marking every corner visited.
            let fan_start = c;
            loop {
                visited[usize::from(c)] = true;
                match act.swing_right(c) {
                    Some(r) if r != fan_start => c = r,
                    _ => break,
                }
            }
        }
        count
    }

    /// The encoder's attribute-vertex count for each non-position attribute, in
    /// attribute order.
    fn encoder_attribute_vertices(path: &str) -> Vec<usize> {
        let mesh = load_obj(path).unwrap();
        let faces = mesh.faces;
        let mut attributes = mesh.attributes;
        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);
        let adss = build_attribute_ds(&ds, &pos_corner_table, attributes);
        adss.iter()
            .filter(|a| a.att_data().get_attribute_type() != AttributeType::Position)
            .map(|a| a.num_vertices())
            .collect()
    }

    /// The decoded attribute-vertex count for each non-position attribute, from the
    /// seam edges rebuilt over the decoded position corner table.
    fn decoded_attribute_vertices(path: &str) -> Vec<usize> {
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode(mesh, &mut buffer, Config::default()).unwrap();
        let mut reader = draco_oxide_core::bit_coder::Reader::new(&buffer);
        let header = draco_oxide_decoder::header::decode_header(&mut reader).unwrap();
        let conn = draco_oxide_decoder::connectivity::decode_connectivity(
            &mut reader,
            header.encoder_method,
        )
        .unwrap();
        let conn = conn.edgebreaker().unwrap();
        let num_corners = conn.num_faces * 3;
        (0..conn.num_attribute_data)
            .map(|i| count_attribute_vertices(&conn.attribute_corner_table(i), num_corners))
            .collect()
    }

    fn assert_attribute_vertices_match(path: &str) {
        assert_eq!(
            decoded_attribute_vertices(path),
            encoder_attribute_vertices(path),
            "attribute-vertex counts mismatch for {path}"
        );
    }

    #[test]
    fn tetrahedron() {
        // Normals (no seams) plus texture coordinates (seams split two vertices).
        assert_attribute_vertices_match("../tests/data/tetrahedron.obj");
    }

    #[test]
    fn cube_quads() {
        assert_attribute_vertices_match("../tests/data/cube_quads.obj");
    }

    #[test]
    fn cube_flat() {
        assert_attribute_vertices_match("../tests/data/cube_flat.obj");
    }

    #[test]
    fn sphere() {
        assert_attribute_vertices_match("../tests/data/sphere.obj");
    }

    #[test]
    fn open_box() {
        assert_attribute_vertices_match("../tests/data/open_box.obj");
    }

    #[test]
    fn bunny() {
        assert_attribute_vertices_match("../tests/data/bunny.obj");
    }
}
