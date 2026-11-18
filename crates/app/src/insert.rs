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
/// The most room a new video takes: it's its own size, made smaller to fit in this.
pub const VIDEO_ROOM: (f32, f32) = (960.0, 540.0);
/// How thick a new line or arrow is drawn.
const LINE_WIDTH: &str = "6";

/// What Undo calls putting each kind of thing on a slide.
pub const INSERT_PICTURE: &str = "Insert picture";
pub const INSERT_SHAPE: &str = "Insert shape";
pub const INSERT_TEXT_BOX: &str = "Insert text box";
pub const INSERT_ICON: &str = "Insert icon";
pub const INSERT_TABLE: &str = "Insert table";
pub const INSERT_COMPONENT: &str = "Insert component";
pub const INSERT_VIDEO: &str = "Insert video";

/// How big a new icon is, as its `size` is written: its width and its height.
pub const ICON_SIZE: &str = "128";

/// The kinds of shape the Shapes list offers, by the talk's names for them.
pub const SHAPE_KINDS: [&str; 6] = ["rect", "rounded", "ellipse", "line", "arrow", "callout"];

/// How big a new video `size` pixels big is put on a slide, in slide units: its own size, its
/// shape kept, made smaller to fit in [`VIDEO_ROOM`]. One with no size is the room itself.
pub fn video_size(size: (u32, u32)) -> (f32, f32) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    if w <= 0.0 || h <= 0.0 {
        return VIDEO_ROOM;
    }
    let scale = (VIDEO_ROOM.0 / w).min(VIDEO_ROOM.1 / h).min(1.0);
    (w * scale, h * scale)
}

/// The top-left corner, in slide units, of a new thing of `size`: centred on the slide, moved
/// 40 down and right for each time that exact place is in `taken`, the top-left corners of the
/// things on the slide, at most ten times, and then held inside the slide. A thing bigger than
/// the slide goes at its corner. Whole numbers, as they're written.
pub fn place_for(size: (f32, f32), taken: &[(f32, f32)]) -> (f32, f32) {
    let (w, h) = size;
    let at = step_aside((((SLIDE.0 - w) / 2.0).round(), ((SLIDE.1 - h) / 2.0).round()), taken);
    let held = |at: f32, length: f32, room: f32| at.min((room - length).round()).max(0.0);
    (held(at.0, w, SLIDE.0), held(at.1, h, SLIDE.1))
}

/// `at`, a top-left corner in slide units, moved 40 down and right for each time that exact
/// place is in `taken`, the top-left corners of the things on the slide, at most ten times.
pub fn step_aside(at: (f32, f32), taken: &[(f32, f32)]) -> (f32, f32) {
    let step = step_aside_together(&[at], taken);
    (at.0 + step, at.1 + step)
}

/// How far down and right, the same for all of them, things put on a slide at the top-left
/// corners `at` are moved so that none lands exactly on one of `taken`, the corners of the
/// things there: the fewest steps of 40 that does it, at most ten. Moved together, things put
/// together keep how they lay.
pub fn step_aside_together(at: &[(f32, f32)], taken: &[(f32, f32)]) -> f32 {
    let is_taken = |(x, y): (f32, f32)| taken.iter().any(|&(tx, ty)| (tx - x).abs() < 0.5 && (ty - y).abs() < 0.5);
    let mut step = 0.0;
    for _ in 0..MOST_STEPS {
        if !at.iter().any(|&(x, y)| is_taken((x + step, y + step))) {
            break;
        }
        step += STEP;
    }
    step
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
