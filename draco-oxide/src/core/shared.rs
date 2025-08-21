use draco_nd_vector::impl_ndvector_ops;
use super::attribute::ComponentDataType;
use crate::core::bit_coder::ReaderErr;

use core::fmt;
use std::{
    ops,
    cmp,
    mem,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttributeValueIdx(usize);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CornerIdx(usize);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeIdx(usize);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceIdx(usize);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointIdx(usize);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VertexIdx(usize);

macro_rules! idx_op_impl {
    ($($trait_:ident, $method:ident, $op:tt, $t:ty);*) => {
        $(
            impl ops::$trait_<$t> for $t {
                type Output = Self;

                fn $method(self, other: Self) -> Self::Output {
                    Self( self.0 $op other.0 )
                }
            }
        )*
    };
}

macro_rules! all_idx_ops_impl {
    ($($t:ty),*) => {
        $(
            idx_op_impl! {
                Add, add, +, $t;
                Sub, sub, -, $t;
                Mul, mul, *, $t;
                Div, div, /, $t
            }
        )*
    };
}

all_idx_ops_impl!{
    AttributeValueIdx,
    CornerIdx,
    EdgeIdx,
    FaceIdx,
    PointIdx,
    VertexIdx
}

macro_rules! idx_debug_impl {
    ($($t:ty),*) => {
        $(
            impl fmt::Debug for $t {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(f)
                }
            }
        )*
    };
}

idx_debug_impl!{
    AttributeValueIdx,
    CornerIdx,
    EdgeIdx,
    FaceIdx,
    PointIdx,
    VertexIdx
}

macro_rules! vec_with_new_idx {
    ($($Idx:ident),*) => {
        $(
            paste::paste! {
                /// Vector wrapper indexed by `$Idx` and named `Vec$Idx` (e.g. `VecIsize`).
                #[derive(Debug, Clone, Default, PartialEq, Eq)]
                pub struct [<Vec $Idx:camel>]<T> {
                    inner: ::std::vec::Vec<T>,
                }

                impl<T: Clone> [<Vec $Idx:camel>]<T> {
                    #[allow(unused)]
                    /// Create an empty vector.
                    pub fn new() -> Self {
                        Self { inner: ::std::vec::Vec::new() }
                    }

                    #[allow(unused)]
                    /// Create with pre-allocated capacity.
                    pub fn with_capacity(capacity: usize) -> Self {
                        Self { inner: ::std::vec::Vec::with_capacity(capacity) }
                    }

                    #[allow(unused)]
                    /// Push a value onto the end.
                    pub fn push(&mut self, value: T) {
                        self.inner.push(value)
                    }

                    #[allow(unused)]
                    pub fn len(&self) -> usize { self.inner.len() }
                    #[allow(unused)]
                    pub fn is_empty(&self) -> bool { self.inner.is_empty() }
                    #[allow(unused)]
                    pub fn resize(&mut self, new_len: usize, value: T) {
                        self.inner.resize(new_len, value);
                    }

                    #[allow(unused)]
                    pub fn reserve(&mut self, additional: usize) {
                        self.inner.reserve(additional);
                    }

                    #[allow(unused)]
                    /// Take the inner Vec if you need it back.
                    pub fn into_inner(self) -> ::std::vec::Vec<T> { self.inner }

                    #[allow(unused)]
                    pub fn iter(&self) -> impl Iterator<Item = &T> {
                        self.inner.iter()
                    }

                    #[allow(unused)]
                    pub fn remove(&mut self, idx: $Idx) -> T {
                        self.inner.remove(usize::from(idx))
                    }

                    #[allow(unused)]
                    pub fn clear(&mut self) {
                        self.inner.clear();
                    }
                }

                impl<T> ::std::ops::Index<$Idx> for [<Vec $Idx:camel>]<T> {
                    type Output = T;
                    fn index(&self, idx: $Idx) -> &Self::Output {
                        // Convert the `$Idx` (e.g., isize) to usize, panicking if negative/overflow.
                        let i: usize = ::std::convert::TryInto::<usize>::try_into(idx)
                            .ok()
                            .expect("index does not fit in usize (negative or too large)");
                        &self.inner[i]
                    }
                }

                impl<T> ::std::ops::IndexMut<$Idx> for [<Vec $Idx:camel>]<T> {
                    fn index_mut(&mut self, idx: $Idx) -> &mut Self::Output {
                        let i: usize = ::std::convert::TryInto::<usize>::try_into(idx)
                            .ok()
                            .expect("index does not fit in usize (negative or too large)");
                        &mut self.inner[i]
                    }
                }

                impl<T> ::std::convert::From<::std::vec::Vec<T>> for [<Vec $Idx:camel>]<T> {
                    fn from(inner: ::std::vec::Vec<T>) -> Self {
                        Self { inner }
                    }
                }

                impl<T> ::std::iter::IntoIterator for [<Vec $Idx:camel>]<T> {
                    type Item = T;
                    type IntoIter = ::std::vec::IntoIter<T>;

                    fn into_iter(self) -> Self::IntoIter {
                        self.inner.into_iter()
                    }
                }
            }
        )*
    };
}

