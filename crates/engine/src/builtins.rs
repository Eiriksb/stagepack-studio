//! The components every talk can use. They are ordinary components, made the way a
//! talk's own are.

use toolkit::prelude::*;

use crate::{ButtonAction, ButtonFault, ButtonLook};

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

/// A terminal on a slide, laid out by the engine itself as its recording's last screen or a panel
/// naming its command.
fn terminal_component() -> NativeComponent {
    fn schema() -> ComponentSchema {
        // Either `src` or `run` is wanted, so neither is needed alone.
        let fields = vec![
            PropField::new("src", PropKind::File(vec!["cast".to_string()]), true),
            PropField::new("run", PropKind::Text, true),
            PropField::new("cols", PropKind::Whole, true),
            PropField::new("rows", PropKind::Whole, true),
        ];
        ComponentSchema::new(crate::terminal_screen::TERMINAL_TAG, fields, false)
    }
    fn render(_: &Props, _: &Ctx) -> Element {
        Element::Empty
    }
    NativeComponent { name: crate::terminal_screen::TERMINAL_TAG, schema, render }
}

/// The tag of a button on a slide.
pub const BUTTON_TAG: &str = "Button";
/// What's said of a button that gives none of `run`, `do` and `step`.
pub const BUTTON_NO_ACTION: &str = "This button has no action.";
/// What's said of a button that gives more than one of them.
pub const BUTTON_MANY_ACTIONS: &str = "This button has more than one action: run, do or step.";
/// What's said of a button whose `step` isn't one of the slide's: a whole number from 0 to its last click.
pub const BUTTON_STEP_PAST: &str = "This button's step is past the slide's last click.";
/// What's said of a button whose `in` names no live terminal on its slide.
pub const BUTTON_IN_NOTHING: &str = "This button's `in` names no live terminal on this slide.";
/// What's said of a button whose `on` names nothing on its slide that takes an action.
pub const BUTTON_ON_NOTHING: &str = "This button's `on` names nothing it can act on.";
/// What's said written in the text, not placed on the slide.
pub const BUTTON_IN_TEXT: &str = "This button works only when it's placed on the slide. Drag it out of the text to place it.";
/// The looks a button can have, as a talk writes them, the first being the one it has if it says none.
const BUTTON_LOOKS: [(&str, ButtonLook); 3] = [("primary", ButtonLook::Primary), ("secondary", ButtonLook::Secondary), ("quiet", ButtonLook::Quiet)];

/// A button on a slide. The engine lays it out itself, as a video, so what this gives is never
/// shown: it's here for what it takes, and so a talk's own component can't take its name. Its
/// `id` is a placement attribute, which no component is given.
fn button_component() -> NativeComponent {
    fn schema() -> ComponentSchema {
        let text = |name: &str| PropField::new(name, PropKind::Text, true);
        let looks = BUTTON_LOOKS.iter().map(|(name, _)| name.to_string()).collect();
        let fields = vec![
            PropField::new("label", PropKind::Text, false),
            PropField::new("look", PropKind::Choice(looks), true),
            text("run"),
            text("in"),
            text("do"),
            text("on"),
            text("value"),
            PropField::new("step", PropKind::Whole, true),
        ];
        ComponentSchema::new(BUTTON_TAG, fields, false)
    }
    fn render(_: &Props, _: &Ctx) -> Element {
        Element::Empty
    }
    NativeComponent { name: BUTTON_TAG, schema, render }
}

/// The look a button's `look` names: primary if it names none, and primary, with what's said of
/// it, if it names one it can't have.
pub(crate) fn read_button_look(value: Option<&str>) -> (ButtonLook, Option<String>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else { return (ButtonLook::Primary, None) };
    match BUTTON_LOOKS.iter().find(|(name, _)| name.eq_ignore_ascii_case(value)) {
        Some((_, look)) => (*look, None),
        None => (ButtonLook::Primary, Some(format!("A button's look can be primary, secondary or quiet, not {value}, so it's primary"))),
    }
}

/// What pressing the button `tag` does: the one of `run`, `do` and `step` it gives, or why it
/// does nothing.
pub(crate) fn read_button_action(tag: &slides_format::Tag) -> Result<ButtonAction, ButtonFault> {
    let (run, act, step) = (tag.attr("run"), tag.attr("do"), tag.attr("step"));
    let given = [run.is_some(), act.is_some(), step.is_some()].into_iter().filter(|given| *given).count();
    let named = |name: &str| tag.attr(name).unwrap_or_default().trim().to_string();
    match (given, run, act, step) {
        (0, ..) => Err(ButtonFault::NoAction),
        (2.., ..) => Err(ButtonFault::ManyActions),
        // Typed as it's written: it's matched exactly when it's asked about.
        (_, Some(text), ..) => Ok(ButtonAction::Run { text: text.to_string(), terminal: named("in") }),
        (_, _, Some(action), _) => Ok(ButtonAction::Do { action: action.trim().to_string(), target: named("on"), value: tag.attr("value").unwrap_or_default().to_string() }),
        (_, _, _, Some(step)) => step.trim().parse::<u32>().map(ButtonAction::Step).map_err(|_| ButtonFault::BadStep),
        _ => Err(ButtonFault::NoAction),
    }
}

/// What's said of a button that does nothing for `fault`.
pub fn button_fault_notice(fault: ButtonFault) -> &'static str {
    match fault {
        ButtonFault::NoAction => BUTTON_NO_ACTION,
        ButtonFault::ManyActions => BUTTON_MANY_ACTIONS,
        ButtonFault::BadStep => BUTTON_STEP_PAST,
    }
}

/// `from` taken `amount` of the way to `to`.
fn mixed(from: Color, to: Color, amount: f32) -> Color {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round().clamp(0.0, 255.0) as u8;
    Color::rgb(channel(from.r, to.r), channel(from.g, to.g), channel(from.b, to.b))
}

/// How light `colour` looks, from 0 to 1.
fn lightness(colour: Color) -> f32 {
    (0.2126 * f32::from(colour.r) + 0.7152 * f32::from(colour.g) + 0.0722 * f32::from(colour.b)) / 255.0
}

/// The fill and the text colour of a button that looks `look`, from `theme`: the accent, a tint
/// of it, or a faint tint of the text colour, on the background. The words are whichever of the
/// background and text colours stands out more from the fill.
pub(crate) fn button_colours(look: ButtonLook, theme: &Theme) -> (Color, Color) {
    let fill = match look {
        ButtonLook::Primary => theme.accent,
        ButtonLook::Secondary => mixed(theme.background, theme.accent, 0.18),
        ButtonLook::Quiet => mixed(theme.background, theme.text, 0.08),
    };
    let apart = |colour: Color| (lightness(colour) - lightness(fill)).abs();
    let words = if apart(theme.background) > apart(theme.text) { theme.background } else { theme.text };
    (fill, words)
}

pub fn builtin_registry() -> Registry {
    let mut registry = Registry::default();
    let components = [boxed::Box_component(), Shape_component(), Icon_component(), Picture_component(), crate::chart::Chart_component(), video_component(), model_component(), terminal_component(), button_component()];
    for component in components {
        registry.add(component);
    }
    registry
}
