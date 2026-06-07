//! Cross-crate round-trip / integration tests relocated here from `draco-oxide-core`
//! and `draco-oxide-decoder` during the crate split. They exercise core/decoder
//! functionality but need the encoder (and `io`/decoder), which only the
//! `draco-oxide` crate sees together.

mod attribute_corner_table {
    use crate::io::obj::load_obj;
    use draco_oxide_core::attribute::AttributeType;
    use draco_oxide_core::corner_table::attribute_corner_table::AttributeCornerTable;
    use draco_oxide_core::corner_table::CornerTable;
    use draco_oxide_core::corner_table::GenericCornerTable;
    use draco_oxide_core::types::{CornerIdx, VertexIdx};
    #[test]
    fn test_no_att_seam() {
        // read the test data from a corner table

        let mut mesh = load_obj("../tests/data/sphere.obj").unwrap();
        let faces = mesh.faces;

        let att = mesh
            .attributes
            .iter()
            .find(|att| att.get_attribute_type() == AttributeType::Position)
            .unwrap();

        let corner_table = CornerTable::new(&faces, &att);
        let att = mesh
            .attributes
            .iter_mut()
            .find(|att| att.get_attribute_type() == AttributeType::Normal)
            .unwrap();
        let attr_corner_table = AttributeCornerTable::new(&corner_table, att);
        assert_eq!(
            attr_corner_table.num_vertices(),
            corner_table.num_vertices()
        );
        assert_eq!(
            attr_corner_table.corner_to_vertex.len(),
            corner_table.num_corners()
        );
        assert_eq!(
            attr_corner_table.vertex_to_attribute_map.len(),
            corner_table.num_vertices()
        );
        assert_eq!(
            attr_corner_table.left_most_corners.len(),
            corner_table.num_vertices()
        );
        assert_eq!(
            attr_corner_table.is_edge_on_seam.len(),
            corner_table.num_corners()
        );
        assert_eq!(
            attr_corner_table.is_vertex_on_seam.len(),
            corner_table.num_vertices()
        );
        assert!(attr_corner_table
            .is_edge_on_seam
            .iter()
            .all(|&x| x == false));
        assert!(attr_corner_table
            .is_vertex_on_seam
            .iter()
            .all(|&x| x == false));
        assert!(attr_corner_table
            .left_most_corners
            .iter()
            .all(|&x| usize::from(x) < corner_table.num_corners()));
        assert!(attr_corner_table
            .corner_to_vertex
            .iter()
            .all(|&x| usize::from(x) < corner_table.num_vertices()));

        // check the opposite corners
        for c in 0..corner_table.num_corners() {
            let c = CornerIdx::from(c);
            assert_eq!(
                attr_corner_table.opposite(c, &corner_table),
                corner_table.opposite(c)
            );
        }

        // check vertices
        for c in 0..corner_table.num_corners() {
            let c = CornerIdx::from(c);
            assert_eq!(
                attr_corner_table.vertex_idx(c),
                corner_table.vertex_idx(c),
                "attr corner_to_vertex: {:?}",
                attr_corner_table.corner_to_vertex,
            );
        }

        // no attribute seams, so all edges and vertices are not on a seam.
        attr_corner_table.is_edge_on_seam.iter().all(|&x| !x);
        attr_corner_table.is_vertex_on_seam.iter().all(|&x| !x);
    }

    #[test]
    fn test_att_seam() {
        let mut tetrahedron = load_obj("../tests/data/tetrahedron.obj").unwrap();
        let faces = tetrahedron.faces;
        let corner_table = CornerTable::new(&faces, &tetrahedron.attributes[0]);

        let tex_att = tetrahedron
            .attributes
            .iter_mut()
            .find(|att| att.get_attribute_type() == AttributeType::TextureCoordinate)
            .unwrap();
        let attr_corner_table = AttributeCornerTable::new(&corner_table, tex_att);
        assert_eq!(
            attr_corner_table.num_vertices(),
            corner_table.num_vertices() + 2
        );
        assert_eq!(
            attr_corner_table.corner_to_vertex.len(),
            corner_table.num_corners()
        );
        assert_eq!(attr_corner_table.corner_to_vertex[0], 0.into());
        assert_eq!(attr_corner_table.swing_left(4.into(), &corner_table), None);
        assert_eq!(attr_corner_table.swing_right(4.into(), &corner_table), None);
        assert_eq!(attr_corner_table.swing_left(8.into(), &corner_table), None);
        assert_eq!(attr_corner_table.swing_right(8.into(), &corner_table), None);
        assert_eq!(attr_corner_table.swing_left(10.into(), &corner_table), None);
        assert_eq!(
            attr_corner_table.swing_right(10.into(), &corner_table),
            None
        );
        let seam_edge_corners = [3, 5, 6, 7, 9, 11];
        for c in seam_edge_corners {
            let c = CornerIdx::from(c);
            assert!(
                attr_corner_table.is_corner_opposite_to_seam_edge(c),
                "Corner {:?} is not opposite to a seam edge, but it should be. is_edge_on_seam: {:?}",
                c, attr_corner_table.is_edge_on_seam
            )
        }
        let left_most_corners = [6, 5, 11, 10, 8, 4];
        for (v, left_most_corner) in left_most_corners.into_iter().enumerate() {
            let v = VertexIdx::from(v);
            let left_most_corner = CornerIdx::from(left_most_corner);
            assert_eq!(
                attr_corner_table.left_most_corner(v), left_most_corner,
                "Left most corner for vertex {:?} is {:?}, but it should be {:?}. left_most_corners: {:?}",
                v,
                attr_corner_table.left_most_corner(v),
                left_most_corner,
                attr_corner_table.left_most_corners
            );
            assert!(attr_corner_table
                .swing_left(left_most_corner, &corner_table)
                .is_none(),);
        }
    }
}

