//! Picking several things at once, with Shift or Ctrl and a click, Ctrl+A or Tab, and picking a
//! layout's area by its frame; what's done to all of them at once, and what's refused for an area.

use std::ops::Range;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{Focusable as _, Modifiers, Pixels, Point, TestAppContext};
use slides_engine::ElementBox;
use stagepack_studio::inspector::{INSPECTOR_HEADING_ID, INSPECTOR_PUT_BACK_ID, field_id};
use stagepack_studio::notes_panel::NOTES_ID;
use stagepack_studio::selection::{Selection, Thing};
use stagepack_studio::slide_editor::{SELECTION_OUTLINE_ID, region_frame_id, selection_outline_id};

const TALK: &str = "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1300\" y=\"300\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1600\" y=\"300\" w=\"200\" h=\"200\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n---\n\n# Two\n";
const RECT: &str = "<Shape kind=\"rect\"";
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";
const ICON: &str = "<Icon";
const AREA_STAYS: &str = "A layout's area can't be deleted or copied. Choose another layout to change the areas.";

fn selection(typing: &Typing, cx: &TestAppContext) -> Option<Selection> {
    typing.document(cx).read_with(cx, |document, _| document.selection().cloned())
}

/// The blocks selected, by where their text starts, in the order picked.
fn blocks(typing: &Typing, cx: &TestAppContext) -> Vec<Range<usize>> {
    selection(typing, cx)
        .map(|selection| {
            selection
                .things
                .into_iter()
                .filter_map(|thing| match thing {
                    Thing::Block(range) => Some(range),
                    Thing::Region(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Range<usize> {
    thing(typing, cx, start).source
}

fn click_thing(typing: &Typing, cx: &mut TestAppContext, start: &str, modifiers: Modifiers) {
    let element = thing(typing, cx, start);
    let at = middle(typing, cx, &element);
    typing.clicks(cx, at, 1, modifiers);
}

/// Selects the things starting with each of `starts`, the first with a click and the rest with
/// Shift and a click.
fn select_things(typing: &Typing, cx: &mut TestAppContext, starts: &[&str]) {
    for (index, start) in starts.iter().enumerate() {
        click_thing(typing, cx, start, if index == 0 { Modifiers::default() } else { Modifiers::shift() });
    }
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

/// Undoes the last step, as the Undo tool does, wherever the keyboard is.
fn undo(typing: &Typing, cx: &mut TestAppContext) {
    typing.document(cx).update(cx, |document, cx| {
        document.undo().expect("undone");
        cx.notify();
    });
    typing.settle(cx);
}

fn label_of(typing: &Typing, cx: &mut TestAppContext, id: &str) -> Option<String> {
    let id = gpui_kit::SharedString::from(id.to_string());
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

/// The flow of the slide's main text.
fn body_flow(typing: &Typing, cx: &mut TestAppContext) -> usize {
    let layout = typing.layout(cx);
    (0..typing.flows(cx, typing.current(cx)).len())
        .find(|&flow| layout.region_of_flow(flow) == Some("body"))
        .expect("the main text")
}

/// A point on the left line of area `name`'s frame, halfway down.
fn on_frame(typing: &Typing, cx: &mut TestAppContext, name: &str) -> Point<Pixels> {
    let rect = typing.layout(cx).region(name).expect("the area").rect;
    typing.at(cx, rect.x, rect.y + rect.h / 2.)
}

/// Starts typing in the main text, and selects its area with a press on its frame.
fn select_body(typing: &Typing, cx: &mut TestAppContext) {
    let body = body_flow(typing, cx);
    typing.click_text(cx, body, pos(0, 3));
    assert!(typing.editing(cx).is_some(), "typing in the main text");
    let at = on_frame(typing, cx, "body");
    typing.click(cx, at);
    assert_eq!(selection(typing, cx).map(|selection| selection.things), Some(vec![Thing::Region("body".into())]));
}

fn type_in_field(typing: &Typing, cx: &mut TestAppContext, name: &str, text: &str) {
    click_on(typing, cx, gpui_kit::SharedString::from(field_id(name)));
    typing.press(cx, "ctrl-a");
    typing.type_text(cx, text);
    typing.press(cx, "enter");
}

#[gpui_kit::test]
fn shift_click_adds_and_takes_away(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let (rect, ellipse, icon) = (of(&typing, cx, RECT), of(&typing, cx, ELLIPSE), of(&typing, cx, ICON));
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert_eq!(blocks(&typing, cx), vec![rect.clone()]);
    assert_eq!(selected(&typing, cx), Some((0, rect.clone())), "one thing is the one selected");
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(blocks(&typing, cx), vec![rect.clone(), ellipse.clone()]);
    assert_eq!(selected(&typing, cx), None, "two aren't one");
    assert_eq!(typing.status(cx).as_deref(), Some("2 things selected"));
    // The last picked has the outline the editor always drew; the others have their own.
    let drawn = |start: &str, typing: &Typing, cx: &mut TestAppContext| {
        let element = thing(typing, cx, start);
        middle(typing, cx, &element)
    };
    let last = outline(&typing, cx).expect("an outline");
    assert!(last.contains(&drawn(ELLIPSE, &typing, cx)), "the last picked is the ellipse");
    let first = bounds_of(&typing, cx, selection_outline_id(0)).expect("the other outline");
    assert!(first.contains(&drawn(RECT, &typing, cx)));
    click_thing(&typing, cx, ICON, Modifiers::control());
    assert_eq!(blocks(&typing, cx), vec![rect.clone(), ellipse.clone(), icon.clone()]);
    assert_eq!(typing.status(cx).as_deref(), Some("3 things selected"));
    assert!(bounds_of(&typing, cx, selection_outline_id(1)).is_some());
    click_thing(&typing, cx, RECT, Modifiers::shift());
    assert_eq!(blocks(&typing, cx), vec![ellipse.clone(), icon], "Shift takes one away again");
    click_thing(&typing, cx, ICON, Modifiers::shift());
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(selection(&typing, cx), None, "taking the last away leaves nothing selected");
    assert!(outline(&typing, cx).is_none());
    assert_eq!(typing.source(cx), TALK, "selecting writes nothing");
}

#[gpui_kit::test]
fn ctrl_a_selects_every_thing_and_no_area(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_thing(&typing, cx, ELLIPSE, Modifiers::default());
    typing.press(cx, "ctrl-a");
    let every = [RECT, ELLIPSE, ICON].map(|start| of(&typing, cx, start)).to_vec();
    let mut picked = blocks(&typing, cx);
    picked.sort_by_key(|range| range.start);
    assert_eq!(picked, every);
    let things = selection(&typing, cx).expect("a selection").things;
    assert_eq!(things.len(), 3, "and no area");
    assert_eq!(typing.status(cx).as_deref(), Some("3 things selected"));
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_click_without_shift_selects_one_again(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    select_things(&typing, cx, &[RECT, ELLIPSE, ICON]);
    assert_eq!(blocks(&typing, cx).len(), 3);
    click_thing(&typing, cx, ELLIPSE, Modifiers::default());
    assert_eq!(blocks(&typing, cx), vec![of(&typing, cx, ELLIPSE)], "a click on one of them");
    click_thing(&typing, cx, RECT, Modifiers::shift());
    click_thing(&typing, cx, ICON, Modifiers::default());
    assert_eq!(blocks(&typing, cx), vec![of(&typing, cx, ICON)], "a click on another");
    assert_eq!(typing.status(cx).as_deref(), Some("Icon selected"));
}

#[gpui_kit::test]
fn delete_takes_every_selected_thing_in_one_step(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    select_things(&typing, cx, &[ICON, RECT]);
    typing.press(cx, "delete");
    let source = typing.source(cx);
    assert!(!source.contains(RECT) && !source.contains(ICON), "both are gone");
    assert!(source.contains(ELLIPSE), "and the other stays");
    assert_eq!(selection(&typing, cx), None);
    assert_eq!(undo_label(&typing, cx), Some("Delete"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "one undo brings both back");
    assert!(!can_undo(&typing, cx), "it was one step");
}

#[gpui_kit::test]
fn copy_and_paste_of_two_things_brings_both(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    // Picked in the other order: the copy is in the order they're written.
    select_things(&typing, cx, &[ELLIPSE, RECT]);
    typing.press(cx, "ctrl-c");
    let (rect, ellipse) = (of(&typing, cx, RECT), of(&typing, cx, ELLIPSE));
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied, Some(format!("{}\n\n{}", &TALK[rect], &TALK[ellipse])));
    typing.press(cx, "ctrl-v");
    let source = typing.source(cx);
    assert_eq!(source.matches("<Shape").count(), 4, "both are pasted");
    let picked = blocks(&typing, cx);
    assert_eq!(picked.len(), 2, "and both are selected");
    let first_new = source.match_indices(RECT).nth(1).expect("the pasted rectangle").0;
    assert!(picked.iter().all(|range| range.start >= first_new), "the pasted ones");
    assert_eq!(undo_label(&typing, cx), Some("Paste"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "one step");
}

#[gpui_kit::test]
fn duplicate_of_two_things_selects_the_copies(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    select_things(&typing, cx, &[RECT, ELLIPSE]);
    typing.press(cx, "ctrl-d");
    let source = typing.source(cx);
    assert_eq!(source.matches("<Shape").count(), 4);
    let picked = blocks(&typing, cx);
    assert_eq!(picked.len(), 2);
    let texts: Vec<&str> = picked.iter().map(|range| &source[range.clone()]).collect();
    assert!(texts.iter().any(|text| text.starts_with(RECT)) && texts.iter().any(|text| text.starts_with(ELLIPSE)));
    let originals = [of(&typing, cx, RECT), of(&typing, cx, ELLIPSE)];
    assert!(picked.iter().all(|range| range.start > originals[1].end), "the copies, not the originals");
    assert_eq!(undo_label(&typing, cx), Some("Duplicate"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "one step");
}

#[gpui_kit::test]
fn undo_brings_back_the_things_and_the_selection(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let (rect, icon) = (of(&typing, cx, RECT), of(&typing, cx, ICON));
    select_things(&typing, cx, &[ICON, RECT]);
    typing.press(cx, "delete");
    typing.press(cx, "ctrl-z");
    assert_eq!(selection(&typing, cx), Some(Selection { slide: 0, things: vec![Thing::Block(icon), Thing::Block(rect)] }));
    assert!(outline(&typing, cx).is_some());
    assert!(bounds_of(&typing, cx, selection_outline_id(0)).is_some());
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(selection(&typing, cx), None, "Redo takes them away again");
    assert!(!typing.source(cx).contains(ICON));
}

#[gpui_kit::test]
fn a_press_on_an_areas_frame_selects_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let body = body_flow(&typing, cx);
    assert!(bounds_of(&typing, cx, region_frame_id("body")).is_none(), "no frame before typing");
    typing.click_text(cx, body, pos(0, 3));
    let frame = bounds_of(&typing, cx, region_frame_id("body")).expect("the frame shows while typing");
    let rect = typing.layout(cx).region("body").expect("the area").rect;
    let corner = typing.at(cx, rect.x, rect.y);
    assert!((frame.origin.x - corner.x).abs() <= gpui_kit::px(1.) && (frame.origin.y - corner.y).abs() <= gpui_kit::px(1.));
    let at = on_frame(&typing, cx, "body");
    // Three pixels inside the line is on it.
    typing.click(cx, at + gpui_kit::point(gpui_kit::px(3.), gpui_kit::px(0.)));
    assert_eq!(selection(&typing, cx), Some(Selection { slide: 0, things: vec![Thing::Region("body".into())] }));
    assert!(typing.editing(cx).is_none(), "typing ended");
    assert_eq!(typing.status(cx).as_deref(), Some("Main text area selected"));
    assert!(outline(&typing, cx).is_some(), "the area has an outline");
    assert_eq!(selected(&typing, cx), None, "an area isn't a block");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_press_on_its_text_still_types(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let body = body_flow(&typing, cx);
    typing.click_text(cx, body, pos(0, 3));
    typing.click_text(cx, body, pos(0, 8));
    assert!(typing.editing(cx).is_some(), "still typing");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 8)));
    assert_eq!(selection(&typing, cx), None);
    typing.type_text(cx, "X");
    assert!(typing.source(cx).contains("Some worXds here"));
    // At the start of a line that touches the frame, the press is on the text, and types.
    typing.click_text(cx, body, pos(0, 0));
    assert!(typing.editing(cx).is_some(), "still typing at the start");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 0)));
    assert_eq!(selection(&typing, cx), None);
}

#[gpui_kit::test]
fn tab_reaches_the_areas_after_the_things(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let things = [RECT, ELLIPSE, ICON].map(|start| Thing::Block(of(&typing, cx, start)));
    let areas: Vec<Thing> = typing.layout(cx).regions().iter().map(|region| Thing::Region(region.name.clone())).collect();
    assert!(areas.contains(&Thing::Region("title".into())) && areas.contains(&Thing::Region("body".into())));
    click_thing(&typing, cx, RECT, Modifiers::default());
    let order: Vec<Thing> = things.iter().cloned().chain(areas.iter().cloned()).collect();
    for next in order.iter().skip(1).chain([&order[0]]) {
        typing.press(cx, "tab");
        assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![next.clone()]));
    }
    typing.press(cx, "shift-tab");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![order.last().unwrap().clone()]));
    let title = order.iter().position(|thing| *thing == Thing::Region("title".into())).unwrap();
    for _ in 0..order.len() {
        if selection(&typing, cx).map(|selection| selection.things) == Some(vec![order[title].clone()]) {
            break;
        }
        typing.press(cx, "shift-tab");
    }
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![order[title].clone()]), "Shift+Tab reaches the title");
    assert_eq!(typing.status(cx).as_deref(), Some("Title area selected"));
    assert!(outline(&typing, cx).is_some());
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn delete_on_an_area_says_why_and_changes_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    select_body(&typing, cx);
    for key in ["delete", "backspace", "ctrl-x", "ctrl-c", "ctrl-d"] {
        typing.press(cx, key);
        assert_eq!(typing.status(cx).as_deref(), Some(AREA_STAYS), "{key}");
        assert_eq!(typing.source(cx), TALK, "{key}");
        assert!(!can_undo(&typing, cx), "{key}");
        assert!(selection(&typing, cx).is_some(), "{key}: the area stays selected");
    }
    // An area among things: the things are taken, and the area stays.
    click_thing(&typing, cx, RECT, Modifiers::shift());
    typing.press(cx, "delete");
    assert!(!typing.source(cx).contains(RECT));
    assert!(typing.source(cx).contains("Some words here"));
    assert_eq!(
        selection(&typing, cx).map(|selection| selection.things),
        Some(vec![Thing::Region("body".into())]),
        "the area stays selected"
    );
    assert!(outline(&typing, cx).is_some());
}

