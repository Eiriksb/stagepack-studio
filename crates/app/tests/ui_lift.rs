//! Dragging a thing that lies in the slide's text, a table, a picture or a shape, out to a place
//! of its own: a dashed outline of it follows the pointer, and once let go of it's written there.

use std::ops::Range;

#[path = "support/typing.rs"]
mod typing;
#[path = "support/components.rs"]
mod components;

use components::components_talk;
use typing::*;

use gpui_kit::{Bounds, Modifiers, Pixels, Point, SharedString, TestAppContext, point, px};
use slides_engine::{Hit, Rect};
use slides_format::{FlowPlace, number_text};
use stagepack_studio::arrange::Handle;
use stagepack_studio::inspector::field_id;
use stagepack_studio::selection::Thing;
use stagepack_studio::slide_editor::{LIFT_OUTLINE_ID, SNAP_GUIDE_ID, handle_id};
use stagepack_studio::text_editing::FlowKey;

const TABLE: &str = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
const TALK: &str = "---\ntitle: T\n---\n\n# Things\n\nSome words here\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\nMore words\n\n---\n\n# Two\n";
const SHAPE: &str = "<Shape kind=\"rect\" fill=\"#2563eb\" />";
const SHAPE_TALK: &str = "---\ntitle: T\n---\n\n# Things\n\nSome words here\n\n<Shape kind=\"rect\" fill=\"#2563eb\" />\n\nMore words\n";
const PICTURE: &str = "![A photo](assets/photo.png)";
const PICTURE_TALK: &str = "---\ntitle: T\n---\n\n# Things\n\nSome words here\n\n![A photo](assets/photo.png)\n\nMore words\n";
const DRAG_OUT_FIRST: &str = "Drag it out of the text first.";
const IN_A_PLACED_BOX: &str = "Drag the box it's in instead.";
const PART_OF_IT: &str = "It's part of what it sits in, and moves with it.";
const MOVE_OUT: &str = "Move out of the text";
/// Held while dragging where a test is about what a drag writes, not about snapping.
const NO_SNAP: Modifiers = Modifiers { alt: true, shift: false, control: false, platform: false, function: false };

/// The talk `source` in a window that's the active one, as a window being used is.
fn open(cx: &mut TestAppContext, source: &str) -> Typing {
    let typing = typing::open(cx, source);
    typing.with_window(cx, |window, _| window.activate_window());
    typing.settle(cx);
    typing
}

/// The talk `source`, with the showcase's photo beside it.
fn open_with_photo(cx: &mut TestAppContext, source: &str) -> Typing {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/showcase/assets/photo.png"), folder.path().join("assets/photo.png")).unwrap();
    let typing = open_folder(cx, folder, None);
    typing.with_window(cx, |window, _| window.activate_window());
    typing.settle(cx);
    typing
}

