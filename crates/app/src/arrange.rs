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

/// The upright box that holds all of `things`, each as it covers the slide turned, if there are
/// any.
pub fn holding(things: impl IntoIterator<Item = Placed>) -> Option<Rect> {
    let edges = things.into_iter().map(|placed| bounds(&placed)).map(|rect| (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h));
    let (left, top, right, bottom) = edges.reduce(|(l0, t0, r0, b0), (l1, t1, r1, b1)| (l0.min(l1), t0.min(t1), r0.max(r1), b0.max(b1)))?;
    Some(Rect { x: left, y: top, w: right - left, h: bottom - top })
}

/// Which line things are lined up on: an edge or the middle, across or down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignTo {
    Left,
    Centre,
    Right,
    Top,
    Middle,
    Bottom,
}

impl AlignTo {
    /// Whether things lined up on it move across, rather than up or down.
    fn across(self) -> bool {
        matches!(self, AlignTo::Left | AlignTo::Centre | AlignTo::Right)
    }

    /// Where on `rect` this line is, across or down.
    fn line(self, rect: &Rect) -> f32 {
        match self {
            AlignTo::Left => rect.x,
            AlignTo::Centre => rect.x + rect.w / 2.,
            AlignTo::Right => rect.x + rect.w,
            AlignTo::Top => rect.y,
            AlignTo::Middle => rect.y + rect.h / 2.,
            AlignTo::Bottom => rect.y + rect.h,
        }
    }
}

/// `things` lined up on the line `to`: one thing on the slide's, several on that of the box
/// that holds them all. Each moves, across or down only, so that the box it covers as it's
/// turned meets the line; sizes and turns stay as they were. Each is held, as a move is, to
/// keep part of it on the slide, so a line far off it takes a thing only part of the way.
pub fn aligned(things: &[Placed], to: AlignTo) -> Vec<Placed> {
    let slide = Rect { x: 0., y: 0., w: SLIDE_WIDTH, h: SLIDE_HEIGHT };
    let target = if things.len() == 1 { Some(slide) } else { holding(things.iter().copied()) };
    let Some(target) = target else {
        return things.to_vec();
    };
    let line = to.line(&target);
    things
        .iter()
        .map(|placed| {
            let by = line - to.line(&bounds(placed));
            kept_moved(placed, if to.across() { (by, 0.) } else { (0., by) })
        })
        .collect()
}

