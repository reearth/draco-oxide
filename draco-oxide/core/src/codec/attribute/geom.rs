use crate::types::{DataValue, NdVector, Vector};

#[allow(unused)]
pub fn rotation_matrix_from<Data, const N: usize>(axis: Data, angle: f64) -> [Data; 3]
where
    Data: Vector<N>,
    Data::Component: DataValue,
{
    let cos_angle = Data::Component::from_f64(angle.cos());
    let sin_angle = Data::Component::from_f64(angle.sin());
    let one_minus_cos = Data::Component::one() - cos_angle;
    let mut r1 = Data::zero();
    let mut r2 = Data::zero();
    let mut r3 = Data::zero();
    unsafe {
        *r1.get_unchecked_mut(0) =
            cos_angle + *axis.get_unchecked(0) * *axis.get_unchecked(0) * one_minus_cos;
        *r1.get_unchecked_mut(1) = *axis.get_unchecked(0) * *axis.get_unchecked(1) * one_minus_cos
            - *axis.get_unchecked(2);
        *r1.get_unchecked_mut(2) = *axis.get_unchecked(0) * *axis.get_unchecked(2) * one_minus_cos
            + *axis.get_unchecked(1);

        *r2.get_unchecked_mut(0) = *axis.get_unchecked(1) * *axis.get_unchecked(0) * one_minus_cos
            + *axis.get_unchecked(2) * sin_angle;
        *r2.get_unchecked_mut(1) =
            cos_angle + *axis.get_unchecked(1) * *axis.get_unchecked(1) * one_minus_cos;
        *r2.get_unchecked_mut(2) = *axis.get_unchecked(1) * *axis.get_unchecked(2) * one_minus_cos
            - *axis.get_unchecked(0) * sin_angle;

        *r3.get_unchecked_mut(0) = *axis.get_unchecked(2) * *axis.get_unchecked(0) * one_minus_cos
            - *axis.get_unchecked(1) * sin_angle;
        *r3.get_unchecked_mut(1) = *axis.get_unchecked(2) * *axis.get_unchecked(1) * one_minus_cos
            + *axis.get_unchecked(0) * sin_angle;
        *r3.get_unchecked_mut(2) =
            cos_angle + *axis.get_unchecked(2) * *axis.get_unchecked(2) * one_minus_cos;
    };
    [r1, r2, r3]
}

use crate::types::Abs;
/// Transforms the data to the octahedron space.
/// Make sure that the data is three dimensional.
pub fn octahedral_transform<const N: usize, Data>(v: Data) -> NdVector<2, f32>
where
    Data: Vector<N>,
    Data::Component: DataValue,
{
    assert!(N == 3);
    assert!(
        v != Data::zero(),
        "Zero vector cannot be transformed to octahedron space as it is not a unit vector."
    );
    if !Data::Component::get_dyn().is_float() {
        let mut float_v = NdVector::<3, f32>::zero();
        unsafe {
            *float_v.get_unchecked_mut(0) = v.get_unchecked(0).to_f64() as f32;
            *float_v.get_unchecked_mut(1) = v.get_unchecked(1).to_f64() as f32;
            *float_v.get_unchecked_mut(2) = v.get_unchecked(2).to_f64() as f32;
        }
        float_v.normalize();
        return octahedral_transform(float_v);
    }
    let x = unsafe { v.get_unchecked(0) };
    let y = unsafe { v.get_unchecked(1) };
    let z = unsafe { v.get_unchecked(2) };

    // abs_sum is guaranteed to be a non-zero vector as we checked above.
    let abs_sum = x.abs() + y.abs() + z.abs();

    let mut u = *y / abs_sum;
    let mut v = *z / abs_sum;

    if *x < Data::Component::zero() {
        let one = Data::Component::one();
        let u_out = if u < Data::Component::zero() {
            v.abs() - one
        } else {
            one - v.abs()
        };
        let v_out = if v < Data::Component::zero() {
            u.abs() - one
        } else {
            one - u.abs()
        };
        (u, v) = (u_out, v_out);
    }

    let mut out = NdVector::<2, _>::zero();
    unsafe {
        *out.get_unchecked_mut(0) = u.to_f64() as f32;
        *out.get_unchecked_mut(1) = v.to_f64() as f32;
    }

    out
}