fn selection(typing: &Typing, cx: &TestAppContext) -> Option<Vec<Thing>> {
    typing.document(cx).read_with(cx, |document, _| document.selection().map(|selection| selection.things.clone()))
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn undo(typing: &Typing, cx: &mut TestAppContext) {
    typing.press(cx, "ctrl-z");
}

/// How many window pixels a slide unit is.
fn unit(typing: &Typing, cx: &TestAppContext) -> f32 {
    f32::from(typing.slide_bounds(cx).size.width) / 1920.
}

/// How far from `from` to `to` is on the slide, in slide units.
fn on_slide(typing: &Typing, cx: &TestAppContext, from: Point<Pixels>, to: Point<Pixels>) -> (f32, f32) {
    let unit = unit(typing, cx);
    (f32::from(to.x - from.x) / unit, f32::from(to.y - from.y) / unit)
}

/// `value` as it's written: to a whole unit.
fn whole(value: f32) -> String {
    number_text(value.round())
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

/// The source of the current slide's first table.
fn table_source(typing: &Typing, cx: &TestAppContext) -> Range<usize> {
    let slide = typing.current(cx);
    typing
        .flows(cx, slide)
        .iter()
        .find_map(|flow| match &flow.place {
            FlowPlace::Cell { table, .. } => Some(table.clone()),
            _ => None,
        })
        .expect("a table")
}

/// A point on the line under the first cell of the current slide's first table, which a press
/// on picks the table.
fn border_point(typing: &Typing, cx: &mut TestAppContext) -> Point<Pixels> {
    let layout = typing.layout(cx);
    let table = table_source(typing, cx);
    let rect = layout.cell_rect(&table, 0, 0).expect("the first cell");
    let x = rect.x + rect.w / 2.;
    let band: Vec<f32> = (0..120)
        .map(|step| rect.y + rect.h - 6. + step as f32 * 0.1)
        .filter(|&y| matches!(layout.hit(x, y), Some(Hit::Element(element)) if element.source == table))
        .collect();
    assert!(!band.is_empty(), "the line under the cell picks the table");
    typing.at(cx, x, band[band.len() / 2])
}

/// Where the current slide's first table is drawn.
fn table_rect(typing: &Typing, cx: &mut TestAppContext) -> Rect {
    let table = table_source(typing, cx);
    typing.layout(cx).element(&table).expect("the table is drawn").rect
}

/// The talk `talk` with `block`, and the blank line after it, taken from the first slide, and
/// written at the end of that slide's content in a box at `place`.
fn lifted(talk: &str, block: &str, place: (f32, f32, f32, f32)) -> String {
    let (x, y, w, h) = place;
    let taken = talk.replacen(&format!("{block}\n\n"), "", 1);
    let boxed = format!("<Box x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\">\n\n{block}\n\n</Box>\n", whole(x), whole(y), whole(w), whole(h));
    match taken.find("\n---\n\n# Two") {
        Some(end) => format!("{}\n{boxed}{}", &taken[..end], &taken[end..]),
        None => format!("{taken}\n{boxed}"),
    }
}

/// Drags the table of `TALK`, opened, by its border by `by` window pixels, with snapping off, and
/// returns where it was drawn before and how far that went on the slide.
fn drag_table_out(typing: &Typing, cx: &mut TestAppContext, by: Point<Pixels>) -> (Rect, (f32, f32)) {
    let before = table_rect(typing, cx);
    let from = border_point(typing, cx);
    let to = from + by;
    drag_through(typing, cx, from, &[from + point(px(20.), px(10.)), to], NO_SNAP);
    (before, on_slide(typing, cx, from, to))
}

/// The range of the box the slide's text ends with.
fn last_box(typing: &Typing, cx: &TestAppContext) -> Range<usize> {
    let source = typing.source(cx);
    let start = source.rfind("<Box ").expect("a box");
    let end = source[start..].find("</Box>\n").expect("its end") + start + "</Box>".len();
    start..end
}

#[gpui_kit::test]
fn a_table_dragged_out_is_in_a_box_where_it_was_dropped(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let (before, (dx, dy)) = drag_table_out(&typing, cx, point(px(150.), px(120.)));
    let expected = lifted(TALK, TABLE.trim_end(), (before.x + dx, before.y + dy, before.w, before.h));
    assert_eq!(typing.source(cx), expected);
    assert!(typing.source(cx).contains(TABLE), "the table's own bytes are as they were");
    let shown = typing.layout(cx).element(&last_box(&typing, cx)).expect("the box is drawn");
    assert!(shown.placed);
    assert_eq!((shown.rect.x, shown.rect.y), ((before.x + dx).round(), (before.y + dy).round()));
    // The table in it is drawn at the size of the outline it was dragged as.
    let table = table_rect(&typing, cx);
    assert!((table.w - before.w.round()).abs() <= 0.5 && (table.h - before.h.round()).abs() <= 0.5, "{table:?} as {before:?}");
    assert_eq!((table.x, table.y), (shown.rect.x, shown.rect.y));
}

#[gpui_kit::test]
fn a_dashed_outline_of_it_follows_the_pointer(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let before = table_rect(&typing, cx);
    let from = border_point(&typing, cx);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    let unit = unit(&typing, cx);
    for step in 1..=3 {
        let to = from + point(px(30. * step as f32), px(20. * step as f32));
        typing.move_held(cx, to, NO_SNAP);
        let outline = bounds_of(&typing, cx, LIFT_OUTLINE_ID).expect("the outline shows");
        let (dx, dy) = on_slide(&typing, cx, from, to);
        let corner = typing.at(cx, before.x + dx, before.y + dy);
        assert!((outline.origin.x - corner.x).abs() <= px(1.) && (outline.origin.y - corner.y).abs() <= px(1.), "step {step}: {outline:?}");
        assert!((f32::from(outline.size.width) - before.w * unit).abs() <= 1., "its drawn size");
        assert!((f32::from(outline.size.height) - before.h * unit).abs() <= 1.);
        assert_eq!(typing.source(cx), TALK, "nothing is written while dragging");
    }
    typing.mouse_up(cx, from + point(px(90.), px(60.)), 1, NO_SNAP);
    assert!(bounds_of(&typing, cx, LIFT_OUTLINE_ID).is_none(), "gone once let go of");
}

#[gpui_kit::test]
fn the_outline_snaps_as_a_move_does(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let before = table_rect(&typing, cx);
    let from = border_point(&typing, cx);
    // To three units from the slide's left edge, which it snaps to.
    let to = from - point(px((before.x - 3.) * unit(&typing, cx)), px(0.)) + point(px(0.), px(200.));
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(-20.), px(10.)), Modifiers::default());
    typing.move_held(cx, to, Modifiers::default());
    assert!(bounds_of(&typing, cx, SNAP_GUIDE_ID).is_some(), "a guide shows the line met");
    let outline = bounds_of(&typing, cx, LIFT_OUTLINE_ID).expect("the outline shows");
    assert!((outline.origin.x - typing.slide_bounds(cx).origin.x).abs() <= px(1.), "{outline:?}");
    typing.mouse_up(cx, to, 1, Modifiers::default());
    assert!(typing.source(cx).contains("<Box x=\"0\" y=\""), "{}", typing.source(cx));
}

