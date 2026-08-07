//! Point-cloud codec round trips (bitstream 2.3, kd-tree method).
//!
//! Covers draco-oxide against itself over every compression level and every
//! supported component type, and against the reference implementation in both
//! directions. The kd-tree algorithm does not preserve point order, so decoded
//! clouds are compared as point sets by symmetric Hausdorff distance rather
//! than index by index.
//!
//! The interop halves are skipped when the reference binaries are absent
//! (build them with `scripts/build-draco.sh`, or set `DRACO_ENCODER` /
//! `DRACO_DECODER`).

use std::path::{Path, PathBuf};
use std::process::Command;

use draco_oxide::core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide::core::bit_coder::ByteWriter;
use draco_oxide::core::point_cloud::PointCloud;
use draco_oxide::core::types::{ConfigType, NdVector, PointIdx, Vector};
use draco_oxide::core::utils::bit_coder::leb128_write;
use draco_oxide::decode;
use draco_oxide::encode::{encode_point_cloud, PointCloudConfig, Quantization};

const LEVELS: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];

fn find_binary(env_var: &str, name: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        let p = PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    let default = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../third_party/draco/_build")
        .join(name);
    default.is_file().then_some(default)
}

/// A deterministic pseudo-random cloud with anisotropic per-axis extents.
fn sample_positions(n: usize) -> Vec<NdVector<3, f32>> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f32 / (1u64 << 53) as f32
    };
    (0..n)
        .map(|_| NdVector::from([next() * 2.0 - 1.0, next() * 2.0, next() + 3.0]))
        .collect()
}

fn position_attribute(values: Vec<NdVector<3, f32>>) -> Attribute {
    Attribute::from_without_removing_duplicates::<NdVector<3, f32>, 3>(
        AttributeId::new(0),
        values,
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    )
}

fn encode(pc: PointCloud, level: u8, bits: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    let cfg = <PointCloudConfig as ConfigType>::default()
        .with_compression_level(level)
        .with_quantization(Quantization::Bits(bits));
    encode_point_cloud(pc, &mut buf, cfg).expect("encode point cloud");
    buf
}

fn positions_of(pc: &PointCloud, att_index: usize) -> Vec<[f32; 3]> {
    let att = &pc.attributes()[att_index];
    (0..pc.num_points())
        .map(|p| {
            let v: NdVector<3, f32> = att.get(PointIdx::from(p));
            [*v.get(0), *v.get(1), *v.get(2)]
        })
        .collect()
}