vec_with_new_idx!(
    AttributeValueIdx,
    CornerIdx,
    EdgeIdx,
    FaceIdx,
    PointIdx,
    VertexIdx
);

macro_rules! idx_impl {
    ($($t:ty),*) => {
        $(
            impl From<usize> for $t {
                fn from(idx: usize) -> Self {
                    Self(idx)
                }
            }

            impl From<$t> for usize {
                fn from(idx: $t) -> Self {
                    idx.0
                }
            }
        )*
    };
}

idx_impl! {
    AttributeValueIdx,
    CornerIdx,
    EdgeIdx,
    FaceIdx,
    PointIdx,
    VertexIdx
}


pub trait Float: DataValue + ops::Div<Output=Self> + ops::Neg<Output=Self> {
    fn sqrt(self)-> Self;
}

impl Float for f32 {
    fn sqrt(self)-> Self {
        self.sqrt()
    }
}

impl Float for f64 {
    fn sqrt(self)-> Self {
        self.sqrt()
    }
}

pub trait ConfigType {
    fn default()-> Self;
}

pub trait ToUsize {
    #[allow(unused)]
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

impl_to_usize_float!(u8, u16, u32, u64, i8, i16, i32, i64);

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

impl_abs!(negatable: f32, f64, i8, i16, i32, i64);
impl_abs!(non_negatable: u8, u16, u32, u64);

pub trait Acos {
    #[allow(unused)]
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
impl_acos!(non_float: u8, u16, u32, u64, i8, i16, i32, i64);


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
impl_max!(u8, u16, u32, u64, i8, i16, i32, i64);


/// A trait that defines the basic mathematical operations and properties for the elements of the vector.
pub trait DataValue: 
    Clone + Copy + fmt::Debug + PartialEq + PartialOrd
    + Portable
    + Into<serde_json::Value> 
    + Abs + Max
    + ops::Add<Output=Self> + ops::Sub<Output=Self> + ops::Mul<Output=Self> + ops::Div<Output=Self>
    + ops::AddAssign + ops::SubAssign + ops::MulAssign + ops::DivAssign
{
    fn get_dyn() -> ComponentDataType;
    fn zero() -> Self;
    fn one() -> Self;
    fn from_u64(data: u64) -> Self;
    fn to_u64(self) -> u64;
    fn to_i64(self) -> i64;
    fn from_i64(data: i64) -> Self;
    fn from_f64(data: f64) -> Self;
    fn to_f64(self) -> f64;
}

macro_rules! impl_data_value {
    (int: $(($t:ty, $component_type: expr)),*) => {
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

                fn to_i64(self) -> i64 {
                    self as i64
                }

                fn from_i64(data: i64) -> Self {
                    data as $t
                }

                fn from_f64(data: f64) -> Self {
                    data as $t
                }

                fn to_f64(self) -> f64 {
                    self as f64
                }
            }

            impl Portable for $t {
                fn to_bytes(self) -> Vec<u8> {
                    self.to_le_bytes().to_vec()
                }

                fn write_to<W>(self, writer: &mut W) where W: ByteWriter {
                    for b in self.to_le_bytes().iter() {
                        writer.write_u8(*b);
                    }
                }

                fn read_from<R>(reader: &mut R) -> Result<Self, ReaderErr> 
                    where R: ByteReader
                {
                    let mut bytes = [0u8; mem::size_of::<Self>()];
                    for i in 0..bytes.len() {
                        bytes[i] = reader.read_u8()?;
                    }
                    Ok(Self::from_le_bytes(bytes))
                }
            }
        )*
    };

