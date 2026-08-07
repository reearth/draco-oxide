//! Point-cloud stream decoding (bitstream 2.3, kd-tree method).

mod bit_decoder;
mod kd_tree;

use crate::entropy::unzigzag;
use crate::{metadata, AttributeTransform, Err};
use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::point_cloud::PointCloud;
use draco_oxide_core::types::{NdVector, Vector};
use draco_oxide_core::utils::bit_coder::leb128_read;

const METADATA_FLAG_MASK: u16 = 0x8000;
const METHOD_SEQUENTIAL: u8 = 0;
const METHOD_KD_TREE: u8 = 1;

struct Descriptor {
    att_type: AttributeType,
    component_type: ComponentDataType,
    num_components: usize,
}

/// Decodes a point-cloud stream into portable integer attributes and the
/// transform each one needs to reach its original format.
pub(crate) fn decode_portable(bytes: &[u8]) -> Result<(PointCloud, Vec<AttributeTransform>), Err> {
    let mut reader = Reader::new(bytes);

    let mut magic = [0u8; 5];
    for b in &mut magic {
        *b = reader.read_u8()?;
    }
    if &magic != b"DRACO" {
        return Err(Err::InvalidHeader("missing DRACO magic"));
    }
    let major = reader.read_u8()?;
    let minor = reader.read_u8()?;
    let geometry_type = reader.read_u8()?;
    if geometry_type != 0 {
        return Err(Err::InvalidHeader("not a point-cloud stream"));
    }
    if (major, minor) != (2, 3) {
        return Err(Err::UnsupportedVersion(major, minor));
    }
    let method = reader.read_u8()?;
    match method {
        METHOD_KD_TREE => {}
        METHOD_SEQUENTIAL => return Err(Err::Unimplemented),
        _ => return Err(Err::InvalidHeader("unknown encoder method")),
    }
    let flags = reader.read_u16()?;
    if flags & METADATA_FLAG_MASK != 0 {
        metadata::decode_metadata(&mut reader)?;
    }

    let num_points = reader.read_u32()?;
    if num_points > i32::MAX as u32 {
        return Err(Err::MalformedAttribute("negative point count"));
    }
    let num_points = num_points as usize;

    if reader.read_u8()? != 1 {
        return Err(Err::MalformedAttribute(
            "a kd-tree stream carries exactly one attributes decoder",
        ));
    }

    let num_attributes = leb128_read(&mut reader)? as usize;
    if num_attributes == 0 {
        return Err(Err::MalformedAttribute("no attributes"));
    }
    // A descriptor is never shorter than five bytes, so the input bounds the
    // count before it sizes the kd-tree dimension.
    if num_attributes > reader.remaining() / 5 {
        return Err(Err::MalformedAttribute(
            "attribute count exceeds the stream",
        ));
    }
    let mut descriptors = Vec::with_capacity(num_attributes);
    let mut dimension = 0usize;
    for _ in 0..num_attributes {
        let att_type = AttributeType::read_from(&mut reader)
            .map_err(|_| Err::MalformedAttribute("unknown attribute type"))?;
        let component_type = ComponentDataType::read_from(&mut reader)
            .map_err(|_| Err::MalformedAttribute("unknown component type"))?;
        crate::check_component_type(component_type)?;
        let num_components = reader.read_u8()? as usize;
        if !(1..=4).contains(&num_components) {
            return Err(Err::MalformedAttribute("unsupported number of components"));
        }
        let _normalized = reader.read_u8()?;
        let _unique_id = leb128_read(&mut reader)?;
        dimension += num_components;
        descriptors.push(Descriptor {
            att_type,
            component_type,
            num_components,
        });
    }

    let compression_level = reader.read_u8()?;
    let flat = kd_tree::decode_points(&mut reader, dimension, num_points, compression_level)?;

    let mut transforms = Vec::with_capacity(num_attributes);
    for desc in &descriptors {
        match desc.component_type {
            ComponentDataType::F32 => {
                let mut min = Vec::with_capacity(desc.num_components);
                for _ in 0..desc.num_components {
                    min.push(f32::from_bits(reader.read_u32()?));
                }
                let delta_max = f32::from_bits(reader.read_u32()?);
                let bits = reader.read_u8()?;
                if !(1..=31).contains(&bits) {
                    return Err(Err::MalformedAttribute("quantization bits out of range"));
                }
                transforms.push(AttributeTransform::Quantized {
                    min,
                    delta_max,
                    bits,
                });
            }
            ComponentDataType::U8
            | ComponentDataType::U16
            | ComponentDataType::U32
            | ComponentDataType::I8
            | ComponentDataType::I16
            | ComponentDataType::I32 => transforms.push(AttributeTransform::Integer {
                component_type: desc.component_type,
            }),
            _ => {
                return Err(Err::MalformedAttribute(
                    "unsupported component type in a kd-tree stream",
                ))
            }
        }
    }

    // The kd-tree carries signed attributes shifted to unsigned; the shifts
    // trail the quantization parameters.
    let mut shifts: Vec<Vec<i32>> = vec![Vec::new(); num_attributes];
    for (desc, shift) in descriptors.iter().zip(&mut shifts) {
        if signed_bounds(desc.component_type).is_some() {
            for _ in 0..desc.num_components {
                shift.push(unzigzag(leb128_read(&mut reader)? as u32));
            }
        }
    }

    let mut attributes = Vec::with_capacity(num_attributes);
    let mut offset = 0usize;
    for (i, (desc, shift)) in descriptors.iter().zip(&shifts).enumerate() {
        attributes.push(build_attribute(
            i, desc, shift, &flat, dimension, offset, num_points,
        )?);
        offset += desc.num_components;
    }

    let point_cloud = PointCloud::new(attributes)
        .map_err(|_| Err::MalformedAttribute("inconsistent attributes"))?;
    Ok((point_cloud, transforms))
}

