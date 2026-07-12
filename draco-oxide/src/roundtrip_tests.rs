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

// These round-trip tests need `draco-oxide-decoder`, which is not published to
// crates.io yet and so is not a dependency of `draco-oxide`. Restore this module
// (and the `decoder` feature gate) once the decoder crate is published.
/*
#[cfg(feature = "decoder")]
mod symbol_coding {
    use crate::encode::entropy::symbol_coding;
    use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
    use draco_oxide_decoder::decode::entropy::symbol_coding::{decode_symbols, Err};

    #[test]
    fn test_encode_decode_symbols() -> Result<(), Err> {
        let len = 100;
        let symbols = (0..len).map(|x| (x * x * x) % 23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(),
            1,
            SymbolEncodingMethod::LengthCoded,
            &mut buffer,
        )
        .unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(len as usize, 1, &mut reader)?;
        assert_eq!(
            reader.next(),
            None,
            "Reader should be empty after decoding all symbols"
        );
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }

    #[test]
    fn test_encode_decode_symbols_multi_components() -> Result<(), Err> {
        let len = 300;
        let symbols = (0..len).map(|x| (x * x * x) % 23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(),
            3,
            SymbolEncodingMethod::LengthCoded,
            &mut buffer,
        )
        .unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(len as usize, 3, &mut reader)?;
        assert_eq!(
            reader.next(),
            None,
            "Reader should be empty after decoding all symbols"
        );
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }

    #[test]
    fn test_encode_decode_symbols_direct_coded() -> Result<(), Err> {
        let len = 100;
        let symbols = (0..len).map(|x| (x * x * x) % 23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(),
            1,
            SymbolEncodingMethod::DirectCoded,
            &mut buffer,
        )
        .unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(len as usize, 1, &mut reader)?;
        assert_eq!(
            reader.next(),
            None,
            "Reader should be empty after decoding all symbols"
        );
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }

    #[test]
    fn test_encode_decode_symbols_direct_coded_multi_components() -> Result<(), Err> {
        let len = 300;
        let symbols = (0..len).map(|x| (x * x * x) % 23).collect::<Vec<_>>();
        let mut buffer = Vec::new();
        symbol_coding::encode_symbols(
            symbols.clone(),
            3,
            SymbolEncodingMethod::DirectCoded,
            &mut buffer,
        )
        .unwrap();
        let mut reader = buffer.into_iter();
        let decoded_symbols = decode_symbols(len as usize, 3, &mut reader)?;
        assert_eq!(
            reader.next(),
            None,
            "Reader should be empty after decoding all symbols"
        );
        assert_eq!(decoded_symbols, symbols);
        Ok(())
    }
}
*/
