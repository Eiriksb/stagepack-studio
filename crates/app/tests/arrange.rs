//! The geometry of moving things on a slide: the box a turned thing covers, and keeping a thing
//! on the slide.

use proptest::prelude::*;
use slides_engine::Rect;
use stagepack_studio::arrange::{Placed, bounds, kept_move, kept_on_slide, straight};

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
