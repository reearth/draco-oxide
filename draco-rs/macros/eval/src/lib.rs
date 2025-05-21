// lib.rs
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn log_call(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let block = &input_fn.block;
    let sig = &input_fn.sig;

    let expanded = quote! {
        #sig {
            println!("Calling function: {}", stringify!(#fn_name));
            #block
        }
    };

    expanded.into()
}
