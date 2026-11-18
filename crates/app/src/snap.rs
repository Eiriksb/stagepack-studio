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
/// `others` or of the slide, upright lines if `vertical`, and that really puts it there. Of
/// several as near, the first wins: the slide's before the others', in their order, and each
/// box's start, middle and end in that order, met by the moving start, middle and end in turn;
/// a shift either way as near is no different.
/// A `within` that isn't a number, or is less than none or endless, snaps to nothing.
fn nearest(moving: &[f32], vertical: bool, others: &[Rect], within: f32) -> Option<f32> {
    if !within.is_finite() || within < 0. || moving.iter().any(|line| !line.is_finite()) {
        return None;
    }
    let mut best: Option<f32> = None;
    for target in targets(others) {
        let ((start, length), _) = along(target, vertical);
        for line in lines(start, length).into_iter().filter(|line| line.is_finite()) {
            for &from in moving {
                let shift = line - from;
                // Far out, where the numbers are too coarse, a shift can miss its line.
                let lands = (from + shift - line).abs() <= MET;
                if lands && shift.abs() <= within && best.is_none_or(|best| shift.abs() < best.abs()) {
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
        for line in lines(start, length).into_iter().filter(|line| line.is_finite()) {
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
/// met, running the length of both boxes. Nothing within reach gives no shift and no guide, and
/// so does a box that isn't a number, endless, or less than nothing wide or tall.
pub fn snap_move(moving: Rect, others: &[Rect], within: f32) -> Snapped {
    if !usable(&moving) {
        return Snapped::default();
    }
    let mut across = nearest(&lines(moving.x, moving.w), true, others, within);
    let mut down = nearest(&lines(moving.y, moving.h), false, others, within);
    // A way whose shifted box meets no line, as can happen far out, doesn't snap, as an edge
    // doesn't.
    let meets = |shift: Option<f32>, start: f32, length: f32, vertical: bool| {
        shift.filter(|shift| !guides_met(&lines(start + shift, length), vertical, None, others).is_empty())
    };
    across = meets(across, moving.x, moving.w, true);
    down = meets(down, moving.y, moving.h, false);
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

/// A guide for each line of the slide and of `others` that one of `moving`'s lines is on, as
/// [`snap_move`] gives them, without snapping it: what a box shown where it is meets.
pub fn guides_on(moving: Rect, others: &[Rect]) -> Vec<Guide> {
    if !usable(&moving) {
        return Vec::new();
    }
    let mut guides = guides_met(&lines(moving.x, moving.w), true, Some((moving.y, moving.h)), others);
    guides.extend(guides_met(&lines(moving.y, moving.h), false, Some((moving.x, moving.w)), others));
    guides
}
