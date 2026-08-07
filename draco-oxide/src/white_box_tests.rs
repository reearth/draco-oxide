//! White-box tests over crate-private encoder internals: attribute data
//! structures, traversal sequences, entropy round trips, connectivity, and
//! seam handling. Black-box integration tests live in the `tests` crate.

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
    /// traversal order via point indices, this is the shared encoder/decoder
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

// Full symbol-coding round trip: the encoder's writers against the decoder's
// readers, at both levels of the stack. The `RansSymbolEncoder`/`RansSymbolDecoder`
// pair covers the frequency-table format itself; `encode_symbols`/`decode_symbols`
// cover the DirectCoded and LengthCoded framing around it.
#[cfg(feature = "decoder")]
mod symbol_coding {
    use crate::encode::entropy::rans::RansSymbolEncoder;
    use crate::encode::entropy::symbol_coding;
    use draco_oxide_core::bit_coder::{ByteWriter, Reader};
    use draco_oxide_core::codec::entropy::SymbolEncodingMethod;
    use draco_oxide_decoder::entropy::rans::RansSymbolDecoder;
    use draco_oxide_decoder::entropy::{decode_symbols, start_symbol_decoder, AnySymbolDecoder};
    use draco_oxide_decoder::Err;

    /// Histogram of `symbols` over the alphabet `0..=max`.
    fn histogram(symbols: &[usize]) -> Vec<usize> {
        let max = *symbols.iter().max().unwrap();
        let mut freq = vec![0usize; max + 1];
        for &s in symbols {
            freq[s] += 1;
        }
        freq
    }

    /// Codes `symbols` with [`RansSymbolEncoder`] at `precision` and decodes the
    /// result back with [`RansSymbolDecoder`].
    fn rans_symbol_round_trip(symbols: &[usize], precision: usize) {
        let mut buf: Vec<u8> = Vec::new();
        let mut enc =
            RansSymbolEncoder::new(&mut buf, histogram(symbols), None, precision).unwrap();
        for &s in symbols.iter().rev() {
            enc.write(s).unwrap();
        }
        enc.flush().unwrap();

        let mut reader = Reader::new(&buf);
        let mut dec = RansSymbolDecoder::new(&mut reader, symbols.len(), precision).unwrap();
        let decoded: Vec<usize> = (0..symbols.len()).map(|_| dec.decode()).collect();
        assert_eq!(decoded, symbols);
    }

    #[test]
    fn rans_symbol_decoder_round_trip() {
        rans_symbol_round_trip(&[0, 1, 2, 1, 0, 3, 3, 2, 1, 0, 0, 1, 2, 3, 0, 3, 3, 1], 12);
    }

    #[test]
    fn rans_symbol_decoder_handles_zero_runs() {
        // A sparse alphabet exercises the zero-run flag in the frequency table.
        rans_symbol_round_trip(&[0, 9, 9, 0, 0, 9, 3, 3, 9, 0, 9, 9, 0, 3, 9], 12);
    }

    #[test]
    fn rans_symbol_decoder_high_precision_large_alphabet() {
        // A large alphabet forces multi-byte frequency-table entries (precision 20)
        // and drives the rANS state through the wider u22/u30 tag layouts.
        let mut symbols = Vec::new();
        let mut x = 7usize;
        for _ in 0..4000 {
            x = (x * 1103515245 + 12345) % 6000;
            symbols.push(x);
        }
        rans_symbol_round_trip(&symbols, 20);
    }

    #[test]
    fn rans_symbol_decoder_single_symbol_alphabet() {
        // Every value identical: one symbol takes the whole probability mass.
        rans_symbol_round_trip(&[0; 20], 12);
    }

    #[test]
    fn decode_symbols_direct_round_trip() {
        let symbols: Vec<u64> = vec![0, 1, 2, 1, 0, 3, 3, 2, 1, 0, 0, 1, 2, 3, 0, 2, 1, 3];

        // Reproduce the DirectCoded framing: method byte, bit-length byte (8 maps to
        // precision 12 in the decoder), then the `RansSymbolEncoder` payload.
        let mut buf: Vec<u8> = Vec::new();
        SymbolEncodingMethod::DirectCoded.write_to(&mut buf);
        buf.write_u8(8);
        let flat: Vec<usize> = symbols.iter().map(|&s| s as usize).collect();
        let mut enc = RansSymbolEncoder::new(&mut buf, histogram(&flat), None, 12).unwrap();
        for &s in flat.iter().rev() {
            enc.write(s).unwrap();
        }
        enc.flush().unwrap();

        // A trailing sentinel confirms `decode_symbols` consumes exactly the payload.
        buf.write_u8(0xAB);

        let mut reader = Reader::new(&buf);
        let decoded = decode_symbols(&mut reader, symbols.len(), 1).unwrap();
        assert_eq!(decoded, symbols);
        assert_eq!(reader.read_u8().unwrap(), 0xAB);
    }