#[gpui_kit::test]
fn an_areas_place_fields_write_its_settings_line(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    select_body(&typing, cx);
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Main text area"));
    type_in_field(&typing, cx, "x", "120");
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", "title: T\nregion.body: x=120\n"));
    type_in_field(&typing, cx, "w", "800");
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", "title: T\nregion.body: x=120 w=800\n"));
    let rect = typing.layout(cx).region("body").expect("the area").rect;
    assert_eq!((rect.x, rect.w), (120., 800.), "the area is where it says");
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Region("body".into())]));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", "title: T\nregion.body: x=120\n"));
    assert_eq!(
        selection(&typing, cx).map(|selection| selection.things),
        Some(vec![Thing::Region("body".into())]),
        "undo selects the area again"
    );
}

#[gpui_kit::test]
fn put_back_removes_the_place_in_one_step(cx: &mut TestAppContext) {
    let moved = TALK.replace("title: T\n", "title: T\nregion.body: click=1 x=200 y=250 w=800 h=400 rotate=5\n");
    let typing = open(cx, &moved);
    select_body(&typing, cx);
    click_on(&typing, cx, INSPECTOR_PUT_BACK_ID);
    assert_eq!(typing.source(cx), TALK.replace("title: T\n", "title: T\nregion.body: click=1\n"), "only the place goes");
    assert_eq!(undo_label(&typing, cx), Some("Put back"));
    assert!(!typing.layout(cx).region("body").expect("the area").moved);
    // Not moved, the button does nothing.
    click_on(&typing, cx, INSPECTOR_PUT_BACK_ID);
    assert_eq!(undo_label(&typing, cx), Some("Put back"));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), moved, "one step");
    // An area its layout has where it is: nothing to put back.
    let plain = open(cx, TALK);
    select_body(&plain, cx);
    click_on(&plain, cx, INSPECTOR_PUT_BACK_ID);
    assert_eq!(plain.source(cx), TALK);
    assert!(!can_undo(&plain, cx));
}

