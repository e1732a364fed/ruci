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

                    fields.named.push(
                        // 存一个可选的addr, 可作为指定的连接目标。 这样该mapper的decode行为就像一个 socks5一样
                        syn::Field::parse_named
                            .parse2(quote! { pub fixed_target_addr: Option<net::Addr> })
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

use syn;

#[proc_macro_derive(CommonMapperExt)]
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
            fn configured_target_addr(&self) -> Option<net::Addr> {
                self.fixed_target_addr.clone()
            }
            fn is_tail_of_chain(&self) -> bool {
                self.is_tail_of_chain
            }

            fn set_configured_target_addr(&mut self, a: Option<net::Addr>) {
                self.fixed_target_addr = a
            }
            fn set_is_tail_of_chain(&mut self, is: bool) {
                self.is_tail_of_chain = is
            }

        }
    };
    gen.into()
}

#[proc_macro_derive(DefaultMapperExt)]
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
            fn set_configured_target_addr(&mut self, _a: Option<net::Addr>) {}
            fn set_is_tail_of_chain(&mut self, _is: bool) {}
            fn configured_target_addr(&self) -> Option<net::Addr> {
                None
            }
            fn is_tail_of_chain(&self) -> bool {
                false
            }

        }
    };
    gen.into()
}
