//! Resizing and turning a thing, or a layout's area, by the handles on it.

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{CursorStyle, InputEvent as _, Modifiers, MouseMoveEvent, Pixels, Point, SharedString, TestAppContext, point, px};
use slides_engine::{CaretRect, Rect};
use slides_format::number_text;
use stagepack_studio::arrange::Handle;
use stagepack_studio::inspector::field_id;
use stagepack_studio::selection::{Selection, Thing};
use stagepack_studio::slide_editor::handle_id;

const TALK: &str = "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1300\" y=\"300\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1600\" y=\"300\" w=\"200\" h=\"200\" />\n\n---\n\n# Two\n";
const RECT: &str = "<Shape kind=\"rect\"";
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";
const RECT_SIZE: &str = "x=\"1300\" y=\"300\" w=\"200\" h=\"200\"";

fn selection(typing: &Typing, cx: &TestAppContext) -> Option<Selection> {
    typing.document(cx).read_with(cx, |document, _| document.selection().cloned())
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn placing(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.placing().is_some())
}

/// Undoes the last step, as the Undo tool does.
fn undo(typing: &Typing, cx: &mut TestAppContext) {
    typing.document(cx).update(cx, |document, cx| {
        document.undo().expect("undone");
        cx.notify();
    });
    typing.settle(cx);
}

/// Clicks the middle of the thing whose source starts with `start`.
fn click_thing(typing: &Typing, cx: &mut TestAppContext, start: &str, modifiers: Modifiers) {
    let element = thing(typing, cx, start);
    let at = middle(typing, cx, &element);
    typing.clicks(cx, at, 1, modifiers);
}

/// Where the middle of `handle` is drawn, if it is.
fn handle_at(typing: &Typing, cx: &mut TestAppContext, handle: Handle) -> Option<Point<Pixels>> {
    let bounds = bounds_of(typing, cx, SharedString::from(handle_id(handle)))?;
    Some(bounds.center())
}

fn no_handles(typing: &Typing, cx: &mut TestAppContext) -> bool {
    Handle::ALL.iter().all(|&handle| handle_at(typing, cx, handle).is_none())
}

/// Presses `handle`, moves it by `by` in the window's pixels, with `modifiers`, going some of
/// the way first, and lets go.
fn drag_handle(typing: &Typing, cx: &mut TestAppContext, handle: Handle, by: Point<Pixels>, modifiers: Modifiers) {
    let from = handle_at(typing, cx, handle).expect("the handle shows");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(by.x / 2., by.y / 2.), modifiers);
    typing.move_held(cx, from + by, modifiers);
    typing.mouse_up(cx, from + by, 1, modifiers);
}

/// Drags the turn handle to `to`, in the window, with `modifiers`.
fn turn_to(typing: &Typing, cx: &mut TestAppContext, to: Point<Pixels>, modifiers: Modifiers) {
    let from = handle_at(typing, cx, Handle::Turn).expect("the turn handle shows");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, to, modifiers);
    typing.mouse_up(cx, to, 1, modifiers);
}

/// How far `by`, in the window's pixels, goes on the slide, in slide units.
fn units(typing: &Typing, cx: &TestAppContext, by: f32) -> f32 {
    by / (f32::from(typing.slide_bounds(cx).size.width) / 1920.)
}

/// `value` grown by `by`, as it's written: to a whole unit.
fn written(value: f32, by: f32) -> String {
    number_text((value + by).round())
}

/// The value of attribute `name` in the tag of the thing whose source starts with `start`.
fn attr(typing: &Typing, cx: &mut TestAppContext, start: &str, name: &str) -> Option<f32> {
    let source = typing.source(cx);
    let tag = &source[thing(typing, cx, start).source];
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[at..].split('"').next()?.parse().ok()
}

/// The point of the thing `start` at (`x`, `y`) of it before it's turned, on the slide.
fn turned_point(typing: &Typing, cx: &mut TestAppContext, start: &str, (fx, fy): (f32, f32)) -> (f32, f32) {
    let element = thing(typing, cx, start);
    let Rect { x, y, w, h } = element.rect;
    let turned = CaretRect { x: 0., top: 0., height: 0., rotate: element.rotate, centre: (x + w / 2., y + h / 2.) };
    turned.on_slide(x + fx * w, y + fy * h)
}