#[gpui_kit::test]
fn the_text_closes_up_and_nothing_else_changes(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let flow_of = |typing: &Typing, cx: &mut TestAppContext, words: &str| {
        let slide = typing.current(cx);
        let flows = typing.flows(cx, slide);
        let flow = flows.iter().position(|flow| flow.paras.iter().any(|para| para.spans.iter().any(|span| span.text == words))).expect("the words");
        let para = flows[flow].paras.iter().position(|para| para.spans.iter().any(|span| span.text == words)).unwrap();
        typing.layout(cx).caret(flow, pos(para, 0)).expect("a caret").top
    };
    let (some, more) = (flow_of(&typing, cx, "Some words here"), flow_of(&typing, cx, "More words"));
    drag_table_out(&typing, cx, point(px(150.), px(120.)));
    assert_eq!(flow_of(&typing, cx, "Some words here"), some, "the words before stay");
    assert!(flow_of(&typing, cx, "More words") < more - 50., "the words after close up");
    let source = typing.source(cx);
    assert_eq!(&source[source.find("\n---\n\n# Two").unwrap()..], "\n---\n\n# Two\n", "the next slide is as it was");
    assert_eq!(typing.current(cx), 0);
}

#[gpui_kit::test]
fn it_is_one_undo_step_and_undo_is_exact(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let table = table_source(&typing, cx);
    drag_table_out(&typing, cx, point(px(150.), px(120.)));
    assert_ne!(typing.source(cx), TALK);
    assert_eq!(undo_label(&typing, cx), Some(MOVE_OUT));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK, "one step takes it back");
    assert!(!can_undo(&typing, cx));
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(table)]), "and selects the table where it was");
}

