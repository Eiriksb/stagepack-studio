//! The geometry of moving, resizing and turning things on a slide: the box a turned thing
//! covers, keeping a thing on the slide, and where a handle takes it.

use proptest::prelude::*;
use slides_engine::{CaretRect, Rect};
use stagepack_studio::arrange::{Handle, MIN_SIZE, Placed, bounds, handle_point, kept_move, kept_on_slide, resized, straight, turned_to};

/// The corners of `placed`, turned about its centre.
fn corners(placed: &Placed) -> [(f32, f32); 4] {
    let Rect { x, y, w, h } = placed.rect;
    let (cx, cy) = (x + w / 2., y + h / 2.);
    let (sin, cos) = placed.rotate.to_radians().sin_cos();
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(px, py)| {
        let (dx, dy) = (px - cx, py - cy);
        (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos)
    })
}

fn rect() -> impl Strategy<Value = Rect> {
    (-3840f32..7680., -3840f32..7680., 0f32..3840., 0f32..3840.).prop_map(|(x, y, w, h)| Rect { x, y, w, h })
}

/// How much of `low..low + length` lies on `0..side`.
fn on(low: f32, length: f32, side: f32) -> f32 {
    ((low + length).min(side) - low.max(0.)).max(0.)
}

proptest! {
    #[test]
    fn bounds_hold_every_corner(rect in rect(), rotate in -360f32..360.) {
        let placed = Placed { rect, rotate };
        let held = bounds(&placed);
        let slack = 0.01 * (1. + rect.w + rect.h);
        let mut touched = [false; 4];
        for (x, y) in corners(&placed) {
            prop_assert!(held.x - slack <= x && x <= held.x + held.w + slack, "{x} is outside {held:?}");
            prop_assert!(held.y - slack <= y && y <= held.y + held.h + slack, "{y} is outside {held:?}");
            touched[0] |= (x - held.x).abs() <= slack;
            touched[1] |= (x - held.x - held.w).abs() <= slack;
            touched[2] |= (y - held.y).abs() <= slack;
            touched[3] |= (y - held.y - held.h).abs() <= slack;
        }
        prop_assert!(touched.iter().all(|&side| side), "no wider than it needs to be: {held:?}");
    }

    #[test]
    fn kept_on_slide_keeps_sixteen_units(rect in rect()) {
        let kept = kept_on_slide(rect);
        prop_assert_eq!((kept.w, kept.h), (rect.w, rect.h), "it's only moved");
        prop_assert!(on(kept.x, kept.w, 1920.) >= rect.w.min(16.) - 0.01, "{:?}", kept);
        prop_assert!(on(kept.y, kept.h, 1080.) >= rect.h.min(16.) - 0.01, "{:?}", kept);
        prop_assert!((-3840. ..=7680.).contains(&kept.x) && (-3840. ..=7680.).contains(&kept.y));
    }

    #[test]
    fn kept_on_slide_leaves_a_rect_already_inside_be(x in 0f32..1920., y in 0f32..1080., w in 0f32..1920., h in 0f32..1080.) {
        let rect = Rect { x: x.min(1920. - w), y: y.min(1080. - h), w, h };
        prop_assert_eq!(kept_on_slide(rect), rect);
    }

    #[test]
    fn a_move_keeps_every_thing_on_the_slide_or_no_further_off(
        things in proptest::collection::vec((rect(), -360f32..360.), 1..4),
        dx in -20000f32..20000.,
        dy in -20000f32..20000.,
    ) {
        let places: Vec<Placed> = things.into_iter().map(|(rect, rotate)| Placed { rect, rotate }).collect();
        let (mx, my) = kept_move(&places, (dx, dy));
        prop_assert!(mx.abs() <= dx.abs() + 0.01 && my.abs() <= dy.abs() + 0.01, "never further than asked");
        for placed in &places {
            let before = bounds(placed);
            let after = Rect { x: before.x + mx, y: before.y + my, ..before };
            prop_assert!(on(after.x, after.w, 1920.) + 0.01 >= on(before.x, before.w, 1920.).min(after.w.min(16.)));
            prop_assert!(on(after.y, after.h, 1080.) + 0.01 >= on(before.y, before.h, 1080.).min(after.h.min(16.)));
        }
    }
}

#[test]
fn an_upright_thing_covers_itself() {
    let rect = Rect { x: 10., y: 20., w: 300., h: 100. };
    assert_eq!(bounds(&Placed { rect, rotate: 0. }), rect);
    let quarter = bounds(&Placed { rect, rotate: 90. });
    assert!((quarter.w - 100.).abs() < 0.01 && (quarter.h - 300.).abs() < 0.01, "{quarter:?}");
}