fn move_pointer(typing: &Typing, cx: &mut TestAppContext, at: Point<Pixels>) {
    typing.with_window(cx, |window, cx| {
        window.dispatch_event(MouseMoveEvent { position: at, pressed_button: None, modifiers: Modifiers::default() }.to_platform_input(), cx);
        window.render_frame(cx);
    });
    typing.settle(cx);
}

fn pointer(typing: &Typing, cx: &mut TestAppContext) -> Option<CursorStyle> {
    cx.read(|cx| typing.view.read(cx).slide_editor().read(cx).pointer())
}

#[gpui_kit::test]
fn dragging_a_corner_resizes_and_writes_what_changed(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    for handle in Handle::ALL {
        assert!(handle_at(&typing, cx, handle).is_some(), "{handle:?} shows");
    }
    let size = bounds_of(&typing, cx, SharedString::from(handle_id(Handle::BottomRight))).expect("a handle").size;
    assert_eq!((size.width, size.height), (px(9.), px(9.)));
    let (top, corner) = (handle_at(&typing, cx, Handle::Top).expect("top"), handle_at(&typing, cx, Handle::Turn).expect("turn"));
    assert!((top.y - corner.y - px(24.)).abs() <= px(0.5) && (top.x - corner.x).abs() <= px(0.5), "the turn handle is 24 pixels above");
    let by = point(px(40.), px(20.));
    drag_handle(&typing, cx, Handle::BottomRight, by, Modifiers::default());
    let (dx, dy) = (units(&typing, cx, 40.), units(&typing, cx, 20.));
    let place = format!("x=\"1300\" y=\"300\" w=\"{}\" h=\"{}\"", written(200., dx), written(200., dy));
    assert_eq!(typing.source(cx), TALK.replacen(RECT_SIZE, &place, 1));
    assert_eq!(undo_label(&typing, cx), Some("Resize"));
    assert!(!placing(&typing, cx));
    // An edge writes only its own side; the left one moves the left too.
    undo(&typing, cx);
    drag_handle(&typing, cx, Handle::Left, point(px(-30.), px(15.)), Modifiers::default());
    let dx = units(&typing, cx, 30.);
    let place = format!("x=\"{}\" y=\"300\" w=\"{}\" h=\"200\"", written(1300., -dx), written(200., dx));
    assert_eq!(typing.source(cx), TALK.replacen(RECT_SIZE, &place, 1));
}

#[gpui_kit::test]
fn the_pointer_shows_what_a_handle_does(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    for (handle, shown) in [
        (Handle::TopLeft, CursorStyle::ResizeUpLeftDownRight),
        (Handle::Top, CursorStyle::ResizeUpDown),
        (Handle::TopRight, CursorStyle::ResizeUpRightDownLeft),
        (Handle::Right, CursorStyle::ResizeLeftRight),
        (Handle::Turn, CursorStyle::OpenHand),
    ] {
        let at = handle_at(&typing, cx, handle).expect("a handle");
        move_pointer(&typing, cx, at);
        assert_eq!(pointer(&typing, cx), Some(shown), "{handle:?}");
    }
    let rect = thing(&typing, cx, RECT);
    let away = middle(&typing, cx, &rect);
    move_pointer(&typing, cx, away);
    assert_eq!(pointer(&typing, cx), None);
    // Turned a quarter, the side handles resize the other way.
    let typing = open(cx, &TALK.replacen("h=\"200\" />", "h=\"200\" rotate=\"90\" />", 1));
    click_thing(&typing, cx, RECT, Modifiers::default());
    let at = handle_at(&typing, cx, Handle::Right).expect("a handle");
    move_pointer(&typing, cx, at);
    assert_eq!(pointer(&typing, cx), Some(CursorStyle::ResizeUpDown));
    // While turning, the hand is closed.
    let from = handle_at(&typing, cx, Handle::Turn).expect("the turn handle");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(30.), px(10.)), Modifiers::default());
    assert_eq!(pointer(&typing, cx), Some(CursorStyle::ClosedHand));
    typing.press(cx, "escape");
}