mod sequence {
    use crate::encode::connectivity::{encode_connectivity, ConnectivityEncoderOutput};
    use crate::io::obj::load_obj;
    use draco_oxide_core::codec::attribute::sequence::Traverser;
    use draco_oxide_core::corner_table::GenericCornerTable;
    use draco_oxide_core::types::ConfigType;

    #[test]
    fn test_traverser() {
        let mut mesh = load_obj("../tests/data/tetrahedron.obj").unwrap();
        let out: crate::encode::connectivity::ConnectivityEncoderOutput<'_> = encode_connectivity(
            &mesh.faces,
            &mut mesh.attributes,
            &mut Vec::new(),
            &crate::encode::Config::default(),
        )
        .unwrap();

        let (ct, corners) = if let ConnectivityEncoderOutput::Edgebreaker(edgebreaker_out) = out {
            (
                edgebreaker_out.corner_table,
                edgebreaker_out.corners_of_edgebreaker,
            )
        } else {
            panic!("Expected Edgebreaker Output");
        };

        let ct_pos = ct.universal_corner_table();
        let sequence_points = Traverser::new(ct_pos, corners.clone())
            .compute_seqeunce()
            .iter()
            .map(|c| ct_pos.point_idx(*c))
            .collect::<Vec<_>>();
        assert_eq!(
            sequence_points
                .into_iter()
                .map(|c| usize::from(c))
                .collect::<Vec<_>>(),
            vec![3, 1, 0, 2]
        );

        let ct_nor = &ct.attribute_corner_table(1).unwrap();
        let sequence_normals = Traverser::new(ct_nor, corners.clone())
            .compute_seqeunce()
            .iter()
            .map(|c| ct_nor.point_idx(*c))
            .collect::<Vec<_>>();
        assert_eq!(
            sequence_normals
                .into_iter()
                .map(|c| usize::from(c))
                .collect::<Vec<_>>(),
            vec![3, 1, 0, 2]
        );

        let ct_tex = &ct.attribute_corner_table(2).unwrap();
        let sequence_tex_coords = Traverser::new(ct_tex, corners)
            .compute_seqeunce()
            .iter()
            .map(|c| ct_tex.point_idx(*c))
            .collect::<Vec<_>>();
        assert_eq!(
            sequence_tex_coords
                .into_iter()
                .map(|c| usize::from(c))
                .collect::<Vec<_>>(),
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

    /// Computes (attr_idx, sequence_len, digest) for the universal corner table
    /// (attr 0) and every attribute corner table of `mesh`. The digest captures
    /// the exact `Vec<CornerIdx>` traversal order via point indices — this is the
    /// shared encoder/decoder symmetry that must stay byte-identical.
    fn sequence_fingerprints(path: &str) -> Vec<(usize, usize, u64)> {
        let mut mesh = load_obj(path).unwrap();
        let out = encode_connectivity(
            &mesh.faces,
            &mut mesh.attributes,
            &mut Vec::new(),
            &crate::encode::Config::default(),
        )
        .unwrap();

        let (ct, corners) = if let ConnectivityEncoderOutput::Edgebreaker(eb) = out {
            (eb.corner_table, eb.corners_of_edgebreaker)
        } else {
            panic!("Expected Edgebreaker Output for {path}");
        };

        let mut fps = Vec::new();

        let ct_pos = ct.universal_corner_table();
        let seq: Vec<usize> = Traverser::new(ct_pos, corners.clone())
            .compute_seqeunce()
            .iter()
            .map(|c| usize::from(ct_pos.point_idx(*c)))
            .collect();
        fps.push((0, seq.len(), digest(&seq)));

        let mut attr_idx = 1;
        while let Some(ct_attr) = ct.attribute_corner_table(attr_idx) {
            let seq: Vec<usize> = Traverser::new(&ct_attr, corners.clone())
                .compute_seqeunce()
                .iter()
                .map(|c| usize::from(ct_attr.point_idx(*c)))
                .collect();
            fps.push((attr_idx, seq.len(), digest(&seq)));
            attr_idx += 1;
        }

        fps
    }

    /// Byte-identical oracle for `compute_sequence`. The expected fingerprints
    /// were captured from the pre-optimization implementation; any change that
    /// alters the traversal order on these meshes (boundaries, handles) trips
    /// this test. torus.obj carries topological handles, which is exactly the
    /// case the handle-detection scan-and-remove blocks exist to handle.
    #[test]
    fn oracle_compute_sequence() {
        let cases: &[(&str, &[(usize, usize, u64)])] = &[
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
    const EXPECT_TETRAHEDRON: &[(usize, usize, u64)] = &[
        (0, 4, 18054049684469353541),
        (1, 4, 18054049684469353541),
        (2, 6, 3159456026337658052),
    ];
    const EXPECT_SPHERE: &[(usize, usize, u64)] = &[
        (0, 114, 17737425019064467876),
        (1, 114, 17737425019064467876),
    ];
    const EXPECT_PUNCTURED_SPHERE: &[(usize, usize, u64)] = &[
        (0, 114, 17132826066695074116),
        (1, 114, 17132826066695074116),
    ];
    const EXPECT_TORUS: &[(usize, usize, u64)] = &[(0, 2051, 930682351741064974)];
    const EXPECT_BUNNY: &[(usize, usize, u64)] = &[
        (0, 34834, 3080192193140594432),
        (1, 34834, 3080192193140594432),
    ];
}

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
