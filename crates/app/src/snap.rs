//! Where a thing being moved or resized snaps to: the slide's edges and middle, and the edges
//! and middles of the things around it, and the guide lines that show it. Only numbers, so
//! it's tested alone.

use slides_engine::{Rect, SLIDE_HEIGHT, SLIDE_WIDTH};

/// How near two lines are, in slide units, to be the same line.
const MET: f32 = 0.01;

/// A line on the slide, in slide units: up and down at `at` across, from `from` to `to` down,
/// or, not `vertical`, across at `at` down, from `from` to `to` across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub at: f32,
    pub from: f32,
    pub to: f32,
}

/// How far a box snaps across and down, and the lines it then meets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapped {
    pub dx: f32,
    pub dy: f32,
    pub guides: Vec<Guide>,
}

/// The three lines of a box one way: its start, its middle and its end.
fn lines(start: f32, length: f32) -> [f32; 3] {
    [start, start + length / 2., start + length]
}

/// Where `rect` is along one way, and along the other: across for an upright line.
fn along(rect: &Rect, vertical: bool) -> ((f32, f32), (f32, f32)) {
    if vertical { ((rect.x, rect.w), (rect.y, rect.h)) } else { ((rect.y, rect.h), (rect.x, rect.w)) }
}

fn usable(rect: &Rect) -> bool {
    [rect.x, rect.y, rect.w, rect.h].iter().all(|value| value.is_finite()) && rect.w >= 0. && rect.h >= 0.
}

/// The slide, then each of `others` that can be snapped to.
fn targets(others: &[Rect]) -> impl Iterator<Item = &Rect> + '_ {
    const SLIDE: Rect = Rect { x: 0., y: 0., w: SLIDE_WIDTH, h: SLIDE_HEIGHT };
    std::iter::once(&SLIDE).chain(others.iter().filter(|rect| usable(rect)))
}

/// The smallest shift, at most `within`, that puts one of `moving`'s lines on a line of one of
/// `others` or of the slide, upright lines if `vertical`: the first of several as near.
fn nearest(moving: &[f32], vertical: bool, others: &[Rect], within: f32) -> Option<f32> {
    if within.is_nan() || within < 0. || moving.iter().any(|line| !line.is_finite()) {
        return None;
    }
    let mut best: Option<f32> = None;
    for target in targets(others) {
        let ((start, length), _) = along(target, vertical);
        for line in lines(start, length) {
            for &from in moving {
                let shift = line - from;
                if shift.abs() <= within && best.is_none_or(|best| shift.abs() < best.abs()) {
                    best = Some(shift);
                }
            }
        }
    }
    best
}

/// A guide for each line of the slide and of `others` that one of `moving`, a box's lines one
/// way, is on, upright ones if `vertical`, running the length of the box with the line and of
/// `beside`, where the moving box is the other way, if it's given. Two on the same line are one.
fn guides_met(moving: &[f32], vertical: bool, beside: Option<(f32, f32)>, others: &[Rect]) -> Vec<Guide> {
    let mut guides: Vec<Guide> = Vec::new();
    for target in targets(others) {
        let ((start, length), (across, size)) = along(target, vertical);
        for line in lines(start, length) {
            if !moving.iter().any(|&on| (on - line).abs() <= MET) {
                continue;
            }
            let (from, to) = match beside {
                Some((start, length)) => (across.min(start), (across + size).max(start + length)),
                None => (across, across + size),
            };
            match guides.iter_mut().find(|guide| (guide.at - line).abs() <= MET) {
                Some(guide) => (guide.from, guide.to) = (guide.from.min(from), guide.to.max(to)),
                None => guides.push(Guide { vertical, at: line, from, to }),
            }
        }
    }
    guides
}

/// The smallest shift, at most `within` each way, that puts `moving`'s left, middle or right on
/// the slide's left, middle or right, or on another of `others`' left, middle or right, and the
/// same for top, middle and bottom: each way decided on its own. A guide is given for each line
/// met, running the length of both boxes. Nothing within reach gives no shift and no guide.
pub fn snap_move(moving: Rect, others: &[Rect], within: f32) -> Snapped {
    if !usable(&moving) {
        return Snapped::default();
    }
    let across = nearest(&lines(moving.x, moving.w), true, others, within);
    let down = nearest(&lines(moving.y, moving.h), false, others, within);
    let (dx, dy) = (across.unwrap_or(0.), down.unwrap_or(0.));
    let now = Rect { x: moving.x + dx, y: moving.y + dy, ..moving };
    let mut guides = Vec::new();
    if across.is_some() {
        guides.extend(guides_met(&lines(now.x, now.w), true, Some((now.y, now.h)), others));
    }
    if down.is_some() {
        guides.extend(guides_met(&lines(now.y, now.h), false, Some((now.x, now.w)), others));
    }
    Snapped { dx, dy, guides }
}

/// The smallest shift, at most `within`, that puts `edge`, one side of a box being resized, on
/// a line of the slide or of `others`, as [`snap_move`] does for a box's lines: an upright side
/// if `vertical`. Gives the shift and the guide, which runs the length of what has the line;
/// nothing within reach gives `None`.
pub fn snap_edge(edge: f32, vertical: bool, others: &[Rect], within: f32) -> Option<(f32, Guide)> {
    let shift = nearest(&[edge], vertical, others, within)?;
    let guide = guides_met(&[edge + shift], vertical, None, others).into_iter().next()?;
    Some((shift, guide))
}
