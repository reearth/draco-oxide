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

	let mut abs_sum = x.abs() + y.abs() + z.abs();
	if abs_sum == Data::Component::zero() {
		abs_sum = Data::Component::one();
	}

	let mut u = *x / abs_sum;
	let mut v = *y / abs_sum;

	let lies_in_upper_half = *z > Data::Component::zero();
	if !lies_in_upper_half {
		let one = Data::Component::one();
		let minus_one = Data::Component::zero() - one;
		let temp_u = u;
		let temp_u_sign = if temp_u > Data::Component::zero() {
			one
		} else {
			minus_one
		};
		let temp_v = v;
		let temp_v_sign = if temp_v > Data::Component::zero() {
			one
		} else {
			minus_one
		};
		u = (Data::Component::one() - temp_v.abs()) * temp_u_sign;
		v = (Data::Component::one() - temp_u.abs()) * temp_v_sign;
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

	let mut abs_sum = u.abs() + v.abs();
	if abs_sum == 0.0 {
		abs_sum = 1.0;
	}

	let mut x = *u / abs_sum;
	let mut y = *v / abs_sum;
	let mut z = 0.0;

	if u+v > 1.0 {
		let temp_x = x;
		let temp_x_sign = if temp_x > 0.0 {
			1.0
		} else {
			-1.0
		};
		let temp_y = y;
		let temp_y_sign = if temp_y > 0.0 {
			1.0
		} else {
			-1.0
		};
		x = (1.0 - temp_y.abs()) * temp_x_sign;
		y = (1.0 - temp_x.abs()) * temp_y_sign;
		z = (1.0 - u.abs() - v.abs()) * if u+v<=1.0 { 1.0 } else { -1.0 };
	}

	let mut out = Data::zero();
	// safety condition is upheld
	*out.get_unchecked_mut(0) = Data::Component::from_f64(x);
	*out.get_unchecked_mut(1) = Data::Component::from_f64(y);
	*out.get_unchecked_mut(2) = Data::Component::from_f64(z);

	out
}