/// A talk with a picture 400 by 300, in a folder with its file.
fn open_picture(cx: &mut TestAppContext) -> (Typing, String) {
    open_picture_with(cx, "<Picture src=\"assets/photo.png\" x=\"300\" y=\"300\" w=\"400\" h=\"300\" />")
}

/// A talk with the picture `picture`, in a folder with its file.
fn open_picture_with(cx: &mut TestAppContext, picture: &str) -> (Typing, String) {
    let talk = format!("---\ntitle: T\n---\n\n# Picture\n\n{picture}\n");
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("slides.md"), &talk).unwrap();
    let photo = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/showcase/assets/photo.png")).unwrap();
    std::fs::write(folder.path().join("assets/photo.png"), photo).unwrap();
    (open_folder(cx, folder, None), talk)
}

#[gpui_kit::test]
fn a_picture_keeps_its_shape_and_shift_frees_it(cx: &mut TestAppContext) {
    let (typing, talk) = open_picture(cx);
    assert!(thing(&typing, cx, "<Picture").keeps_shape);
    click_thing(&typing, cx, "<Picture", Modifiers::default());
    drag_handle(&typing, cx, Handle::BottomRight, point(px(60.), px(10.)), Modifiers::default());
    let (w, h) = (attr(&typing, cx, "<Picture", "w").expect("w"), attr(&typing, cx, "<Picture", "h").expect("h"));
    assert!(w > 400. && (w / h - 4. / 3.).abs() < 0.01, "{w} by {h}");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), talk);
    drag_handle(&typing, cx, Handle::BottomRight, point(px(60.), px(10.)), Modifiers::shift());
    let (dx, dy) = (units(&typing, cx, 60.), units(&typing, cx, 10.));
    let free = format!("w=\"{}\" h=\"{}\"", written(400., dx), written(300., dy));
    assert_eq!(typing.source(cx), talk.replacen("w=\"400\" h=\"300\"", &free, 1));
    // An edge never keeps the shape.
    undo(&typing, cx);
    drag_handle(&typing, cx, Handle::Right, point(px(50.), px(0.)), Modifiers::default());
    assert_eq!(typing.source(cx), talk.replacen("w=\"400\"", &format!("w=\"{}\"", written(400., units(&typing, cx, 50.))), 1));
}

#[gpui_kit::test]
fn a_shape_is_free_and_shift_keeps_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    drag_handle(&typing, cx, Handle::BottomRight, point(px(60.), px(10.)), Modifiers::default());
    let (w, h) = (attr(&typing, cx, RECT, "w").expect("w"), attr(&typing, cx, RECT, "h").expect("h"));
    assert!(w > h + 10., "free: {w} by {h}");
    undo(&typing, cx);
    drag_handle(&typing, cx, Handle::BottomRight, point(px(60.), px(10.)), Modifiers::shift());
    let (w, h) = (attr(&typing, cx, RECT, "w").expect("w"), attr(&typing, cx, RECT, "h").expect("h"));
    assert!(w > 200. && w == h, "kept: {w} by {h}");
}

#[gpui_kit::test]
fn the_turn_handle_writes_rotate(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let rect = thing(&typing, cx, RECT);
    let centre = middle(&typing, cx, &rect);
    // Straight out to the right of its middle: a quarter turn.
    turn_to(&typing, cx, centre + point(px(150.), px(0.)), Modifiers::default());
    assert_eq!(typing.source(cx), TALK.replacen("h=\"200\" />", "h=\"200\" rotate=\"90\" />", 1));
    assert_eq!(undo_label(&typing, cx), Some("Turn"));
    assert_eq!(selection(&typing, cx).map(|selection| selection.things.len()), Some(1), "it stays selected");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK);
    // Towards 37 degrees, in whole degrees; with Shift, in steps of fifteen.
    let (sin, cos) = 37f32.to_radians().sin_cos();
    let towards = centre + point(px(200. * sin), px(-200. * cos));
    turn_to(&typing, cx, towards, Modifiers::default());
    let rotate = attr(&typing, cx, RECT, "rotate").expect("turned");
    assert!((rotate - 37.).abs() <= 1. && rotate == rotate.round(), "{rotate}");
    undo(&typing, cx);
    turn_to(&typing, cx, towards, Modifiers::shift());
    assert_eq!(attr(&typing, cx, RECT, "rotate"), Some(30.));
}

