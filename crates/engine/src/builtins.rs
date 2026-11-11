//! The components every talk can use. They are ordinary components, made the way a
//! talk's own are.

use toolkit::prelude::*;

const DEFAULT_ICON_SIZE: f32 = 64.0;
/// The space a box with a fill or an outline keeps between its edge and its content.
const DEFAULT_BOX_PADDING: f32 = 24.0;

/// `Box` has a module to itself, so its name can't be mistaken for the standard `Box`.
mod boxed {
    use toolkit::prelude::*;

    #[component]
    pub fn Box(fill: Option<String>, outline: Option<String>, outline_width: f32, radius: f32, padding: Option<f32>, children: Children) -> Element {
        let padding = padding.unwrap_or(if fill.is_some() || outline.is_some() { super::DEFAULT_BOX_PADDING } else { 0.0 });
        let kind = if radius > 0.0 { ShapeKind::Rounded } else { ShapeKind::Rect };
        // In a stack, so that they start at the corner as in a text box. A shape centres anything else.
        super::shape_element(kind, fill, outline, outline_width, radius, padding, col![children.into()])
    }
}

/// A colour attribute. One that can't be read counts as absent, rather than as black.
fn colour(value: Option<String>) -> Option<Color> {
    value.as_deref().and_then(Color::parse)
}

/// A shape that takes its size from the box it's placed in.
fn shape_element(
    kind: ShapeKind,
    fill: Option<String>,
    outline: Option<String>,
    outline_width: f32,
    radius: f32,
    padding: f32,
    child: Element,
) -> Element {
    let outline = colour(outline);
    Element::Shape {
        kind,
        width: 0.0,
        height: 0.0,
        fill: colour(fill),
        outline,
        outline_width: if outline.is_some() && outline_width == 0.0 { 1.0 } else { outline_width },
        radius,
        padding,
        child: Some(Box::new(child)),
    }
}

#[component]
fn Shape(kind: String, fill: Option<String>, outline: Option<String>, outline_width: f32, children: Children) -> Element {
    let kind = match kind.as_str() {
        "rounded" => ShapeKind::Rounded,
        "ellipse" => ShapeKind::Ellipse,
        "line" => ShapeKind::Line,
        "arrow" => ShapeKind::Arrow,
        "callout" => ShapeKind::Callout,
        _ => ShapeKind::Rect,
    };
    shape_element(kind, fill, outline, outline_width, 0.0, 0.0, children.into())
}

#[component]
fn Icon(name: String, size: Option<f32>, color: Option<String>) -> Element {
    let size = size.filter(|size| *size > 0.0).unwrap_or(DEFAULT_ICON_SIZE);
    Element::Icon { name, size, color: colour(color) }
}

#[component]
fn Picture(src: PictureFile) -> Element {
    picture(src.0, 0.0, 0.0)
}

pub fn builtin_registry() -> Registry {
    let mut registry = Registry::default();
    for component in [boxed::Box_component(), Shape_component(), Icon_component(), Picture_component(), crate::chart::Chart_component()] {
        registry.add(component);
    }
    registry
}
