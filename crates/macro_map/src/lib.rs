use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parser, parse_macro_input, DeriveInput};

#[proc_macro_attribute]
pub fn map_ext_fields(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    match &mut ast.data {
        syn::Data::Struct(ref mut struct_data) => {
            if let syn::Fields::Named(fields) = &mut struct_data.fields {
                fields.named.push(
                    syn::Field::parse_named
                        .parse2(quote! { pub ext_fields: Option<map::MapExtFields> })
                        .unwrap(),
                );
            };

            quote! {
                #ast
            }
            .into()
        }
        _ => panic!("`add_map_common_field` has to be used with structs "),
    }
}

#[proc_macro_derive(MapExt)]
pub fn common_ext_macro_derive(input: TokenStream) -> TokenStream {
    // 基于 input 构建 AST 语法树
    let ast: DeriveInput = syn::parse(input).unwrap();

    // 构建特征实现代码
    impl_common_map_ext_macro(&ast)
}

fn impl_common_map_ext_macro(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl map::MapExt for #name {


            fn get_ext_fields(&self) -> Option<&map::MapExtFields>{self.ext_fields.as_ref()}
            fn set_ext_fields(&mut self, fs: Option<map::MapExtFields>){
                self.ext_fields = fs
            }
        }
    };
    gen.into()
}

#[proc_macro_derive(NoMapExt)]
pub fn ext_macro_derive(input: TokenStream) -> TokenStream {
    // 基于 input 构建 AST 语法树
    let ast: DeriveInput = syn::parse(input).unwrap();

    // 构建特征实现代码
    impl_map_ext_macro(&ast)
}

fn impl_map_ext_macro(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl map::MapExt for #name {

            fn get_ext_fields(&self) -> Option<&map::MapExtFields>{ None}
            fn set_ext_fields(&mut self, fs: Option<map::MapExtFields>){
            }
        }
    };
    gen.into()
}