/// A slide with a thing one unit wide, one covering the whole slide, sixty at one place, and one
/// mostly off the slide.
fn extremes() -> String {
    let mut slide = String::from("---\ntitle: T\n---\n\n# Edges\n\n");
    slide.push_str("<Shape kind=\"rect\" fill=\"#cccccc\" x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" />\n\n");
    for _ in 0..60 {
        slide.push_str("<Shape kind=\"rect\" fill=\"#2563eb\" x=\"300\" y=\"300\" w=\"100\" h=\"100\" />\n\n");
    }
    slide.push_str("<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1850\" y=\"500\" w=\"400\" h=\"100\" />\n\n");
    slide.push_str("<Shape kind=\"rect\" fill=\"#ff0000\" x=\"900\" y=\"700\" w=\"1\" h=\"200\" />\n");
    slide
}

#[gpui_kit::test]
fn every_thing_can_be_reached(cx: &mut TestAppContext) {
    let talk = extremes();
    let typing = open(cx, &talk);
    let things: Vec<ElementBox> = typing.layout(cx).elements().to_vec();
    assert_eq!(things.len(), 63);
    let thin = thing(&typing, cx, "<Shape kind=\"rect\" fill=\"#ff0000\"");
    let at = middle(&typing, cx, &thin);
    typing.click(cx, at);
    assert_eq!(blocks(&typing, cx), vec![thin.source.clone()], "a click on the thing one unit wide picks it");
    let areas = typing.layout(cx).regions().len();
    let mut seen: Vec<Thing> = Vec::new();
    for _ in 0..things.len() + areas {
        typing.press(cx, "tab");
        let picked = selection(&typing, cx).expect("something is selected").things;
        assert_eq!(picked.len(), 1);
        seen.push(picked[0].clone());
    }
    let reached: Vec<Range<usize>> = seen
        .iter()
        .filter_map(|thing| match thing {
            Thing::Block(range) => Some(range.clone()),
            Thing::Region(_) => None,
        })
        .collect();
    let mut every: Vec<Range<usize>> = things.iter().map(|element| element.source.clone()).collect();
    every.sort_by_key(|range| range.start);
    let mut sorted = reached.clone();
    sorted.sort_by_key(|range| range.start);
    assert_eq!(sorted, every, "Tab visits every thing exactly once");
    assert_eq!(seen.len() - reached.len(), areas, "and every area once");
    assert_eq!(seen.last(), Some(&Thing::Block(thin.source)), "and comes round again");
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn the_notes_taking_the_keyboard_lets_go(cx: &mut TestAppContext) {
    for area in [false, true] {
        let typing = open(cx, TALK);
        if area {
            select_body(&typing, cx);
        } else {
            select_things(&typing, cx, &[RECT, ELLIPSE]);
        }
        assert!(outline(&typing, cx).is_some());
        click_on(&typing, cx, NOTES_ID);
        let notes_have_it = typing.with_window(cx, |window, cx| typing.view.read(cx).notes_panel().read(cx).is_focused(window, cx));
        assert!(notes_have_it, "area {area}: the notes have the keyboard");
        assert_eq!(selection(&typing, cx), None, "area {area}: nothing is selected");
        assert!(outline(&typing, cx).is_none(), "area {area}: no outline");
        assert!(bounds_of(&typing, cx, selection_outline_id(0)).is_none());
        typing.type_text(cx, "hi");
        typing.press(cx, "backspace");
        assert!(typing.source(cx).contains(RECT) && typing.source(cx).contains(ELLIPSE), "area {area}: the keys are the notes'");
    }
}

#[gpui_kit::test]
fn nothing_redraws_while_two_things_sit_selected(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    select_things(&typing, cx, &[RECT, ICON]);
    assert_eq!(blocks(&typing, cx).len(), 2);
    let editor = cx.read(|cx| typing.view.read(cx).slide_editor().clone());
    let canvas = cx.read(|cx| typing.view.read(cx).slide_canvas().clone());
    let (editor_count, _a) = redraws(cx, &editor);
    let (view_count, _b) = redraws(cx, &typing.view);
    let (canvas_count, _c) = redraws(cx, &canvas);
    wait(cx, 20_000);
    assert_eq!((editor_count.get(), view_count.get(), canvas_count.get()), (0, 0, 0));
    assert!(outline(&typing, cx).is_some());
    assert!(bounds_of(&typing, cx, SELECTION_OUTLINE_ID).is_some());
}

#[gpui_kit::test]
fn undo_of_a_duplicate_or_a_paste_selects_what_was_selected(cx: &mut TestAppContext) {
    for paste in [false, true] {
        let typing = open(cx, TALK);
        let (rect, ellipse) = (of(&typing, cx, RECT), of(&typing, cx, ELLIPSE));
        select_things(&typing, cx, &[ELLIPSE, RECT]);
        if paste {
            typing.press(cx, "ctrl-c");
            typing.press(cx, "ctrl-v");
        } else {
            typing.press(cx, "ctrl-d");
        }
        assert_eq!(typing.source(cx).matches("<Shape").count(), 4, "paste {paste}");
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), TALK, "paste {paste}");
        let back = Selection { slide: 0, things: vec![Thing::Block(ellipse.clone()), Thing::Block(rect.clone())] };
        assert_eq!(selection(&typing, cx), Some(back), "paste {paste}: the two picked before, in the order picked");
        assert!(bounds_of(&typing, cx, selection_outline_id(0)).is_some());
        typing.press(cx, "ctrl-shift-z");
        assert_eq!(blocks(&typing, cx).len(), 2, "paste {paste}: Redo selects the copies again");
        assert!(blocks(&typing, cx).iter().all(|range| range.start > ellipse.end), "paste {paste}");
    }
}