#[gpui_kit::test]
fn turning_back_to_zero_removes_it(cx: &mut TestAppContext) {
    let turned = TALK.replacen("h=\"200\" />", "h=\"200\" rotate=\"30\" />", 1);
    let typing = open(cx, &turned);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let rect = thing(&typing, cx, RECT);
    let centre = middle(&typing, cx, &rect);
    turn_to(&typing, cx, centre + point(px(0.), px(-180.)), Modifiers::default());
    assert_eq!(typing.source(cx), TALK, "upright, it has no turn written");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), turned);
}

#[gpui_kit::test]
fn a_turned_shape_resizes_from_its_own_corner(cx: &mut TestAppContext) {
    let turned = TALK.replacen("h=\"200\" />", "h=\"200\" rotate=\"37\" />", 1);
    let typing = open(cx, &turned);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let before = turned_point(&typing, cx, RECT, (0., 0.));
    // Its own bottom-right corner, where it's drawn turned.
    let corner = turned_point(&typing, cx, RECT, (1., 1.));
    let shown = handle_at(&typing, cx, Handle::BottomRight).expect("the handle");
    assert!((shown - typing.at(cx, corner.0, corner.1)).x.abs() <= px(1.) && (shown - typing.at(cx, corner.0, corner.1)).y.abs() <= px(1.));
    drag_handle(&typing, cx, Handle::BottomRight, point(px(50.), px(30.)), Modifiers::default());
    assert_ne!(typing.source(cx), turned);
    assert_eq!(attr(&typing, cx, RECT, "rotate"), Some(37.), "the turn stays");
    let after = turned_point(&typing, cx, RECT, (0., 0.));
    assert!((after.0 - before.0).abs() <= 0.75 && (after.1 - before.1).abs() <= 0.75, "{before:?} went to {after:?}");
}

#[gpui_kit::test]
fn an_area_resizes_and_its_text_wraps_in_the_preview(cx: &mut TestAppContext) {
    let talk = TALK.replace("Some words here", "Some words here that go on for long enough to need a second line once the area is narrow");
    let typing = open(cx, &talk);
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text");
    let height = |typing: &Typing, cx: &mut TestAppContext| -> f32 {
        typing.layout(cx).paras().iter().filter(|placed| placed.flow == body).map(|placed| placed.rect.h).sum()
    };
    let tall = height(&typing, cx);
    let area = layout.region("body").expect("the area").rect;
    // Picked by its frame.
    typing.click_text(cx, body, pos(0, 3));
    let frame = typing.at(cx, area.x, area.y + area.h / 2.);
    typing.click(cx, frame);
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Region("body".into())]));
    assert!(typing.editing(cx).is_none());
    let from = handle_at(&typing, cx, Handle::Right).expect("the area has handles");
    let narrower = typing.at(cx, area.x + 400., area.y + area.h / 2.);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from - point(px(20.), px(0.)), Modifiers::default());
    typing.move_held(cx, narrower, Modifiers::default());
    assert!(placing(&typing, cx));
    assert!(height(&typing, cx) > tall + 1., "the text wraps as it's shown");
    assert_eq!(typing.source(cx), talk, "nothing written yet");
    typing.mouse_up(cx, narrower, 1, Modifiers::default());
    // Only its width changed, and only that is written.
    let w = written(area.w, units(&typing, cx, f32::from(narrower.x - from.x)));
    assert_eq!(typing.source(cx), talk.replace("title: T\n", &format!("title: T\nregion.body: w={w}\n")));
    assert_eq!(undo_label(&typing, cx), Some("Resize"));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn two_things_selected_show_no_handles(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert!(!no_handles(&typing, cx));
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(selection(&typing, cx).map(|selection| selection.things.len()), Some(2));
    assert!(no_handles(&typing, cx));
}

