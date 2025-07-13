use crate::core::shared::{DataValue, NdVector, Vector};

pub(super) fn rotation_matrix_from<Data>(axis: Data, angle: f64) -> [Data; 3] 
    where
        Data: Vector,
        Data::Component: DataValue
{
    let cos_angle = Data::Component::from_f64(angle.cos());
    let sin_angle = Data::Component::from_f64(angle.sin());
    let one_minus_cos = Data::Component::one() - cos_angle;
    let mut r1  = Data::zero();
    let mut r2  = Data::zero();
    let mut r3  = Data::zero();
    unsafe {
        *r1.get_unchecked_mut(0) = cos_angle + *axis.get_unchecked(0) * *axis.get_unchecked(0) * one_minus_cos;
        *r1.get_unchecked_mut(1) = *axis.get_unchecked(0) * *axis.get_unchecked(1) * one_minus_cos - *axis.get_unchecked(2);
        *r1.get_unchecked_mut(2) = *axis.get_unchecked(0) * *axis.get_unchecked(2) * one_minus_cos + *axis.get_unchecked(1);

        *r2.get_unchecked_mut(0) = *axis.get_unchecked(1) * *axis.get_unchecked(0) * one_minus_cos + *axis.get_unchecked(2) * sin_angle;
        *r2.get_unchecked_mut(1) = cos_angle + *axis.get_unchecked(1) * *axis.get_unchecked(1) * one_minus_cos;
        *r2.get_unchecked_mut(2) = *axis.get_unchecked(1) * *axis.get_unchecked(2) * one_minus_cos - *axis.get_unchecked(0) * sin_angle;

        *r3.get_unchecked_mut(0) = *axis.get_unchecked(2) * *axis.get_unchecked(0) * one_minus_cos - *axis.get_unchecked(1) * sin_angle;
        *r3.get_unchecked_mut(1) = *axis.get_unchecked(2) * *axis.get_unchecked(1) * one_minus_cos + *axis.get_unchecked(0) * sin_angle;
        *r3.get_unchecked_mut(2) = cos_angle + *axis.get_unchecked(2) * *axis.get_unchecked(2) * one_minus_cos;
    };
    [
        r1,
        r2,
        r3,
    ]
}



use crate::core::shared::Abs;
/// Transforms the data to the octahedron space.
/// Make sure that the data is three dimensional.
pub(super) unsafe fn octahedral_transform<Data>(v: Data) -> NdVector<2, f64>
	where 
		Data: Vector,
		Data::Component: DataValue
{
	let x = v.get_unchecked(0);
	let y = v.get_unchecked(1);
	let z = v.get_unchecked(2);

	let abs_sum = x.abs() + y.abs() + z.abs();

	let mut u = *x / abs_sum;
	let mut v = *y / abs_sum;

	if *z < Data::Component::zero() {
		let one = Data::Component::one();
		let minus_one = Data::Component::zero() - one;
		let u_sign = if u > Data::Component::zero() {
			one
		} else {
			minus_one
		};
		let v_sign = if v > Data::Component::zero() {
			one
		} else {
			minus_one
		};
		(u, v) = (
			(Data::Component::one() - v.abs()) * u_sign,
			(Data::Component::one() - u.abs()) * v_sign
		);
	}

	let mut out = NdVector::<2, _>::zero();
	unsafe {
		*out.get_unchecked_mut(0) = u.to_f64();
		*out.get_unchecked_mut(1) = v.to_f64();
	}

	out
}


/// Data is transformed back from the octahedron space.
/// Safety:
/// 'Data' must be three dimensional.
pub(super) unsafe fn octahedral_inverse_transform<Data>(v: NdVector<2, f64>) -> Data 
	where 
		Data: Vector,
		Data::Component: DataValue
{
	let u = v.get_unchecked(0);
	let v = v.get_unchecked(1);

	let mut x = *u;
	let mut y = *v;
	let z = 1.0 - u.abs() - v.abs();

	if u.abs()+v.abs() > 1.0 {
		let x_sign = if x > 0.0 {
			1.0
		} else {
			-1.0
		};
		let y_sign = if y > 0.0 {
			1.0
		} else {
			-1.0
		};
		x = (1.0 - v.abs()) * x_sign;
		y = (1.0 - u.abs()) * y_sign;
	}

	// normalize the vector
	let norm = (x*x + y*y + z*z).sqrt();

	let mut out = Data::zero();
	// safety condition is upheld
	*out.get_unchecked_mut(0) = Data::Component::from_f64(x/norm);
	*out.get_unchecked_mut(1) = Data::Component::from_f64(y/norm);
	*out.get_unchecked_mut(2) = Data::Component::from_f64(z/norm);

	out
}


#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::shared::NdVector;
	use crate::core::shared::Dot;

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
			let transformed = unsafe { octahedral_transform(n) };
			let recovered = unsafe { octahedral_inverse_transform(transformed) };
			let diff = n - recovered;
			let diff_norm_squared = diff.dot(diff);
			assert!(diff_norm_squared < 1e-10, "Difference is too large: {}, v={:?}, recovered={:?}", diff_norm_squared, v, recovered);
		}
	}
}