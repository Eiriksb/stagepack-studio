//! Snapping a box being moved, or an edge being resized, to the slide and to the boxes around
//! it: never further than it reaches, always onto a line really shared, the nearest first.

use proptest::prelude::*;
use slides_engine::{Rect, SLIDE_HEIGHT, SLIDE_WIDTH};
use stagepack_studio::snap::{Guide, Snapped, snap_edge, snap_move};

/// The three lines of a box one way.
fn lines(start: f32, length: f32) -> [f32; 3] {
    [start, start + length / 2., start + length]
}

/// Every line one way of the slide and of `others`: upright ones if `vertical`.
fn targets(others: &[Rect], vertical: bool) -> Vec<f32> {
    let slide = Rect { x: 0., y: 0., w: SLIDE_WIDTH, h: SLIDE_HEIGHT };
    std::iter::once(&slide)
        .chain(others)
        .flat_map(|rect| if vertical { lines(rect.x, rect.w) } else { lines(rect.y, rect.h) })
        .collect()
}

/// The least distance between one of `moving` and one of `targets`.
fn least_gap(moving: &[f32], targets: &[f32]) -> f32 {
    moving.iter().flat_map(|from| targets.iter().map(move |to| (to - from).abs())).fold(f32::INFINITY, f32::min)
}

fn moved(rect: Rect, snapped: &Snapped) -> Rect {
    Rect { x: rect.x + snapped.dx, y: rect.y + snapped.dy, ..rect }
}

fn a_rect() -> impl Strategy<Value = Rect> {
    (-200f32..2100., -200f32..1300., 1f32..800., 1f32..600.).prop_map(|(x, y, w, h)| Rect { x, y, w, h })
}

/// Whole numbers, as things are most often placed, so that lines often meet exactly.
fn a_whole_rect() -> impl Strategy<Value = Rect> {
    (-20i32..200, -20i32..120, 1i32..80, 1i32..60).prop_map(|(x, y, w, h)| Rect { x: x as f32 * 10., y: y as f32 * 10., w: w as f32 * 10., h: h as f32 * 10. })
}

fn some_rects() -> impl Strategy<Value = (Rect, Vec<Rect>, f32)> {
    let rect = prop_oneof![a_rect(), a_whole_rect()];
    (rect.clone(), prop::collection::vec(rect, 0..8), 0f32..40.)
}

proptest! {
    #[test]
    fn a_snap_never_moves_further_than_within((moving, others, within) in some_rects()) {
        let snapped = snap_move(moving, &others, within);
        prop_assert!(snapped.dx.abs() <= within && snapped.dy.abs() <= within, "{snapped:?}");
        for vertical in [true, false] {
            let edge = if vertical { moving.x + moving.w } else { moving.y };
            if let Some((shift, _)) = snap_edge(edge, vertical, &others, within) {
                prop_assert!(shift.abs() <= within);
            }
        }
    }

    #[test]
    fn after_a_snap_a_line_really_is_shared((moving, others, within) in some_rects()) {
        let snapped = snap_move(moving, &others, within);
        let now = moved(moving, &snapped);
        for guide in &snapped.guides {
            let (mine, theirs) = if guide.vertical { (lines(now.x, now.w), targets(&others, true)) } else { (lines(now.y, now.h), targets(&others, false)) };
            prop_assert!(least_gap(&mine, &[guide.at]) <= 0.01, "{guide:?} isn't on {now:?}");
            prop_assert!(least_gap(&[guide.at], &theirs) <= 0.01, "{guide:?} is on nothing else");
            // It runs the length of the moving box.
            let (start, length) = if guide.vertical { (now.y, now.h) } else { (now.x, now.w) };
            prop_assert!(guide.from <= start + 0.01 && start + length <= guide.to + 0.01, "{guide:?} for {now:?}");
        }
        // A shift either way comes with a guide that way.
        prop_assert!(snapped.dx == 0. || snapped.guides.iter().any(|guide| guide.vertical));
        prop_assert!(snapped.dy == 0. || snapped.guides.iter().any(|guide| !guide.vertical));
        for vertical in [true, false] {
            let edge = if vertical { moving.x } else { moving.y + moving.h };
            if let Some((shift, guide)) = snap_edge(edge, vertical, &others, within) {
                prop_assert_eq!(guide.vertical, vertical);
                prop_assert!((edge + shift - guide.at).abs() <= 0.01, "{guide:?}");
                prop_assert!(least_gap(&[guide.at], &targets(&others, vertical)) <= 0.01);
            }
        }
    }

    #[test]
    fn nothing_near_means_no_move_and_no_guide((moving, others, within) in some_rects()) {
        let snapped = snap_move(moving, &others, within);
        let far_across = least_gap(&lines(moving.x, moving.w), &targets(&others, true)) > within;
        let far_down = least_gap(&lines(moving.y, moving.h), &targets(&others, false)) > within;
        if far_across {
            prop_assert_eq!(snapped.dx, 0.);
            prop_assert!(!snapped.guides.iter().any(|guide| guide.vertical));
        }
        if far_down {
            prop_assert_eq!(snapped.dy, 0.);
            prop_assert!(!snapped.guides.iter().any(|guide| !guide.vertical));
        }
        let edge = moving.x;
        if least_gap(&[edge], &targets(&others, true)) > within {
            prop_assert_eq!(snap_edge(edge, true, &others, within), None);
        }
    }

    #[test]
    fn the_nearest_line_wins((moving, others, within) in some_rects()) {
        let snapped = snap_move(moving, &others, within);
        let across = least_gap(&lines(moving.x, moving.w), &targets(&others, true));
        let down = least_gap(&lines(moving.y, moving.h), &targets(&others, false));
        if across <= within {
            prop_assert!((snapped.dx.abs() - across).abs() <= 0.001, "{} for {across}", snapped.dx);
        }
        if down <= within {
            prop_assert!((snapped.dy.abs() - down).abs() <= 0.001, "{} for {down}", snapped.dy);
        }
        let edge = moving.y;
        if let Some((shift, _)) = snap_edge(edge, false, &others, within) {
            prop_assert!((shift.abs() - least_gap(&[edge], &targets(&others, false))).abs() <= 0.001);
        }
    }

    #[test]
    fn snapping_twice_changes_nothing_more((moving, others, within) in some_rects()) {
        let once = snap_move(moving, &others, within);
        let twice = snap_move(moved(moving, &once), &others, within);
        prop_assert!(twice.dx.abs() <= 0.01 && twice.dy.abs() <= 0.01, "{twice:?}");
        prop_assert_eq!(once.guides.len(), twice.guides.len());
        let edge = moving.x + moving.w;
        if let Some((shift, _)) = snap_edge(edge, true, &others, within) {
            let (again, _) = snap_edge(edge + shift, true, &others, within).expect("still on the line");
            prop_assert!(again.abs() <= 0.01);
        }
    }
}

