//! Phase-3 reconstruction: integer attributes back to their original format.

use crate::{AttributeTransform, Err};
use draco_oxide_core::attribute::{Attribute, ComponentDataType};
use draco_oxide_core::codec::attribute::geom::octahedral_inverse_transform;
use draco_oxide_core::types::{NdVector, Vector};

/// Applies `transform` to the portable integer attribute.
pub(crate) fn dequantize_attribute(
    att: Attribute,
    transform: &AttributeTransform,
) -> Result<Attribute, Err> {
    match transform {
        AttributeTransform::Integer { component_type } => narrow_integer(att, *component_type),
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
        AttributeTransform::Raw { component_type } => match component_type {
            ComponentDataType::F32 => match att.get_num_components() {
                1 => reinterpret_f32::<1>(att),
                2 => reinterpret_f32::<2>(att),
                3 => reinterpret_f32::<3>(att),
                4 => reinterpret_f32::<4>(att),
                _ => Err(Err::MalformedAttribute("unsupported number of components")),
            },
            _ => narrow_integer(att, *component_type),
        },
    }
}

/// Narrows the portable i32 values back to the declared component type, the
/// counterpart of the encoder's widening. Declarations without a narrower form
/// (i32 itself, 64-bit and float types) pass through untouched.
fn narrow_integer(att: Attribute, component_type: ComponentDataType) -> Result<Attribute, Err> {
    match att.get_num_components() {
        1 => Ok(narrow_typed::<1>(att, component_type)),
        2 => Ok(narrow_typed::<2>(att, component_type)),
        3 => Ok(narrow_typed::<3>(att, component_type)),
        4 => Ok(narrow_typed::<4>(att, component_type)),
        _ => Err(Err::MalformedAttribute("unsupported number of components")),
    }
}

fn narrow_typed<const N: usize>(att: Attribute, component_type: ComponentDataType) -> Attribute
where
    NdVector<N, i32>: Vector<N, Component = i32>,
    NdVector<N, i8>: Vector<N, Component = i8>,
    NdVector<N, u8>: Vector<N, Component = u8>,
    NdVector<N, i16>: Vector<N, Component = i16>,
    NdVector<N, u16>: Vector<N, Component = u16>,
    NdVector<N, u32>: Vector<N, Component = u32>,
{
    match component_type {
        ComponentDataType::I8 => narrow_to::<i8, N>(att),
        ComponentDataType::U8 => narrow_to::<u8, N>(att),
        ComponentDataType::I16 => narrow_to::<i16, N>(att),
        ComponentDataType::U16 => narrow_to::<u16, N>(att),
        ComponentDataType::U32 => narrow_to::<u32, N>(att),
        _ => att,
    }
}

/// The per-component cast from the portable i32, bit-truncating like the
/// reference's `StoreTypedValues`.
fn narrow_to<T, const N: usize>(att: Attribute) -> Attribute
where
    T: draco_oxide_core::types::DataValue + Copy,
    NdVector<N, T>: Vector<N, Component = T>,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let vals = att.unique_vals_as_slice::<NdVector<N, i32>>();
    let values: Vec<NdVector<N, T>> = vals
        .iter()
        .map(|q| {
            let mut v = NdVector::<N, T>::zero();
            for j in 0..N {
                *v.get_mut(j) = T::from_i64(*q.get(j) as i64);
            }
            v
        })
        .collect();
    rebuild(att, values)
}

/// Reads back the f32 values a generic attribute carried as bit patterns.
fn reinterpret_f32<const N: usize>(att: Attribute) -> Result<Attribute, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
    NdVector<N, f32>: Vector<N, Component = f32>,
{
    let vals = att.unique_vals_as_slice::<NdVector<N, i32>>();
    let values: Vec<NdVector<N, f32>> = vals
        .iter()
        .map(|bits| {
            let mut v = NdVector::<N, f32>::zero();
            for j in 0..N {
                *v.get_mut(j) = f32::from_bits(*bits.get(j) as u32);
            }
            v
        })
        .collect();
    Ok(rebuild(att, values))
}

/// Inverse quantization: `value = min + q * delta_max / (2^bits - 1)`.
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
    let mut min_arr = [0.0f32; N];
    for (dst, &src) in min_arr.iter_mut().zip(min.iter().take(N)) {
        *dst = src;
    }

    let vals = att.unique_vals_as_slice::<NdVector<N, i32>>();
    let values: Vec<NdVector<N, f32>> = vals
        .iter()
        .map(|q| {
            let mut v = NdVector::<N, f32>::zero();
            for (j, &min_j) in min_arr.iter().enumerate() {
                *v.get_mut(j) = min_j + *q.get(j) as f32 * step;
            }
            v
        })
        .collect();

    Ok(rebuild(att, values))
}

/// Octahedral integers back to unit normals.
fn dequantize_octahedral(att: Attribute, bits: u8) -> Result<Attribute, Err> {
    if att.get_num_components() != 2 {
        return Err(Err::MalformedAttribute(
            "octahedral attribute must have 2 portable components",
        ));
    }
    let scale = ((1u64 << (bits - 1)) - 1) as f32;

    let vals = att.unique_vals_as_slice::<NdVector<2, i32>>();
    let values: Vec<NdVector<3, f32>> = vals
        .iter()
        .map(|q| {
            let oct = NdVector::<2, f32>::from([
                *q.get(0) as f32 / scale - 1.0,
                *q.get(1) as f32 / scale - 1.0,
            ]);
            // SAFETY: the output type is three dimensional.
            unsafe { octahedral_inverse_transform(oct) }
        })
        .collect();

    Ok(rebuild(att, values))
}

/// Rebuilds `att` with `values`, keeping identity, type, and point map.
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