    /// Builds a LengthCoded stream the way Google's `EncodeTaggedSymbols` does:
    /// the method byte, the rANS-coded per-group bit widths, then the values as
    /// LSB-first bit fields of that width, padded to a whole byte.
    fn tagged_stream(values: &[u32], num_components: usize) -> Vec<u8> {
        let widths: Vec<u8> = values
            .chunks(num_components)
            .map(|group| {
                let max = group.iter().copied().max().unwrap_or(0);
                (32 - max.leading_zeros()).max(1) as u8
            })
            .collect();

        let mut buf: Vec<u8> = Vec::new();
        SymbolEncodingMethod::LengthCoded.write_to(&mut buf);

        // Tags span 1..=32, which fixes the tag alphabet's rANS precision at 12.
        let mut freq = vec![0usize; 33];
        for &w in &widths {
            freq[w as usize] += 1;
        }
        let mut enc = RansSymbolEncoder::new(&mut buf, freq, None, 12).unwrap();
        for &w in widths.iter().rev() {
            enc.write(w as usize).unwrap();
        }
        enc.flush().unwrap();

        let mut bits: Vec<bool> = Vec::new();
        for (group, &w) in values.chunks(num_components).zip(&widths) {
            for &v in group {
                for b in 0..w {
                    bits.push((v >> b) & 1 == 1);
                }
            }
        }
        for chunk in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &b) in chunk.iter().enumerate() {
                byte |= (b as u8) << i;
            }
            buf.write_u8(byte);
        }
        buf
    }

    #[test]
    fn decode_symbols_tagged_round_trip() {
        // Groups of three whose bit widths differ, so a wrong width desynchronizes
        // the value field rather than merely corrupting one symbol.
        let values: Vec<u32> = vec![1, 0, 1, 500, 12, 3, 7, 7, 6, 0, 0, 0, 131071, 2, 40];
        let mut buf = tagged_stream(&values, 3);
        // A trailing sentinel confirms the value field's length is derived correctly.
        buf.write_u8(0xAB);

        let mut reader = Reader::new(&buf);
        let decoded = decode_symbols(&mut reader, values.len() / 3, 3).unwrap();
        assert_eq!(
            decoded,
            values.iter().map(|&v| v as u64).collect::<Vec<_>>()
        );
        assert_eq!(reader.read_u8().unwrap(), 0xAB);
    }

    #[test]
    fn tagged_decoder_pops_in_stream_order() {
        // The lazy consumer must see exactly what the batch decoder returns.
        let values: Vec<u32> = (0..64).map(|i| (i * 37) % 1024).collect();
        let buf = tagged_stream(&values, 2);

        let mut reader = Reader::new(&buf);
        let batch = decode_symbols(&mut reader, values.len() / 2, 2).unwrap();

        let mut reader = Reader::new(&buf);
        let mut decoder = match start_symbol_decoder(&mut reader, values.len(), 2).unwrap() {
            AnySymbolDecoder::Tagged(d) => d,
            AnySymbolDecoder::Direct(_) => panic!("stream is LengthCoded"),
        };
        let popped: Vec<u64> = (0..values.len()).map(|_| decoder.decode() as u64).collect();
        assert_eq!(popped, batch);
    }

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
    use crate::encode::{encode_mesh, Config};
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
        encode_mesh(mesh, &mut buffer, Config::default()).unwrap();
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
        assert_laplacian_equivalent("../tests/data/torus.obj");
    }

    #[test]
    fn bunny() {
        // Large enough that even the sparse Laplacian's matvecs cost seconds
        // in a debug build; the structural check suffices here.
        assert_structurally_equivalent("../tests/data/bunny.obj");
    }
}

// Attribute-seam round trip: decode the per-attribute seam edges and check the
// resulting attribute connectivity has the same number of attribute vertices as
// the encoder's, per non-position attribute. The attribute-vertex count is a
// topological invariant, so it holds under the encoder/decoder corner relabeling.
#[cfg(feature = "decoder")]
mod attribute_seams {
    use crate::encode::ds::{build_attribute_ds, build_global_ds};
    use crate::encode::{encode_mesh, Config};
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
        encode_mesh(mesh, &mut buffer, Config::default()).unwrap();
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
