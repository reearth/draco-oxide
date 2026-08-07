//! Integer-typed attributes ride the integer codec with their declared type.
//!
//! Encodes a synthetic mesh carrying u8, u16, and i32 attributes, checks the
//! reference `draco_decoder` accepts the stream, and checks draco-oxide's own
//! decoder reproduces both the values and the declared component types. The
//! reference decoder is resolved like the profile harness does; the interop
//! assertions are skipped without it.

use std::path::{Path, PathBuf};
use std::process::Command;

use draco_oxide::core::attribute::{AttributeDomain, AttributeType, ComponentDataType};
use draco_oxide::core::mesh::builder::MeshBuilder;
use draco_oxide::core::types::{ConfigType, NdVector, Vector};
use draco_oxide::encode::{self, encode_mesh};

/// Locate Google Draco's `draco_decoder`, or `None` if it isn't available.
fn find_draco_decoder() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DRACO_DECODER") {
        let p = PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    let default =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../third_party/draco/_build/draco_decoder");
    default.is_file().then_some(default)
}

/// An 8x2 grid strip with one value per point for each added attribute.
fn grid_builder() -> (MeshBuilder, usize) {
    let mut builder = MeshBuilder::new();
    let mut positions: Vec<NdVector<3, f32>> = Vec::new();
    let mut faces = Vec::new();
    for i in 0..8usize {
        positions.push(NdVector::from([i as f32, 0.0, 0.0]));
        positions.push(NdVector::from([i as f32, 1.0, 0.0]));
    }
    let num_points = positions.len();
    for i in 0..7usize {
        let a = 2 * i;
        faces.push([a, a + 1, a + 2]);
        faces.push([a + 1, a + 3, a + 2]);
    }
    builder.set_connectivity_attribute(faces);
    builder.add_attribute(
        positions,
        AttributeType::Position,
        AttributeDomain::Position,
        vec![],
    );
    (builder, num_points)
}

