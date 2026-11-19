//! The components every talk can use. They are ordinary components, made the way a
//! talk's own are.

use toolkit::prelude::*;

pub(crate) const DEFAULT_ICON_SIZE: f32 = 64.0;
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

/// The kinds of shape a `Shape` can be. Anything else a talk gives is a rectangle.
#[derive(Prop, Clone, Copy)]
enum Kind {
    Rect,
    Rounded,
    Ellipse,
    Line,
    Arrow,
    Callout,
}

#[component]
fn Shape(kind: Kind, fill: Option<String>, outline: Option<String>, outline_width: f32, children: Children) -> Element {
    let kind = match kind {
        Kind::Rect => ShapeKind::Rect,
        Kind::Rounded => ShapeKind::Rounded,
        Kind::Ellipse => ShapeKind::Ellipse,
        Kind::Line => ShapeKind::Line,
        Kind::Arrow => ShapeKind::Arrow,
        Kind::Callout => ShapeKind::Callout,
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

/// A video on a slide. The engine lays it out itself, as its still, so what this gives is
/// never shown: it's here for what it takes, and so a talk's own component can't take its name.
fn video_component() -> NativeComponent {
    fn schema() -> ComponentSchema {
        let videos = ["mp4", "m4v", "mov"].map(String::from).to_vec();
        let pictures = ["png", "jpg", "jpeg", "webp", "svg"].map(String::from).to_vec();
        let fields = vec![
            PropField::new("src", PropKind::File(videos), false),
            PropField::new("poster", PropKind::File(pictures), true),
            PropField::new("loop", PropKind::Flag, true),
        ];
        ComponentSchema::new(crate::elements::VIDEO_TAG, fields, false)
    }
    fn render(_: &Props, _: &Ctx) -> Element {
        Element::Empty
    }
    NativeComponent { name: crate::elements::VIDEO_TAG, schema, render }
}

/// A 3D model on a slide, laid out by the engine itself as its still, as a video is.
fn model_component() -> NativeComponent {
    fn schema() -> ComponentSchema {
        let models = ["glb", "ply"].map(String::from).to_vec();
        let fields = vec![
            PropField::new("src", PropKind::File(models), false),
            PropField::new("turn", PropKind::Choice(vec!["auto".to_string()]), true),
            PropField::new("view", PropKind::Text, true),
            PropField::new("views", PropKind::Text, true),
        ];
        ComponentSchema::new(crate::elements::MODEL_TAG, fields, false)
    }
    fn render(_: &Props, _: &Ctx) -> Element {
        Element::Empty
    }
    NativeComponent { name: crate::elements::MODEL_TAG, schema, render }
}

/// A terminal recording on a slide, laid out by the engine itself as its last screen.
fn terminal_component() -> NativeComponent {
    fn schema() -> ComponentSchema {
        let fields = vec![PropField::new("src", PropKind::File(vec!["cast".to_string()]), false)];
        ComponentSchema::new(crate::terminal_screen::TERMINAL_TAG, fields, false)
    }
    fn render(_: &Props, _: &Ctx) -> Element {
        Element::Empty
    }
    NativeComponent { name: crate::terminal_screen::TERMINAL_TAG, schema, render }
}

pub fn builtin_registry() -> Registry {
    let mut registry = Registry::default();
    let components = [boxed::Box_component(), Shape_component(), Icon_component(), Picture_component(), crate::chart::Chart_component(), video_component(), model_component(), terminal_component()];
    for component in components {
        registry.add(component);
    }
    registry
}
