use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parser, parse_macro_input, DeriveInput};

#[proc_macro_attribute]
pub fn common_mapper_field(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    match &mut ast.data {
        syn::Data::Struct(ref mut struct_data) => {
            match &mut struct_data.fields {
                syn::Fields::Named(fields) => {
                    fields.named.push(
                        syn::Field::parse_named
                            .parse2(quote! { pub is_tail_of_chain: bool })
                            .unwrap(),
                    );
                }
                _ => (),
            }

            quote! {
                #ast
            }
            .into()
        }
        _ => panic!("`add_mapper_common_field` has to be used with structs "),
    }
}