/// Data is transformed back from the octahedron space.
/// # Safety
/// 'Data' must be three dimensional.
#[allow(unused)]
pub unsafe fn octahedral_inverse_transform<Data, const N: usize>(v: NdVector<2, f32>) -> Data
where
    Data: Vector<N>,
    Data::Component: DataValue,
{
    let u = v.get_unchecked(0);
    let v = v.get_unchecked(1);

    let x = 1.0 - u.abs() - v.abs();
    let mut y = *u;
    let mut z = *v;

    if u.abs() + v.abs() > 1.0 {
        let y_sign = if y > 0.0 { 1.0 } else { -1.0 };
        let z_sign = if z > 0.0 { 1.0 } else { -1.0 };
        y = (1.0 - v.abs()) * y_sign;
        z = (1.0 - u.abs()) * z_sign;
    }

    // normalize the vector
    let norm = (x * x + y * y + z * z).sqrt();

    let mut out = Data::zero();
    // safety condition is upheld
    *out.get_unchecked_mut(0) = Data::Component::from_f64((x / norm) as f64);
    *out.get_unchecked_mut(1) = Data::Component::from_f64((y / norm) as f64);
    *out.get_unchecked_mut(2) = Data::Component::from_f64((z / norm) as f64);

    out
}

/// The octahedron inside-out flip on centered quantized coordinates, matching
/// the reference implementation's `OctahedronToolBox::InvertDiamond` exactly.
/// It is an involution on the valid range `[-center, center]^2`, which the
/// decoder relies on to undo the encoder-side flip.
pub fn invert_diamond(v: &mut NdVector<2, i32>, center: i32) {
    let s = *v.get(0);
    let t = *v.get(1);
    let (sign_s, sign_t) = if s >= 0 && t >= 0 {
        (1, 1)
    } else if s <= 0 && t <= 0 {
        (-1, -1)
    } else {
        (if s > 0 { 1 } else { -1 }, if t > 0 { 1 } else { -1 })
    };

    let corner_s = sign_s * center;
    let corner_t = sign_t * center;
    let mut us = 2 * s - corner_s;
    let mut ut = 2 * t - corner_t;
    if sign_s * sign_t >= 0 {
        let tmp = us;
        us = -ut;
        ut = -tmp;
    } else {
        std::mem::swap(&mut us, &mut ut);
    }
    // `us`/`ut` are even here (2x minus an odd corner, negated/swapped, plus the
    // odd corner back), so the halving is exact.
    *v.get_mut(0) = (us + corner_s) / 2;
    *v.get_mut(1) = (ut + corner_t) / 2;
}

/// Maps octahedron-square boundary duplicates to one canonical representative
/// per normal. The quantized range is `[0, max]` with `max = 2 * (2^(bits-1) - 1)`
/// (254 for 8 bits); the four square corners all encode the -x pole and
/// canonicalize to `(max, max)`, the only corner representation that survives
/// the octahedral prediction transform round trip unchanged.
pub fn into_faithful_oct_quantization(
    vec: NdVector<2, i32>,
    quantization_bits: u8,
) -> NdVector<2, i32> {
    let max = 2 * ((1 << (quantization_bits - 1)) - 1);
    let half = max / 2;
    let u = *vec.get(0);
    let v = *vec.get(1);
    let mut x = u;
    let mut y = v;
    if ((u == max || u == 0) && v == 0) || (u == 0 && v == max) {
        return NdVector::<2, i32>::from([max, max]);
    } else if u == 0 && v > half {
        y = half - (v - half)
    } else if u == max && v < half {
        y = half + (half - v);
    } else if v == max && u < half {
        x = half + (half - u);
    } else if v == 0 && u > half {
        x = half - (u - half);
    }
    NdVector::<2, i32>::from([x, y])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Dot;
    use crate::types::NdVector;

    #[test]
    fn test_octahedral_transform() {
        let vs = {
            vec![
                NdVector::from([1_f64, 0.0, 0.0]),
                NdVector::from([0.0, 1.0, 0.0]),
                NdVector::from([0.0, 0.0, 1.0]),
                NdVector::from([-1.0, 0.0, 0.0]),
                NdVector::from([0.0, -1.0, 0.0]),
                NdVector::from([0.0, 0.0, -1.0]),
                NdVector::from([1.0, 1.0, 1.0]),
                NdVector::from([-1.0, -1.0, -1.0]),
                NdVector::from([1.0, -1.0, 1.0]),
                NdVector::from([-1.0, 1.0, -1.0]),
                NdVector::from([1.0, 1.0, -1.0]),
                NdVector::from([-1.0, -1.0, 1.0]),
                NdVector::from([1.0, -1.0, -1.0]),
            ]
        };
        for v in vs {
            // normalize the vector
            let n = v / v.dot(v).sqrt();
            // Safety:
            // inputs are three dimensional
            let transformed = octahedral_transform(n);
            let recovered = unsafe { octahedral_inverse_transform(transformed) };
            let diff = n - recovered;
            let diff_norm_squared = diff.dot(diff);
            assert!(
                diff_norm_squared < 1e-10,
                "Difference is too large: {}, v={:?}, recovered={:?}",
                diff_norm_squared,
                v,
                recovered
            );
        }
    }
}