#[gpui_kit::test]
fn an_area_its_layout_no_longer_has_is_let_go_of(cx: &mut TestAppContext) {
    let columns = TALK.replace("title: T\n", "title: T\nlayout: two-columns\n");
    for outside in [true, false] {
        let typing = open(cx, TALK);
        select_body(&typing, cx);
        let document = typing.document(cx);
        if outside {
            let text = columns.clone();
            cx.update(|cx| {
                document.update(cx, |document, cx| {
                    document.reload(Some(text));
                    cx.notify();
                })
            });
        } else {
            let at = TALK.find("title: T\n").unwrap() + "title: T\n".len();
            let change = slides_format::Change::single(slides_format::SourceEdit { range: at..at, text: "layout: two-columns\n".into() });
            cx.update(|cx| {
                document.update(cx, |document, cx| {
                    document.edit("Change layout", change, 0).expect("made");
                    cx.notify();
                })
            });
        }
        typing.settle(cx);
        assert_eq!(typing.source(cx), columns, "outside {outside}");
        assert_eq!(selection(&typing, cx), None, "outside {outside}: the main text is no area of two columns");
        assert!(outline(&typing, cx).is_none(), "outside {outside}");
        assert_ne!(typing.status(cx).as_deref(), Some("Main text area selected"), "outside {outside}");
        assert_ne!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Main text area"), "outside {outside}");
    }
}