#[gpui_kit::test]
fn the_lifted_table_can_be_typed_in_and_gains_a_row(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    drag_table_out(&typing, cx, point(px(150.), px(120.)));
    let lifted = typing.source(cx);
    let slide = typing.current(cx);
    let flow = typing
        .flows(cx, slide)
        .iter()
        .position(|flow| matches!(flow.place, FlowPlace::Cell { row: 1, column: 1, .. }))
        .expect("its last cell can be typed in");
    typing.click_text(cx, flow, pos(0, 1));
    assert!(matches!(typing.editing(cx).map(|editing| editing.flow), Some(FlowKey::Cell { row: 1, column: 1, .. })), "the click types in the cell");
    typing.type_text(cx, "x");
    assert_eq!(typing.source(cx), lifted.replacen("| 1 | 2 |", "| 1 | 2x |", 1));
    typing.press(cx, "tab");
    assert_eq!(typing.source(cx), lifted.replacen("| 1 | 2 |", "| 1 | 2x |\n|  |  |", 1), "a row, in the box");
}

#[gpui_kit::test]
fn a_shape_tag_in_the_text_gets_its_place_written_and_no_box(cx: &mut TestAppContext) {
    let typing = open(cx, SHAPE_TALK);
    let shape = thing(&typing, cx, SHAPE);
    assert!(!shape.placed);
    let from = middle(&typing, cx, &shape);
    let to = from + point(px(200.), px(80.));
    drag_through(&typing, cx, from, &[from + point(px(20.), px(5.)), to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let Rect { x, y, w, h } = shape.rect;
    let placed = format!("<Shape kind=\"rect\" fill=\"#2563eb\" x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\" />", whole(x + dx), whole(y + dy), whole(w), whole(h));
    assert_eq!(typing.source(cx), SHAPE_TALK.replacen(SHAPE, &placed, 1));
    assert!(thing(&typing, cx, "<Shape").placed);
    assert_eq!(undo_label(&typing, cx), Some(MOVE_OUT));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), SHAPE_TALK);
}

#[gpui_kit::test]
fn a_hand_written_picture_is_lifted_in_a_box(cx: &mut TestAppContext) {
    let typing = open_with_photo(cx, PICTURE_TALK);
    let picture = thing(&typing, cx, PICTURE);
    let from = middle(&typing, cx, &picture);
    let to = from + point(px(-100.), px(60.));
    drag_through(&typing, cx, from, &[from + point(px(-20.), px(5.)), to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let Rect { x, y, w, h } = picture.rect;
    assert_eq!(typing.source(cx), lifted(PICTURE_TALK, PICTURE, (x + dx, y + dy, w, h)));
    assert_eq!(typing.status(cx).as_deref(), Some("Picture in a box selected"));
    let shown = thing(&typing, cx, PICTURE).rect;
    assert!((shown.w - w.round()).abs() <= 0.5 && (shown.h - h.round()).abs() <= 0.5, "drawn at the outline's size: {shown:?}");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), PICTURE_TALK);
}

#[gpui_kit::test]
fn the_lifted_thing_stays_selected(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    drag_table_out(&typing, cx, point(px(150.), px(120.)));
    // What moves now is the box the table is in, so that's what's selected.
    let boxed = last_box(&typing, cx);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(boxed.clone())]));
    assert_eq!(typing.status(cx).as_deref(), Some("Table in a box selected"));
    let rect = typing.layout(cx).element(&boxed).expect("the box is drawn").rect;
    let outline = outline(&typing, cx).expect("its outline");
    let corner = typing.at(cx, rect.x, rect.y);
    assert!((outline.origin.x - corner.x).abs() <= px(1.) && (outline.origin.y - corner.y).abs() <= px(1.));
    // A shape keeps its own place in the text, and stays selected there.
    let typing = open(cx, SHAPE_TALK);
    let from = middle_of(&typing, cx, SHAPE);
    drag_through(&typing, cx, from, &[from + point(px(30.), px(10.)), from + point(px(90.), px(40.))], NO_SNAP);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(thing(&typing, cx, "<Shape").source)]));
    assert_eq!(typing.status(cx).as_deref(), Some("Shape selected"));
}

fn middle_of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Point<Pixels> {
    let element = thing(typing, cx, start);
    middle(typing, cx, &element)
}