#[test]
fn straight_keeps_the_larger_direction() {
    assert_eq!(straight((30., -10.)), (30., 0.));
    assert_eq!(straight((5., -12.)), (0., -12.));
}

#[test]
fn a_thing_dragged_far_away_keeps_sixteen_units_on() {
    let placed = Placed { rect: Rect { x: 1300., y: 300., w: 200., h: 200. }, rotate: 0. };
    assert_eq!(kept_move(&[placed], (-5000., 5000.)), (-1484., 764.));
}

/// The eight handles on a thing's sides and corners, in the order [`Handle::ALL`] has them.
fn side_handles() -> [Handle; 8] {
    std::array::from_fn(|at| Handle::ALL[at])
}

/// The handle across the thing from `handle`.
fn opposite(handle: Handle) -> Handle {
    let at = side_handles().iter().position(|&side| side == handle).expect("a handle on a side");
    side_handles()[(at + 4) % 8]
}

fn side_handle() -> impl Strategy<Value = Handle> {
    (0usize..8).prop_map(|at| side_handles()[at])
}

fn corner_handle() -> impl Strategy<Value = Handle> {
    prop_oneof![Just(Handle::TopLeft), Just(Handle::TopRight), Just(Handle::BottomRight), Just(Handle::BottomLeft)]
}

/// Any turn, and the ones worth trying each time.
fn turn() -> impl Strategy<Value = f32> {
    prop_oneof![Just(0f32), Just(37.), Just(90.), Just(180.), Just(359.9), Just(-360.), -360f32..360.]
}

fn sized(low: f32) -> impl Strategy<Value = Rect> {
    (-3840f32..7680., -3840f32..7680., low..3840., low..3840.).prop_map(|(x, y, w, h)| Rect { x, y, w, h })
}

fn pointer() -> impl Strategy<Value = (f32, f32)> {
    (-6000f32..10000., -6000f32..10000.)
}

fn near((ax, ay): (f32, f32), (bx, by): (f32, f32), within: f32) -> bool {
    (ax - bx).abs() <= within && (ay - by).abs() <= within
}

/// How far apart two turns are, in degrees, the short way round.
fn apart(a: f32, b: f32) -> f32 {
    let off = (a - b).rem_euclid(360.);
    off.min(360. - off)
}

/// The degrees that point a thing's top from `centre` towards `to`.
fn pointing(centre: (f32, f32), to: (f32, f32)) -> f32 {
    (to.0 - centre.0).atan2(centre.1 - to.1).to_degrees()
}

fn centre(placed: &Placed) -> (f32, f32) {
    (placed.rect.x + placed.rect.w / 2., placed.rect.y + placed.rect.h / 2.)
}