#[gpui_kit::test]
fn the_notes_given_the_keyboard_without_the_mouse_let_go_and_keep_their_keys(cx: &mut TestAppContext) {
    for area in [false, true] {
        let typing = open(cx, TALK);
        if area {
            select_body(&typing, cx);
        } else {
            select_things(&typing, cx, &[RECT, ELLIPSE]);
        }
        // The window is in front, as it is when someone moves the keyboard with Tab or a shortcut.
        typing.with_window(cx, |window, _| window.activate_window());
        typing.settle(cx);
        let notes = cx.read(|cx| typing.view.read(cx).notes_panel().read(cx).field().focus_handle(cx));
        typing.with_window(cx, |window, cx| notes.focus(window, cx));
        typing.settle(cx);
        assert_eq!(selection(&typing, cx), None, "area {area}: let go of as soon as the notes have the keyboard");
        assert!(outline(&typing, cx).is_none(), "area {area}: no outline before anything is typed");
        typing.type_text(cx, "hi");
        let notes_have_it = typing.with_window(cx, |window, cx| typing.view.read(cx).notes_panel().read(cx).is_focused(window, cx));
        assert!(notes_have_it, "area {area}: the letters didn't take the keyboard back for the slide");
        assert_eq!(selection(&typing, cx), None, "area {area}: nothing on the slide is selected");
        assert!(outline(&typing, cx).is_none(), "area {area}");
        for _ in 0..3 {
            typing.press(cx, "backspace");
        }
        typing.press(cx, "delete");
        let source = typing.source(cx);
        assert!(source.contains(RECT) && source.contains(ELLIPSE) && source.contains(ICON), "area {area}: nothing on the slide is taken");
        assert!(source.contains("Some words here"), "area {area}");
        assert_eq!(cx.read(|cx| typing.view.read(cx).notes_panel().read(cx).text(cx)), "", "area {area}: the keys were the notes'");
    }
}

