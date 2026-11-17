//! Moving things and a layout's areas: dragging them with the mouse, and the arrow keys.

use std::ops::Range;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::{Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px};
use slides_format::number_text;
use stagepack_studio::inspector::field_id;
use stagepack_studio::selection::{Selection, Thing};

const TALK: &str = "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1300\" y=\"300\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1600\" y=\"300\" w=\"200\" h=\"200\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n---\n\n# Two\n";
const RECT: &str = "<Shape kind=\"rect\"";
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";
const RECT_PLACE: &str = "x=\"1300\" y=\"300\"";
const DRAG_OUT_FIRST: &str = "Drag it out of the text first.";
/// Held while dragging where a test is about what a drag writes or shows, not about snapping:
/// the things and areas here have lines within a snap of where some drags end.
const NO_SNAP: Modifiers = Modifiers { alt: true, shift: false, control: false, platform: false, function: false };

fn selection(typing: &Typing, cx: &TestAppContext) -> Option<Selection> {
    typing.document(cx).read_with(cx, |document, _| document.selection().cloned())
}

fn of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Range<usize> {
    thing(typing, cx, start).source
}

fn click_thing(typing: &Typing, cx: &mut TestAppContext, start: &str, modifiers: Modifiers) {
    let element = thing(typing, cx, start);
    let at = middle(typing, cx, &element);
    typing.clicks(cx, at, 1, modifiers);
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

/// Undoes the last step, as the Undo tool does, wherever the keyboard is.
fn undo(typing: &Typing, cx: &mut TestAppContext) {
    typing.document(cx).update(cx, |document, cx| {
        document.undo().expect("undone");
        cx.notify();
    });
    typing.settle(cx);
}

/// How far from `from` to `to` is on the slide, in slide units.
fn on_slide(typing: &Typing, cx: &TestAppContext, from: Point<Pixels>, to: Point<Pixels>) -> (f32, f32) {
    let unit = f32::from(typing.slide_bounds(cx).size.width) / 1920.;
    (f32::from(to.x - from.x) / unit, f32::from(to.y - from.y) / unit)
}

/// `value` moved by `by`, as it's written once let go of: to a whole unit.
fn written(value: f32, by: f32) -> String {
    number_text((value + by).round())
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

/// The middle of the thing starting with `start`.
fn middle_of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Point<Pixels> {
    let element = thing(typing, cx, start);
    middle(typing, cx, &element)
}

/// The flow of the slide's main text.
fn body_flow(typing: &Typing, cx: &mut TestAppContext) -> usize {
    let layout = typing.layout(cx);
    (0..typing.flows(cx, typing.current(cx)).len())
        .find(|&flow| layout.region_of_flow(flow) == Some("body"))
        .expect("the main text")
}

/// A point on the left line of area `name`'s frame, a quarter of the way down: away from the
/// handles at its corners and the middle of its side, which resize it once it's selected.
fn on_frame(typing: &Typing, cx: &mut TestAppContext, name: &str) -> Point<Pixels> {
    let rect = typing.layout(cx).region(name).expect("the area").rect;
    typing.at(cx, rect.x, rect.y + rect.h / 4.)
}

#[gpui_kit::test]
fn a_drag_moves_a_shape_and_writes_only_x_and_y(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(90.), px(40.));
    drag_through(&typing, cx, from, &[to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let place = format!("x=\"{}\" y=\"{}\"", written(1300., dx), written(300., dy));
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, &place, 1));
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, RECT))]), "it stays selected");
    assert!(!placing(&typing, cx));
}

#[gpui_kit::test]
fn a_drag_is_one_undo_step(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(120.), px(-30.));
    typing.drag(cx, from, to);
    assert_ne!(typing.source(cx), TALK);
    assert_eq!(undo_label(&typing, cx), Some("Move"));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK, "one step takes the drag back");
    assert!(!can_undo(&typing, cx));
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, RECT))]), "and selects it again");
}

#[gpui_kit::test]
fn a_drag_of_two_things_moves_both_by_the_same_amount(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(60.), px(25.));
    drag_through(&typing, cx, from, &[from + point(px(20.), px(5.)), to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let expected = TALK
        .replacen(RECT_PLACE, &format!("x=\"{}\" y=\"{}\"", written(1300., dx), written(300., dy)), 1)
        .replacen("x=\"1600\" y=\"300\"", &format!("x=\"{}\" y=\"{}\"", written(1600., dx), written(300., dy)), 1);
    assert_eq!(typing.source(cx), expected);
    assert_eq!(selection(&typing, cx).map(|selection| selection.things.len()), Some(2), "both stay selected");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK, "one step");
}

