use crate::color::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ShapeKind {
    Rect,
    Rounded,
    Ellipse,
    Line,
    Arrow,
    Callout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
        /// The corner radius of a `Rounded` shape, where 0 means the usual one.
        radius: f32,
        /// Space kept clear between the edge and what is inside.
        padding: f32,
        child: Option<Box<Element>>,
    },
    Picture { src: String, width: f32, height: f32 },
    Icon { name: String, size: f32, color: Option<Color> },
    Path {
        data: String,
        width: f32,
        height: f32,
        fill: Option<Color>,
        #[serde(default)]
        stroke: Option<Color>,
        #[serde(default)]
        stroke_width: f32,
    },
    /// A fixed area, with each thing in it placed by hand.
    Canvas { width: f32, height: f32, items: Vec<Placed> },
    /// What can't be shown, and why: a labelled box on the slide.
    Problem { message: String },
    /// Where the content between a component's tags goes.
    Children,
    Empty,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Something on a canvas, with its top-left corner at (`x`, `y`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    /// The width to lay it out in, where 0 means its own width.
    #[serde(default)]
    pub width: f32,
    /// Where text sits inside that width.
    #[serde(default)]
    pub align: TextAlign,
    pub element: Element,
}

impl Placed {
    pub fn centered_in(mut self, width: f32) -> Placed {
        self.width = width;
        self.align = TextAlign::Center;
        self
    }

    pub fn right_aligned_in(mut self, width: f32) -> Placed {
        self.width = width;
        self.align = TextAlign::Right;
        self
    }
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
    Element::Shape { kind, width, height, fill: None, outline: None, outline_width: 0.0, radius: 0.0, padding: 0.0, child: None }
}

pub fn picture(src: impl Into<String>, width: f32, height: f32) -> Element {
    Element::Picture { src: src.into(), width, height }
}

pub fn icon(name: impl Into<String>, size: f32) -> Element {
    Element::Icon { name: name.into(), size, color: None }
}

pub fn path(data: impl Into<String>, width: f32, height: f32) -> Element {
    Element::Path { data: data.into(), width, height, fill: None, stroke: None, stroke_width: 0.0 }
}

pub fn canvas(width: f32, height: f32, items: Vec<Placed>) -> Element {
    Element::Canvas { width, height, items }
}

/// Places `element` on a canvas, at its own width.
pub fn at(x: f32, y: f32, element: Element) -> Placed {
    Placed { x, y, width: 0.0, align: TextAlign::Left, element }
}

pub fn problem(message: impl Into<String>) -> Element {
    Element::Problem { message: message.into() }
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
macro_rules! col {
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

    /// The line around a path. Other elements are unchanged.
    pub fn stroke(mut self, color: Color, width: f32) -> Element {
        if let Element::Path { stroke, stroke_width, .. } = &mut self {
            *stroke = Some(color);
            *stroke_width = width;
        }
        self
    }

    pub fn gap(mut self, value: f32) -> Element {
        if let Element::Stack { gap, .. } = &mut self {
            *gap = value;
        }
        self
    }

    /// The space a shape keeps clear between its edge and what is inside. Other elements are unchanged.
    pub fn padding(mut self, value: f32) -> Element {
        if let Element::Shape { padding, .. } = &mut self {
            *padding = value;
        }
        self
    }

    /// What is inside a shape. Other elements are unchanged.
    pub fn child(mut self, inside: Element) -> Element {
        if let Element::Shape { child, .. } = &mut self {
            *child = Some(Box::new(inside));
        }
        self
    }

    /// The corner radius of a `Rounded` shape or a callout. Other elements are unchanged.
    pub fn radius(mut self, value: f32) -> Element {
        if let Element::Shape { radius, .. } = &mut self {
            *radius = value;
        }
        self
    }
}