#[gpui_kit::test]
fn a_press_just_outside_the_frame_beside_a_line_of_text_selects_the_area(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let body = body_flow(&typing, cx);
    typing.click_text(cx, body, pos(0, 3));
    let rect = typing.layout(cx).region("body").expect("the area").rect;
    let line = typing.point_of(cx, body, pos(0, 0));
    let edge = typing.at(cx, rect.x, rect.y);
    // Beside the first line, two pixels outside the left line.
    let at = gpui_kit::point(edge.x - gpui_kit::px(2.), line.y);
    typing.click(cx, at);
    assert_eq!(selection(&typing, cx), Some(Selection { slide: 0, things: vec![Thing::Region("body".into())] }));
    assert!(typing.editing(cx).is_none());
}

/// A box with a thing inside it, and a thing beside it.
const NESTED: &str = "---\ntitle: T\n---\n\n# Nested\n\n\
<Box x=\"1000\" y=\"300\" w=\"800\" h=\"500\" fill=\"#dbeafe\">\n\n<Icon name=\"arrow-right\" size=\"96\" />\n\nSome words\n\n</Box>\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1300\" y=\"850\" w=\"200\" h=\"100\" />\n";

#[gpui_kit::test]
fn a_box_and_what_is_in_it_are_deleted_and_copied_once(cx: &mut TestAppContext) {
    let typing = open(cx, NESTED);
    let (outer, inner, shape) = (of(&typing, cx, "<Box"), of(&typing, cx, "<Icon"), of(&typing, cx, RECT));
    let document = typing.document(cx);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            for thing in [&inner, &outer, &shape] {
                assert!(document.select_also(0, Thing::Block(thing.clone())));
            }
            cx.notify();
        })
    });
    assert_eq!(blocks(&typing, cx), vec![inner, outer.clone(), shape.clone()]);
    let copied = document.read_with(cx, |document, _| document.copy_selected()).expect("copied").0;
    assert_eq!(copied, format!("{}\n\n{}", &NESTED[outer.clone()], &NESTED[shape.clone()]), "the box once, with what's in it");
    typing.press(cx, "delete");
    let left = typing.source(cx);
    assert!(!left.contains("<Box") && !left.contains("<Icon") && !left.contains(RECT), "{left}");
    assert!(left.contains("# Nested"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), NESTED, "one step");
}

