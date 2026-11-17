//! Where things go as they're moved on a slide: the box a turned thing covers, and how far a
//! move can take things before they'd leave the slide. Only numbers, so it's tested alone.

use slides_engine::Rect;

/// How many pixels the pointer goes, with the button down, before a press is a drag.
pub const MOVE_STARTS: f32 = 4.0;

/// What's said when a thing written in the slide's text, which has no place of its own, is
/// moved with the keys.
pub const DRAG_OUT_FIRST: &str = "Drag it out of the text first.";

const SLIDE_WIDTH: f32 = 1920.0;
const SLIDE_HEIGHT: f32 = 1080.0;
/// How much of a thing, in slide units each way, stays on the slide however it's moved.
const KEPT_ON: f32 = 16.0;
/// The farthest a thing can be placed from the slide's corner, either way, as the engine holds it.
const MIN_POSITION: f32 = -3840.0;
const MAX_POSITION: f32 = 7680.0;

/// Where a thing or an area is: its rectangle before it's turned, in slide units, and the
/// degrees it's turned by about the rectangle's centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub rect: Rect,
    pub rotate: f32,
}

/// The upright box that `placed`, turned, covers.
pub fn bounds(placed: &Placed) -> Rect {
    let Rect { x, y, w, h } = placed.rect;
    let (sin, cos) = placed.rotate.to_radians().sin_cos();
    let (sin, cos) = (sin.abs(), cos.abs());
    let (width, height) = (w * cos + h * sin, w * sin + h * cos);
    let (cx, cy) = (x + w / 2., y + h / 2.);
    Rect { x: cx - width / 2., y: cy - height / 2., w: width, h: height }
}

/// `rect` moved, if it has to be, so that at least [`KEPT_ON`] units of it are on the slide
/// each way, or all of it where it's smaller than that, and held within where the engine can
/// place a thing.
pub fn kept_on_slide(rect: Rect) -> Rect {
    let along = |at: f32, length: f32, side: f32| {
        let need = length.min(KEPT_ON);
        at.clamp(need - length, side - need).clamp(MIN_POSITION, MAX_POSITION)
    };
    Rect { x: along(rect.x, rect.w, SLIDE_WIDTH), y: along(rect.y, rect.h, SLIDE_HEIGHT), ..rect }
}

/// `placed` moved by `by`, in slide units.
pub fn moved(placed: &Placed, (dx, dy): (f32, f32)) -> Placed {
    Placed { rect: Rect { x: placed.rect.x + dx, y: placed.rect.y + dy, ..placed.rect }, rotate: placed.rotate }
}

/// `by` kept to one direction, across or up and down, whichever it goes further in.
pub fn straight((dx, dy): (f32, f32)) -> (f32, f32) {
    if dx.abs() >= dy.abs() { (dx, 0.) } else { (0., dy) }
}

/// As much of the move `by` as every one of `things` can take together: each keeps at least
/// [`KEPT_ON`] units on the slide each way, as [`kept_on_slide`] has it, and stays where the
/// engine can place it; one that's further off than that already is never taken further.
pub fn kept_move(things: &[Placed], (dx, dy): (f32, f32)) -> (f32, f32) {
    // How far each can go, either way, holding to 0 for one already outside.
    let mut across = (f32::NEG_INFINITY, f32::INFINITY);
    let mut down = (f32::NEG_INFINITY, f32::INFINITY);
    let hold = |range: &mut (f32, f32), low: f32, high: f32| {
        range.0 = range.0.max(low.min(0.));
        range.1 = range.1.min(high.max(0.));
    };
    for placed in things {
        let covers = bounds(placed);
        let (need_x, need_y) = (covers.w.min(KEPT_ON), covers.h.min(KEPT_ON));
        hold(&mut across, need_x - covers.x - covers.w, SLIDE_WIDTH - need_x - covers.x);
        hold(&mut down, need_y - covers.y - covers.h, SLIDE_HEIGHT - need_y - covers.y);
        hold(&mut across, MIN_POSITION - placed.rect.x, MAX_POSITION - placed.rect.x);
        hold(&mut down, MIN_POSITION - placed.rect.y, MAX_POSITION - placed.rect.y);
    }
    let within = |by: f32, (low, high): (f32, f32)| if by.is_finite() { by.clamp(low, high) } else { 0. };
    (within(dx, across), within(dy, down))
}