#[gpui_kit::test]
fn a_drag_of_an_area_writes_its_settings_line(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let body = body_flow(&typing, cx);
    let area = typing.layout(cx).region("body").expect("the area").rect;
    // A press on the frame of the area being typed in picks it, and drags it.
    typing.click_text(cx, body, pos(0, 3));
    let from = on_frame(&typing, cx, "body");
    let to = from + point(px(80.), px(0.));
    typing.drag(cx, from, to);
    let (dx, _) = on_slide(&typing, cx, from, to);
    let x = written(area.x, dx);
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", &format!("title: T\nregion.body: x={x}\n")));
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Region("body".into())]));
    assert_eq!(undo_label(&typing, cx), Some("Move"));
    // Selected, it's dragged by its frame again.
    let from = on_frame(&typing, cx, "body");
    let to = from + point(px(0.), px(50.));
    typing.drag(cx, from, to);
    let (_, dy) = on_slide(&typing, cx, from, to);
    let y = written(area.y, dy);
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", &format!("title: T\nregion.body: x={x} y={y}\n")));
    undo(&typing, cx);
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn shift_keeps_the_move_straight(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let through = [from + point(px(20.), px(8.)), from + point(px(100.), px(30.))];
    drag_through(&typing, cx, from, &through, Modifiers { shift: true, ..NO_SNAP });
    let (dx, _) = on_slide(&typing, cx, from, through[1]);
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, &format!("x=\"{}\" y=\"300\"", written(1300., dx)), 1));
    // Up and down more than across, it goes up or down.
    let typing = open(cx, TALK);
    let through = [from + point(px(10.), px(-90.))];
    drag_through(&typing, cx, from, &through, Modifiers { shift: true, ..NO_SNAP });
    let (_, dy) = on_slide(&typing, cx, from, through[0]);
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, &format!("x=\"1300\" y=\"{}\"", written(300., dy)), 1));
}

#[gpui_kit::test]
fn the_slide_shows_the_thing_under_the_pointer_while_dragging(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let before = outline(&typing, cx).expect("an outline");
    let from = middle_of(&typing, cx, RECT);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    for step in 1..=4 {
        let to = from + point(px(25. * step as f32), px(10. * step as f32));
        typing.move_held(cx, to, NO_SNAP);
        let now = outline(&typing, cx).expect("an outline");
        let moved = now.origin - before.origin;
        assert!((moved.x - px(25. * step as f32)).abs() <= px(1.) && (moved.y - px(10. * step as f32)).abs() <= px(1.), "step {step}: {moved:?}");
        assert_eq!(now.size, before.size);
        assert_eq!(typing.source(cx), TALK, "nothing is written while dragging");
        assert!(placing(&typing, cx));
        let shown = thing(&typing, cx, RECT).rect;
        assert!(shown.x > 1300., "the slide shows it moved: {shown:?}");
    }
    let to = from + point(px(100.), px(40.));
    typing.mouse_up(cx, to, 1, NO_SNAP);
    assert_ne!(typing.source(cx), TALK);
    assert!(!placing(&typing, cx));
}

/// Presses on the rectangle and drags it some way, button still down.
fn press_and_move(typing: &Typing, cx: &mut TestAppContext) -> Point<Pixels> {
    let from = middle_of(typing, cx, RECT);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    let to = from + point(px(70.), px(30.));
    typing.move_held(cx, from + point(px(20.), px(10.)), Modifiers::default());
    typing.move_held(cx, to, Modifiers::default());
    assert!(placing(typing, cx), "dragging");
    to
}

/// That nothing was written, no step made, and the slide shows what the talk says.
fn left_nothing(typing: &Typing, cx: &mut TestAppContext, source: &str, label: Option<&'static str>, what: &str) {
    assert_eq!(typing.source(cx), source, "{what}: the source is as it was");
    assert_eq!(undo_label(typing, cx), label, "{what}: no step");
    assert!(!placing(typing, cx), "{what}: nothing is shown moved");
    let layout = typing.layout(cx);
    let source = typing.source(cx);
    let rect = layout.elements().iter().find(|element| source[element.source.clone()].starts_with(RECT)).expect("the rect").rect;
    let place: Vec<&str> = source[thing(typing, cx, RECT).source].split('"').collect();
    assert_eq!(format!("{}", rect.x), place[5], "{what}: the slide shows it where the talk has it");
}

