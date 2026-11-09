//! What components are built from: elements, props and the `#[component]` macro.

mod color;
mod element;
mod props;

pub use color::{Color, Theme};
pub use element::{icon, path, picture, shape, text, Children, Element, ShapeKind};
#[doc(hidden)]
pub use element::stack;
pub use props::{Ctx, NativeComponent, Prop, PropInfo, Props, Registry};
pub use toolkit_macros::component;

pub mod prelude {
    pub use crate::{
        column, component, icon, path, picture, row, shape, text, Children, Color, Ctx, Element, NativeComponent, Prop,
        PropInfo, Props, Registry, ShapeKind, Theme,
    };
}