fn handle_at(typing: &Typing, cx: &mut TestAppContext, handle: Handle) -> Option<Bounds<Pixels>> {
    bounds_of(typing, cx, SharedString::from(handle_id(handle)))
}

#[gpui_kit::test]
fn once_lifted_it_has_handles_and_moves_as_any_thing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    drag_table_out(&typing, cx, point(px(150.), px(120.)));
    assert!(handle_at(&typing, cx, Handle::BottomRight).is_some(), "it has handles");
    let lifted = typing.source(cx);
    let boxed = last_box(&typing, cx);
    let rect = typing.layout(cx).element(&boxed).expect("the box").rect;
    // Dragged again by the table's border, it's the box that moves, as any thing does.
    let from = border_point(&typing, cx);
    let to = from + point(px(-60.), px(-40.));
    drag_through(&typing, cx, from, &[from + point(px(-20.), px(-10.)), to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, from, to);
    let place = format!("<Box x=\"{}\" y=\"{}\"", whole(rect.x + dx), whole(rect.y + dy));
    let was = format!("<Box x=\"{}\" y=\"{}\"", whole(rect.x), whole(rect.y));
    assert_eq!(typing.source(cx), lifted.replacen(&was, &place, 1));
    assert_eq!(undo_label(&typing, cx), Some("Move"));
    // And nudged with the keys.
    typing.press(cx, "right");
    assert!(typing.source(cx).contains(&format!("<Box x=\"{}\"", whole(rect.x + dx + 1.))));
}

#[gpui_kit::test]
fn escape_leaves_it_in_the_text(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let table = table_source(&typing, cx);
    let from = border_point(&typing, cx);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(20.), px(10.)), NO_SNAP);
    typing.move_held(cx, from + point(px(120.), px(80.)), NO_SNAP);
    assert!(bounds_of(&typing, cx, LIFT_OUTLINE_ID).is_some());
    typing.press(cx, "escape");
    assert!(bounds_of(&typing, cx, LIFT_OUTLINE_ID).is_none(), "the outline goes at once");
    typing.move_held(cx, from + point(px(140.), px(80.)), NO_SNAP);
    typing.mouse_up(cx, from + point(px(140.), px(80.)), 1, NO_SNAP);
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(table)]), "it stays selected");
}

/// Presses on `from`, drags some way and says what the slide says, before letting go there.
fn drag_saying(typing: &Typing, cx: &mut TestAppContext, from: Point<Pixels>) -> Option<String> {
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(20.), px(10.)), NO_SNAP);
    typing.move_held(cx, from + point(px(120.), px(80.)), NO_SNAP);
    assert!(bounds_of(typing, cx, LIFT_OUTLINE_ID).is_none(), "no outline");
    let said = typing.status(cx);
    typing.mouse_up(cx, from + point(px(120.), px(80.)), 1, NO_SNAP);
    said
}

#[gpui_kit::test]
fn a_thing_inside_a_component_is_refused_in_words(cx: &mut TestAppContext) {
    // One of the talk's own components that draws what's written between its tags.
    let talk = "---\ntitle: T\n---\n\n# Things\n\n<Callout title=\"Note\">\n\n<Icon name=\"star\" />\n\n</Callout>\n\nMore words\n";
    let folder = components_talk(None);
    std::fs::write(folder.path().join("slides.md"), talk).unwrap();
    let typing = open_folder(cx, folder, None);
    typing.with_window(cx, |window, _| window.activate_window());
    typing.settle(cx);
    let icon = thing(&typing, cx, "<Icon");
    let from = middle(&typing, cx, &icon);
    typing.click(cx, from);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(icon.source.clone())]), "the icon is what's pressed on");
    assert_eq!(drag_saying(&typing, cx, from).as_deref(), Some(PART_OF_IT));
    assert_eq!(typing.source(cx), talk);
    assert!(!can_undo(&typing, cx));
    // So in a box with no place of its own.
    let talk = "---\ntitle: T\n---\n\n# Things\n\n<Box fill=\"#dbeafe\">\n\n<Icon name=\"star\" />\n\n</Box>\n\nMore words\n";
    let typing = open(cx, talk);
    let from = middle_of(&typing, cx, "<Icon");
    assert_eq!(drag_saying(&typing, cx, from).as_deref(), Some(PART_OF_IT));
    assert_eq!(typing.source(cx), talk);
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn a_table_in_a_placed_box_with_more_says_to_drag_the_box(cx: &mut TestAppContext) {
    let talk = "---\ntitle: T\n---\n\n# Things\n\n<Box x=\"200\" y=\"500\" w=\"900\" h=\"400\">\n\nAbove\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n</Box>\n";
    let typing = open(cx, talk);
    let from = border_point(&typing, cx);
    typing.click(cx, from);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(table_source(&typing, cx))]));
    assert_eq!(drag_saying(&typing, cx, from).as_deref(), Some(IN_A_PLACED_BOX));
    assert_eq!(typing.source(cx), talk);
    assert!(!can_undo(&typing, cx));
}