#[gpui_kit::test]
fn an_unfinished_drag_leaves_nothing(cx: &mut TestAppContext) {
    // Escape.
    let typing = open(cx, TALK);
    let to = press_and_move(&typing, cx);
    typing.press(cx, "escape");
    assert!(!placing(&typing, cx), "escape: put back at once");
    typing.move_held(cx, to + point(px(30.), px(0.)), Modifiers::default());
    typing.mouse_up(cx, to, 1, Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "escape");
    assert!(selection(&typing, cx).is_some(), "escape: it stays selected");

    // Let go outside the window.
    let typing = open(cx, TALK);
    press_and_move(&typing, cx);
    typing.move_held(cx, point(px(-40.), px(-40.)), Modifiers::default());
    typing.mouse_up(cx, point(px(-40.), px(-40.)), 1, Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "outside the window");

    // Let go outside the window, where the window never heard of it.
    let typing = open(cx, TALK);
    let to = press_and_move(&typing, cx);
    typing.with_window(cx, |window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(gpui_kit::MouseMoveEvent { position: to, pressed_button: None, modifiers: Modifiers::default() }.to_platform_input(), cx);
    });
    typing.settle(cx);
    left_nothing(&typing, cx, TALK, None, "the button let go of unheard");

    // The window deactivated.
    let typing = open(cx, TALK);
    typing.with_window(cx, |window, _| window.activate_window());
    typing.settle(cx);
    let to = press_and_move(&typing, cx);
    VisualTestContext::from_window(typing.handle, cx).deactivate_window();
    typing.settle(cx);
    assert!(!placing(&typing, cx), "deactivated: put back at once");
    typing.mouse_up(cx, to, 1, Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "deactivated");

    // Ctrl+Z with the button down: the undo is made, and the drag ends.
    let typing = open(cx, TALK);
    let earlier = middle_of(&typing, cx, ELLIPSE);
    typing.drag(cx, earlier, earlier + point(px(40.), px(0.)));
    assert_eq!(undo_label(&typing, cx), Some("Move"));
    let to = press_and_move(&typing, cx);
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "the earlier move is undone");
    typing.move_held(cx, to + point(px(30.), px(0.)), Modifiers::default());
    typing.mouse_up(cx, to, 1, Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "ctrl-z");

    // Page Down with the button down.
    let typing = open(cx, TALK);
    let to = press_and_move(&typing, cx);
    typing.press(cx, "pagedown");
    assert_eq!(typing.current(cx), 1);
    assert!(!placing(&typing, cx), "page down: put back at once");
    typing.move_held(cx, to + point(px(30.), px(0.)), Modifiers::default());
    typing.mouse_up(cx, to, 1, Modifiers::default());
    typing.press(cx, "pageup");
    left_nothing(&typing, cx, TALK, None, "page down");

    // The file changed outside with the button down.
    let typing = open(cx, TALK);
    let to = press_and_move(&typing, cx);
    let outside = TALK.replace("# Two", "# Two, changed");
    typing.view.update(cx, |view, cx| view.reload_for_test(Some(outside.clone()), cx));
    typing.settle(cx);
    assert_eq!(typing.source(cx), outside);
    typing.move_held(cx, to + point(px(30.), px(0.)), Modifiers::default());
    typing.mouse_up(cx, to, 1, Modifiers::default());
    let label = undo_label(&typing, cx);
    assert_ne!(label, Some("Move"));
    left_nothing(&typing, cx, &outside, label, "reloaded");

    // A press and a release with no move between.
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.mouse_up(cx, from, 1, Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "no move");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, RECT))]), "a click selects");
}

#[gpui_kit::test]
fn a_small_wobble_is_a_click_and_writes_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    drag_through(&typing, cx, from, &[from + point(px(2.), px(2.)), from + point(px(3.), px(0.))], Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "a wobble");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, RECT))]), "it's a click");
    // Of one of two selected, a wobble selects it alone, as a click does.
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    drag_through(&typing, cx, from, &[from + point(px(0.), px(3.))], Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "a wobble on two");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, RECT))]));
}

#[gpui_kit::test]
fn arrow_keys_nudge_by_one_and_shift_by_ten(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    typing.press(cx, "right");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1301\" y=\"300\"", 1));
    typing.press(cx, "shift-down");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1301\" y=\"310\"", 1));
    typing.press(cx, "shift-left");
    typing.press(cx, "up");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1291\" y=\"309\"", 1));
    assert_eq!(typing.current(cx), 0, "the slide stays");
    assert_eq!(undo_label(&typing, cx), Some("Move"));
    assert!(!placing(&typing, cx));
}

