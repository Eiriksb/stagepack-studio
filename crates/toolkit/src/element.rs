use crate::color::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect,
    Rounded,
    Ellipse,
    Line,
    Arrow,
    Callout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Element {
    Text { text: String, size: Option<f32>, bold: bool, color: Option<Color> },
    Stack { horizontal: bool, gap: f32, children: Vec<Element> },
    Shape {
        kind: ShapeKind,
        width: f32,
        height: f32,
        fill: Option<Color>,
        outline: Option<Color>,
        outline_width: f32,
        child: Option<Box<Element>>,
    },
    Picture { src: String, width: f32, height: f32 },
    Icon { name: String, size: f32, color: Option<Color> },
    Path { data: String, width: f32, height: f32, fill: Option<Color> },
    /// Where the content between a component's tags goes.
    Children,
    Empty,
}

/// What a component's body receives for the content between its tags.
pub struct Children;

impl From<Children> for Element {
    fn from(_: Children) -> Element {
        Element::Children
    }
}

pub fn text(text: impl Into<String>) -> Element {
    Element::Text { text: text.into(), size: None, bold: false, color: None }
}

pub fn shape(kind: ShapeKind, width: f32, height: f32) -> Element {
    Element::Shape { kind, width, height, fill: None, outline: None, outline_width: 0.0, child: None }
}

pub fn picture(src: impl Into<String>, width: f32, height: f32) -> Element {
    Element::Picture { src: src.into(), width, height }
}

pub fn icon(name: impl Into<String>, size: f32) -> Element {
    Element::Icon { name: name.into(), size, color: None }
}

pub fn path(data: impl Into<String>, width: f32, height: f32) -> Element {
    Element::Path { data: data.into(), width, height, fill: None }
}

#[doc(hidden)]
pub fn stack(horizontal: bool, children: Vec<Element>) -> Element {
    Element::Stack { horizontal, gap: 0.0, children }
}

#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => { $crate::stack(true, vec![$($child),*]) };
}

#[macro_export]
macro_rules! column {
    ($($child:expr),* $(,)?) => { $crate::stack(false, vec![$($child),*]) };
}

impl Element {
    /// Font size for text, edge length for an icon. Other elements are unchanged.
    pub fn size(mut self, value: f32) -> Element {
        match &mut self {
            Element::Text { size, .. } => *size = Some(value),
            Element::Icon { size, .. } => *size = value,
            _ => {}
        }
        self
    }

    pub fn bold(mut self) -> Element {
        if let Element::Text { bold, .. } = &mut self {
            *bold = true;
        }
        self
    }

    /// Text and icon colour, or the fill of a shape or path.
    pub fn fill(mut self, value: Color) -> Element {
        match &mut self {
            Element::Text { color, .. } | Element::Icon { color, .. } => *color = Some(value),
            Element::Shape { fill, .. } | Element::Path { fill, .. } => *fill = Some(value),
            _ => {}
        }
        self
    }

    /// Outlines a shape; a shape with no width yet gets one of 1.
    pub fn outline(mut self, value: Color) -> Element {
        if let Element::Shape { outline, outline_width, .. } = &mut self {
            *outline = Some(value);
            if *outline_width == 0.0 {
                *outline_width = 1.0;
            }
        }
        self
    }

    pub fn gap(mut self, value: f32) -> Element {
        if let Element::Stack { gap, .. } = &mut self {
            *gap = value;
        }
        self
    }
}