#[gpui_kit::test]
fn a_thing_in_the_text_shows_no_handles(cx: &mut TestAppContext) {
    let talk = "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n<Shape kind=\"rect\" fill=\"#2563eb\" />\n";
    let typing = open(cx, talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert!(selection(&typing, cx).is_some());
    assert!(no_handles(&typing, cx));
    // Nor does an area being typed in.
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text");
    typing.click_text(cx, body, pos(0, 3));
    assert!(typing.editing(cx).is_some());
    assert!(no_handles(&typing, cx));
}

#[gpui_kit::test]
fn escape_during_a_resize_leaves_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let from = handle_at(&typing, cx, Handle::BottomRight).expect("the handle");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(30.), px(30.)), Modifiers::default());
    typing.move_held(cx, from + point(px(60.), px(40.)), Modifiers::default());
    assert!(placing(&typing, cx), "resizing");
    assert!(thing(&typing, cx, RECT).rect.w > 200., "the slide shows it bigger");
    typing.press(cx, "escape");
    assert!(!placing(&typing, cx), "put back at once");
    typing.move_held(cx, from + point(px(90.), px(40.)), Modifiers::default());
    typing.mouse_up(cx, from + point(px(90.), px(40.)), 1, Modifiers::default());
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
    assert!(!placing(&typing, cx));
    assert_eq!(thing(&typing, cx, RECT).rect.w, 200.);
    assert!(selection(&typing, cx).is_some(), "it stays selected");
}

#[gpui_kit::test]
fn a_one_unit_thing_can_still_be_grabbed(cx: &mut TestAppContext) {
    let talk = TALK.replacen(RECT_SIZE, "x=\"1300\" y=\"300\" w=\"1\" h=\"1\"", 1);
    let typing = open(cx, &talk);
    let source = thing(&typing, cx, RECT).source;
    typing.document(cx).update(cx, |document, cx| {
        document.select(0, source);
        cx.notify();
    });
    typing.settle(cx);
    // Its handles are drawn apart, each to be had on its own.
    let (top_left, bottom_right) = (handle_at(&typing, cx, Handle::TopLeft).expect("a handle"), handle_at(&typing, cx, Handle::BottomRight).expect("a handle"));
    assert!(bottom_right.x - top_left.x >= px(12.) && bottom_right.y - top_left.y >= px(12.), "{top_left:?} {bottom_right:?}");
    drag_handle(&typing, cx, Handle::BottomRight, point(px(40.), px(30.)), Modifiers::default());
    let (dx, dy) = (units(&typing, cx, 40.), units(&typing, cx, 30.));
    let place = format!("x=\"1300\" y=\"300\" w=\"{}\" h=\"{}\"", written(1., dx), written(1., dy));
    assert_eq!(typing.source(cx), TALK.replacen(RECT_SIZE, &place, 1));
}