#[test]
fn the_slides_centre() {
    // 200 wide, its middle 5 short of the slide's.
    let moving = Rect { x: 855., y: 103., w: 200., h: 100. };
    let snapped = snap_move(moving, &[], 6.);
    assert_eq!((snapped.dx, snapped.dy), (5., 0.));
    assert_eq!(snapped.guides, vec![Guide { vertical: true, at: 960., from: 0., to: SLIDE_HEIGHT }]);
    // And down the middle too.
    let snapped = snap_move(Rect { y: 488., ..moving }, &[], 6.);
    assert_eq!((snapped.dx, snapped.dy), (5., 2.));
    assert_eq!(snapped.guides.len(), 2);
    assert_eq!(snapped.guides[1], Guide { vertical: false, at: 540., from: 0., to: SLIDE_WIDTH });
    // Too far.
    assert_eq!(snap_move(Rect { x: 840., ..moving }, &[], 6.), Snapped::default());
}

#[test]
fn left_edge_to_anothers_right_edge() {
    let other = Rect { x: 300., y: 300., w: 200., h: 100. };
    let moving = Rect { x: 504., y: 600., w: 150., h: 150. };
    let snapped = snap_move(moving, &[other], 6.);
    assert_eq!((snapped.dx, snapped.dy), (-4., 0.));
    // From the top of the other to the bottom of the one moved.
    assert_eq!(snapped.guides, vec![Guide { vertical: true, at: 500., from: 300., to: 750. }]);
    // A right edge being resized, 3 short of the other's left.
    let (shift, guide) = snap_edge(297., true, &[other], 6.).expect("in reach");
    assert_eq!(shift, 3.);
    assert_eq!(guide, Guide { vertical: true, at: 300., from: 300., to: 400. });
    assert_eq!(snap_edge(280., true, &[other], 6.), None);
}

#[test]
fn centres_line_up() {
    let other = Rect { x: 100., y: 700., w: 300., h: 100. };
    // Its middle at 248, the other's at 250; its top 3 from the other's middle.
    let moving = Rect { x: 198., y: 747., w: 100., h: 40. };
    let snapped = snap_move(moving, &[other], 6.);
    assert_eq!((snapped.dx, snapped.dy), (2., 3.));
    assert!(snapped.guides.contains(&Guide { vertical: true, at: 250., from: 700., to: 800. }), "{:?}", snapped.guides);
    assert!(snapped.guides.contains(&Guide { vertical: false, at: 750., from: 100., to: 400. }), "{:?}", snapped.guides);
}
