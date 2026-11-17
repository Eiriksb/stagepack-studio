//! Snapping a thing being moved or resized to the slide and to the things around it, with the
//! guide lines that show it while the button is down.

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::{Bounds, Modifiers, Pixels, Point, SharedString, TestAppContext, point, px};
use slides_format::number_text;
use stagepack_studio::arrange::Handle;
use stagepack_studio::slide_editor::{SNAP_GUIDE_ID, handle_id, snap_guide_id};

/// A heading, whose area runs from 100 to 1820 across and 60 to 200 down, and two shapes: the
/// rectangle's lines across are at 300, 400 and 500, and down at 700, 750 and 800; the
/// ellipse's at 1300, 1400 and 1500, and 700, 800 and 900. The slide's own are at 0, 960 and
/// 1920, and 0, 540 and 1080.
const TALK: &str = "---\ntitle: T\n---\n\n# Snap\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"300\" y=\"700\" w=\"200\" h=\"100\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1300\" y=\"700\" w=\"200\" h=\"200\" />\n";
const RECT: &str = "<Shape kind=\"rect\"";
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";

/// How many window pixels a slide unit is.
fn unit(typing: &Typing, cx: &TestAppContext) -> f32 {
    f32::from(typing.slide_bounds(cx).size.width) / 1920.
}

/// `by` slide units, in the window's pixels.
fn pixels(typing: &Typing, cx: &TestAppContext, (x, y): (f32, f32)) -> Point<Pixels> {
    let unit = unit(typing, cx);
    point(px(x * unit), px(y * unit))
}

/// How far `by`, in the window's pixels, goes on the slide, in slide units.
fn units(typing: &Typing, cx: &TestAppContext, by: Point<Pixels>) -> (f32, f32) {
    let unit = unit(typing, cx);
    (f32::from(by.x) / unit, f32::from(by.y) / unit)
}

/// `value` moved by `by`, as it's written: to a whole unit.
fn written(value: f32, by: f32) -> String {
    number_text((value + by).round())
}

fn middle_of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Point<Pixels> {
    let element = thing(typing, cx, start);
    middle(typing, cx, &element)
}

fn click_thing(typing: &Typing, cx: &mut TestAppContext, start: &str, modifiers: Modifiers) {
    let at = middle_of(typing, cx, start);
    typing.clicks(cx, at, 1, modifiers);
}

fn handle_at(typing: &Typing, cx: &mut TestAppContext, handle: Handle) -> Point<Pixels> {
    bounds_of(typing, cx, SharedString::from(handle_id(handle))).expect("the handle shows").center()
}

/// Every guide drawn, in the order they're given.
fn guides(typing: &Typing, cx: &mut TestAppContext) -> Vec<Bounds<Pixels>> {
    let first = bounds_of(typing, cx, SNAP_GUIDE_ID);
    first.into_iter().chain((1..).map_while(|n| bounds_of(typing, cx, SharedString::from(snap_guide_id(n))))).collect()
}

/// Whether a guide is drawn up and down the slide at `x` of it, a pixel wide.
fn upright_guide_at(typing: &Typing, cx: &mut TestAppContext, x: f32) -> bool {
    let at = typing.at(cx, x, 0.).x;
    guides(typing, cx).iter().any(|guide| guide.size.width == px(1.) && guide.size.height > px(1.) && (guide.center().x - at).abs() <= px(1.))
}

/// The value of attribute `name` in the tag of the thing whose source starts with `start`.
fn attr(typing: &Typing, cx: &mut TestAppContext, start: &str, name: &str) -> Option<f32> {
    let source = typing.source(cx);
    let tag = &source[thing(typing, cx, start).source];
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[at..].split('"').next()?.parse().ok()
}

/// Presses at `from`, moves through each of `through` with the button held and `modifiers`,
/// and lets go at the last.
fn drag_through(typing: &Typing, cx: &mut TestAppContext, from: Point<Pixels>, through: &[Point<Pixels>], modifiers: Modifiers) {
    typing.mouse_down(cx, from, 1, Modifiers::default());
    for at in through {
        typing.move_held(cx, *at, modifiers);
    }
    typing.mouse_up(cx, *through.last().unwrap_or(&from), 1, modifiers);
}

