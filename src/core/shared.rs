use nd_vector::impl_ndvector_ops;

use super::attribute::ComponentDataType;

use core::fmt;
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
    fn sqrt(self)-> Self;
}

impl Float for f32 {
    type Bits = u32;
    fn from_bits(bits: Self::Bits)-> Self {
        Self::from_bits(bits)
    }
    fn to_bits(self)-> Self::Bits {
        self.to_bits()
    }
    fn sqrt(self)-> Self {
        self.sqrt()
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
    fn sqrt(self)-> Self {
        self.sqrt()
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

pub trait Abs {
    fn abs(self) -> Self;
}
macro_rules! impl_abs {
    (negatable: $($t:ty),*) => {
        $(
            impl Abs for $t {
                fn abs(self) -> Self {
                    self.abs()
                }
            }
        )*
    };
    (non_negatable: $($t:ty),*) => {
        $(
            impl Abs for $t {
                fn abs(self) -> Self {
                    self
                }
            }
        )*
    };
}

impl_abs!(negatable: f32, f64);
impl_abs!(non_negatable: u8, u16, u32, u64);

pub trait Acos {
    fn acos(self) -> Self;
}

macro_rules! impl_acos {
    (float: $($t:ty),*) => {
        $(
            impl Acos for $t {
                fn acos(self) -> Self {
                    self.acos()
                }
            }
        )*
    };
    (non_float: $($t:ty),*) => {
        $(
            impl Acos for $t {
                fn acos(self) -> Self {
                    panic!("Acos is not defined for non-float types")
                }
            }
        )*
    };
}
impl_acos!(float: f32, f64);
impl_acos!(non_float: u8, u16, u32, u64);


pub trait Max {
    const MAX_VALUE: Self;
}

macro_rules! impl_max {
    ($($t:ty),*) => {
        $(
            impl Max for $t {
                const MAX_VALUE: Self = Self::MAX;
            }
        )*
    };
}
impl_max!(f32, f64);
impl_max!(u8, u16, u32, u64);


pub trait DataValue: 
    Clone + Copy + PartialEq + PartialOrd
    + Abs + Max
    + ops::Add<Output=Self> + ops::Sub<Output=Self> + ops::Mul<Output=Self> + ops::Div<Output=Self>
    + ops::AddAssign + ops::SubAssign + ops::MulAssign + ops::DivAssign
{
    fn get_dyn() -> ComponentDataType;
    fn zero() -> Self;
    fn one() -> Self;
    fn from_u64(data: u64) -> Self;
    fn to_u64(self) -> u64;
    fn from_f64(data: f64) -> Self;
    fn to_f64(self) -> f64;
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

                fn from_f64(data: f64) -> Self {
                    data as $t
                }

                fn to_f64(self) -> f64 {
                    self as f64
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

impl<const N: usize, T: Float> NdVector<N, T> {
    pub fn normalize(self) -> Self {
        let mut out = self;
        let norm_inverse = T::one() / self.norm();
        for i in 0..N {
            unsafe {
                *out.data.get_unchecked_mut(i) *= norm_inverse;
            }
        }
        out
    }

    pub fn norm(self) -> T {
        let mut norm = T::zero();
        for i in 0..N {
            unsafe {
                norm += *self.data.get_unchecked(i) * *self.data.get_unchecked(i);
            }
        }
        norm.sqrt()
    }
}

impl<const N: usize, T> From<[T;N]> for NdVector<N, T> {
    fn from(data: [T;N]) -> Self {
        NdVector { data }
    }
}

impl<const N: usize, T> fmt::Debug for NdVector<N, T> 
    where T: fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.data)
    }
}


use std::ops::Index;
use std::ops::IndexMut;
impl_ndvector_ops!();


pub trait Vector:
    Clone + Copy + PartialEq
    + ops::Add<Output=Self> + ops::Sub<Output=Self> + ops::Mul<Self::Component, Output=Self> + ops::Div<Self::Component, Output=Self> 
    + ops::AddAssign + ops::SubAssign + ops::Mul<Self::Component, Output=Self> + ops::Div<Self::Component, Output=Self>
    + ElementWiseMul<Output = Self> + ElementWiseDiv<Output = Self>
    + Dot<Product=Self::Component> + Cross
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

pub trait Cross {
    fn cross(self, other: Self) -> Self;
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



impl<const N: usize, T> Cross for NdVector<N, T> 
    where T: DataValue
{
    fn cross(self, other: Self) -> Self {
        if N == 3 {
            unsafe {
                let mut out = [T::zero(); N];
                *out.get_unchecked_mut(1) = *self.data.get_unchecked(1) * *other.data.get_unchecked(2) - *self.data.get_unchecked(2) * *other.data.get_unchecked(1);
                *out.get_unchecked_mut(1) = *self.data.get_unchecked(2) * *other.data.get_unchecked(0) - *self.data.get_unchecked(0) * *other.data.get_unchecked(2);
                *out.get_unchecked_mut(1) = *self.data.get_unchecked(0) * *other.data.get_unchecked(1) - *self.data.get_unchecked(1) * *other.data.get_unchecked(0);
                NdVector {
                    data: out
                }
            }
        } else {
            unreachable!("Cross product is only defined for 3D vectors")
        }
    }
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