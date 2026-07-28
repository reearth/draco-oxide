//! Phase-3 reconstruction (`dequantize` feature): integer attributes back to
//! their original format, and octahedral values back to unit normals. Mirrors
//! Google's `TransformAttributesToOriginalFormat`.

use crate::{AttributeTransform, Err};
use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::codec::attribute::geom::octahedral_inverse_transform;
use draco_oxide_core::types::{AttributeValueIdx, NdVector, Vector};

/// Applies `transform` to the portable integer attribute `att`, returning the
/// attribute in its original (float) format. `AttributeTransform::None`
/// attributes are passed through unchanged.
pub(crate) fn dequantize_attribute(
    att: Attribute,
    transform: &AttributeTransform,
) -> Result<Attribute, Err> {
    match transform {
        AttributeTransform::None => Ok(att),
        AttributeTransform::Quantized {
            min,
            delta_max,
            bits,
        } => match att.get_num_components() {
            1 => dequantize_typed::<1>(att, min, *delta_max, *bits),
            2 => dequantize_typed::<2>(att, min, *delta_max, *bits),
            3 => dequantize_typed::<3>(att, min, *delta_max, *bits),
            4 => dequantize_typed::<4>(att, min, *delta_max, *bits),
            _ => Err(Err::MalformedAttribute("unsupported number of components")),
        },
        AttributeTransform::Octahedral { bits } => dequantize_octahedral(att, *bits),
    }
}

/// Inverse of the encoder's coordinate-wise quantization:
/// `value = min + q * delta_max / (2^bits - 1)`.
fn dequantize_typed<const N: usize>(
    att: Attribute,
    min: &[f32],
    delta_max: f32,
    bits: u8,
) -> Result<Attribute, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
    NdVector<N, f32>: Vector<N, Component = f32>,
{
    let max_quantized = ((1u64 << bits) - 1) as f32;
    let step = if max_quantized > 0.0 {
        delta_max / max_quantized
    } else {
        0.0
    };

    let mut values = Vec::with_capacity(att.num_unique_values());
    for i in 0..att.num_unique_values() {
        let q: NdVector<N, i32> = att.get_unique_val(AttributeValueIdx::from(i));
        let mut v = NdVector::<N, f32>::zero();
        for (j, &min_j) in min.iter().enumerate().take(N) {
            *v.get_mut(j) = min_j + *q.get(j) as f32 * step;
        }
        values.push(v);
    }

    Ok(rebuild(att, values))
}

/// Octahedral integers back to unit normals: undo the `[0, 2^bits - 1]` scaling
/// into the `[-1, 1]` octahedron square, then invert the octahedral mapping.
fn dequantize_octahedral(att: Attribute, bits: u8) -> Result<Attribute, Err> {
    if att.get_num_components() != 2 {
        return Err(Err::MalformedAttribute(
            "octahedral attribute must have 2 portable components",
        ));
    }
    let scale = ((1u64 << (bits - 1)) - 1) as f32;

    let mut values = Vec::with_capacity(att.num_unique_values());
    for i in 0..att.num_unique_values() {
        let q: NdVector<2, i32> = att.get_unique_val(AttributeValueIdx::from(i));
        let oct = NdVector::<2, f32>::from([
            *q.get(0) as f32 / scale - 1.0,
            *q.get(1) as f32 / scale - 1.0,
        ]);
        // SAFETY: the output type is three dimensional.
        let normal: NdVector<3, f32> = unsafe { octahedral_inverse_transform(oct) };
        values.push(normal);
    }

    Ok(rebuild(att, values))
}

/// Rebuilds `att` with `values` as its unique values, keeping identity, type,
/// and the point-to-value map.
fn rebuild<Data, const N: usize>(att: Attribute, values: Vec<Data>) -> Attribute
where
    Data: Vector<N>,
    Data::Component: draco_oxide_core::types::DataValue,
{
    let id = att.get_id();
    let ty = att.get_attribute_type();
    let domain = att.get_domain();
    let parents = att.get_parents().clone();
    let map = att.take_point_to_att_val_map();
    let mut out = Attribute::from_without_removing_duplicates(id, values, ty, domain, parents);
    out.set_point_to_att_val_map(map);
    out
}