/// Decodes a point-cloud stream and applies every attribute transform.
#[cfg(feature = "dequantize")]
pub(crate) fn decode(bytes: &[u8]) -> Result<PointCloud, Err> {
    let (point_cloud, transforms) = decode_portable(bytes)?;
    let attributes = point_cloud
        .into_attributes()
        .into_iter()
        .zip(&transforms)
        .map(|(att, transform)| crate::attribute::dequantize::dequantize_attribute(att, transform))
        .collect::<Result<Vec<_>, _>>()?;
    PointCloud::new(attributes).map_err(|_| Err::MalformedAttribute("inconsistent attributes"))
}

/// The inclusive range of a signed component type, or `None` if the type is not
/// one the kd-tree shifts.
fn signed_bounds(component_type: ComponentDataType) -> Option<(i64, i64)> {
    match component_type {
        ComponentDataType::I8 => Some((i8::MIN as i64, i8::MAX as i64)),
        ComponentDataType::I16 => Some((i16::MIN as i64, i16::MAX as i64)),
        ComponentDataType::I32 => Some((i32::MIN as i64, i32::MAX as i64)),
        _ => None,
    }
}

/// Extracts one attribute's columns as portable i32 values.
fn build_attribute(
    index: usize,
    desc: &Descriptor,
    shift: &[i32],
    flat: &[u32],
    dimension: usize,
    offset: usize,
    num_points: usize,
) -> Result<Attribute, Err> {
    match desc.num_components {
        1 => build_attribute_typed::<1>(index, desc, shift, flat, dimension, offset, num_points),
        2 => build_attribute_typed::<2>(index, desc, shift, flat, dimension, offset, num_points),
        3 => build_attribute_typed::<3>(index, desc, shift, flat, dimension, offset, num_points),
        4 => build_attribute_typed::<4>(index, desc, shift, flat, dimension, offset, num_points),
        _ => Err(Err::MalformedAttribute("unsupported number of components")),
    }
}

fn build_attribute_typed<const N: usize>(
    index: usize,
    desc: &Descriptor,
    shift: &[i32],
    flat: &[u32],
    dimension: usize,
    offset: usize,
    num_points: usize,
) -> Result<Attribute, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let bounds = signed_bounds(desc.component_type);
    let mut values = Vec::with_capacity(num_points);
    for p in 0..num_points {
        let mut v = NdVector::<N, i32>::zero();
        for c in 0..N {
            let raw = flat[p * dimension + offset + c];
            *v.get_mut(c) = match bounds {
                // Checked in i64 so the narrowing below cannot overflow; both
                // the value and the shift come from the stream.
                Some((lo, hi)) => {
                    let value = raw as i64 + shift[c] as i64;
                    if value < lo || value > hi {
                        return Err(Err::MalformedAttribute(
                            "signed attribute value out of range for its component type",
                        ));
                    }
                    value as i32
                }
                // Unsigned and quantized values ride the portable i32 as their
                // bit pattern; the transform narrows them back.
                None => raw as i32,
            };
        }
        values.push(v);
    }
    Ok(Attribute::from_without_removing_duplicates::<_, N>(
        AttributeId::new(index),
        values,
        desc.att_type,
        AttributeDomain::Position,
        Vec::new(),
    ))
}