proptest! {
    #[test]
    fn the_opposite_corner_stays_put_at_any_turn(
        rect in sized(0.),
        rotate in turn(),
        handle in side_handle(),
        to in pointer(),
        keep_shape in any::<bool>(),
    ) {
        let start = Placed { rect, rotate };
        let anchor = handle_point(&start, opposite(handle), 0.);
        let resized = resized(&start, handle, to, keep_shape);
        let now = handle_point(&resized, opposite(handle), 0.);
        prop_assert!(near(anchor, now, 0.5), "{anchor:?} went to {now:?}: {resized:?}");
        prop_assert_eq!(resized.rotate, rotate, "a resize doesn't turn it");
    }

    #[test]
    fn a_thing_never_gets_smaller_than_eight_units(
        rect in sized(0.),
        rotate in turn(),
        handle in side_handle(),
        to in pointer(),
        keep_shape in any::<bool>(),
    ) {
        let start = Placed { rect, rotate };
        let resized = resized(&start, handle, to, keep_shape);
        let Rect { w, h, .. } = resized.rect;
        let across = !matches!(handle, Handle::Top | Handle::Bottom);
        let down = !matches!(handle, Handle::Left | Handle::Right);
        // A side the handle changes is never under eight units nor over the most a thing can
        // be, and one it doesn't change stays as it was.
        for (side, before, changes) in [(w, rect.w, across), (h, rect.h, down)] {
            if changes {
                prop_assert!((MIN_SIZE - 0.01..=3840.01).contains(&side), "{side} from {before}: {resized:?}");
            } else {
                prop_assert_eq!(side, before);
            }
        }
        // Nor does it flip: the handle stays on the same side of the one across from it.
        let anchor = handle_point(&start, opposite(handle), 0.);
        let (was, is) = (handle_point(&start, handle, 0.), handle_point(&resized, handle, 0.));
        let along = (was.0 - anchor.0) * (is.0 - anchor.0) + (was.1 - anchor.1) * (is.1 - anchor.1);
        if rect.w >= 1. && rect.h >= 1. {
            prop_assert!(along > 0., "{was:?} and {is:?} are either side of {anchor:?}");
        }
    }

    #[test]
    fn keeping_the_shape_keeps_the_proportions(
        rect in sized(MIN_SIZE),
        rotate in turn(),
        handle in corner_handle(),
        to in pointer(),
    ) {
        let start = Placed { rect, rotate };
        // Only where both sides can keep within their limits together.
        let fits = (MIN_SIZE / rect.w).max(MIN_SIZE / rect.h) <= (3840. / rect.w).min(3840. / rect.h);
        prop_assume!(fits);
        let resized = resized(&start, handle, to, true);
        let (before, after) = (rect.w / rect.h, resized.rect.w / resized.rect.h);
        prop_assert!((after / before - 1.).abs() <= 0.01, "{before} became {after}: {resized:?}");
    }

    #[test]
    fn an_edge_handle_changes_one_side_only(
        rect in sized(0.),
        rotate in turn(),
        handle in prop_oneof![Just(Handle::Top), Just(Handle::Right), Just(Handle::Bottom), Just(Handle::Left)],
        to in pointer(),
        keep_shape in any::<bool>(),
    ) {
        let resized = resized(&Placed { rect, rotate }, handle, to, keep_shape);
        if matches!(handle, Handle::Top | Handle::Bottom) {
            prop_assert_eq!(resized.rect.w, rect.w);
        } else {
            prop_assert_eq!(resized.rect.h, rect.h);
        }
    }

    #[test]
    fn handles_sit_on_the_turned_thing(rect in sized(0.), rotate in turn()) {
        let placed = Placed { rect, rotate };
        let Rect { x, y, w, h } = rect;
        let turned = CaretRect { x: 0., top: 0., height: 0., rotate, centre: centre(&placed) };
        let corners = [(Handle::TopLeft, (x, y)), (Handle::TopRight, (x + w, y)), (Handle::BottomRight, (x + w, y + h)), (Handle::BottomLeft, (x, y + h))];
        for (handle, (cx, cy)) in corners {
            let engine = turned.on_slide(cx, cy);
            prop_assert!(near(handle_point(&placed, handle, 30.), engine, 0.05), "{handle:?}");
        }
        // The turn handle is beyond the middle of the top edge, the gap away.
        let top = turned.on_slide(x + w / 2., y - 30.);
        prop_assert!(near(handle_point(&placed, Handle::Turn, 30.), top, 0.05));
    }

    #[test]
    fn turning_points_the_top_at_the_pointer(rect in sized(0.), rotate in turn(), to in pointer()) {
        let start = Placed { rect, rotate };
        let middle = centre(&start);
        prop_assume!(!near(middle, to, 1.));
        let turned = turned_to(&start, to, false);
        prop_assert_eq!(turned.rect, rect, "a turn doesn't move it");
        prop_assert_eq!(turned.rotate, turned.rotate.round(), "whole degrees");
        prop_assert!((-180. ..=180.).contains(&turned.rotate));
        prop_assert!(apart(turned.rotate, pointing(middle, to)) <= 0.51, "{} for {}", turned.rotate, pointing(middle, to));
    }

    #[test]
    fn stepped_turns_are_multiples_of_fifteen(rect in sized(0.), rotate in turn(), to in pointer()) {
        let start = Placed { rect, rotate };
        let middle = centre(&start);
        prop_assume!(!near(middle, to, 1.));
        let turned = turned_to(&start, to, true);
        prop_assert_eq!(turned.rotate % 15., 0.);
        prop_assert!((-180. ..=180.).contains(&turned.rotate));
        prop_assert!(apart(turned.rotate, pointing(middle, to)) <= 7.51);
    }
}

#[test]
fn a_corner_dragged_out_makes_it_bigger() {
    let start = Placed { rect: Rect { x: 100., y: 100., w: 200., h: 100. }, rotate: 0. };
    assert_eq!(resized(&start, Handle::BottomRight, (400., 300.), false).rect, Rect { x: 100., y: 100., w: 300., h: 200. });
    // Kept to its shape, the corner goes as far as the pointer does the furthest way.
    assert_eq!(resized(&start, Handle::BottomRight, (400., 300.), true).rect, Rect { x: 100., y: 100., w: 400., h: 200. });
    // Past the corner across from it, it stays eight units, on its own side.
    assert_eq!(resized(&start, Handle::TopLeft, (900., 900.), false).rect, Rect { x: 292., y: 192., w: 8., h: 8. });
    assert_eq!(resized(&start, Handle::Turn, (900., 900.), false), start);
}