    (float: $(($t:ty, $uint_t:ty, $component_type: expr)),*) => {
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

                fn to_i64(self) -> i64 {
                    self as i64
                }

                fn from_i64(data: i64) -> Self {
                    data as $t
                }

                fn from_f64(data: f64) -> Self {
                    data as $t
                }

                fn to_f64(self) -> f64 {
                    self as f64
                }
            }

            impl Portable for $t {
                fn to_bytes(self) -> Vec<u8> {
                    self.to_le_bytes().to_vec()
                }

                fn write_to<W>(self, writer: &mut W) where W: ByteWriter {
                    let bits = self.to_bits();
                    for b in bits.to_le_bytes().iter() {
                        writer.write_u8(*b);
                    }
                }

                fn read_from<R>(reader: &mut R) -> Result<Self, ReaderErr> 
                    where R: ByteReader
                {
                    let mut bytes = [0u8; mem::size_of::<Self>()];
                    for i in 0..bytes.len() {
                        bytes[i] = reader.read_u8()?;
                    }
                    Ok(Self::from_bits(<$uint_t>::from_le_bytes(bytes)))
                }
            }
        )*
    };
}

impl_data_value!(int: 
    (u8, ComponentDataType::U8),
    (u16, ComponentDataType::U16),
    (u32, ComponentDataType::U32),
    (u64, ComponentDataType::U64),
    (i8, ComponentDataType::I8),
    (i16, ComponentDataType::I16),
    (i32, ComponentDataType::I32),
    (i64, ComponentDataType::I64)
);

impl_data_value!(float: 
    (f32, u32, ComponentDataType::F32),
    (f64, u64, ComponentDataType::F64)
);


/// An array of `N` elements of type `T`, where `T` is often a [DataValue], 
/// which is a trait that defines the basic mathematical operations 
/// and properties for the elements of the vector. This means that
/// `NdVector` can be used to represent vectors in `N`-dimensional space (or a free module)
///  over `T`. The vector operations for this type are implemented on a static basis, 
/// meaning that the operations are defined for a fixed number of dimensions at compile time 
/// without looping over `N` elements.
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

impl<const N: usize,T> From<NdVector<N,T>> for serde_json::Value 
    where [T; N]: Into<serde_json::Value>
{
    fn from(vector: NdVector<N,T>) -> Self {
        vector.data.into()
    }
}


use std::ops::Index;
use std::ops::IndexMut;
use crate::prelude::{ByteReader, ByteWriter};
use crate::shared::attribute::Portable;
impl_ndvector_ops!();


pub trait Vector<const N: usize>:
    Clone + Copy + fmt::Debug + PartialEq
    + Into<serde_json::Value> 
    + ops::Add<Output=Self> + ops::Sub<Output=Self> + ops::Mul<Self::Component, Output=Self> + ops::Div<Self::Component, Output=Self> 
    + ops::AddAssign + ops::SubAssign + ops::Mul<Self::Component, Output=Self> + ops::Div<Self::Component, Output=Self>
    + ElementWiseMul<Output = Self> + ElementWiseDiv<Output = Self>
    + Dot<Product=Self::Component> + Cross
{
	type Component: DataValue;
    fn zero() -> Self;
	fn get(&self, index: usize) -> &Self::Component;
	fn get_mut(&mut self, index: usize) -> &mut Self::Component;
	unsafe fn get_unchecked(&self, index: usize) -> &Self::Component;
    unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut Self::Component;
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


impl<const N: usize, T> Cross for NdVector<N, T> 
    where T: DataValue
{
    fn cross(self, other: Self) -> Self {
        if N == 3 {
            unsafe {
                let mut out = [T::zero(); N];
                *out.get_unchecked_mut(0) = *self.data.get_unchecked(1) * *other.data.get_unchecked(2) - *self.data.get_unchecked(2) * *other.data.get_unchecked(1);
                *out.get_unchecked_mut(1) = *self.data.get_unchecked(2) * *other.data.get_unchecked(0) - *self.data.get_unchecked(0) * *other.data.get_unchecked(2);
                *out.get_unchecked_mut(2) = *self.data.get_unchecked(0) * *other.data.get_unchecked(1) - *self.data.get_unchecked(1) * *other.data.get_unchecked(0);
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