/// Where the rectangle's middle, at 400 across, is 2 short of the slide's, at 960.
const NEAR_THE_CENTRE: (f32, f32) = (558., 0.);
/// Where none of the rectangle's lines is near another's.
const NEAR_NOTHING: (f32, f32) = (680., -120.);

#[gpui_kit::test]
fn a_drag_near_the_slides_centre_lands_on_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let to = from + pixels(&typing, cx, NEAR_THE_CENTRE);
    drag_through(&typing, cx, from, &[from + pixels(&typing, cx, (200., 0.)), to], Modifiers::default());
    assert_eq!(typing.source(cx), TALK.replacen("x=\"300\"", "x=\"860\"", 1), "its middle is the slide's");
}

#[gpui_kit::test]
fn a_guide_shows_while_snapped_and_goes_on_release(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    assert!(guides(&typing, cx).is_empty());
    let from = middle_of(&typing, cx, RECT);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + pixels(&typing, cx, (200., 0.)), Modifiers::default());
    assert!(!upright_guide_at(&typing, cx, 960.));
    typing.move_held(cx, from + pixels(&typing, cx, NEAR_THE_CENTRE), Modifiers::default());
    assert!(upright_guide_at(&typing, cx, 960.), "down the slide's middle: {:?}", guides(&typing, cx));
    let slide = typing.slide_bounds(cx);
    let centre = bounds_of(&typing, cx, SNAP_GUIDE_ID).expect("a guide");
    assert!((centre.size.height - slide.size.height).abs() <= px(1.), "the slide's whole height: {centre:?}");
    typing.move_held(cx, from + pixels(&typing, cx, NEAR_NOTHING), Modifiers::default());
    assert_eq!(guides(&typing, cx), Vec::new(), "near nothing, no guide");
    typing.move_held(cx, from + pixels(&typing, cx, NEAR_THE_CENTRE), Modifiers::default());
    assert!(upright_guide_at(&typing, cx, 960.));
    typing.mouse_up(cx, from + pixels(&typing, cx, NEAR_THE_CENTRE), 1, Modifiers::default());
    assert_eq!(guides(&typing, cx), Vec::new(), "gone once let go of");
    assert_eq!(typing.source(cx), TALK.replacen("x=\"300\"", "x=\"860\"", 1));
}

#[gpui_kit::test]
fn alt_switches_snapping_off(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let to = from + pixels(&typing, cx, NEAR_THE_CENTRE);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + pixels(&typing, cx, (200., 0.)), Modifiers::alt());
    typing.move_held(cx, to, Modifiers::alt());
    assert_eq!(guides(&typing, cx), Vec::new(), "no guide with Alt");
    typing.mouse_up(cx, to, 1, Modifiers::alt());
    let (dx, _) = units(&typing, cx, to - from);
    assert_eq!(typing.source(cx), TALK.replacen("x=\"300\"", &format!("x=\"{}\"", written(300., dx)), 1));
    assert_ne!(written(300., dx), "860");
}

#[gpui_kit::test]
fn a_resize_snaps_its_moving_edge_to_another_thing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    // Its right side, at 500, to 3 short of the ellipse's left.
    let from = handle_at(&typing, cx, Handle::Right);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + pixels(&typing, cx, (400., 0.)), Modifiers::default());
    typing.move_held(cx, from + pixels(&typing, cx, (797., 0.)), Modifiers::default());
    assert!(upright_guide_at(&typing, cx, 1300.), "on the ellipse's left: {:?}", guides(&typing, cx));
    typing.mouse_up(cx, from + pixels(&typing, cx, (797., 0.)), 1, Modifiers::default());
    assert_eq!(typing.source(cx), TALK.replacen("w=\"200\" h=\"100\"", "w=\"1000\" h=\"100\"", 1));
    assert_eq!(guides(&typing, cx), Vec::new());
    // Its bottom, at 800, to 3 short of the ellipse's.
    let from = handle_at(&typing, cx, Handle::Bottom);
    drag_through(&typing, cx, from, &[from + pixels(&typing, cx, (0., 50.)), from + pixels(&typing, cx, (0., 97.))], Modifiers::default());
    assert_eq!(typing.source(cx), TALK.replacen("w=\"200\" h=\"100\"", "w=\"1000\" h=\"200\"", 1));
}