#[gpui_kit::test]
fn two_things_on_one_line_are_taken_together(cx: &mut TestAppContext) {
    let talk = "---\ntitle: T\n---\n\n# Line\n\n\
<Icon name=\"arrow-right\" x=\"1000\" y=\"300\" size=\"96\" /> <Icon name=\"star\" x=\"1300\" y=\"300\" size=\"96\" />\n";
    let typing = open(cx, talk);
    let icons: Vec<Range<usize>> = typing.layout(cx).elements().iter().map(|element| element.source.clone()).collect();
    assert_eq!(icons.len(), 2, "two things");
    let document = typing.document(cx);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            assert!(document.select_also(0, Thing::Block(icons[0].clone())));
            assert!(document.select_also(0, Thing::Block(icons[1].clone())));
            cx.notify();
        })
    });
    typing.settle(cx);
    let copied = document.read_with(cx, |document, _| document.copy_selected()).expect("copied").0;
    assert_eq!(copied, format!("{}\n\n{}", &talk[icons[0].clone()], &talk[icons[1].clone()]));
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), "---\ntitle: T\n---\n\n# Line\n", "both go, with their line");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), talk, "one step");
}

#[gpui_kit::test]
fn several_things_are_followed_through_edits(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let (rect, ellipse) = (of(&typing, cx, RECT), of(&typing, cx, ELLIPSE));
    select_things(&typing, cx, &[ELLIPSE, RECT]);
    let document = typing.document(cx);
    // From outside, before them both, while nothing is unsaved.
    let text = TALK.replacen("# Shapes", "# Shapes here", 1);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            document.reload(Some(text));
            cx.notify();
        })
    });
    typing.settle(cx);
    let h = " here".len();
    let (rect, ellipse) = (rect.start + h..rect.end + h, ellipse.start + h..ellipse.end + h);
    assert_eq!(blocks(&typing, cx), vec![ellipse.clone(), rect.clone()]);
    let edit = |cx: &mut TestAppContext, at: usize, text: &str| {
        let change = slides_format::Change::single(slides_format::SourceEdit { range: at..at, text: text.into() });
        cx.update(|cx| {
            document.update(cx, |document, cx| {
                document.edit("Edit", change, 0).expect("made");
                cx.notify();
            })
        });
        typing.settle(cx);
    };
    // Inside the rectangle's tag: it grows, and the ellipse after it moves along.
    let attr = " stroke=\"red\"";
    edit(cx, rect.start + "<Shape".len(), attr);
    let n = attr.len();
    let (rect, ellipse) = (rect.start..rect.end + n, ellipse.start + n..ellipse.end + n);
    assert_eq!(blocks(&typing, cx), vec![ellipse.clone(), rect.clone()]);
    // Before them both.
    let words = typing.source(cx).find("Some words").unwrap();
    edit(cx, words, "More ");
    let m = "More ".len();
    assert_eq!(blocks(&typing, cx), vec![ellipse.start + m..ellipse.end + m, rect.start + m..rect.end + m]);
    assert!(bounds_of(&typing, cx, selection_outline_id(0)).is_some() && outline(&typing, cx).is_some());
}

