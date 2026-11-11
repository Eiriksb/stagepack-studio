//! What components are built from: elements, props and the `#[component]` macro.

mod color;
mod element;
mod interface;
mod props;
#[cfg(target_arch = "wasm32")]
mod wasm;

pub use color::{Color, Theme};
pub use element::{at, canvas, icon, path, picture, problem, shape, text, Children, Element, Placed, ShapeKind, TextAlign};
#[doc(hidden)]
pub use element::stack;
pub use interface::{
    asset_path_allowed, ComponentSchema, CtxData, PropField, PropKind, RenderInput, RenderOutput, ABI_VERSION, RESERVED_NAMES,
};
pub use props::{Ctx, DataFile, NativeComponent, PictureFile, Prop, Props, Registry};
pub use toolkit_macros::{component, Prop};

/// For the code `#[component]` generates. A component is one file that can only name
/// this crate, so everything that code needs is reached from here.
#[doc(hidden)]
pub mod __private {
    #[cfg(target_arch = "wasm32")]
    pub use crate::wasm::{alloc, render, schema};
}

pub mod prelude {
    pub use crate::{
        asset_path_allowed, at, canvas, col, component, icon, path, picture, problem, row, shape, text, Children, Color,
        ComponentSchema, Ctx, CtxData, DataFile, Element, NativeComponent, PictureFile, Placed, Prop, PropField, PropKind,
        Props, Registry, RenderInput, RenderOutput, ShapeKind, TextAlign, Theme, ABI_VERSION,
    };
}
