use proc_macro::TokenStream;
use quote::quote;
use std::env;

#[proc_macro]
pub fn impl_ndvector_ops(_: TokenStream) -> TokenStream {
    let n: usize = env::var("MAX_VECTOR_DIM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4); // Default max vec size is 4 if missing or invalid

    let indices_add = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) + *rhs.data.get_unchecked(#i) }
    });
    let indices_sub = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) - *rhs.data.get_unchecked(#i) }
    });
    let indices_mul = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) * rhs }
    });
    let indices_div = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) / rhs }
    });
    let indices_cast_f64 = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) as u64 }
    });
    let indices_cast_f32 = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) as u64 }
    });
    let indices_elem_mul = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) * *self.data.get_unchecked(#i) }
    });
    let indices_elem_div = (0..n).map(|i| {
        quote! { *self.data.get_unchecked(#i) / *self.data.get_unchecked(#i) }
    });

    let expanded = quote! {
        impl<T> std::ops::Add for NdVector<#n, T> 
            where 
                T: std::ops::Add<Output = T> + Copy + Clone
        {
            type Output = NdVector<#n, T>;

            fn add(self, rhs: Self) -> Self::Output {
                NdVector { data: 
                    unsafe { [#(#indices_add),*] }
                }
            }
        }

        impl<T> std::ops::Sub for NdVector<#n, T> 
            where 
                T: std::ops::Sub<Output = T> + Copy + Clone
        {
            type Output = NdVector<#n, T>;

            fn sub(self, rhs: Self) -> Self::Output {
                NdVector { data: 
                    unsafe { [#(#indices_sub),*] }
                }
            }
        }

        impl<T> std::ops::Mul<T> for NdVector<#n, T> 
            where 
                T: std::ops::Mul<Output = T> + Copy + Clone
        {
            type Output = NdVector<#n, T>;

            fn mul(self, rhs: T) -> Self::Output {
                NdVector { data: 
                    unsafe { [#(#indices_mul),*] }
                }
            }
        }

        impl<T> std::ops::Div<T> for NdVector<#n, T> 
        where 
            T: std::ops::Div<Output = T> + Copy + Clone
        {
            type Output = NdVector<#n, T>;

            fn div(self, rhs: T) -> Self::Output {
                NdVector { data: 
                    unsafe { [#(#indices_div),*] }
                }
            }
        }

        impl Cast for NdVector<#n, f64> {
            type Output = NdVector<#n, u64>;
            fn cast(self) -> Self::Output {
                NdVector {
                    data: unsafe { [#(#indices_cast_f64),*] }
                }
            }
        }

        impl Cast for NdVector<#n, f32> {
            type Output = NdVector<#n, u64>;
            fn cast(self) -> Self::Output {
                NdVector {
                    data: unsafe { [#(#indices_cast_f32),*] }
                }
            }
        }

        impl ElementWiseMul<Self> for NdVector<#n, f64> {
            type Output = Self;
            fn elem_mul(self, rhs: Self) -> Self::Output {
                NdVector {
                    data: unsafe { [#(#indices_elem_mul),*] }
                }
            }
        }

        impl ElementWiseDiv<Self> for NdVector<#n, f32> {
            type Output = Self;
            fn elem_div(self, rhs: Self) -> Self::Output {
                NdVector {
                    data: unsafe { [#(#indices_elem_div),*] }
                }
            }
        }
    };

    TokenStream::from(expanded)
}