/// Symmetric Hausdorff distance between two point sets.
fn hausdorff(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    fn one_sided(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
        a.iter().fold(0.0f32, |worst, p| {
            let nearest = b.iter().fold(f32::INFINITY, |best, q| {
                let d = (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f32>();
                best.min(d)
            });
            worst.max(nearest.sqrt())
        })
    }
    one_sided(a, b).max(one_sided(b, a))
}

/// The largest error uniform quantization can introduce: half a step of the
/// largest per-axis extent, plus room for float rounding.
fn quantization_tolerance(values: &[NdVector<3, f32>], bits: u8) -> f32 {
    let mut range = 0.0f32;
    for c in 0..3 {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in values {
            lo = lo.min(*v.get(c));
            hi = hi.max(*v.get(c));
        }
        range = range.max(hi - lo);
    }
    let step = range / ((1u32 << bits) - 1) as f32;
    // A point can move by half a step on each of the three axes.
    step * 3.0f32.sqrt() / 2.0 * 1.01
}

#[test]
fn round_trip_over_every_compression_level() {
    let values = sample_positions(2000);
    let bits = 12;
    let tolerance = quantization_tolerance(&values, bits);
    let expected: Vec<[f32; 3]> = values
        .iter()
        .map(|v| [*v.get(0), *v.get(1), *v.get(2)])
        .collect();

    for level in LEVELS {
        let pc = PointCloud::new(vec![position_attribute(values.clone())]).expect("point cloud");
        let buf = encode(pc, level, bits);
        let decoded = decode::decode_point_cloud(&buf).expect("decode point cloud");
        assert_eq!(decoded.num_points(), values.len(), "level {level}");
        let got = positions_of(&decoded, 0);
        let d = hausdorff(&expected, &got);
        assert!(
            d <= tolerance,
            "level {level}: hausdorff {d} exceeds quantization tolerance {tolerance}"
        );
    }
}

#[test]
fn generic_decode_yields_the_point_cloud_variant() {
    let pc = PointCloud::new(vec![position_attribute(sample_positions(64))]).expect("point cloud");
    let buf = encode(pc, 6, 11);
    assert!(matches!(
        decode::decode(&buf),
        Ok(decode::Geometry::PointCloud(_))
    ));
}

#[test]
fn small_and_degenerate_clouds_round_trip() {
    for n in [1usize, 2, 3, 5] {
        let values = sample_positions(n);
        let pc = PointCloud::new(vec![position_attribute(values.clone())]).expect("point cloud");
        let buf = encode(pc, 6, 11);
        let decoded = decode::decode_point_cloud(&buf).expect("decode");
        assert_eq!(decoded.num_points(), n, "{n} points");
    }

    // Every point identical: the quantization range degenerates to a unit
    // interval and every value must come back as the same point.
    let same = vec![NdVector::from([1.5f32, -2.5, 7.0]); 32];
    let pc = PointCloud::new(vec![position_attribute(same.clone())]).expect("point cloud");
    let buf = encode(pc, 6, 11);
    let decoded = decode::decode_point_cloud(&buf).expect("decode");
    assert_eq!(decoded.num_points(), 32);
    for p in positions_of(&decoded, 0) {
        assert!(
            (p[0] - 1.5).abs() < 1e-4 && (p[1] + 2.5).abs() < 1e-4 && (p[2] - 7.0).abs() < 1e-4,
            "collapsed cloud decoded to {p:?}"
        );
    }
}

#[test]
fn integer_attributes_round_trip_exactly() {
    let n = 500usize;
    let positions = sample_positions(n);
    let colors: Vec<NdVector<3, u8>> = (0..n)
        .map(|i| {
            NdVector::from([
                (i % 256) as u8,
                ((i * 7) % 256) as u8,
                ((i * 13) % 251) as u8,
            ])
        })
        .collect();
    let signed: Vec<NdVector<2, i32>> = (0..n)
        .map(|i| NdVector::from([i as i32 - 250, -(i as i32) * 3 + 100]))
        .collect();

    let pc = PointCloud::new(vec![
        position_attribute(positions),
        Attribute::from_without_removing_duplicates::<NdVector<3, u8>, 3>(
            AttributeId::new(1),
            colors.clone(),
            AttributeType::Color,
            AttributeDomain::Position,
            Vec::new(),
        ),
        Attribute::from_without_removing_duplicates::<NdVector<2, i32>, 2>(
            AttributeId::new(2),
            signed.clone(),
            AttributeType::Custom,
            AttributeDomain::Position,
            Vec::new(),
        ),
    ])
    .expect("point cloud");

    let buf = encode(pc, 6, 14);
    let decoded = decode::decode_point_cloud(&buf).expect("decode");
    assert_eq!(decoded.num_points(), n);
    assert_eq!(
        decoded.attributes()[1].get_component_type(),
        ComponentDataType::U8
    );
    assert_eq!(
        decoded.attributes()[2].get_component_type(),
        ComponentDataType::I32
    );

    // Integer attributes are carried losslessly, so every decoded point must
    // reproduce one of the input rows exactly; the kd-tree reorders points, so
    // match on the whole row.
    let mut expected: Vec<(Vec<u8>, Vec<i32>)> = colors
        .iter()
        .zip(&signed)
        .map(|(c, s)| {
            (
                (0..3).map(|i| *c.get(i)).collect(),
                (0..2).map(|i| *s.get(i)).collect(),
            )
        })
        .collect();
    let mut got: Vec<(Vec<u8>, Vec<i32>)> = (0..n)
        .map(|p| {
            let c: NdVector<3, u8> = decoded.attributes()[1].get(PointIdx::from(p));
            let s: NdVector<2, i32> = decoded.attributes()[2].get(PointIdx::from(p));
            (
                (0..3).map(|i| *c.get(i)).collect(),
                (0..2).map(|i| *s.get(i)).collect(),
            )
        })
        .collect();
    expected.sort();
    got.sort();
    assert_eq!(expected, got, "integer attributes must round trip exactly");
}

#[test]
fn corrupted_streams_are_rejected_without_panicking() {
    let pc = PointCloud::new(vec![position_attribute(sample_positions(300))]).expect("point cloud");
    let base = encode(pc, 6, 11);

    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut rng = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for trial in 0..3000usize {
        let mut bytes = base.clone();
        match trial % 3 {
            0 => bytes.truncate(rng() as usize % base.len()),
            1 => {
                for _ in 0..1 + rng() as usize % 4 {
                    let i = rng() as usize % bytes.len();
                    bytes[i] = (rng() % 256) as u8;
                }
            }
            // Flip one bit past the header, where the payload lives.
            _ => {
                let i = 11 + rng() as usize % (bytes.len() - 11);
                bytes[i] ^= 1 << (rng() % 8);
            }
        }
        // Either outcome is fine; the decoder must not panic on any input.
        let _ = decode::decode_point_cloud(&bytes);
    }
}

/// A well-formed level-0 stream with the given counts. The counts are what a
/// hostile stream inflates, so they are built by hand rather than mutated.
fn synthetic_stream(num_attributes: u64, num_points: u32) -> Vec<u8> {
    let mut w: Vec<u8> = Vec::new();
    for b in b"DRACO" {
        w.write_u8(*b);
    }
    w.write_u8(2);
    w.write_u8(3);
    w.write_u8(0); // point cloud
    w.write_u8(1); // kd-tree
    w.write_u16(0); // flags
    w.write_u32(num_points);
    w.write_u8(1); // one attributes decoder
    leb128_write(num_attributes, &mut w);
    for i in 0..num_attributes {
        AttributeType::Custom.write_to(&mut w);
        ComponentDataType::U8.write_to(&mut w);
        w.write_u8(4); // components
        w.write_u8(0); // normalized
        leb128_write(i, &mut w);
    }
    w.write_u8(0); // compression level
    w.write_u32(0); // bit length
    w.write_u32(num_points);
    for _ in 0..4 {
        w.write_u32(4);
        w.write_u32(0);
    }
    w
}

#[test]
fn an_attribute_count_beyond_the_stream_is_rejected() {
    // Restate the attribute count, the single byte at offset 16, as one the
    // stream cannot back: a descriptor is never shorter than five bytes.
    let base = synthetic_stream(1, 1);
    assert_eq!(base[16], 1, "attribute count is not where expected");
    let mut bytes = base[..16].to_vec();
    leb128_write(1_000_000, &mut bytes);
    bytes.extend_from_slice(&base[17..]);

    let err = decode::decode_point_cloud(&bytes).expect_err("count must be rejected");
    assert!(
        err.to_string()
            .contains("attribute count exceeds the stream"),
        "unexpected error: {err}"
    );
}

#[test]
fn an_unallocatable_point_count_is_rejected() {
    // A multi-terabyte point array from a 550-byte stream: it has to come back
    // as an error rather than abort in the allocator.
    let bytes = synthetic_stream(100, i32::MAX as u32);
    let err = decode::decode_point_cloud(&bytes).expect_err("point count must be rejected");
    assert!(
        err.to_string().contains("can be allocated"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_crafted_signed_minimum_is_rejected() {
    let values: Vec<NdVector<1, i32>> = vec![
        NdVector::from([0i32]),
        NdVector::from([2_000_000_000i32]),
        NdVector::from([1_000_000_000i32]),
        NdVector::from([7i32]),
    ];
    let att = Attribute::from_without_removing_duplicates::<NdVector<1, i32>, 1>(
        AttributeId::new(0),
        values,
        AttributeType::Custom,
        AttributeDomain::Position,
        Vec::new(),
    );
    let pc = PointCloud::new(vec![att]).expect("point cloud");
    let mut buf = Vec::new();
    encode_point_cloud(pc, &mut buf, <PointCloudConfig as ConfigType>::default()).expect("encode");

    // The stream ends with the zigzag varint of the minimum, zero here; a
    // large one pushes every decoded value past i32.
    assert_eq!(*buf.last().unwrap(), 0);
    buf.pop();
    leb128_write(((2_000_000_000i32 as i64) << 1) as u64, &mut buf);
    let err = decode::decode_point_cloud(&buf).expect_err("minimum must be rejected");
    assert!(
        err.to_string()
            .contains("out of range for its component type"),
        "unexpected error: {err}"
    );
}

#[test]
fn the_portable_tier_reconstructs_the_dequantized_cloud() {
    let n = 400usize;
    let positions = sample_positions(n);
    let colors: Vec<NdVector<3, u8>> = (0..n)
        .map(|i| NdVector::from([(i % 256) as u8, ((i * 7) % 256) as u8, 9u8]))
        .collect();
    let signed: Vec<NdVector<2, i16>> = (0..n)
        .map(|i| NdVector::from([i as i16 - 200, -(i as i16) * 3]))
        .collect();

    let pc = PointCloud::new(vec![
        position_attribute(positions),
        Attribute::from_without_removing_duplicates::<NdVector<3, u8>, 3>(
            AttributeId::new(1),
            colors,
            AttributeType::Color,
            AttributeDomain::Position,
            Vec::new(),
        ),
        Attribute::from_without_removing_duplicates::<NdVector<2, i16>, 2>(
            AttributeId::new(2),
            signed,
            AttributeType::Custom,
            AttributeDomain::Position,
            Vec::new(),
        ),
    ])
    .expect("point cloud");
    let buf = encode(pc, 6, 12);

    let portable = decode::decode_point_cloud_portable(&buf).expect("portable decode");
    assert_eq!(portable.transforms.len(), 3);
    for att in portable.point_cloud.attributes() {
        assert_eq!(
            att.get_component_type(),
            ComponentDataType::I32,
            "portable attributes are integers"
        );
    }
    assert!(matches!(
        portable.transforms[0],
        decode::AttributeTransform::Quantized { bits: 12, .. }
    ));

    // Applying the transforms by hand must land on what the dequantized entry
    // point returns, value for value.
    let dequantized = decode::decode_point_cloud(&buf).expect("dequantized decode");
    let expected = positions_of(&dequantized, 0);
    let decode::AttributeTransform::Quantized {
        ref min,
        delta_max,
        bits,
    } = portable.transforms[0]
    else {
        panic!("positions must carry a quantization transform");
    };
    let step = delta_max / ((1u64 << bits) - 1) as f32;
    let att = &portable.point_cloud.attributes()[0];
    for (p, want) in expected.iter().enumerate() {
        let q: NdVector<3, i32> = att.get(PointIdx::from(p));
        for c in 0..3 {
            let got = min[c] + *q.get(c) as f32 * step;
            assert_eq!(got, want[c], "point {p} component {c}");
        }
    }
}

#[test]
fn a_full_range_signed_attribute_round_trips() {
    // The widest span, i32::MAX - i32::MIN, is exactly the u32 the kd-tree
    // codes and drives the 32-bit-length path.
    let values: Vec<NdVector<1, i32>> = vec![
        NdVector::from([i32::MIN]),
        NdVector::from([i32::MAX]),
        NdVector::from([0i32]),
    ];
    let att = Attribute::from_without_removing_duplicates::<NdVector<1, i32>, 1>(
        AttributeId::new(0),
        values,
        AttributeType::Custom,
        AttributeDomain::Position,
        Vec::new(),
    );
    let pc = PointCloud::new(vec![att]).expect("point cloud");
    let mut buf = Vec::new();
    encode_point_cloud(pc, &mut buf, <PointCloudConfig as ConfigType>::default()).expect("encode");

    let decoded = decode::decode_point_cloud(&buf).expect("decode");
    let mut got: Vec<i32> = (0..decoded.num_points())
        .map(|p| {
            let v: NdVector<1, i32> = decoded.attributes()[0].get(PointIdx::from(p));
            *v.get(0)
        })
        .collect();
    got.sort();
    assert_eq!(got, vec![i32::MIN, 0, i32::MAX]);
}

#[test]
fn google_draco_decodes_oxide_point_clouds() {
    let Some(decoder) = find_binary("DRACO_DECODER", "draco_decoder") else {
        eprintln!(
            "SKIP google_draco_decodes_oxide_point_clouds: `draco_decoder` not found.\n      \
             Build it with `scripts/build-draco.sh`, or set DRACO_DECODER=<path>."
        );
        return;
    };

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/point_cloud");
    std::fs::create_dir_all(&out_dir).expect("create output dir");

    let values = sample_positions(1500);
    let bits = 12;
    let tolerance = quantization_tolerance(&values, bits);
    let expected: Vec<[f32; 3]> = values
        .iter()
        .map(|v| [*v.get(0), *v.get(1), *v.get(2)])
        .collect();

    for level in LEVELS {
        let pc = PointCloud::new(vec![position_attribute(values.clone())]).expect("point cloud");
        let buf = encode(pc, level, bits);
        let drc = out_dir.join(format!("oxide_level{level}.drc"));
        std::fs::write(&drc, &buf).expect("write .drc");

        let decoded_path = out_dir.join(format!("oxide_level{level}.obj"));
        let output = Command::new(&decoder)
            .arg("-i")
            .arg(&drc)
            .arg("-o")
            .arg(&decoded_path)
            .output()
            .expect("spawn draco_decoder");
        assert!(
            output.status.success(),
            "level {level}: draco_decoder failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );

        let got = read_obj_points(&decoded_path);
        assert_eq!(got.len(), values.len(), "level {level}");
        let d = hausdorff(&expected, &got);
        assert!(
            d <= tolerance,
            "level {level}: reference decode differs by {d}, tolerance {tolerance}"
        );
    }
}

#[test]
fn oxide_decodes_google_draco_point_clouds() {
    let (Some(encoder), Some(decoder)) = (
        find_binary("DRACO_ENCODER", "draco_encoder"),
        find_binary("DRACO_DECODER", "draco_decoder"),
    ) else {
        eprintln!(
            "SKIP oxide_decodes_google_draco_point_clouds: reference binaries not found.\n      \
             Build them with `scripts/build-draco.sh`."
        );
        return;
    };

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/point_cloud");
    std::fs::create_dir_all(&out_dir).expect("create output dir");

    let values = sample_positions(1500);
    let source = out_dir.join("reference_source.obj");
    let mut obj = String::new();
    for v in &values {
        obj.push_str(&format!("v {} {} {}\n", v.get(0), v.get(1), v.get(2)));
    }
    std::fs::write(&source, obj).expect("write source obj");

    // -cl N maps to kd-tree level min(N, 6); -cl 0 selects the sequential
    // method instead, which this decoder does not implement.
    for cl in 1..=7 {
        let drc = out_dir.join(format!("reference_cl{cl}.drc"));
        let status = Command::new(&encoder)
            .args(["-point_cloud", "-i"])
            .arg(&source)
            .arg("-o")
            .arg(&drc)
            .args(["-qp", "12", "-cl", &cl.to_string()])
            .output()
            .expect("spawn draco_encoder");
        assert!(
            status.status.success(),
            "cl {cl}: draco_encoder failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );

        // The reference decoder's own output is the oracle: both decoders must
        // reconstruct the same cloud from the same stream.
        let reference_obj = out_dir.join(format!("reference_cl{cl}.obj"));
        let out = Command::new(&decoder)
            .arg("-i")
            .arg(&drc)
            .arg("-o")
            .arg(&reference_obj)
            .output()
            .expect("spawn draco_decoder");
        assert!(out.status.success(), "cl {cl}: reference decode failed");
        let oracle = read_obj_points(&reference_obj);

        let bytes = std::fs::read(&drc).expect("read .drc");
        let decoded = decode::decode_point_cloud(&bytes)
            .unwrap_or_else(|e| panic!("cl {cl}: oxide failed to decode a reference stream: {e}"));
        let got = positions_of(&decoded, 0);

        assert_eq!(got.len(), oracle.len(), "cl {cl}");
        // Both decoders walk the same tree, so the point order matches too.
        // The oracle passes through the reference's OBJ writer, which prints
        // six decimals, so agreement is asserted at that precision.
        for (i, (a, b)) in oracle.iter().zip(&got).enumerate() {
            for c in 0..3 {
                assert!(
                    (a[c] - b[c]).abs() <= 1e-6,
                    "cl {cl}, point {i}: reference {a:?} vs oxide {b:?}"
                );
            }
        }
    }
}

fn read_obj_points(path: &Path) -> Vec<[f32; 3]> {
    std::fs::read_to_string(path)
        .expect("read decoded obj")
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            (it.next() == Some("v")).then(|| {
                let c: Vec<f32> = it.take(3).map(|x| x.parse().expect("obj float")).collect();
                [c[0], c[1], c[2]]
            })
        })
        .collect()
}