#[gpui_kit::test]
fn a_corner_that_keeps_its_shape_snaps_the_way_it_goes_furthest(cx: &mut TestAppContext) {
    // An icon, which keeps its shape, from 300 to 400 across, and a shape whose left is at 500.
    let talk = "---\ntitle: T\n---\n\n# Snap\n\n<Icon name=\"star\" x=\"300\" y=\"300\" size=\"100\" />\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"500\" y=\"700\" w=\"200\" h=\"100\" />\n";
    let typing = open(cx, talk);
    click_thing(&typing, cx, "<Icon", Modifiers::default());
    // Its right side goes furthest, to 3 short of the shape's left: it snaps, and the height
    // follows.
    let from = handle_at(&typing, cx, Handle::BottomRight);
    drag_through(&typing, cx, from, &[from + pixels(&typing, cx, (50., 20.)), from + pixels(&typing, cx, (97., 40.))], Modifiers::default());
    assert_eq!((attr(&typing, cx, "<Icon", "w"), attr(&typing, cx, "<Icon", "h")), (Some(200.), Some(200.)));
}

#[gpui_kit::test]
fn a_turned_thing_snaps_by_the_box_it_covers_when_moved_and_not_when_resized(cx: &mut TestAppContext) {
    // Turned a quarter, it covers 350 to 450 across and 250 to 450 down.
    let turned = "<Shape kind=\"rect\" fill=\"#2563eb\" x=\"300\" y=\"300\" w=\"200\" h=\"100\" rotate=\"90\" />";
    let talk = TALK.replacen("<Shape kind=\"rect\" fill=\"#2563eb\" x=\"300\" y=\"700\" w=\"200\" h=\"100\" />", turned, 1);
    let typing = open(cx, &talk);
    // What it covers, to 2 short of the ellipse's left.
    let from = middle_of(&typing, cx, RECT);
    drag_through(&typing, cx, from, &[from + pixels(&typing, cx, (500., 0.)), from + pixels(&typing, cx, (948., 0.))], Modifiers::default());
    assert_eq!(typing.source(cx), talk.replacen("x=\"300\" y=\"300\"", "x=\"1250\" y=\"300\"", 1));
    // Resized, it doesn't snap: its right handle, at the bottom of what it covers, at 450, to
    // near the ellipse's top, at 700.
    let talk = typing.source(cx);
    let from = handle_at(&typing, cx, Handle::Right);
    let by = pixels(&typing, cx, (0., 247.6));
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(0.), by.y / 2.), Modifiers::default());
    typing.move_held(cx, from + by, Modifiers::default());
    assert_eq!(guides(&typing, cx), Vec::new());
    typing.mouse_up(cx, from + by, 1, Modifiers::default());
    let (_, grown) = units(&typing, cx, by);
    assert_eq!(attr(&typing, cx, RECT, "w"), Some((200. + grown).round()), "{}", typing.source(cx));
    assert_ne!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn arrow_keys_do_not_snap(cx: &mut TestAppContext) {
    // Its middle 3 short of the slide's.
    let talk = TALK.replacen("x=\"300\"", "x=\"857\"", 1);
    let typing = open(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    typing.press(cx, "right");
    assert_eq!(typing.source(cx), talk.replacen("x=\"857\"", "x=\"858\"", 1));
    typing.press(cx, "shift-right");
    assert_eq!(typing.source(cx), talk.replacen("x=\"857\"", "x=\"868\"", 1));
    assert_eq!(guides(&typing, cx), Vec::new());
}

#[gpui_kit::test]
fn things_dragged_together_do_not_snap_to_each_other(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    // Less far than a snap reaches: snapped to where they were, they wouldn't move at all.
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(5.), px(5.));
    drag_through(&typing, cx, from, &[to], Modifiers::default());
    let (dx, dy) = units(&typing, cx, to - from);
    assert!(dx < 6. / unit(&typing, cx));
    let expected = TALK
        .replacen("x=\"300\" y=\"700\"", &format!("x=\"{}\" y=\"{}\"", written(300., dx), written(700., dy)), 1)
        .replacen("x=\"1300\" y=\"700\"", &format!("x=\"{}\" y=\"{}\"", written(1300., dx), written(700., dy)), 1);
    assert_eq!(typing.source(cx), expected);
}
