//! Attribute-value round trips against exact oracles: portable integers must
//! match an independent reimplementation of the quantizers, and dequantized
//! floats must match the oracle's inverse. Stricter than the surface-distance
//! profile tests, which is why these stay hand-written Rust.

// Shared helpers for the attribute round-trip tests: the encoder-side quantized
// values (the portabilization math replayed on the input mesh) and the
// relabeling-invariant face-multiset comparison.
mod attribute_roundtrip_util {
    use draco_oxide::core::attribute::{Attribute, AttributeType};
    use draco_oxide::core::codec::attribute::geom::{float_vector_to_oct, oct_center};
    use draco_oxide::core::mesh::Mesh;
    use draco_oxide::core::types::{NdVector, PointIdx, Vector};
    use draco_oxide::io::obj::load_obj;

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
    /// largest per-component extent, with the min/max scan seeded from the
    /// first value exactly like the encoder's.
    pub fn quant_bounds<const N: usize>(att: &Attribute) -> (Vec<f32>, f32)
    where
        NdVector<N, f32>: Vector<N, Component = f32>,
    {
        let mut min = [0f32; N];
        let mut max = [0f32; N];
        if att.num_unique_values() > 0 {
            let v: NdVector<N, f32> = att.get_unique_val(0usize.into());
            for j in 0..N {
                min[j] = *v.get(j);
                max[j] = *v.get(j);
            }
        }
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
    /// `att` (min/max seeded from the first value, f32 math, truncating
    /// round-half-up).
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
        use draco_oxide::core::codec::attribute::geom::octahedral_inverse_transform;
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
mod dequantized {
    use super::attribute_roundtrip_util::{
        attribute_floats, default_bits, expected_quantized, quant_bounds,
    };
    use draco_oxide::core::attribute::{Attribute, AttributeType};
    use draco_oxide::core::codec::attribute::geom::octahedral_inverse_transform;
    use draco_oxide::core::types::{ConfigType, NdVector, PointIdx, Vector};
    use draco_oxide::decode::decode_mesh;
    use draco_oxide::encode::{encode_mesh, Config, NormalEncoding};
    use draco_oxide::io::obj::load_obj;

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
        encode_mesh(mesh, &mut buffer, Config::default()).unwrap();
        let decoded = decode_mesh(&buffer).unwrap();

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
        decoded: &draco_oxide::core::mesh::Mesh,
        input: &draco_oxide::core::mesh::Mesh,
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
        assert_dequantized_roundtrip("data/tetrahedron.obj");
    }

    #[test]
    fn cube_flat() {
        assert_dequantized_roundtrip("data/cube_flat.obj");
    }

    #[test]
    fn sphere() {
        assert_dequantized_roundtrip("data/sphere.obj");
    }

    #[test]
    fn punctured_sphere() {
        assert_dequantized_roundtrip("data/punctured_sphere.obj");
    }

    #[test]
    fn torus() {
        assert_dequantized_roundtrip("data/torus.obj");
    }

    #[test]
    fn bunny() {
        assert_dequantized_roundtrip("data/bunny.obj");
    }

    /// The zero-CPU normal path: the encoder emits an all-zero correction
    /// stream, so the decoder reconstructs exactly the geometry-derived
    /// predictions. They must come back as finite unit vectors, and the other
    /// attributes must be untouched by the mode.
    #[test]
    fn predicted_only_normals() {
        let path = "data/sphere.obj";
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode_mesh(
            mesh,
            &mut buffer,
            Config::default().with_normals(NormalEncoding::PredictedOnly),
        )
        .unwrap();
        let decoded = decode_mesh(&buffer).unwrap();

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
mod portable_attributes {
    use super::attribute_roundtrip_util::{
        canonicalize, decoded_corner_tuples, expected_corner_tuples,
    };
    use draco_oxide::core::types::ConfigType;
    use draco_oxide::decode::decode_mesh_portable;
    use draco_oxide::encode::{encode_mesh, Config};
    use draco_oxide::io::obj::load_obj;

    fn assert_portable_roundtrip(path: &str) {
        let mesh = load_obj(path).unwrap();
        let mut buffer = Vec::new();
        encode_mesh(mesh, &mut buffer, Config::default()).unwrap();
        let portable = decode_mesh_portable(&buffer).unwrap();

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
        assert_portable_roundtrip("data/tetrahedron.obj");
    }

    #[test]
    fn cube_flat() {
        assert_portable_roundtrip("data/cube_flat.obj");
    }

    #[test]
    fn cube_quads() {
        assert_portable_roundtrip("data/cube_quads.obj");
    }

    #[test]
    fn open_box() {
        assert_portable_roundtrip("data/open_box.obj");
    }

    #[test]
    fn groove_fan() {
        assert_portable_roundtrip("data/groove_fan.obj");
    }

    #[test]
    fn sphere() {
        assert_portable_roundtrip("data/sphere.obj");
    }

    #[test]
    fn punctured_sphere() {
        assert_portable_roundtrip("data/punctured_sphere.obj");
    }

    #[test]
    fn torus() {
        assert_portable_roundtrip("data/torus.obj");
    }

    #[test]
    fn bunny() {
        assert_portable_roundtrip("data/bunny.obj");
    }
}