const ICON_BOX: &str = "---\ntitle: T\n---\n\n# Things\n\n<Box x=\"200\" y=\"500\" w=\"400\" h=\"300\">\n\n<Icon name=\"star\" size=\"96\" />\n\n</Box>\n";

#[gpui_kit::test]
fn a_second_press_picks_what_a_box_holds_and_escape_goes_back(cx: &mut TestAppContext) {
    let typing = open(cx, ICON_BOX);
    let boxed = thing(&typing, cx, "<Box").source;
    let icon = thing(&typing, cx, "<Icon").source;
    let at = middle_of(&typing, cx, "<Icon");
    typing.click(cx, at);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(boxed.clone())]), "the first press picks the box");
    typing.click(cx, at);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(icon.clone())]), "the next, what it holds");
    assert_eq!(typing.status(cx).as_deref(), Some("Icon selected"));
    // Its own form.
    click_on(&typing, cx, SharedString::from(field_id("size")));
    typing.press(cx, "ctrl-a");
    typing.type_text(cx, "120");
    typing.press(cx, "enter");
    assert_eq!(typing.source(cx), ICON_BOX.replacen("size=\"96\"", "size=\"120\"", 1));
    // A press on it again keeps it selected.
    let source = typing.source(cx);
    let icon = thing(&typing, cx, "<Icon").source;
    typing.click(cx, at);
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(icon)]));
    typing.press(cx, "escape");
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(thing(&typing, cx, "<Box").source)]), "Escape goes back to the box");
    typing.press(cx, "escape");
    assert_eq!(selection(&typing, cx), None);
    assert_eq!(typing.source(cx), source);
}

#[gpui_kit::test]
fn a_drag_of_what_a_box_holds_moves_the_box(cx: &mut TestAppContext) {
    let typing = open(cx, ICON_BOX);
    let at = middle_of(&typing, cx, "<Icon");
    typing.click(cx, at);
    typing.click(cx, at);
    let icon = thing(&typing, cx, "<Icon").source;
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(icon)]));
    let to = at + point(px(60.), px(30.));
    drag_through(&typing, cx, at, &[at + point(px(20.), px(10.)), to], NO_SNAP);
    let (dx, dy) = on_slide(&typing, cx, at, to);
    let place = format!("<Box x=\"{}\" y=\"{}\"", whole(200. + dx), whole(500. + dy));
    assert_eq!(typing.source(cx), ICON_BOX.replacen("<Box x=\"200\" y=\"500\"", &place, 1));
    assert_eq!(undo_label(&typing, cx), Some("Move"));
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(thing(&typing, cx, "<Icon").source)]), "what it holds stays selected");
    // From the box selected, a drag on what it holds moves the box and keeps the box selected.
    let typing = open(cx, ICON_BOX);
    typing.click(cx, at);
    drag_through(&typing, cx, at, &[at + point(px(20.), px(10.)), to], NO_SNAP);
    assert_eq!(typing.source(cx), ICON_BOX.replacen("<Box x=\"200\" y=\"500\"", &place, 1));
    assert_eq!(selection(&typing, cx), Some(vec![Thing::Block(thing(&typing, cx, "<Box").source)]));
}

