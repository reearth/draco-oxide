use nd_vector::impl_ndvector_ops;

use super::attribute::ComponentDataType;

use std::{
    ops,
    cmp
};

pub type VertexIdx = usize;
pub type EdgeIdx = usize;
pub type FaceIdx = usize;


pub trait Float: DataValue + ops::Div<Output=Self> {
    type Bits: DataValue;
    fn from_bits(bits: Self::Bits)-> Self;
    fn to_bits(self)-> Self::Bits;
}

impl Float for f32 {
    type Bits = u32;
    fn from_bits(bits: Self::Bits)-> Self {
        Self::from_bits(bits)
    }
    fn to_bits(self)-> Self::Bits {
        self.to_bits()
    }
}

impl Float for f64 {
    type Bits = u64;
    fn from_bits(bits: Self::Bits)-> Self {
        Self::from_bits(bits)
    }
    fn to_bits(self)-> Self::Bits {
        self.to_bits()
    }
}

pub trait ConfigType {
    fn default()-> Self;
}

pub trait ToUsize {
    fn to_usize(self)-> usize;
}

macro_rules! impl_to_usize_float {
    ($($t:ty),*) => {
        $(
            impl ToUsize for $t {
                fn to_usize(self)-> usize {
                    self.to_bits() as usize
                }
            }
        )*
    };
}

impl_to_usize_float!(f32,f64);

macro_rules! impl_to_usize_float {
    ($($t:ty),*) => {
        $(
            impl ToUsize for $t {
                fn to_usize(self)-> usize {
                    self as usize
                }
            }
        )*
    };
}

impl_to_usize_float!(u8, u16, u32, u64);

pub trait DataValue: 
    Clone + Copy + PartialEq + PartialOrd
    + ops::Add<Output=Self> + ops::Sub<Output=Self> + ops::Mul<Output=Self> 
    + ops::AddAssign + ops::SubAssign + ops::MulAssign
{
    fn get_dyn() -> ComponentDataType;
    fn zero() -> Self;
    fn one() -> Self;
    fn from_u64(data: u64) -> Self;
    fn to_u64(self) -> u64;
}

macro_rules! impl_data_value {
    ($(($t:ty, $component_type: expr)),*) => {
        $(
            impl DataValue for $t {
                fn get_dyn() -> ComponentDataType {
                    $component_type
                }
                fn zero() -> Self {
                    0 as $t
                }

                fn one() -> Self {
                    1 as $t
                }

                fn from_u64(data: u64) -> Self {
                    data as $t
                }

                fn to_u64(self) -> u64 {
                    self as u64
                }
            }
        )*
    };
}

impl_data_value!(
    (u8, ComponentDataType::U8),
    (u16, ComponentDataType::U16),
    (u32, ComponentDataType::U32),
    (u64, ComponentDataType::U64),
    (f32, ComponentDataType::F32),
    (f64, ComponentDataType::F64)
);


#[derive(Clone, Copy)]
pub struct NdVector<const N: usize, T> {
    data: [T; N],
}


use std::ops::Index;
use std::ops::IndexMut;
impl_ndvector_ops!();


pub trait Vector:
    Clone + Copy + PartialEq
    + ops::Add<Output=Self> + ops::Sub<Output=Self> + ops::Mul<Self::Component, Output=Self> + ops::Div<Self::Component, Output=Self> 
    + ops::AddAssign + ops::SubAssign + ops::Mul<Self::Component, Output=Self> + ops::Div<Self::Component, Output=Self>
    + ElementWiseMul<Output = Self> + ElementWiseDiv<Output = Self>
{
	type Component: DataValue;
	const NUM_COMPONENTS: usize;
    fn zero() -> Self;
	fn get(&self, index: usize) -> &Self::Component;
	fn get_mut(&mut self, index: usize) -> &mut Self::Component;
	fn get_static<const INDEX: usize>(&self) -> &Self::Component;
	fn get_mut_static<const INDEX: usize>(&mut self) -> &mut Self::Component;
	unsafe fn get_unchecked(&self, index: usize) -> &Self::Component;
	unsafe fn get_unchecked_static<const INDEX: usize>(&self) -> &Self::Component;
	unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut Self::Component;
	unsafe fn get_unchecked_mut_static<const INDEX: usize>(&mut self) -> &mut Self::Component;
}


pub trait Dot {
    type Product;
    fn dot(self, other: Self) -> Self::Product;
}

pub trait ElementWiseMul<Rhs=Self> {
    type Output;
    fn elem_mul(self, other: Rhs) -> Self::Output;
}

pub trait ElementWiseDiv<Rhs=Self> {
    type Output;
    fn elem_div(self, other: Rhs) -> Self::Output;
}


#[derive(Debug)]
pub struct ImplDivErr {
    pub from: ComponentDataType,
}




#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ndvector_add() {
        let vector1 = NdVector { data: [1.0, 2.0, 3.0] };
        let vector2 = NdVector { data: [4.0, 5.0, 6.0] };
        let result: NdVector<3, f32> = vector1 + vector2;
        assert_eq!(result.data, [5.0, 7.0, 9.0]);
    }

    #[test]
    fn test_ndvector_sub() {
        let vector1 = NdVector { data: [4.0, 5.0, 6.0] };
        let vector2 = NdVector { data: [1.0, 2.0, 3.0] };
        let result: NdVector<3, f32> = vector1 - vector2;
        assert_eq!(result.data, [3.0, 3.0, 3.0]);
    }

    #[test]
    fn test_ndvector_dot() {
        let vector1 = NdVector { data: [1_f64, 2.0, 3.0] };
        let vector2 = NdVector { data: [4.0, 5.0, 6.0] };
        let result = vector1.dot(vector2);
        assert_eq!(result, 32.0);
    }

    #[test]
    fn test_ndvector_elem_mul() {
        let vector1 = NdVector { data: [1.0, 2.0, 3.0] };
        let vector2 = NdVector { data: [4.0, 5.0, 6.0] };
        let result = vector1.elem_mul(vector2);
        assert_eq!(result.data, [4.0, 10.0, 18.0]);
    }

    #[test]
    fn test_ndvector_elem_div() {
        let vector1 = NdVector { data: [4.0, 10.0, 18.0, 2.0] };
        let vector2 = NdVector { data: [2.0, 5.0, 3.0, 4.0] };
        let result = vector1.elem_div(vector2);
        assert_eq!(result.data, [2.0, 2.0, 6.0, 0.5]);
    }
}