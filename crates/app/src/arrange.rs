//! Where things go as they're moved, resized and turned on a slide: the box a turned thing
//! covers, how far a move can take things before they'd leave the slide, and where a handle
//! dragged or turned takes a thing. Only numbers, so it's tested alone.

use slides_engine::{MAX_POSITION, MAX_SIZE, MIN_POSITION, Rect, SLIDE_HEIGHT, SLIDE_WIDTH};

/// How many pixels the pointer goes, with the button down, before a press is a drag.
pub const MOVE_STARTS: f32 = 4.0;

/// How much of a thing, in slide units each way, stays on the slide however it's moved.
const KEPT_ON: f32 = 16.0;

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

/// The least a thing's side can be made, in slide units, by dragging a handle.
pub const MIN_SIZE: f32 = 8.0;

/// A handle on a thing selected: one on each corner and side, which resizes it, and one above
/// it, which turns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    Turn,
}

impl Handle {
    pub const ALL: [Handle; 9] = [
        Handle::TopLeft,
        Handle::Top,
        Handle::TopRight,
        Handle::Right,
        Handle::BottomRight,
        Handle::Bottom,
        Handle::BottomLeft,
        Handle::Left,
        Handle::Turn,
    ];

    /// Where the handle is on the thing before it's turned, as parts of its width and height:
    /// the turn handle as the middle of the top.
    fn spot(self) -> (f32, f32) {
        match self {
            Handle::TopLeft => (0., 0.),
            Handle::Top | Handle::Turn => (0.5, 0.),
            Handle::TopRight => (1., 0.),
            Handle::Right => (1., 0.5),
            Handle::BottomRight => (1., 1.),
            Handle::Bottom => (0.5, 1.),
            Handle::BottomLeft => (0., 1.),
            Handle::Left => (0., 0.5),
        }
    }
}

/// `(x, y)` turned by `degrees` about `centre`, as the engine turns a thing.
fn turned((x, y): (f32, f32), (cx, cy): (f32, f32), degrees: f32) -> (f32, f32) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos)
}

fn centre(rect: &Rect) -> (f32, f32) {
    (rect.x + rect.w / 2., rect.y + rect.h / 2.)
}

/// Where `handle` sits on the slide, on `placed` turned: the turn handle `turn_gap` units
/// beyond the middle of its top edge.
pub fn handle_point(placed: &Placed, handle: Handle, turn_gap: f32) -> (f32, f32) {
    let Rect { x, y, w, h } = placed.rect;
    let (fx, fy) = handle.spot();
    let gap = if handle == Handle::Turn { turn_gap } else { 0. };
    turned((x + fx * w, y + fy * h - gap), centre(&placed.rect), placed.rotate)
}

/// `start` with `handle` dragged to `to`, in slide units. The corner or edge across from the
/// handle stays where it is on the slide, at any turn. A side the handle changes is never
/// under [`MIN_SIZE`] nor over the most a thing can be, and the thing never flips over; a side
/// it doesn't change stays as it was. With `keep_shape`, a corner keeps the proportions it
/// started with, where both sides can within those limits; an edge never does. The turn
/// handle resizes nothing.
pub fn resized(start: &Placed, handle: Handle, to: (f32, f32), keep_shape: bool) -> Placed {
    if handle == Handle::Turn {
        return *start;
    }
    let Rect { x, y, w, h } = start.rect;
    let middle = centre(&start.rect);
    // The pointer as it is on the thing before it's turned.
    let (px, py) = turned(to, middle, -start.rotate);
    let (fx, fy) = handle.spot();
    let (ax, ay) = (x + (1. - fx) * w, y + (1. - fy) * h);
    let anchor = turned((ax, ay), middle, start.rotate);
    let side = |part: f32, pointer: f32, across: f32, now: f32| {
        if part == 1. {
            pointer - across
        } else if part == 0. {
            across - pointer
        } else {
            now
        }
    };
    let (raw_w, raw_h) = (side(fx, px, ax, w), side(fy, py, ay, h));
    let held = |length: f32, now: f32, changes: bool| if changes { length.clamp(MIN_SIZE, MAX_SIZE) } else { now };
    let (across, down) = (fx != 0.5, fy != 0.5);
    let (mut new_w, mut new_h) = (held(raw_w, w, across), held(raw_h, h, down));
    if keep_shape && across && down && w > 0. && h > 0. {
        // As far as the pointer goes the furthest way, within what both sides can be.
        let (low, high) = ((MIN_SIZE / w).max(MIN_SIZE / h), (MAX_SIZE / w).min(MAX_SIZE / h));
        if low <= high {
            let scale = (raw_w / w).max(raw_h / h).clamp(low, high);
            (new_w, new_h) = (w * scale, h * scale);
        }
    }
    // The anchor stays where it is on the slide: the middle goes where that leaves it.
    let (ox, oy) = turned(((0.5 - fx) * new_w, (0.5 - fy) * new_h), (0., 0.), start.rotate);
    let (mx, my) = (anchor.0 - ox, anchor.1 - oy);
    Placed { rect: Rect { x: mx - new_w / 2., y: my - new_h / 2., w: new_w, h: new_h }, rotate: start.rotate }
}

/// `start` turned so that its top points from its middle towards `to`, in slide units: in whole
/// degrees from -180 to 180, or in steps of 15 when `stepped`. At its very middle, it stays.
pub fn turned_to(start: &Placed, to: (f32, f32), stepped: bool) -> Placed {
    let (cx, cy) = centre(&start.rect);
    let (dx, dy) = (to.0 - cx, to.1 - cy);
    if dx == 0. && dy == 0. || !dx.is_finite() || !dy.is_finite() {
        return *start;
    }
    let degrees = dx.atan2(-dy).to_degrees();
    let step = if stepped { 15. } else { 1. };
    let rotate = ((degrees / step).round() * step).clamp(-180., 180.);
    // Never -0, which is written as it reads.
    Placed { rect: start.rect, rotate: rotate + 0. }
}