#[gpui_kit::test]
fn tab_reaches_what_a_box_holds(cx: &mut TestAppContext) {
    let typing = open(cx, ICON_BOX);
    let icon = thing(&typing, cx, "<Icon").source;
    let at = middle_of(&typing, cx, "<Icon");
    typing.click(cx, at);
    let reached = (0..4).any(|_| {
        typing.press(cx, "tab");
        selection(&typing, cx) == Some(vec![Thing::Block(icon.clone())])
    });
    assert!(reached);
}

#[gpui_kit::test]
fn delete_takes_what_is_selected_of_a_box(cx: &mut TestAppContext) {
    // What it holds alone.
    let typing = open(cx, ICON_BOX);
    let at = middle_of(&typing, cx, "<Icon");
    typing.click(cx, at);
    typing.click(cx, at);
    typing.press(cx, "delete");
    let source = typing.source(cx);
    assert!(source.contains("<Box x=\"200\"") && !source.contains("<Icon"), "{source}");
    undo(&typing, cx);
    assert_eq!(typing.source(cx), ICON_BOX);
    // The box, and what it holds with it.
    let typing = open(cx, ICON_BOX);
    typing.click(cx, at);
    typing.press(cx, "delete");
    let source = typing.source(cx);
    assert!(!source.contains("<Box") && !source.contains("<Icon"), "{source}");
}

/// Readies a drag of `TALK`'s table out of the text, and moves it some way, button down.
fn start_lift(typing: &Typing, cx: &mut TestAppContext) -> Point<Pixels> {
    let from = border_point(typing, cx);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(20.), px(10.)), NO_SNAP);
    let to = from + point(px(120.), px(80.));
    typing.move_held(cx, to, NO_SNAP);
    assert!(bounds_of(typing, cx, LIFT_OUTLINE_ID).is_some(), "dragging");
    to
}

/// That the drag left no outline, no step and the bytes `source`, once let go of at `to`.
fn lift_left_nothing(typing: &Typing, cx: &mut TestAppContext, to: Point<Pixels>, source: &str, what: &str) {
    assert!(bounds_of(typing, cx, LIFT_OUTLINE_ID).is_none(), "{what}: the outline goes at once");
    typing.move_held(cx, to + point(px(30.), px(0.)), NO_SNAP);
    typing.mouse_up(cx, to, 1, NO_SNAP);
    assert!(bounds_of(typing, cx, LIFT_OUTLINE_ID).is_none(), "{what}");
    assert_eq!(typing.source(cx), source, "{what}");
    assert_ne!(undo_label(typing, cx), Some(MOVE_OUT), "{what}: no step");
}

#[gpui_kit::test]
fn a_lift_ended_any_other_way_leaves_it_in_the_text(cx: &mut TestAppContext) {
    // The keyboard going elsewhere.
    let typing = open(cx, TALK);
    let to = start_lift(&typing, cx);
    let elsewhere = cx.update(|cx| cx.focus_handle());
    typing.with_window(cx, |window, cx| elsewhere.focus(window, cx));
    typing.settle(cx);
    lift_left_nothing(&typing, cx, to, TALK, "focus");

    // An edit arriving: the file changed outside.
    let typing = open(cx, TALK);
    let to = start_lift(&typing, cx);
    let outside = TALK.replace("# Two", "# Two, changed");
    typing.view.update(cx, |view, cx| view.reload_for_test(Some(outside.clone()), cx));
    typing.settle(cx);
    lift_left_nothing(&typing, cx, to, &outside, "an edit");

    // What's selected changing.
    let typing = open(cx, TALK);
    let to = start_lift(&typing, cx);
    typing.press(cx, "ctrl-a");
    lift_left_nothing(&typing, cx, to, TALK, "a selection change");

    // The button let go of where the window never heard.
    let typing = open(cx, TALK);
    let to = start_lift(&typing, cx);
    typing.with_window(cx, |window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(gpui_kit::MouseMoveEvent { position: to, pressed_button: None, modifiers: Modifiers::default() }.to_platform_input(), cx);
    });
    typing.settle(cx);
    lift_left_nothing(&typing, cx, to, TALK, "a lost release");
}