#[gpui_kit::test]
fn a_nudge_of_a_thing_in_the_text_says_why_not(cx: &mut TestAppContext) {
    let talk = "---\ntitle: T\n---\n\n# Shapes\n\n<Shape kind=\"rect\" fill=\"#2563eb\" />\n";
    let typing = open(cx, talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert!(selection(&typing, cx).is_some());
    typing.press(cx, "right");
    assert_eq!(typing.source(cx), talk);
    assert_eq!(typing.status(cx).as_deref(), Some(DRAG_OUT_FIRST));
    assert_eq!(typing.current(cx), 0);
}

#[gpui_kit::test]
fn a_run_of_nudges_is_one_undo_step(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    for _ in 0..3 {
        typing.press(cx, "right");
        wait(cx, 500);
    }
    typing.press(cx, "shift-down");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1303\" y=\"310\"", 1));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK, "one step");
    assert!(!can_undo(&typing, cx));
    // A pause between two makes two.
    typing.press(cx, "right");
    wait(cx, 1_100);
    typing.press(cx, "right");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1301\" y=\"300\"", 1));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_thing_cannot_be_dragged_off_the_slide(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let slide = typing.slide_bounds(cx);
    // Far past the slide's left and bottom, still in the window.
    let to = point(px(2.), px(798.));
    assert!(to.x < slide.origin.x && to.y > slide.origin.y + slide.size.height);
    typing.drag(cx, from, to);
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"-184\" y=\"1064\"", 1), "sixteen units stay on");
    // Nudged on, it stays.
    typing.press(cx, "shift-left");
    typing.press(cx, "down");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"-184\" y=\"1064\"", 1));
}

#[gpui_kit::test]
fn a_negative_left_can_be_typed_in_the_form(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    for (typed, kept) in [("-50", "-50"), ("-9000", "-3840")] {
        click_on(&typing, cx, gpui_kit::SharedString::from(field_id("x")));
        typing.press(cx, "ctrl-a");
        typing.type_text(cx, typed);
        typing.press(cx, "enter");
        assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, &format!("x=\"{kept}\" y=\"300\""), 1));
    }
}

#[gpui_kit::test]
fn left_and_right_change_the_slide_only_when_nothing_is_selected(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    typing.press(cx, "right");
    assert_eq!(typing.current(cx), 1, "nothing selected: on to the next slide");
    typing.press(cx, "left");
    assert_eq!(typing.current(cx), 0);
    click_thing(&typing, cx, RECT, Modifiers::default());
    typing.press(cx, "right");
    typing.press(cx, "left");
    typing.press(cx, "left");
    assert_eq!(typing.current(cx), 0, "selected: the thing moves, not the slide");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1299\" y=\"300\"", 1));
    typing.press(cx, "escape");
    assert!(selection(&typing, cx).is_none());
    typing.press(cx, "right");
    assert_eq!(typing.current(cx), 1, "let go of: the slides again");
}

#[gpui_kit::test]
fn nothing_redraws_after_a_drag_ends(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    typing.drag(cx, from, from + point(px(50.), px(20.)));
    assert_ne!(typing.source(cx), TALK);
    typing.press(cx, "right");
    let editor = cx.read(|cx| typing.view.read(cx).slide_editor().clone());
    let canvas = cx.read(|cx| typing.view.read(cx).slide_canvas().clone());
    let (editor_count, _a) = redraws(cx, &editor);
    let (view_count, _b) = redraws(cx, &typing.view);
    let (canvas_count, _c) = redraws(cx, &canvas);
    wait(cx, 20_000);
    assert_eq!((editor_count.get(), view_count.get(), canvas_count.get()), (0, 0, 0));
    assert!(outline(&typing, cx).is_some());
}

#[gpui_kit::test]
fn a_run_of_nudges_ends_when_another_thing_is_selected(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    typing.press(cx, "right");
    typing.press(cx, "tab");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, ELLIPSE))]));
    typing.press(cx, "right");
    let both = TALK.replacen(RECT_PLACE, "x=\"1301\" y=\"300\"", 1).replacen("x=\"1600\"", "x=\"1601\"", 1);
    assert_eq!(typing.source(cx), both);
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1301\" y=\"300\"", 1), "one undo takes back only the second");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(of(&typing, cx, ELLIPSE))]));
    // Shift and a click to add a thing makes a new run too.
    typing.press(cx, "right");
    click_thing(&typing, cx, RECT, Modifiers::shift());
    typing.press(cx, "right");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), both, "the two moved together are their own step");
}

