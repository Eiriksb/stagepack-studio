//! What a new thing on a slide is written as, and where it goes: the pure part of the Insert
//! tab, which knows nothing of the window.

use toolkit::{Color, Theme};

/// The slide's size, in slide units.
const SLIDE: (f32, f32) = (1920.0, 1080.0);
/// How far down and right a new thing is put from one already where it would go.
const STEP: f32 = 40.0;
/// The most times a new thing steps aside.
const MOST_STEPS: usize = 10;

/// The size of a new shape that has an inside, and of a line or an arrow.
pub const SHAPE_SIZE: (f32, f32) = (480.0, 320.0);
pub const LINE_SIZE: (f32, f32) = (480.0, 120.0);
/// The size of a new text box.
pub const TEXT_BOX_SIZE: (f32, f32) = (640.0, 200.0);
/// How thick a new line or arrow is drawn.
const LINE_WIDTH: &str = "6";

/// What Undo calls putting each kind of thing on a slide.
pub const INSERT_PICTURE: &str = "Insert picture";
pub const INSERT_SHAPE: &str = "Insert shape";
pub const INSERT_TEXT_BOX: &str = "Insert text box";
pub const INSERT_ICON: &str = "Insert icon";

/// How big a new icon is, as its `size` is written: its width and its height.
pub const ICON_SIZE: &str = "128";

/// The kinds of shape the Shapes list offers, by the talk's names for them.
pub const SHAPE_KINDS: [&str; 6] = ["rect", "rounded", "ellipse", "line", "arrow", "callout"];

/// The top-left corner, in slide units, of a new thing of `size`: centred on the slide, moved
/// 40 down and right for each time that exact place is in `taken`, the top-left corners of the
/// things on the slide, at most ten times, and then held inside the slide. A thing bigger than
/// the slide goes at its corner. Whole numbers, as they're written.
pub fn place_for(size: (f32, f32), taken: &[(f32, f32)]) -> (f32, f32) {
    let (w, h) = size;
    let mut at = (((SLIDE.0 - w) / 2.0).round(), ((SLIDE.1 - h) / 2.0).round());
    let is_taken = |(x, y): (f32, f32)| taken.iter().any(|&(tx, ty)| (tx - x).abs() < 0.5 && (ty - y).abs() < 0.5);
    for _ in 0..MOST_STEPS {
        if !is_taken(at) {
            break;
        }
        at = (at.0 + STEP, at.1 + STEP);
    }
    let held = |at: f32, length: f32, room: f32| at.min((room - length).round()).max(0.0);
    (held(at.0, w, SLIDE.0), held(at.1, h, SLIDE.1))
}

/// Whether a shape of kind `kind` is a line, with no inside to fill.
pub fn is_line(kind: &str) -> bool {
    matches!(kind, "line" | "arrow")
}

/// The size a new shape of kind `kind` is given.
pub fn shape_size(kind: &str) -> (f32, f32) {
    if is_line(kind) { LINE_SIZE } else { SHAPE_SIZE }
}

/// The attributes of a new shape of kind `kind`, before where it is: a shape with an inside is
/// filled with the theme's accent, a line or an arrow drawn in its text colour.
pub fn shape_attrs(kind: &str, theme: &Theme) -> Vec<(&'static str, String)> {
    let kind_attr = ("kind", kind.to_string());
    if is_line(kind) {
        vec![kind_attr, ("outline", hex(theme.text)), ("outline-width", LINE_WIDTH.to_string())]
    } else {
        vec![kind_attr, ("fill", hex(theme.accent))]
    }
}

/// `color` as a talk writes it, `#rrggbb`.
pub fn hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// A whole number of slide units, as a place or a size is written.
pub fn whole(value: f32) -> String {
    format!("{}", value.round().max(0.0) as i64)
}
