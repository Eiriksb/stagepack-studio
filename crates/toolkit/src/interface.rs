//! What crosses between the app and a talk's own components, as JSON. Components already
//! built depend on this, so `tests/interface_v1.rs` holds it fixed.

use crate::color::Theme;
use crate::element::Element;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ABI_VERSION: u32 = 1;

/// The moment a component is drawn at, and the room it has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
pub struct RenderInput {
    pub props: BTreeMap<String, String>,
    pub ctx: CtxData,
}

/// What a component drew, and what it looked at to do so.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
pub struct ComponentSchema {
    pub name: String,
    pub props: Vec<PropField>,
    /// Whether content between its tags is used.
    pub children: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropField {
    pub name: String,
    pub kind: PropKind,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// Whether `path` stays inside the talk's `assets` folder.
pub fn asset_path_allowed(path: &str) -> bool {
    path.starts_with("assets/") && !path.contains('\\') && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}