#[gpui_kit::test]
fn a_drag_with_a_thing_in_the_text_moves_nothing(cx: &mut TestAppContext) {
    let talk = "---\ntitle: T\n---\n\n# Shapes\n\n<Shape kind=\"ellipse\" fill=\"#2563eb\" />\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1300\" y=\"300\" w=\"200\" h=\"200\" />\n";
    let typing = open(cx, talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(selection(&typing, cx).map(|selection| selection.things.len()), Some(2));
    let from = middle_of(&typing, cx, RECT);
    drag_through(&typing, cx, from, &[from + point(px(30.), px(0.)), from + point(px(90.), px(20.))], Modifiers::default());
    assert_eq!(typing.source(cx), talk, "neither moves");
    assert!(!placing(&typing, cx));
    assert_eq!(typing.status(cx).as_deref(), Some(DRAG_OUT_FIRST));
}

#[gpui_kit::test]
fn the_keys_work_after_a_release_that_was_never_heard(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    // Pressed, and never let go of as far as the window heard.
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.press(cx, "right");
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, "x=\"1301\" y=\"300\"", 1), "the arrow moves it");
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.press(cx, "escape");
    assert_eq!(selection(&typing, cx), None, "escape lets go");
}

#[gpui_kit::test]
fn a_selection_changed_with_the_button_down_ends_the_drag(cx: &mut TestAppContext) {
    for key in ["tab", "ctrl-a"] {
        let typing = open(cx, TALK);
        let to = press_and_move(&typing, cx);
        typing.press(cx, key);
        assert!(!placing(&typing, cx), "{key}: put back at once");
        let picked = selection(&typing, cx);
        typing.move_held(cx, to + point(px(30.), px(0.)), Modifiers::default());
        typing.mouse_up(cx, to, 1, Modifiers::default());
        left_nothing(&typing, cx, TALK, None, key);
        assert_eq!(selection(&typing, cx), picked, "{key}: what the key selected stays");
    }
}

#[gpui_kit::test]
fn the_keyboard_going_elsewhere_ends_the_drag(cx: &mut TestAppContext) {
    // To somewhere that leaves the selection be, so nothing else ends the drag.
    let typing = open(cx, TALK);
    typing.with_window(cx, |window, _| window.activate_window());
    typing.settle(cx);
    let to = press_and_move(&typing, cx);
    let elsewhere = cx.update(|cx| cx.focus_handle());
    typing.with_window(cx, |window, cx| elsewhere.focus(window, cx));
    typing.settle(cx);
    assert!(!placing(&typing, cx), "put back at once");
    typing.mouse_up(cx, to, 1, Modifiers::default());
    left_nothing(&typing, cx, TALK, None, "the keyboard elsewhere");
    // To the notes and to the list of slides, which let go of what's selected too.
    for to_notes in [true, false] {
        let typing = open(cx, TALK);
        typing.with_window(cx, |window, _| window.activate_window());
        typing.settle(cx);
        let to = press_and_move(&typing, cx);
        let panel = cx.read(|cx| {
            let view = typing.view.read(cx);
            if to_notes { view.notes_panel().read(cx).field().focus_handle(cx) } else { view.slide_strip().read(cx).focus_handle().clone() }
        });
        typing.with_window(cx, |window, cx| panel.focus(window, cx));
        typing.settle(cx);
        assert!(!placing(&typing, cx), "notes {to_notes}: put back at once");
        typing.mouse_up(cx, to, 1, Modifiers::default());
        left_nothing(&typing, cx, TALK, None, "a panel");
        assert_eq!(selection(&typing, cx), None, "notes {to_notes}: nothing is selected");
    }
}