#[test]
fn integer_attributes_round_trip() {
    let (mut builder, num_points) = grid_builder();
    // Values cover both halves of each type's range so a signedness mixup in
    // the declared type surfaces as a value change somewhere downstream.
    let colors: Vec<NdVector<3, u8>> = (0..num_points)
        .map(|i| NdVector::from([(i * 16) as u8, 255 - (i * 16) as u8, 200]))
        .collect();
    let ids: Vec<NdVector<1, u16>> = (0..num_points)
        .map(|i| NdVector::from([40000 + i as u16]))
        .collect();
    let offsets: Vec<NdVector<1, i32>> = (0..num_points)
        .map(|i| NdVector::from([-1000 + i as i32]))
        .collect();
    builder.add_attribute(
        colors,
        AttributeType::Color,
        AttributeDomain::Corner,
        vec![],
    );
    builder.add_attribute(ids, AttributeType::Custom, AttributeDomain::Corner, vec![]);
    builder.add_attribute(
        offsets,
        AttributeType::Custom,
        AttributeDomain::Corner,
        vec![],
    );
    let mesh = builder.build().expect("mesh builds");

    let mut buf = Vec::new();
    encode_mesh(mesh, &mut buf, <encode::Config as ConfigType>::default())
        .expect("encode succeeds");

    // The reference decoder accepts the stream.
    if let Some(decoder) = find_draco_decoder() {
        let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/integer_attributes");
        std::fs::create_dir_all(&out_dir).unwrap();
        let drc = out_dir.join("grid.drc");
        std::fs::write(&drc, &buf).unwrap();
        let ply = out_dir.join("grid.ply");
        let output = Command::new(&decoder)
            .arg("-i")
            .arg(&drc)
            .arg("-o")
            .arg(&ply)
            .output()
            .expect("draco_decoder runs");
        assert!(
            output.status.success(),
            "draco_decoder rejected the integer-attribute stream: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    } else {
        eprintln!("skipping reference-decoder check: draco_decoder not found");
    }

    // Oxide's own decoder reproduces the values in their declared types.
    let decoded = draco_oxide::decode::decode_mesh(&buf).expect("oxide decodes its own stream");
    let mut seen_colors = false;
    let mut seen_ids = false;
    let mut seen_offsets = false;
    for att in decoded.get_attributes() {
        match (att.get_attribute_type(), att.get_num_components()) {
            (AttributeType::Color, 3) => {
                seen_colors = true;
                assert_eq!(att.get_component_type(), ComponentDataType::U8);
                let vals: Vec<[u8; 3]> = (0..att.num_unique_values())
                    .map(|i| {
                        let v: NdVector<3, u8> = att.get_unique_val(i.into());
                        [*v.get(0), *v.get(1), *v.get(2)]
                    })
                    .collect();
                for i in 0..num_points {
                    let expect = [(i * 16) as u8, 255 - (i * 16) as u8, 200];
                    assert!(vals.contains(&expect), "color {expect:?} missing");
                }
            }
            (AttributeType::Custom, 1) => match att.get_component_type() {
                ComponentDataType::U16 => {
                    seen_ids = true;
                    let vals: Vec<u16> = (0..att.num_unique_values())
                        .map(|i| *att.get_unique_val::<NdVector<1, u16>, 1>(i.into()).get(0))
                        .collect();
                    for i in 0..num_points {
                        assert!(vals.contains(&(40000 + i as u16)), "id {i} missing");
                    }
                }
                ComponentDataType::I32 => {
                    seen_offsets = true;
                    let vals: Vec<i32> = (0..att.num_unique_values())
                        .map(|i| *att.get_unique_val::<NdVector<1, i32>, 1>(i.into()).get(0))
                        .collect();
                    for i in 0..num_points {
                        assert!(vals.contains(&(-1000 + i as i32)), "offset {i} missing");
                    }
                }
                other => panic!("unexpected custom component type {other:?}"),
            },
            _ => {}
        }
    }
    assert!(seen_colors && seen_ids && seen_offsets);
}

#[test]
fn integer_attributes_encode_under_sequential_connectivity() {
    let (mut builder, num_points) = grid_builder();
    let ids: Vec<NdVector<1, u16>> = (0..num_points)
        .map(|i| NdVector::from([i as u16]))
        .collect();
    builder.add_attribute(ids, AttributeType::Custom, AttributeDomain::Corner, vec![]);
    let mesh = builder.build().expect("mesh builds");

    let cfg = <encode::Config as ConfigType>::default()
        .with_sequential(<encode::SequentialConfig as ConfigType>::default());
    let mut buf = Vec::new();
    encode_mesh(mesh, &mut buf, cfg).expect("sequential encode succeeds");

    if let Some(decoder) = find_draco_decoder() {
        let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/integer_attributes");
        std::fs::create_dir_all(&out_dir).unwrap();
        let drc = out_dir.join("grid_sequential.drc");
        std::fs::write(&drc, &buf).unwrap();
        let output = Command::new(&decoder)
            .arg("-i")
            .arg(&drc)
            .arg("-o")
            .arg(out_dir.join("grid_sequential.ply"))
            .output()
            .expect("draco_decoder runs");
        assert!(
            output.status.success(),
            "draco_decoder rejected the sequential integer-attribute stream: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn float_custom_attributes_are_quantized_not_truncated() {
    let (mut builder, num_points) = grid_builder();
    let weights: Vec<NdVector<1, f32>> = (0..num_points)
        .map(|i| NdVector::from([i as f32 / num_points as f32]))
        .collect();
    builder.add_attribute(
        weights,
        AttributeType::Custom,
        AttributeDomain::Corner,
        vec![],
    );
    let mesh = builder.build().expect("mesh builds");

    let mut buf = Vec::new();
    encode_mesh(mesh, &mut buf, <encode::Config as ConfigType>::default())
        .expect("encode succeeds");

    if let Some(decoder) = find_draco_decoder() {
        let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs/integer_attributes");
        std::fs::create_dir_all(&out_dir).unwrap();
        let drc = out_dir.join("grid_float_custom.drc");
        std::fs::write(&drc, &buf).unwrap();
        let output = Command::new(&decoder)
            .arg("-i")
            .arg(&drc)
            .arg("-o")
            .arg(out_dir.join("grid_float_custom.ply"))
            .output()
            .expect("draco_decoder runs");
        assert!(
            output.status.success(),
            "draco_decoder rejected the float generic stream: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let decoded = draco_oxide::decode::decode_mesh(&buf).expect("oxide decodes its own stream");
    let mut checked = false;
    for att in decoded.get_attributes() {
        if att.get_attribute_type() == AttributeType::Custom && att.get_num_components() == 1 {
            let vals: Vec<f32> = (0..att.num_unique_values())
                .map(|i| *att.get_unique_val::<NdVector<1, f32>, 1>(i.into()).get(0))
                .collect();
            let tolerance = 1.0 / (1 << 11) as f32;
            for i in 0..num_points {
                let expect = i as f32 / num_points as f32;
                assert!(
                    vals.iter().any(|v| (v - expect).abs() < tolerance),
                    "weight {expect} missing from {vals:?}"
                );
            }
            checked = true;
        }
    }
    assert!(checked, "float custom attribute missing");
}

#[test]
fn sixty_four_bit_integers_are_rejected_cleanly() {
    let (mut builder, num_points) = grid_builder();
    let wide: Vec<NdVector<1, u64>> = (0..num_points)
        .map(|i| NdVector::from([i as u64]))
        .collect();
    builder.add_attribute(wide, AttributeType::Custom, AttributeDomain::Corner, vec![]);
    let mesh = builder.build().expect("mesh builds");

    let mut buf = Vec::new();
    let result = encode_mesh(mesh, &mut buf, <encode::Config as ConfigType>::default());
    assert!(
        result.is_err(),
        "64-bit integers must be rejected, not truncated"
    );
}