#[gpui_kit::test]
fn a_resize_is_one_undo_step(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let from = handle_at(&typing, cx, Handle::TopRight).expect("the handle");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    for step in 1..=5 {
        typing.move_held(cx, from + point(px(12. * step as f32), px(-8. * step as f32)), Modifiers::default());
    }
    typing.mouse_up(cx, from + point(px(60.), px(-40.)), 1, Modifiers::default());
    assert_ne!(typing.source(cx), TALK);
    assert_eq!(undo_label(&typing, cx), Some("Resize"));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK, "one step takes it back");
    assert!(!can_undo(&typing, cx));
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(thing(&typing, cx, RECT).source)]));
    // A handle let go of where it was pressed writes nothing.
    let from = handle_at(&typing, cx, Handle::TopRight).expect("the handle");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(30.), px(0.)), Modifiers::default());
    typing.move_held(cx, from, Modifiers::default());
    typing.mouse_up(cx, from, 1, Modifiers::default());
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn an_icons_size_moves_its_width_and_height(cx: &mut TestAppContext) {
    let sized = "<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" w=\"96\" h=\"96\" />";
    let talk = format!("---\ntitle: T\n---\n\n# Icons\n\n{sized}\n\n<Icon name=\"star\" x=\"200\" y=\"800\" size=\"64\" />\n");
    let typing = open(cx, &talk);
    let set_size = |typing: &Typing, cx: &mut TestAppContext, start: &str, size: &str| {
        click_thing(typing, cx, start, Modifiers::default());
        click_on(typing, cx, SharedString::from(field_id("size")));
        typing.press(cx, "ctrl-a");
        typing.type_text(cx, size);
        typing.press(cx, "enter");
    };
    set_size(&typing, cx, "<Icon name=\"arrow-right\"", "120");
    let bigger = talk.replacen(sized, "<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"120\" w=\"120\" h=\"120\" />", 1);
    assert_eq!(typing.source(cx), bigger);
    undo(&typing, cx);
    assert_eq!(typing.source(cx), talk, "one step");
    // One with no width or height of its own gains none.
    set_size(&typing, cx, "<Icon name=\"star\"", "80");
    assert_eq!(typing.source(cx), talk.replacen("size=\"64\"", "size=\"80\"", 1));
}

#[gpui_kit::test]
fn a_press_halfway_down_a_selected_areas_side_takes_its_handle(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text");
    let area = layout.region("body").expect("the area").rect;
    typing.click_text(cx, body, pos(0, 3));
    let frame = typing.at(cx, area.x, area.y + area.h / 4.);
    typing.click(cx, frame);
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Region("body".into())]));
    // On the frame halfway down its left side: the handle there, which resizes it.
    let side = typing.at(cx, area.x, area.y + area.h / 2.);
    let to = side + point(px(40.), px(0.));
    typing.drag(cx, side, to);
    let by = units(&typing, cx, 40.);
    let (x, w) = (written(area.x, by), written(area.w, -by));
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", &format!("title: T\nregion.body: x={x} w={w}\n")));
    assert_eq!(undo_label(&typing, cx), Some("Resize"));
}

#[gpui_kit::test]
fn a_picture_or_icon_sized_by_itself_reads_back_the_size_it_was_made(cx: &mut TestAppContext) {
    // A picture with no width or height of its own.
    let (typing, _) = open_picture_with(cx, "<Picture src=\"assets/photo.png\" x=\"300\" y=\"300\" />");
    let icon = "<Icon name=\"star\" x=\"1300\" y=\"300\" size=\"96\" />";
    for (typing, start) in [(typing, "<Picture"), (open(cx, &format!("---\ntitle: T\n---\n\n# Icon\n\n{icon}\n")), "<Icon")] {
        click_thing(&typing, cx, start, Modifiers::default());
        let from = handle_at(&typing, cx, Handle::Right).expect("the handle");
        let to = from + point(px(40.), px(0.));
        typing.mouse_down(cx, from, 1, Modifiers::default());
        typing.move_held(cx, from + point(px(20.), px(0.)), Modifiers::default());
        typing.move_held(cx, to, Modifiers::default());
        let shown = thing(&typing, cx, start).rect;
        typing.mouse_up(cx, to, 1, Modifiers::default());
        let (w, h) = (attr(&typing, cx, start, "w"), attr(&typing, cx, start, "h"));
        assert_eq!((w, h), (Some(shown.w.round()), Some(shown.h.round())), "{start}: both are written");
        let now = thing(&typing, cx, start).rect;
        assert!((now.w - shown.w).abs() <= 0.5 && (now.h - shown.h).abs() <= 0.5, "{start}: {shown:?} reads back as {now:?}");
    }
}