/// `things` spaced out across, or down, with the same gap between the boxes each covers as
/// it's turned, in their order along that way by their middles. The first and the last stay
/// where they are, and the others move between them, across or down only, each held as a move
/// is to keep part of it on the slide. `None` for fewer than three.
pub fn distributed(things: &[Placed], across: bool) -> Option<Vec<Placed>> {
    if things.len() < 3 {
        return None;
    }
    let span = |placed: &Placed| {
        let covers = bounds(placed);
        if across { (covers.x, covers.w) } else { (covers.y, covers.h) }
    };
    let mut order: Vec<usize> = (0..things.len()).collect();
    order.sort_by(|&a, &b| {
        let middle = |at: usize| {
            let (start, length) = span(&things[at]);
            start + length / 2.
        };
        middle(a).total_cmp(&middle(b)).then(a.cmp(&b))
    });
    let (first, last) = (order[0], order[order.len() - 1]);
    let (first_start, first_length) = span(&things[first]);
    let between: f32 = order[1..order.len() - 1].iter().map(|&at| span(&things[at]).1).sum();
    let gap = (span(&things[last]).0 - (first_start + first_length) - between) / (things.len() - 1) as f32;
    let mut out = things.to_vec();
    let mut at = first_start + first_length + gap;
    for &index in &order[1..order.len() - 1] {
        let (start, length) = span(&things[index]);
        let by = at - start;
        out[index] = kept_moved(&things[index], if across { (by, 0.) } else { (0., by) });
        at += length + gap;
    }
    Some(out)
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

/// `placed` moved by as much of `by` as [`kept_move`] lets it go.
fn kept_moved(placed: &Placed, by: (f32, f32)) -> Placed {
    moved(placed, kept_move(std::slice::from_ref(placed), by))
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
pub fn turned((x, y): (f32, f32), centre: (f32, f32), degrees: f32) -> (f32, f32) {
    if degrees == 0.0 {
        return (x, y);
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (x - centre.0, y - centre.1);
    (centre.0 + dx * cos - dy * sin, centre.1 + dx * sin + dy * cos)
}

/// The corners of the rectangle at `x`, `top`, `w` wide and `h` tall, turned by `degrees`
/// about `centre`.
pub fn corners(x: f32, top: f32, w: f32, h: f32, centre: (f32, f32), degrees: f32) -> [(f32, f32); 4] {
    [(x, top), (x + w, top), (x + w, top + h), (x, top + h)].map(|corner| turned(corner, centre, degrees))
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

/// The thing `w` wide and `h` tall, turned by `rotate`, with the point of it at `(fx, fy)`, as
/// parts of its width and height, at `anchor` on the slide.
fn placed_about(anchor: (f32, f32), (fx, fy): (f32, f32), (w, h): (f32, f32), rotate: f32) -> Placed {
    let (ox, oy) = turned(((fx - 0.5) * w, (fy - 0.5) * h), (0., 0.), rotate);
    let (mx, my) = (anchor.0 - ox, anchor.1 - oy);
    Placed { rect: Rect { x: mx - w / 2., y: my - h / 2., w, h }, rotate }
}

/// How far `at` lies outside where the engine can place a thing.
fn out_of_place(at: f32) -> f32 {
    (MIN_POSITION - at).max(at - MAX_POSITION).max(0.)
}

/// Whether `now`, resized from `start`, keeps as much on the slide each way as [`kept_move`]
/// keeps of a thing moved, and stays where the engine can place it, or no further from either
/// than `start` was.
fn kept_resize(start: &Placed, now: &Placed) -> bool {
    let (before, after) = (bounds(start), bounds(now));
    let on = |low: f32, length: f32, side: f32| ((low + length).min(side) - low.max(0.)).max(0.);
    let kept = |low0: f32, length0: f32, low1: f32, length1: f32, side: f32| {
        on(low1, length1, side) + 0.01 >= on(low0, length0, side).min(length1.min(KEPT_ON))
    };
    let placeable = |was: f32, is: f32| out_of_place(is) <= out_of_place(was) + 0.01;
    kept(before.x, before.w, after.x, after.w, SLIDE_WIDTH)
        && kept(before.y, before.h, after.y, after.h, SLIDE_HEIGHT)
        && placeable(start.rect.x, now.rect.x)
        && placeable(start.rect.y, now.rect.y)
}

/// `start` with `handle` dragged to `to`, in slide units. The corner or edge across from the
/// handle stays where it is on the slide, at any turn. A side the handle changes is never
/// under [`MIN_SIZE`] nor over the most a thing can be, and the thing never flips over; a side
/// it doesn't change stays as it was. With `keep_shape`, a corner keeps the proportions it
/// started with, where both sides can within those limits; an edge never does. It's held, as
/// a move is, to keep part of it on the slide and where the engine can place it. The turn
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
    let across_from = (1. - fx, 1. - fy);
    let (ax, ay) = (x + across_from.0 * w, y + across_from.1 * h);
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
    // The anchor stays where it is on the slide: the middle goes where that leaves it. Where
    // that would take too much of it off the slide, it goes only part of the way there from
    // where it was.
    let from = (held(w, w, across), held(h, h, down));
    let at = |part: f32| {
        let size = (from.0 + (new_w - from.0) * part, from.1 + (new_h - from.1) * part);
        placed_about(anchor, across_from, size, start.rotate)
    };
    let whole_way = at(1.);
    if kept_resize(start, &whole_way) {
        return whole_way;
    }
    let (mut kept, mut lost) = (0f32, 1f32);
    for _ in 0..32 {
        let half = (kept + lost) / 2.;
        if kept_resize(start, &at(half)) {
            kept = half;
        } else {
            lost = half;
        }
    }
    at(kept)
}

/// How far a number has to change, in slide units, to be written again.
const CHANGED: f32 = 0.01;

/// `now`, resized from `start` by `handle`, as it's written: each side that changed to a whole
/// unit, the corner or edge across from the handle kept where it is on the slide, and then the
/// left and the top that changed to a whole unit, so that corner moves by half a unit at most.
pub fn whole(start: &Placed, now: &Placed, handle: Handle) -> Placed {
    if handle == Handle::Turn {
        return *now;
    }
    let to_whole = |is: f32, was: f32| if (is - was).abs() > CHANGED { is.round() } else { is };
    let size = (to_whole(now.rect.w, start.rect.w), to_whole(now.rect.h, start.rect.h));
    let (fx, fy) = handle.spot();
    let across_from = (1. - fx, 1. - fy);
    let anchor = handle_point(now, opposite(handle), 0.);
    let mut placed = placed_about(anchor, across_from, size, now.rotate);
    placed.rect.x = to_whole(placed.rect.x, start.rect.x);
    placed.rect.y = to_whole(placed.rect.y, start.rect.y);
    placed
}

/// The handle across the thing from `handle`, or the turn handle itself.
fn opposite(handle: Handle) -> Handle {
    match handle {
        Handle::TopLeft => Handle::BottomRight,
        Handle::Top => Handle::Bottom,
        Handle::TopRight => Handle::BottomLeft,
        Handle::Right => Handle::Left,
        Handle::BottomRight => Handle::TopLeft,
        Handle::Bottom => Handle::Top,
        Handle::BottomLeft => Handle::TopRight,
        Handle::Left => Handle::Right,
        Handle::Turn => Handle::Turn,
    }
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