#[gpui_kit::test]
fn a_turned_thing_is_moved_without_writing_its_turn(cx: &mut TestAppContext) {
    let talk = TALK.replacen("w=\"200\" h=\"200\" />", "w=\"200\" h=\"200\" rotate=\"30\" />", 1);
    let typing = open(cx, &talk);
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(60.), px(30.));
    drag_through(&typing, cx, from, &[to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let place = format!("x=\"{}\" y=\"{}\"", written(1300., dx), written(300., dy));
    assert_eq!(typing.source(cx), talk.replacen(RECT_PLACE, &place, 1));
}

#[gpui_kit::test]
fn a_move_straight_down_keeps_a_hand_written_left(cx: &mut TestAppContext) {
    let talk = TALK.replacen(RECT_PLACE, "x=\"1300.25\" y=\"300\"", 1);
    let typing = open(cx, &talk);
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(0.), px(50.));
    drag_through(&typing, cx, from, &[to], NO_SNAP);
    let (_, dy) = on_slide(&typing, cx, from, to);
    assert_eq!(typing.source(cx), talk.replacen("y=\"300\"", &format!("y=\"{}\"", written(300., dy)), 1), "x's bytes stay");
}

#[gpui_kit::test]
fn a_thing_with_a_left_but_no_top_gains_one(cx: &mut TestAppContext) {
    let talk = TALK.replacen(RECT_PLACE, "x=\"1300\"", 1);
    let typing = open(cx, &talk);
    let rect = thing(&typing, cx, RECT);
    assert!(rect.placed, "a left alone places it");
    let from = middle(&typing, cx, &rect);
    let to = from + point(px(40.), px(40.));
    drag_through(&typing, cx, from, &[to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let written_now = typing.source(cx);
    let tag = &written_now[thing(&typing, cx, RECT).source];
    assert!(tag.contains(&format!("x=\"{}\"", written(1300., dx))), "{tag}");
    assert!(tag.contains(&format!("y=\"{}\"", written(rect.rect.y, dy))), "{tag}");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn a_press_on_a_neighbouring_areas_offer_is_that_areas(cx: &mut TestAppContext) {
    // The right column moved to start just past the left one's edge, and empty.
    let talk = "---\ntitle: T\nlayout: two-columns\nregion.right: x=950\n---\n\n# Columns\n\nSome words here\n";
    let typing = open(cx, talk);
    let left = typing.layout(cx).region("left").expect("the left column").rect;
    let flow = (0..typing.flows(cx, 0).len()).find(|&flow| typing.layout(cx).region_of_flow(flow) == Some("left")).expect("its text");
    typing.click_text(cx, flow, pos(0, 3));
    let frame = typing.at(cx, left.x, left.y + left.h / 2.);
    typing.click(cx, frame);
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Region("left".into())]));
    // Just past its right edge, in the gap: still the left column, which a drag there moves,
    // away from the handle on the middle of that side.
    let gap = typing.at(cx, left.x + left.w + 5., left.y + left.h / 4.);
    let to = gap + point(px(0.), px(40.));
    typing.drag(cx, gap, to);
    let (_, dy) = on_slide(&typing, cx, gap, to);
    let moved = talk.replace("region.right: x=950\n", &format!("region.right: x=950\nregion.left: y={}\n", written(left.y, dy)));
    assert_eq!(typing.source(cx), moved, "the gap is the left column's frame");
    undo(&typing, cx);
    // Just as near its edge, but inside the right column's offer: that column is typed in.
    let left = typing.layout(cx).region("left").expect("the left column").rect;
    let offer = typing.at(cx, left.x + left.w + 12., left.y + left.h / 4.);
    typing.click(cx, offer);
    assert_eq!(selection(&typing, cx), None);
    let editing = typing.editing(cx).expect("typing");
    assert_eq!(editing.flow.region(), Some("right"));
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn the_right_button_let_go_of_during_a_drag_does_not_end_it(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseUpEvent};
    let typing = open(cx, TALK);
    let from = middle_of(&typing, cx, RECT);
    let to = from + point(px(90.), px(40.));
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(30.), px(10.)), NO_SNAP);
    typing.move_held(cx, to, NO_SNAP);
    typing.with_window(cx, |window, cx| {
        let up = MouseUpEvent { button: MouseButton::Right, position: to, modifiers: NO_SNAP, click_count: 1 };
        window.dispatch_event(up.to_platform_input(), cx);
    });
    typing.settle(cx);
    assert_eq!(typing.source(cx), TALK, "nothing is written");
    assert!(placing(&typing, cx), "the drag goes on");
    // The left button let go of writes it where it is then.
    let to = to + point(px(20.), px(0.));
    typing.move_held(cx, to, NO_SNAP);
    typing.mouse_up(cx, to, 1, NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let place = format!("x=\"{}\" y=\"{}\"", written(1300., dx), written(300., dy));
    assert_eq!(typing.source(cx), TALK.replacen(RECT_PLACE, &place, 1));
    assert_eq!(undo_label(&typing, cx), Some("Move"));
}
