use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parser, parse_macro_input, DeriveInput};

#[proc_macro_attribute]
pub fn mapper_ext_fields(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    match &mut ast.data {
        syn::Data::Struct(ref mut struct_data) => {
            if let syn::Fields::Named(fields) = &mut struct_data.fields {
                fields.named.push(
                    syn::Field::parse_named
                        .parse2(quote! { pub ext_fields: Option<map::MapperExtFields> })
                        .unwrap(),
                );
            };

            quote! {
                #ast
            }
            .into()
        }
        _ => panic!("`add_mapper_common_field` has to be used with structs "),
    }
}

#[proc_macro_derive(MapperExt)]
pub fn commonext_macro_derive(input: TokenStream) -> TokenStream {
    // 基于 input 构建 AST 语法树
    let ast: DeriveInput = syn::parse(input).unwrap();

    // 构建特征实现代码
    impl_common_mapperext_macro(&ast)
}

fn impl_common_mapperext_macro(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl map::MapperExt for #name {


            fn get_ext_fields(&self) -> Option<&map::MapperExtFields>{self.ext_fields.as_ref()}
            fn set_ext_fields(&mut self, fs: Option<map::MapperExtFields>){
                self.ext_fields = fs
            }
        }
    };
    gen.into()
}

#[proc_macro_derive(NoMapperExt)]
pub fn ext_macro_derive(input: TokenStream) -> TokenStream {
    // 基于 input 构建 AST 语法树
    let ast: DeriveInput = syn::parse(input).unwrap();

    // 构建特征实现代码
    impl_mapperext_macro(&ast)
}

fn impl_mapperext_macro(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl map::MapperExt for #name {

            fn get_ext_fields(&self) -> Option<&map::MapperExtFields>{ None}
            fn set_ext_fields(&mut self, fs: Option<map::MapperExtFields>){
            }
        }
    };
    gen.into()
}
