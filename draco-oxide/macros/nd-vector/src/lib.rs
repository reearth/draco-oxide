use proc_macro::TokenStream;
use quote::quote;
use std::env;
use syn::{parse_macro_input, LitInt};

#[proc_macro]
pub fn impl_ndvector_ops_for_dim(input: TokenStream) -> TokenStream {
    let n = parse_macro_input!(input as LitInt)
        .base10_parse::<usize>()
        .unwrap();

    let indices_add_assign = (0..n).map(|i| {
        quote! { *self.data.get_unchecked_mut(#i) += *rhs.data.get_unchecked(#i); }
    });
    let indices_sub_assign = (0..n).map(|i| {
        quote! { *self.data.get_unchecked_mut(#i) -= *rhs.data.get_unchecked(#i); }
    });
    let indices_mul_assign = (0..n).map(|i| {
        quote! { *self.data.get_unchecked_mut(#i) *= rhs; }
    });
    let indices_div_assign = (0..n).map(|i| {
        quote! { *self.data.get_unchecked_mut(#i) /= rhs; }
    });
    let indices_dot = (0..n).map(|i| {
        quote! { result += *self.data.get_unchecked(#i) * *rhs.data.get_unchecked(#i); }
    });
    let indices_elem_mul = (0..n).map(|i| {
        quote! { *self.data.get_unchecked_mut(#i) *= *rhs.data.get_unchecked(#i); }
    });
    let indices_elem_div = (0..n).map(|i| {
        quote! { *self.data.get_unchecked_mut(#i) /= *rhs.data.get_unchecked(#i); }
    });
    let indices_partial_eq = (0..n).map(|i| {
        quote! { result &= self.data.get_unchecked(#i).eq(rhs.data.get_unchecked(#i)); }
    });
    let indices_portable_to_bytes = (0..n).map(|i| {
        quote! { result.extend((*self.data.get_unchecked(#i)).to_bytes()); }
    });
    let indices_portable_write_to = (0..n).map(|i| {
        quote! { (*self.data.get_unchecked(#i)).write_to(writer); }
    });
    let indices_portable_read_from = (0..n).map(|i| {
        quote! { *data.get_unchecked_mut(#i) = Data::read_from(reader)?; }
    });

    let expanded = quote! {
        impl<T> std::ops::Add for NdVector<#n, T> 
            where 
                T: std::ops::Add<Output = T> + std::ops::AddAssign + Copy
        {
            type Output = NdVector<#n, T>;

            fn add(mut self, rhs: Self) -> Self::Output {
                self+=rhs;
                self
            }
        }

        impl<T> std::ops::AddAssign for NdVector<#n, T> 
            where 
                T: std::ops::AddAssign + Copy
        {
            fn add_assign(&mut self, rhs: Self) {
                unsafe { #(#indices_add_assign)* }
            }
        }

        impl<T> std::ops::Sub for NdVector<#n, T> 
            where 
                T: std::ops::Sub<Output = T> + std::ops::SubAssign + Copy
        {
            type Output = NdVector<#n, T>;

            fn sub(mut self, rhs: Self) -> Self::Output {
                self -= rhs;
                self
            }
        }

        impl<T> std::ops::SubAssign for NdVector<#n, T> 
        where 
            T: std::ops::SubAssign + Copy
        {
            fn sub_assign(&mut self, rhs: Self) {
                unsafe { #(#indices_sub_assign)* }
            }
        }

        impl<T> std::ops::Mul<T> for NdVector<#n, T> 
            where 
                T: std::ops::Mul<Output = T> + std::ops::MulAssign + Copy
        {
            type Output = NdVector<#n, T>;

            fn mul(mut self, rhs: T) -> Self::Output {
                self *= rhs;
                self
            }
        }

        impl<T> std::ops::MulAssign<T> for NdVector<#n, T> 
        where 
            T: std::ops::MulAssign + Copy
        {
            fn mul_assign(&mut self, rhs: T){
                unsafe { #(#indices_mul_assign)* }
            }
        }

        impl<T> std::ops::Div<T> for NdVector<#n, T> 
            where 
                T: std::ops::Div<Output = T> + std::ops::DivAssign + Copy
        {
            type Output = NdVector<#n, T>;

            fn div(mut self, rhs: T) -> Self::Output {
                self /= rhs;
                self
            }
        }

        impl<T> std::ops::DivAssign<T> for NdVector<#n, T> 
        where 
            T: std::ops::DivAssign + Copy
        {
            fn div_assign(&mut self, rhs: T){
                unsafe { #(#indices_div_assign)* }
            }
        }


        impl<Data> Dot for NdVector<#n, Data>
            where Data: DataValue 
        {
            type Product = Data;
            fn dot(self, rhs: Self) -> Self::Product {
                let mut result = Data::zero();
                unsafe {
                    #(#indices_dot)*
                };
                result
            }
        }



        impl<Data> ElementWiseMul<Self> for NdVector<#n, Data> 
            where Data: DataValue + ops::MulAssign
        {
            type Output = Self;
            fn elem_mul(mut self, rhs: Self) -> Self::Output {
                unsafe { #(#indices_elem_mul)* }
                self
            }
        }

        impl<Data> ElementWiseDiv<Self> for NdVector<#n, Data> 
            where Data: DataValue + ops::DivAssign
        {
            type Output = Self;
            fn elem_div(mut self, rhs: Self) -> Self::Output {
                unsafe { #(#indices_elem_div)* }
                self
            }
        }

        impl<Data> cmp::PartialEq for NdVector<#n, Data> 
            where Data: PartialEq
        {
            fn eq(&self, rhs: &Self) -> bool {
                let mut result = true;
                unsafe { #(#indices_partial_eq)* }
                result
            }
        }

        impl<Data> Portable for NdVector<#n, Data> 
            where Data: DataValue
        {
            fn to_bytes(self) -> Vec<u8> {
                let mut result = Vec::with_capacity(#n * size_of::<Data>());
                unsafe { #(#indices_portable_to_bytes)* }
                result
            }
            
            fn write_to<W>(self, writer: &mut W) 
                where W: ByteWriter
            {
                unsafe{ #(#indices_portable_write_to)* }
            }

            fn read_from<R>(reader: &mut R) -> Result<Self, ReaderErr>
                where R: ByteReader
            {
                let mut data = [Data::zero(); #n];
                unsafe { #(#indices_portable_read_from)* }
                Ok(Self {
                    data,
                })
            }
        }


        macro_rules! impl_vector {
            ($($t:ty);* ) => {
            $(
                impl Vector<#n> for NdVector<#n, $t> 
                {
                    type Component = $t;
                    fn zero() -> Self {
                        Self {
                            data: [<$t as DataValue>::zero(); #n],
                        }
                    }
                    fn get(&self, index: usize) -> &Self::Component {
                        self.data.index(index)
                    }
                    
                    fn get_mut(&mut self, index: usize) -> &mut Self::Component {
                        self.data.index_mut(index)
                    }
                    
                    unsafe fn get_unchecked(&self, index: usize) -> &Self::Component {
                        self.data.as_slice().get_unchecked(index)
                    }
                    
                    unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut Self::Component {
                        self.data.as_mut_slice().get_unchecked_mut(index)
                    }
                }
            )*
            };
        }

        impl_vector!(
            u8; u16; u32; u64; i8; i16; i32; i64;
            f32; f64
        );
    };

    TokenStream::from(expanded)
}



#[proc_macro]
pub fn impl_ndvector_ops(_input: TokenStream) -> TokenStream {
    let n: usize = env::var("MAX_VECTOR_DIM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4); // Default max vec size is 4 if missing or invalid

    let expanded = quote! {
        use nd_vector::impl_ndvector_ops_for_dim;
        seq_macro::seq!(N in 1..=#n {
            impl_ndvector_ops_for_dim!(N);
        });
    };

    TokenStream::from(expanded)
}