#[gpui_kit::test]
fn a_nested_icons_size_moves_its_width_and_height(cx: &mut TestAppContext) {
    let icon = "<Icon name=\"star\" size=\"64\" w=\"64\" h=\"64\" />";
    let talk = format!("---\ntitle: T\n---\n\n# Icons\n\n<Box x=\"200\" y=\"300\" w=\"600\" h=\"400\">\n\n{icon}\n\n</Box>\n");
    let typing = open(cx, &talk);
    let source = thing(&typing, cx, "<Icon").source;
    typing.document(cx).update(cx, |document, cx| {
        document.select(0, source);
        cx.notify();
    });
    typing.settle(cx);
    click_on(&typing, cx, SharedString::from(field_id("size")));
    typing.press(cx, "ctrl-a");
    typing.type_text(cx, "80");
    typing.press(cx, "enter");
    assert_eq!(typing.source(cx), talk.replacen(icon, "<Icon name=\"star\" size=\"80\" w=\"80\" h=\"80\" />", 1));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), talk, "one step");
}

#[gpui_kit::test]
fn a_turn_or_a_resize_ended_any_way_leaves_nothing(cx: &mut TestAppContext) {
    for (handle, key) in [(Handle::Turn, "escape"), (Handle::BottomRight, "tab"), (Handle::Turn, "pagedown"), (Handle::Right, "ctrl-a")] {
        let typing = open(cx, TALK);
        click_thing(&typing, cx, RECT, Modifiers::default());
        let from = handle_at(&typing, cx, handle).expect("the handle");
        typing.mouse_down(cx, from, 1, Modifiers::default());
        typing.move_held(cx, from + point(px(30.), px(10.)), Modifiers::default());
        typing.move_held(cx, from + point(px(60.), px(30.)), Modifiers::default());
        assert!(placing(&typing, cx), "{key}: dragging");
        typing.press(cx, key);
        assert!(!placing(&typing, cx), "{key}: put back at once");
        typing.move_held(cx, from + point(px(90.), px(30.)), Modifiers::default());
        typing.mouse_up(cx, from + point(px(90.), px(30.)), 1, Modifiers::default());
        assert_eq!(typing.source(cx), TALK, "{key}");
        assert!(!can_undo(&typing, cx), "{key}");
        assert!(!placing(&typing, cx), "{key}");
    }
}

#[gpui_kit::test]
fn the_pointer_lets_go_of_a_handle_once_the_handles_change(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    for key in ["right", "tab"] {
        let at = handle_at(&typing, cx, Handle::Right).expect("a handle");
        move_pointer(&typing, cx, at);
        assert_eq!(pointer(&typing, cx), Some(CursorStyle::ResizeLeftRight), "{key}");
        typing.press(cx, key);
        typing.with_window(cx, |window, cx| window.render_frame(cx));
        assert_eq!(pointer(&typing, cx), None, "{key}: the handle under it has gone");
    }
}

#[gpui_kit::test]
fn shift_by_a_handle_adds_the_thing_under_it(cx: &mut TestAppContext) {
    // The ellipse starts where the rectangle's right side ends.
    let talk = TALK.replacen("x=\"1600\"", "x=\"1500\"", 1);
    let typing = open(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let handle = handle_at(&typing, cx, Handle::Right).expect("the handle");
    let near = handle + point(px(4.), px(0.));
    typing.clicks(cx, near, 1, Modifiers::shift());
    assert_eq!(selection(&typing, cx).map(|selection| selection.things.len()), Some(2), "the ellipse is added");
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn a_double_click_by_a_handle_but_off_it_types(cx: &mut TestAppContext) {
    let words = "Words that fill the box from side to side and line after line, so that its middle holds text everywhere it can";
    let talk = format!("---\ntitle: T\n---\n\n# Box\n\n<Box x=\"300\" y=\"300\" w=\"500\" h=\"200\">\n\n{words}\n\n</Box>\n");
    let typing = open(cx, &talk);
    let rect = thing(&typing, cx, "<Box").rect;
    let side = typing.at(cx, rect.x, rect.y + rect.h / 2.);
    // Selected by a click, the box has handles; a double click just inside its left side, in
    // reach of the handle there but off it, types.
    let inside = side + point(px(5.5), px(0.));
    typing.click(cx, inside);
    assert!(handle_at(&typing, cx, Handle::Left).is_some());
    typing.clicks(cx, inside, 2, Modifiers::default());
    assert!(typing.editing(cx).is_some(), "typing");
    assert_eq!(typing.source(cx), talk);
}
