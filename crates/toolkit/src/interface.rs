//! What crosses between the app and a talk's own components, as JSON. Components already
//! built depend on this, so `tests/interface_v1.rs` holds it fixed.

use crate::color::Theme;
use crate::element::Element;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ABI_VERSION: u32 = 1;

/// The moment a component is drawn at, and the room it has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CtxData {
    pub click_step: u32,
    pub time: f32,
    pub slide_width: f32,
    pub slide_height: f32,
    /// The width of the box the tag sits in.
    pub width: f32,
    /// The height of that box, or 0 if it grows with its content.
    pub height: f32,
    pub theme: Theme,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RenderInput {
    pub props: BTreeMap<String, String>,
    pub ctx: CtxData,
}

/// What a component drew, and what it looked at to do so.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RenderOutput {
    pub element: Element,
    /// The clicks it asked the slide for.
    #[serde(default)]
    pub steps: u32,
    /// Whether it can differ from one click to the next.
    #[serde(default)]
    pub stepped: bool,
    /// Whether it can differ from one moment to the next.
    #[serde(default)]
    pub animated: bool,
}

/// What a component takes, for whatever shows a form for its tag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ComponentSchema {
    pub name: String,
    pub props: Vec<PropField>,
    /// Whether content between its tags is used.
    pub children: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PropField {
    pub name: String,
    pub kind: PropKind,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PropKind {
    Text,
    Whole,
    Number,
    Flag,
    Color,
    /// A file with one of these extensions.
    File(Vec<String>),
    /// One of these values.
    Choice(Vec<String>),
}

impl CtxData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(click_step: u32, time: f32, slide_width: f32, slide_height: f32, width: f32, height: f32, theme: Theme) -> CtxData {
        CtxData { click_step, time, slide_width, slide_height, width, height, theme }
    }
}

impl RenderInput {
    pub fn new(props: BTreeMap<String, String>, ctx: CtxData) -> RenderInput {
        RenderInput { props, ctx }
    }
}

impl RenderOutput {
    /// `element`, with what was looked at to draw it.
    pub fn new(element: Element, steps: u32, stepped: bool, animated: bool) -> RenderOutput {
        RenderOutput { element, steps, stepped, animated }
    }
}

impl ComponentSchema {
    pub fn new(name: impl Into<String>, props: Vec<PropField>, children: bool) -> ComponentSchema {
        ComponentSchema { name: name.into(), props, children }
    }
}

impl PropField {
    pub fn new(name: impl Into<String>, kind: PropKind, optional: bool) -> PropField {
        PropField { name: name.into(), kind, optional }
    }
}

/// The names a component's argument can't have, as they're kept for the slide: where it
/// is placed and when it appears, and what a later version may add.
///
/// ```compile_fail
/// use toolkit::prelude::*;
///
/// #[component]
/// fn Badge(x: f32) -> Element {
///     text(x.to_string())
/// }
/// ```
///
/// Any other name is fine:
///
/// ```
/// use toolkit::prelude::*;
///
/// #[component]
/// fn Badge(left: f32) -> Element {
///     text(left.to_string())
/// }
/// ```
pub const RESERVED_NAMES: [&str; 16] = include!("../../toolkit-macros/src/reserved_names.rs");

/// Whether `path` stays inside the talk's `assets` folder.
pub fn asset_path_allowed(path: &str) -> bool {
    path.starts_with("assets/") && !path.contains('\\') && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}
