use crate::types::{NdVector, Vector};

pub mod bit_coder;
pub mod debug;
pub mod geom;

pub fn to_positive_i32(val: i32) -> i32 {
    if val >= 0 {
        val << 1
    } else {
        (-(val + 1) << 1) + 1
    }
}

pub fn to_positive_i32_vec<const N: usize>(mut vec: NdVector<N, i32>) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    for i in 0..N {
        *vec.get_mut(i) = to_positive_i32(*vec.get(i));
    }
    vec
}
