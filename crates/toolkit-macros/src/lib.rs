//! `#[component]` turns a function returning an `Element` into a `NativeComponent`, and
//! `#[derive(Prop)]` turns an enum into a choice between its variants.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, FnArg, ItemFn, Pat, Path, Type, parse_macro_input};

/// The names kept for the slide, which `toolkit::RESERVED_NAMES` also holds.
const RESERVED_NAMES: [&str; 16] = include!("reserved_names.rs");

enum Kind {
    Ctx,
    Children,
    Attribute,
}

/// By the last segment, which is what sees `Ctx` in `&Ctx<'_>`.
fn is_named(path: &Path, name: &str) -> bool {
    path.segments.last().is_some_and(|segment| segment.ident == name)
}

fn kind_of(ty: &Type) -> Kind {
    match ty {
        Type::Reference(r) if matches!(&*r.elem, Type::Path(p) if is_named(&p.path, "Ctx")) => Kind::Ctx,
        Type::Path(p) if is_named(&p.path, "Children") => Kind::Children,
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
    let mut fields = Vec::new();
    let mut children = false;
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
            Kind::Children => {
                children = true;
                call_args.push(quote!(::toolkit::Children));
            }
            Kind::Attribute => {
                let attr = pat.ident.to_string().trim_start_matches("r#").replace('_', "-");
                if RESERVED_NAMES.contains(&attr.as_str()) {
                    let message = format!("`{attr}` is kept for the slide, for where a tag is placed and when it appears, so a component's argument can't have that name");
                    return syn::Error::new_spanned(&pat.ident, message).to_compile_error().into();
                }
                call_args.push(quote! {
                    <#ty as ::toolkit::Prop>::from_attr(props.0.get(#attr).map(::std::string::String::as_str))
                });
                fields.push(quote! {
                    ::toolkit::PropField::new(#attr, <#ty as ::toolkit::Prop>::kind(), <#ty as ::toolkit::Prop>::OPTIONAL)
                });
            }
        }
    }

    quote! {
        // Its arguments are its attributes, so there are as many as it takes.
        #[allow(non_snake_case, clippy::too_many_arguments)]
        #function

        #[allow(non_snake_case)]
        pub fn #constructor() -> ::toolkit::NativeComponent {
            fn schema() -> ::toolkit::ComponentSchema {
                ::toolkit::ComponentSchema::new(#tag, ::std::vec![#(#fields),*], #children)
            }
            fn render(props: &::toolkit::Props, ctx: &::toolkit::Ctx) -> ::toolkit::Element {
                #name(#(#call_args),*)
            }
            ::toolkit::NativeComponent { name: #tag, schema, render }
        }

        // In a block, so that these names can't clash with the ones in the component's file.
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn abi_version() -> u32 {
                ::toolkit::ABI_VERSION
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn alloc(len: u32) -> u32 {
                ::toolkit::__private::alloc(len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn schema() -> u64 {
                ::toolkit::__private::schema(#constructor())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn render(ptr: u32, len: u32) -> u64 {
                ::toolkit::__private::render(ptr, len, #constructor())
            }
        };
    }
    .into()
}

/// `LineChart` is written `line-chart`.
fn kebab_case(name: &str) -> String {
    let mut kebab = String::new();
    for (index, letter) in name.chars().enumerate() {
        if letter.is_uppercase() && index > 0 {
            kebab.push('-');
        }
        kebab.extend(letter.to_lowercase());
    }
    kebab
}

#[proc_macro_derive(Prop)]
pub fn derive_prop(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let Data::Enum(choices) = &input.data else {
        return quote!(compile_error!("only an enum can be a choice");).into();
    };
    if choices.variants.iter().any(|variant| !matches!(variant.fields, Fields::Unit)) {
        return quote!(compile_error!("the variants of a choice can't hold anything");).into();
    }
    let Some(first) = choices.variants.first().map(|variant| &variant.ident) else {
        return quote!(compile_error!("a choice needs at least one variant");).into();
    };
    let variants: Vec<_> = choices.variants.iter().map(|variant| &variant.ident).collect();
    let attrs: Vec<_> = variants.iter().map(|variant| kebab_case(&variant.to_string())).collect();

    quote! {
        impl ::toolkit::Prop for #name {
            fn kind() -> ::toolkit::PropKind {
                ::toolkit::PropKind::Choice(::std::vec![#(::std::string::String::from(#attrs)),*])
            }

            fn from_attr(value: ::std::option::Option<&str>) -> Self {
                match value {
                    #(::std::option::Option::Some(#attrs) => #name::#variants,)*
                    _ => #name::#first,
                }
            }
        }
    }
    .into()
}