#[gpui_kit::test]
fn every_thing_at_once_writes_what_one_at_a_time_would(cx: &mut TestAppContext) {
    for talk in [TALK.to_string(), TALK.replace('\n', "\r\n"), extremes()] {
        let crlf = talk.contains("\r\n");
        // Every thing at once, by Ctrl+A.
        let all = open(cx, &talk);
        let first = all.layout(cx).elements()[0].clone();
        let at = all.at(cx, first.rect.x + 0.5, first.rect.y + 0.5);
        all.click(cx, at);
        all.press(cx, "ctrl-a");
        all.press(cx, "delete");
        // One at a time, the last written first.
        let each = open(cx, &talk);
        let document = each.document(cx);
        loop {
            let last = each.layout(cx).elements().iter().map(|element| element.source.clone()).max_by_key(|range| range.start);
            let Some(last) = last else { break };
            cx.update(|cx| {
                document.update(cx, |document, cx| {
                    assert!(document.select(0, last));
                    document.delete_selected().expect("taken");
                    cx.notify();
                })
            });
        }
        assert_eq!(all.source(cx), each.source(cx), "crlf {crlf}: deleting");
        // And copies, by Duplicate: each written after the slide's last block, a blank line
        // before it, as writing them one at a time does. (Each is moved aside from its own
        // thing, so the copies keep how the things lay together.)
        let all = open(cx, &talk);
        let at = all.at(cx, first.rect.x + 0.5, first.rect.y + 0.5);
        all.click(cx, at);
        all.press(cx, "ctrl-a");
        let last_end = all.layout(cx).elements().iter().map(|element| element.source.end).max().unwrap();
        all.press(cx, "ctrl-d");
        let source = all.source(cx);
        let mut copies = blocks(&all, cx);
        copies.sort_by_key(|range| range.start);
        let eol = if crlf { "\r\n" } else { "\n" };
        let written: Vec<&str> = copies.iter().map(|range| &source[range.clone()]).collect();
        let expected = format!("{}{eol}{eol}{}{}", &talk[..last_end], written.join(&format!("{eol}{eol}")), &talk[last_end..]);
        assert_eq!(source, expected, "crlf {crlf}: duplicating");
    }
}

#[gpui_kit::test]
fn copies_of_a_chain_keep_how_the_things_lay(cx: &mut TestAppContext) {
    // B sits where a copy of A moved one step would: both copies go two steps, together.
    let talk = "---\ntitle: T\n---\n\n# Chain\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1100\" y=\"300\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1140\" y=\"340\" w=\"200\" h=\"200\" />\n";
    for paste in [false, true] {
        let typing = open(cx, talk);
        let document = typing.document(cx);
        let (a, b) = (of(&typing, cx, RECT), of(&typing, cx, ELLIPSE));
        cx.update(|cx| {
            document.update(cx, |document, cx| {
                assert!(document.select_also(0, Thing::Block(a.clone())));
                assert!(document.select_also(0, Thing::Block(b.clone())));
                cx.notify();
            })
        });
        typing.settle(cx);
        if paste {
            typing.press(cx, "ctrl-c");
            typing.press(cx, "ctrl-v");
        } else {
            typing.press(cx, "ctrl-d");
        }
        let source = typing.source(cx);
        let mut copies = blocks(&typing, cx);
        copies.sort_by_key(|range| range.start);
        let written: Vec<&str> = copies.iter().map(|range| &source[range.clone()]).collect();
        assert_eq!(
            written,
            [
                "<Shape kind=\"rect\" fill=\"#2563eb\" x=\"1180\" y=\"380\" w=\"200\" h=\"200\" />",
                "<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"1220\" y=\"420\" w=\"200\" h=\"200\" />",
            ],
            "paste {paste}: both two steps along, still 40 apart"
        );
    }
}
