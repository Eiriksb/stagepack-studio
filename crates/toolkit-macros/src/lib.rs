//! `#[component]` turns a function returning an `Element` into a `NativeComponent`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, Pat, Type, parse_macro_input};

enum Kind {
    Ctx,
    Children,
    Attribute,
}

fn kind_of(ty: &Type) -> Kind {
    match ty {
        Type::Reference(r) if matches!(&*r.elem, Type::Path(p) if p.path.is_ident("Ctx")) => Kind::Ctx,
        Type::Path(p) if p.path.is_ident("Children") => Kind::Children,
        _ => Kind::Attribute,
    }
}

#[proc_macro_attribute]
pub fn component(_args: TokenStream, input: TokenStream) -> TokenStream {
    let function = parse_macro_input!(input as ItemFn);
    let name = &function.sig.ident;
    let constructor = format_ident!("{}_component", name);
    let tag = name.to_string();

    let mut call_args = Vec::new();
    let mut info = Vec::new();
    for arg in &function.sig.inputs {
        let FnArg::Typed(typed) = arg else {
            return quote!(compile_error!("components are plain functions");).into();
        };
        let Pat::Ident(pat) = &*typed.pat else {
            return quote!(compile_error!("component arguments must be plain names");).into();
        };
        let ty = &*typed.ty;
        match kind_of(ty) {
            Kind::Ctx => call_args.push(quote!(ctx)),
            Kind::Children => call_args.push(quote!(::toolkit::Children)),
            Kind::Attribute => {
                let attr = pat.ident.to_string().trim_start_matches("r#").replace('_', "-");
                call_args.push(quote! {
                    <#ty as ::toolkit::Prop>::from_attr(props.0.get(#attr).map(::std::string::String::as_str))
                });
                info.push(quote! {
                    ::toolkit::PropInfo { name: #attr, type_name: <#ty as ::toolkit::Prop>::TYPE_NAME }
                });
            }
        }
    }

    quote! {
        #[allow(non_snake_case)]
        #function

        #[allow(non_snake_case)]
        pub fn #constructor() -> ::toolkit::NativeComponent {
            static SCHEMA: &[::toolkit::PropInfo] = &[#(#info),*];
            fn render(props: &::toolkit::Props, ctx: &::toolkit::Ctx) -> ::toolkit::Element {
                #name(#(#call_args),*)
            }
            ::toolkit::NativeComponent { name: #tag, schema: SCHEMA, render }
        }
    }
    .into()
}