/// Drags the thing at `from` out of the text, and says whether it was lifted: then one undo gives
/// `talk` back byte for byte. Otherwise nothing was written, and the slide said why in words.
fn lifted_exactly_or_refused(typing: &Typing, cx: &mut TestAppContext, talk: &str, from: Point<Pixels>, what: &str) -> bool {
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(20.), px(10.)), NO_SNAP);
    typing.move_held(cx, from + point(px(100.), px(60.)), NO_SNAP);
    let said = typing.status(cx);
    typing.mouse_up(cx, from + point(px(100.), px(60.)), 1, NO_SNAP);
    if typing.source(cx) == talk {
        assert!(!can_undo(typing, cx), "{what}");
        let said = said.unwrap_or_default();
        assert!(said.ends_with('.') && !said.ends_with("selected"), "{what}: said why: {said:?}");
        return false;
    }
    assert_eq!(undo_label(typing, cx), Some(MOVE_OUT), "{what}");
    undo(typing, cx);
    assert_eq!(typing.source(cx), talk, "{what}: undo is exact");
    true
}

#[gpui_kit::test]
fn a_lift_is_exact_or_refused_on_odd_slides(cx: &mut TestAppContext) {
    let with_notes = "---\ntitle: T\n---\n\n# Things\n\nSome words here\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\nMore words\n\n<Notes>\nSay this.\n</Notes>\n";
    let typing = open(cx, with_notes);
    let from = border_point(&typing, cx);
    assert!(lifted_exactly_or_refused(&typing, cx, with_notes, from, "notes"), "with notes it's lifted");

    let in_a_list = "---\ntitle: T\n---\n\n# Things\n\n- Item\n\n  | a | b |\n  | --- | --- |\n  | 1 | 2 |\n\n- Next\n";
    let typing = open(cx, in_a_list);
    let from = border_point(&typing, cx);
    assert!(lifted_exactly_or_refused(&typing, cx, in_a_list, from, "a list"), "a list: lifted");

    let only_a_table = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let typing = open(cx, only_a_table);
    let from = border_point(&typing, cx);
    assert!(lifted_exactly_or_refused(&typing, cx, only_a_table, from, "only a table"), "only a table: lifted");

    let only_a_picture = "![A photo](assets/photo.png)\n";
    let typing = open_with_photo(cx, only_a_picture);
    let from = middle_of(&typing, cx, PICTURE);
    assert!(lifted_exactly_or_refused(&typing, cx, only_a_picture, from, "only a picture"), "only a picture: lifted");

    let crlf = TALK.replace('\n', "\r\n");
    let typing = open(cx, &crlf);
    let from = border_point(&typing, cx);
    assert!(lifted_exactly_or_refused(&typing, cx, &crlf, from, "crlf"), "with CRLF it's lifted");
}

#[gpui_kit::test]
fn several_selected_with_one_in_the_text_are_refused(cx: &mut TestAppContext) {
    let talk = "---\ntitle: T\n---\n\n# Things\n\n<Shape kind=\"rect\" fill=\"#2563eb\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1300\" y=\"300\" w=\"200\" h=\"200\" />\n";
    let typing = open(cx, talk);
    let ellipse = middle_of(&typing, cx, "<Shape kind=\"ellipse\"");
    typing.click(cx, ellipse);
    let rect = middle_of(&typing, cx, "<Shape kind=\"rect\"");
    typing.clicks(cx, rect, 1, Modifiers::shift());
    assert_eq!(selection(&typing, cx).map(|things| things.len()), Some(2));
    drag_through(&typing, cx, rect, &[rect + point(px(30.), px(0.)), rect + point(px(90.), px(20.))], NO_SNAP);
    assert_eq!(typing.source(cx), talk);
    assert!(bounds_of(&typing, cx, LIFT_OUTLINE_ID).is_none());
    assert_eq!(typing.status(cx).as_deref(), Some(DRAG_OUT_FIRST));
}
