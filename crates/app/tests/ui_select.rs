//! Picking a thing on the slide with a click, the outline that shows it, and taking it away.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{Bounds, Modifiers, Pixels, Point, TestAppContext, px, size};
use slides_engine::{ComponentState, ElementBox, Fonts, Hit, Pixmap};
use slides_format::FlowPlace;
use stagepack_studio::builds::Compile;
use stagepack_studio::clipboard::{self, AppData, CopiedFile, MISSING_PICTURES, PastedFiles};
use stagepack_studio::recovery::{Recovery, hash_text};
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::clipboard_tools::{COPY_ID, CUT_ID};
use stagepack_studio::document::Document;
use stagepack_studio::insert_tools::InsertTextBox;
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::selection::Thing;
use stagepack_studio::slide_strip::strip_row_id;

const SHOWCASE: &str = include_str!("../../../fixtures/showcase/slides.md");
/// The showcase's slide of shapes, an icon, a picture and a table.
const THINGS: usize = 5;
/// The showcase's slide with a box of words and nothing else.
const BOX: usize = 4;
const ICON: &str = "<Icon name=\"arrow-right\" x=\"100\" y=\"840\" size=\"96\" />";
const CANNOT_REMOVE: &str = "That couldn't be taken away without changing what's around it.";

/// A point on the line under the first cell of the table `table`, in the window, which a click
/// on picks the table: the middle of the band a click there does, whatever the rounding.
fn border(typing: &Typing, cx: &mut TestAppContext, table: &ElementBox) -> Point<Pixels> {
    let layout = typing.layout(cx);
    let rect = layout.cell_rect(&table.source, 0, 0).expect("the table's first cell");
    let x = rect.x + rect.w / 2.;
    let band: Vec<f32> = (0..120)
        .map(|step| rect.y + rect.h - 6. + step as f32 * 0.1)
        .filter(|&y| matches!(layout.hit(x, y), Some(Hit::Element(element)) if element.source == table.source))
        .collect();
    assert!(!band.is_empty(), "the line under the cell picks the table");
    typing.at(cx, x, band[band.len() / 2])
}

/// The window's box around `element` as it's drawn, turned.
fn drawn_box(typing: &Typing, cx: &TestAppContext, element: &ElementBox) -> Bounds<Pixels> {
    let rect = element.rect;
    let centre = (rect.x + rect.w / 2., rect.y + rect.h / 2.);
    let (sin, cos) = element.rotate.to_radians().sin_cos();
    let corners = [(rect.x, rect.y), (rect.x + rect.w, rect.y), (rect.x + rect.w, rect.y + rect.h), (rect.x, rect.y + rect.h)]
        .map(|(x, y)| {
            let (dx, dy) = (x - centre.0, y - centre.1);
            typing.at(cx, centre.0 + dx * cos - dy * sin, centre.1 + dx * sin + dy * cos)
        });
    let left = corners.iter().map(|at| at.x).fold(px(f32::MAX), Pixels::min);
    let right = corners.iter().map(|at| at.x).fold(px(f32::MIN), Pixels::max);
    let top = corners.iter().map(|at| at.y).fold(px(f32::MAX), Pixels::min);
    let bottom = corners.iter().map(|at| at.y).fold(px(f32::MIN), Pixels::max);
    Bounds { origin: gpui_kit::point(left, top), size: size(right - left, bottom - top) }
}

fn within_a_pixel(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
    let near = |a: Pixels, b: Pixels| (a - b).abs() <= px(1.);
    near(a.left(), b.left()) && near(a.right(), b.right()) && near(a.top(), b.top()) && near(a.bottom(), b.bottom())
}

/// The flow of the current slide written in the tag whose source starts with `start`.
fn flow_in(typing: &Typing, cx: &mut TestAppContext, start: &str) -> usize {
    let source = typing.source(cx);
    let slide = typing.current(cx);
    typing
        .flows(cx, slide)
        .iter()
        .position(|flow| matches!(&flow.place, FlowPlace::Tag(range) if source[range.clone()].starts_with(start)))
        .expect("the tag's text is a flow")
}

#[gpui_kit::test]
fn a_click_selects_and_escape_lets_go(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    for (start, said) in [
        ("<Picture", "Picture selected"),
        ("<Shape kind=\"ellipse\"", "Shape selected"),
        ("<Shape kind=\"arrow\"", "Shape selected"),
        ("<Icon", "Icon selected"),
        ("| Layout", "Table selected"),
        ("<Box", "Text box selected"),
    ] {
        let element = thing(&typing, cx, start);
        // A click in a table's cell types in it: the table is picked by a line around its cells.
        let at = if start.starts_with('|') { border(&typing, cx, &element) } else { middle(&typing, cx, &element) };
        typing.click(cx, at);
        assert_eq!(selected(&typing, cx), Some((THINGS, element.source.clone())), "{start}");
        assert_eq!(typing.status(cx).as_deref(), Some(said));
        assert!(typing.editing(cx).is_none(), "{start}: nothing is typed in");
        assert!(typing.caret(cx).is_none());
        assert!(outline(&typing, cx).is_some(), "{start}: the outline shows");
    }
    typing.press(cx, "escape");
    assert_eq!(selected(&typing, cx), None);
    assert!(outline(&typing, cx).is_none());
    assert_ne!(typing.status(cx).as_deref(), Some("Text box selected"));
    assert_eq!(typing.source(cx), SHOWCASE, "selecting writes nothing");
    let unsaved = typing.document(cx).read_with(cx, |document, _| document.has_unsaved_changes());
    assert!(!unsaved, "and leaves nothing unsaved");
    typing.press(cx, "pagedown");
    assert_eq!(typing.current(cx), THINGS + 1, "the keys are the slides' again");
}

#[gpui_kit::test]
fn a_positioned_text_box_and_a_chart_say_what_they_are(cx: &mut TestAppContext) {
    let source = "# T\n\n<Text x=\"100\" y=\"300\" w=\"600\" h=\"200\">\n\nHello\n\n</Text>\n\n<Chart kind=\"bar\" data=\"a,1\" x=\"900\" y=\"300\" w=\"600\" h=\"400\" />\n\n<StatCard x=\"100\" y=\"700\" w=\"300\" h=\"200\" />\n";
    let typing = open(cx, source);
    for (start, said) in [("<Text", "Text box selected"), ("<Chart", "Chart selected"), ("<StatCard", "Stat Card selected")] {
        let element = thing(&typing, cx, start);
        let at = middle(&typing, cx, &element);
        typing.click(cx, at);
        assert_eq!(selected(&typing, cx).map(|(_, range)| range), Some(element.source.clone()), "{start}");
        assert_eq!(typing.status(cx).as_deref(), Some(said));
    }
}

#[gpui_kit::test]
fn delete_takes_away_only_the_selected_thing(cx: &mut TestAppContext) {
    for key in ["delete", "backspace"] {
        let typing = open(cx, SHOWCASE);
        go_to(&typing, cx, THINGS);
        let icon = thing(&typing, cx, "<Icon");
        let at = middle(&typing, cx, &icon);
        typing.click(cx, at);
        typing.press(cx, key);
        assert_eq!(typing.source(cx), SHOWCASE.replacen(&format!("{ICON}\n\n"), "", 1), "{key}");
        assert_eq!(selected(&typing, cx), None);
        assert!(outline(&typing, cx).is_none());
        let (label, unsaved) = typing
            .document(cx)
            .read_with(cx, |document, _| (document.undo_label(), document.has_unsaved_changes()));
        assert_eq!(label, Some("Delete"));
        assert!(unsaved);
        assert_eq!(typing.slide_count(cx), 7);
        assert_eq!(typing.current(cx), THINGS, "{key} takes the thing, not the slide");
    }
}

#[gpui_kit::test]
fn undo_brings_it_back_selected(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let picture = thing(&typing, cx, "<Picture");
    let at = middle(&typing, cx, &picture);
    typing.click(cx, at);
    typing.press(cx, "delete");
    assert!(!typing.source(cx).contains("<Picture"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE);
    assert_eq!(selected(&typing, cx), Some((THINGS, picture.source.clone())));
    assert!(outline(&typing, cx).is_some());
    assert_eq!(typing.status(cx).as_deref(), Some("Picture selected"));
    typing.press(cx, "ctrl-shift-z");
    assert!(!typing.source(cx).contains("<Picture"));
    assert_eq!(selected(&typing, cx), None, "made again, there's nothing to select");
    typing.press(cx, "ctrl-z");
    assert_eq!(selected(&typing, cx), Some((THINGS, picture.source)));
    // Selected again, it's the keyboard's: it can be taken away again.
    typing.press(cx, "delete");
    assert!(!typing.source(cx).contains("<Picture"));
}

#[gpui_kit::test]
fn a_second_click_on_a_boxes_text_types_in_it(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, BOX);
    let flow = flow_in(&typing, cx, "<Box");
    let the_box = thing(&typing, cx, "<Box");
    let at = typing.point_of(cx, flow, pos(0, 4));
    typing.click(cx, at);
    assert_eq!(selected(&typing, cx), Some((BOX, the_box.source.clone())), "the first click selects the box");
    assert!(typing.editing(cx).is_none());
    typing.click(cx, at);
    let editing = typing.editing(cx).expect("the second types in it");
    assert_eq!(editing.edit.selection, caret_at(pos(0, 4)));
    assert_eq!(selected(&typing, cx), Some((BOX, the_box.source.clone())), "and the box stays selected");
    typing.type_text(cx, "X");
    assert_eq!(typing.source(cx), SHOWCASE.replacen("Just a", "JustX a", 1));
    let grown = the_box.source.start..the_box.source.end + 1;
    assert_eq!(selected(&typing, cx), Some((BOX, grown)), "the selection follows the box");
}

#[gpui_kit::test]
fn a_double_click_types_at_once(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, BOX);
    let flow = flow_in(&typing, cx, "<Box");
    let at = typing.point_of(cx, flow, pos(0, 6));
    typing.clicks(cx, at, 2, Modifiers::default());
    let editing = typing.editing(cx).expect("a double click types in the box");
    assert_eq!(editing.edit.selection, caret_at(pos(0, 6)), "with the caret where it was clicked");
    assert!(typing.caret(cx).is_some());
}

#[gpui_kit::test]
fn escape_from_typing_keeps_the_box_selected(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, BOX);
    let flow = flow_in(&typing, cx, "<Box");
    let the_box = thing(&typing, cx, "<Box");
    let at = typing.point_of(cx, flow, pos(0, 4));
    typing.clicks(cx, at, 2, Modifiers::default());
    typing.type_text(cx, "ab");
    typing.press(cx, "escape");
    assert!(typing.editing(cx).is_none());
    assert!(typing.caret(cx).is_none());
    let grown = the_box.source.start..the_box.source.end + 2;
    assert_eq!(selected(&typing, cx), Some((BOX, grown)));
    assert!(outline(&typing, cx).is_some());
    typing.press(cx, "escape");
    assert_eq!(selected(&typing, cx), None);
    assert!(outline(&typing, cx).is_none());
}

#[gpui_kit::test]
fn typing_elsewhere_drops_the_selection(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let picture = thing(&typing, cx, "<Picture");
    let at = middle(&typing, cx, &picture);
    typing.click(cx, at);
    assert!(selected(&typing, cx).is_some());
    // The title is the region's text: the caret goes there at once.
    let title = typing.point_of(cx, 0, pos(0, 3));
    typing.click(cx, title);
    assert!(typing.editing(cx).is_some(), "the title is typed in");
    assert_eq!(selected(&typing, cx), None);
    assert!(outline(&typing, cx).is_none());
    // A click on nothing lets go too.
    typing.click(cx, at);
    assert!(selected(&typing, cx).is_some());
    let nothing = typing.at(cx, 1900., 1060.);
    typing.click(cx, nothing);
    assert_eq!(selected(&typing, cx), None);
    // As does one on the desk.
    typing.click(cx, at);
    let slide = typing.slide_bounds(cx);
    typing.click(cx, slide.origin - gpui_kit::point(px(4.), px(4.)));
    assert_eq!(selected(&typing, cx), None);
}

#[gpui_kit::test]
fn tab_goes_to_the_next_thing(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let things = ["<Box", "<Shape kind=\"ellipse\"", "<Shape kind=\"arrow\"", "<Icon", "<Picture", "| Layout"]
        .map(|start| Thing::Block(thing(&typing, cx, start).source));
    // After the things come the layout's areas.
    let areas: Vec<Thing> = typing.layout(cx).regions().iter().map(|region| Thing::Region(region.name.clone())).collect();
    let in_order: Vec<Thing> = things.iter().cloned().chain(areas).collect();
    let first = thing(&typing, cx, "<Box");
    let at = middle(&typing, cx, &first);
    typing.click(cx, at);
    let picked = |typing: &Typing, cx: &TestAppContext| typing.document(cx).read_with(cx, |document, _| document.selection().map(|selection| selection.things.clone()));
    for next in in_order.iter().skip(1).chain([&in_order[0]]) {
        typing.press(cx, "tab");
        assert_eq!(picked(&typing, cx), Some(vec![next.clone()]));
    }
    typing.press(cx, "shift-tab");
    assert_eq!(picked(&typing, cx), Some(vec![in_order.last().unwrap().clone()]), "shift-tab goes back round");
    for _ in 0..in_order.len() {
        if picked(&typing, cx) == Some(vec![things[4].clone()]) {
            break;
        }
        typing.press(cx, "shift-tab");
    }
    assert_eq!(picked(&typing, cx), Some(vec![things[4].clone()]), "Shift+Tab reaches the picture");
    assert_eq!(typing.status(cx).as_deref(), Some("Picture selected"));
    typing.press(cx, "shift-tab");
    assert_eq!(picked(&typing, cx), Some(vec![things[3].clone()]));
    assert_eq!(typing.source(cx), SHOWCASE);
}

#[gpui_kit::test]
fn deleting_a_nested_element_leaves_its_parent(cx: &mut TestAppContext) {
    let icon = "<Icon name=\"arrow-right\" size=\"96\" />";
    let source = format!("# T\n\n<Box x=\"100\" y=\"300\" w=\"800\" h=\"500\" fill=\"#dbeafe\">\n\n{icon}\n\nSome words\n\n</Box>\n");
    let typing = open(cx, &source);
    let element = thing(&typing, cx, "<Icon");
    let at = middle(&typing, cx, &element);
    typing.click(cx, at);
    assert_eq!(typing.status(cx).as_deref(), Some("Icon selected"));
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), source.replacen(&format!("{icon}\n\n"), "", 1));
    assert!(typing.source(cx).contains("<Box"), "the box is still there");
}

#[gpui_kit::test]
fn a_thing_that_cannot_be_removed_says_so(cx: &mut TestAppContext) {
    let source = "# T\n\n- a\n- b\n\n<Icon name=\"arrow-right\" size=\"96\" />\n\n- c\n";
    let typing = open(cx, source);
    let element = thing(&typing, cx, "<Icon");
    let at = middle(&typing, cx, &element);
    typing.click(cx, at);
    assert!(selected(&typing, cx).is_some());
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), source);
    assert_eq!(typing.status(cx).as_deref(), Some(CANNOT_REMOVE));
    assert_eq!(selected(&typing, cx).map(|(_, range)| range), Some(element.source), "it stays selected");
    let can_undo = typing.document(cx).read_with(cx, |document, _| document.can_undo());
    assert!(!can_undo, "nothing to undo");
}

#[gpui_kit::test]
fn an_outside_change_that_removes_the_thing_drops_the_selection(cx: &mut TestAppContext) {
    let icon = "<Icon name=\"arrow-right\" x=\"100\" y=\"600\" size=\"96\" />";
    let source = format!("# T\n\nwords\n\n{icon}\n");
    let typing = open(cx, &source);
    let element = thing(&typing, cx, "<Icon");
    let at = middle(&typing, cx, &element);
    typing.click(cx, at);
    let reload = |cx: &mut TestAppContext, text: String| {
        let document = typing.document(cx);
        cx.update(|cx| {
            document.update(cx, |document, cx| {
                document.reload(Some(text));
                cx.notify();
            })
        });
        typing.settle(cx);
    };
    // A change before it moves it along.
    reload(cx, format!("# T\n\nmore words\n\n{icon}\n"));
    let moved = element.source.start + 5..element.source.end + 5;
    assert_eq!(selected(&typing, cx), Some((0, moved)));
    assert!(outline(&typing, cx).is_some());
    reload(cx, "# T\n\nmore words\n".to_string());
    assert_eq!(selected(&typing, cx), None);
    assert!(outline(&typing, cx).is_none());
}

#[gpui_kit::test]
fn the_outline_is_where_the_thing_is_drawn(cx: &mut TestAppContext) {
    let source = "# T\n\n<Box x=\"600\" y=\"300\" w=\"600\" h=\"300\" rotate=\"30\" fill=\"#dbeafe\">\n\nTurned\n\n</Box>\n\n<Shape kind=\"rect\" x=\"100\" y=\"700\" w=\"300\" h=\"200\" fill=\"orange\" />\n";
    let typing = open(cx, source);
    for (width, height) in [(1280., 800.), (900., 700.)] {
        cx.simulate_window_resize(typing.handle, size(px(width), px(height)));
        typing.settle(cx);
        for start in ["<Box", "<Shape"] {
            let element = thing(&typing, cx, start);
            let at = middle(&typing, cx, &element);
            typing.press(cx, "escape");
            typing.click(cx, at);
            assert_eq!(selected(&typing, cx).map(|(_, range)| range), Some(element.source.clone()));
            let drawn = outline(&typing, cx).expect("the outline shows");
            let expected = drawn_box(&typing, cx, &element);
            assert!(within_a_pixel(drawn, expected), "{start} at {width}: {drawn:?} for {expected:?}");
        }
    }
}

#[gpui_kit::test]
fn delete_while_typing_deletes_a_letter_not_the_box(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, BOX);
    let flow = flow_in(&typing, cx, "<Box");
    let at = typing.point_of(cx, flow, pos(0, 4));
    typing.click(cx, at);
    typing.click(cx, at);
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), SHOWCASE.replacen("Just a", "Justa", 1));
    typing.press(cx, "backspace");
    assert_eq!(typing.source(cx), SHOWCASE.replacen("Just a", "Jusa", 1));
    typing.press(cx, "tab");
    assert!(typing.editing(cx).is_some(), "tab is the text's too");
    assert!(selected(&typing, cx).is_some());
}

#[gpui_kit::test]
fn a_selected_thing_survives_typing_on_another_slide_and_coming_back(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let picture = thing(&typing, cx, "<Picture");
    let at = middle(&typing, cx, &picture);
    typing.click(cx, at);
    for _ in 0..THINGS - 1 {
        typing.press(cx, "pageup");
    }
    assert_eq!(typing.current(cx), 1);
    assert!(outline(&typing, cx).is_none(), "it's not shown on another slide");
    let title = typing.point_of(cx, 0, pos(0, 0));
    typing.click(cx, title);
    typing.type_text(cx, "Z");
    typing.press(cx, "escape");
    assert!(typing.source(cx).contains("# ZThe content layout"));
    for _ in 0..THINGS - 1 {
        typing.press(cx, "pagedown");
    }
    assert_eq!(typing.current(cx), THINGS);
    let moved = picture.source.start + 1..picture.source.end + 1;
    assert_eq!(selected(&typing, cx), Some((THINGS, moved)));
    let drawn = outline(&typing, cx).expect("the outline is back");
    assert!(within_a_pixel(drawn, drawn_box(&typing, cx, &picture)));
    assert_eq!(typing.status(cx).as_deref(), Some("Picture selected"));
    typing.press(cx, "delete");
    assert!(!typing.source(cx).contains("<Picture"));
    assert!(typing.source(cx).contains("# ZThe content layout"));
}

#[gpui_kit::test]
fn nothing_redraws_while_a_thing_sits_selected(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let picture = thing(&typing, cx, "<Picture");
    let at = middle(&typing, cx, &picture);
    typing.click(cx, at);
    assert!(selected(&typing, cx).is_some());
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
fn typing_given_back_by_undo_or_redo_drops_the_selection(cx: &mut TestAppContext) {
    for redo in [false, true] {
        let typing = open(cx, SHOWCASE);
        go_to(&typing, cx, THINGS);
        let title = typing.point_of(cx, 0, pos(0, 3));
        typing.click(cx, title);
        typing.type_text(cx, "Z");
        assert!(typing.source(cx).contains("# ShaZpes"));
        typing.press(cx, "escape");
        if redo {
            typing.press(cx, "ctrl-z");
            typing.press(cx, "escape");
        }
        let picture = thing(&typing, cx, "<Picture");
        let at = middle(&typing, cx, &picture);
        typing.click(cx, at);
        assert!(selected(&typing, cx).is_some());
        typing.press(cx, if redo { "ctrl-shift-z" } else { "ctrl-z" });
        assert!(typing.editing(cx).is_some(), "redo {redo}: typing goes on in the title");
        assert_eq!(selected(&typing, cx), None, "redo {redo}: and the picture is let go of");
        assert!(outline(&typing, cx).is_none());
        typing.press(cx, "delete");
        assert!(typing.source(cx).contains("<Picture"), "redo {redo}: delete is the text's");
    }
}

// Copying things, and what a paste brings with it.

const SCORE_SOURCE: &str = include_str!("../../components/tests/fixtures/v1/score.rs");
const SCORE_BUILD: &[u8] = include_bytes!("../../components/tests/fixtures/v1/score.wasm");
const PICTURE: &str = "<Picture src=\"assets/photo.png\" x=\"300\" y=\"300\" w=\"400\" h=\"300\" />";
const SCORE: &str = "<Score team=\"Reds\" points=\"3\" x=\"300\" y=\"300\" w=\"600\" h=\"300\" />";

fn on_clipboard(cx: &mut TestAppContext) -> (Option<String>, Option<String>) {
    cx.read_from_clipboard().map(|item| (item.text(), item.metadata().cloned())).unwrap_or_default()
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

/// Clicks row `row` of the list of slides, which takes the keyboard.
fn click_row(typing: &Typing, cx: &mut TestAppContext, row: usize) {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(strip_row_id(row), cx);
    });
    typing.settle(cx);
}

/// Selects the thing on the current slide whose source starts with `start`.
fn pick(typing: &Typing, cx: &mut TestAppContext, start: &str) -> ElementBox {
    let element = thing(typing, cx, start);
    let at = if start.starts_with('|') { border(typing, cx, &element) } else { middle(typing, cx, &element) };
    typing.click(cx, at);
    assert_eq!(selected(typing, cx).map(|(_, range)| range), Some(element.source.clone()), "{start} is selected");
    element
}

/// The text of the thing selected.
fn selected_text(typing: &Typing, cx: &TestAppContext) -> Option<String> {
    let (_, range) = selected(typing, cx)?;
    Some(typing.source(cx)[range].to_string())
}

/// Saves the talk and waits for it to be written.
fn save(typing: &Typing, cx: &mut TestAppContext) {
    typing.press(cx, "ctrl-s");
    cx.run_until_parked();
    typing.settle(cx);
}

/// A folder holding a talk of one slide holding `thing`, and the picture its fixture has.
fn talk_with(thing: &str) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), format!("# From\n\n{thing}\n")).unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/photo.png"), photo()).unwrap();
    folder
}

fn photo() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/showcase/assets/photo.png")).unwrap()
}

/// A folder holding a talk whose slide shows Score, with its source, and its build as the index
/// says it was built from `built_from`.
fn score_talk(built_from: &str) -> tempfile::TempDir {
    let folder = talk_with(SCORE);
    for sub in ["components", "build"] {
        std::fs::create_dir(folder.path().join(sub)).unwrap();
    }
    std::fs::write(folder.path().join("components/score.rs"), SCORE_SOURCE).unwrap();
    std::fs::write(folder.path().join("build/score.wasm"), SCORE_BUILD).unwrap();
    let index = format!(r#"{{"format":1,"components":[{{"tag":"Score","file":"score","source_hash":"{built_from}"}}]}}"#);
    std::fs::write(folder.path().join("build/index.json"), index).unwrap();
    folder
}

/// Opens the talk in `folder` with a compiler that counts its calls, and build permissions kept
/// in `permissions`.
fn open_counting(cx: &mut TestAppContext, folder: tempfile::TempDir, permissions: &std::path::Path) -> (Typing, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let compile: Compile = Arc::new(move |_: &str, _: &str| {
        counted.fetch_add(1, Ordering::SeqCst);
        Err(Vec::new())
    });
    let typing = open_folder(cx, folder, None);
    let file = permissions.to_path_buf();
    cx.update(|cx| {
        typing.view.update(cx, |view, _| {
            view.set_compile_for_test(compile);
            view.set_permissions_for_test(BuildPermissions::load(file));
        })
    });
    (typing, calls)
}

fn state_of(typing: &Typing, cx: &TestAppContext, tag: &str) -> Option<ComponentState> {
    typing.document(cx).read_with(cx, |document, _| document.components().iter().find(|info| info.tag == tag).map(|info| info.state))
}

#[gpui_kit::test]
fn copy_and_paste_a_shape_gives_a_second_one_offset_and_selected(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let shape = "<Shape kind=\"ellipse\" x=\"700\" y=\"560\" w=\"240\" h=\"200\" fill=\"orange\" outline=\"black\" outline-width=\"4\" />";
    pick(&typing, cx, "<Shape kind=\"ellipse\"");
    typing.press(cx, "ctrl-c");
    assert_eq!(typing.source(cx), SHOWCASE, "copying changes nothing");
    let (plain, metadata) = on_clipboard(cx);
    assert_eq!(plain.as_deref(), Some(shape), "other programs get its Markdown");
    assert!(metadata.is_some_and(|metadata| metadata.contains("\"element\"")));
    typing.press(cx, "ctrl-v");
    let moved = shape.replace("x=\"700\" y=\"560\"", "x=\"740\" y=\"600\"");
    assert_eq!(typing.source(cx).matches("<Shape kind=\"ellipse\"").count(), 2);
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(moved.as_str()), "the copy is selected, 40 down and right");
    assert_eq!(undo_label(&typing, cx), Some("Paste"));
    assert_eq!(typing.current(cx), THINGS);
    // Again, and it steps aside from both.
    typing.press(cx, "ctrl-v");
    let again = shape.replace("x=\"700\" y=\"560\"", "x=\"780\" y=\"640\"");
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(again.as_str()));
    typing.press(cx, "ctrl-z");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE, "each paste is one step");
}

#[gpui_kit::test]
fn cut_then_undo_brings_it_back(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    pick(&typing, cx, "<Icon");
    typing.press(cx, "ctrl-x");
    assert_eq!(typing.source(cx), SHOWCASE.replacen(&format!("{ICON}\n\n"), "", 1));
    assert_eq!(undo_label(&typing, cx), Some("Cut"));
    assert_eq!(typing.slide_count(cx), 7, "the thing, not the slide");
    assert_eq!(on_clipboard(cx).0.as_deref(), Some(ICON));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE);
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(ICON), "selected again");
    // And it can be pasted back.
    typing.press(cx, "ctrl-v");
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(ICON.replace("x=\"100\" y=\"840\"", "x=\"140\" y=\"880\"").as_str()));
}

#[gpui_kit::test]
fn duplicate_leaves_the_clipboard_alone(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("keep me".to_string()));
    pick(&typing, cx, "<Icon");
    typing.press(cx, "ctrl-d");
    assert_eq!(typing.slide_count(cx), 7, "the thing is duplicated, not the slide");
    assert_eq!(typing.source(cx).matches("<Icon").count(), SHOWCASE.matches("<Icon").count() + 1);
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(ICON.replace("x=\"100\" y=\"840\"", "x=\"140\" y=\"880\"").as_str()));
    assert_eq!(on_clipboard(cx), (Some("keep me".to_string()), None));
    assert_eq!(undo_label(&typing, cx), Some("Duplicate"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE, "one step");
}

#[gpui_kit::test]
fn a_picture_pasted_into_another_talk_brings_its_file(cx: &mut TestAppContext) {
    let from = open_folder(cx, talk_with(PICTURE), None);
    let into = open(cx, "# Into\n");
    pick(&from, cx, "<Picture");
    from.press(cx, "ctrl-c");
    click_row(&into, cx, 0);
    into.press(cx, "ctrl-v");
    assert_eq!(into.source(cx), format!("# Into\n\n{PICTURE}\n"));
    assert_eq!(selected_text(&into, cx).as_deref(), Some(PICTURE));
    let notices = into.document(cx).update(cx, |document, _| document.notices());
    assert!(notices.is_empty(), "the second talk draws it: {notices:?}");
    assert_ne!(into.status(cx).as_deref(), Some(MISSING_PICTURES));
    assert!(into.document(cx).read_with(cx, |document, _| document.has_unsaved_changes()));
    save(&into, cx);
    let folder = into.file.parent().unwrap().to_path_buf();
    assert_eq!(std::fs::read(folder.join("assets/photo.png")).unwrap(), photo(), "saved, its folder holds the file");
}

#[gpui_kit::test]
fn a_picture_whose_name_is_taken_is_renamed_in_the_pasted_tag(cx: &mut TestAppContext) {
    let from = open_folder(cx, talk_with(PICTURE), None);
    let taken = tempfile::tempdir().unwrap();
    std::fs::write(taken.path().join("slides.md"), "# Into\n").unwrap();
    std::fs::create_dir(taken.path().join("assets")).unwrap();
    let theirs = Pixmap::new(2, 2).unwrap().encode_png().unwrap();
    std::fs::write(taken.path().join("assets/photo.png"), &theirs).unwrap();
    let into = open_folder(cx, taken, None);
    pick(&from, cx, "<Picture");
    from.press(cx, "ctrl-c");
    click_row(&into, cx, 0);
    into.press(cx, "ctrl-v");
    assert_eq!(into.source(cx), format!("# Into\n\n{}\n", PICTURE.replace("assets/photo.png", "assets/photo-2.png")));
    save(&into, cx);
    let folder = into.file.parent().unwrap().to_path_buf();
    assert_eq!(std::fs::read(folder.join("assets/photo.png")).unwrap(), theirs, "the talk's own is left as it was");
    assert_eq!(std::fs::read(folder.join("assets/photo-2.png")).unwrap(), photo());
}

#[gpui_kit::test]
fn a_slide_pasted_into_another_talk_brings_its_pictures(cx: &mut TestAppContext) {
    let from = open_folder(cx, talk_with(PICTURE), None);
    let into = open(cx, "# Into\n");
    click_row(&from, cx, 0);
    from.press(cx, "ctrl-c");
    click_row(&into, cx, 0);
    into.press(cx, "ctrl-v");
    assert_eq!(into.slide_count(cx), 2);
    assert!(into.source(cx).contains(PICTURE));
    assert!(into.document(cx).update(cx, |document, _| document.notices()).is_empty());
    assert_eq!(into.status(cx), None, "nothing is missing");
    save(&into, cx);
    assert_eq!(std::fs::read(into.file.parent().unwrap().join("assets/photo.png")).unwrap(), photo());
}

#[gpui_kit::test]
fn a_component_pasted_into_a_talk_without_it_brings_source_and_build_and_builds_nothing(cx: &mut TestAppContext) {
    let hash = slides_components::source_hash(SCORE_SOURCE.as_bytes());
    for (built_from, state) in [(hash.clone(), ComponentState::UpToDate), ("0".repeat(64), ComponentState::OutOfDate)] {
        let from = open_folder(cx, score_talk(&built_from), None);
        assert_eq!(state_of(&from, cx, "Score"), Some(state));
        let app_data = tempfile::tempdir().unwrap();
        let permissions = app_data.path().join("permissions.json");
        let into_folder = tempfile::tempdir().unwrap();
        std::fs::write(into_folder.path().join("slides.md"), "# Into\n").unwrap();
        let (into, calls) = open_counting(cx, into_folder, &permissions);
        pick(&from, cx, "<Score");
        from.press(cx, "ctrl-c");
        click_row(&into, cx, 0);
        into.press(cx, "ctrl-v");
        assert_eq!(into.source(cx), format!("# Into\n\n{SCORE}\n"));
        assert_eq!(state_of(&into, cx, "Score"), Some(state), "paired as in the talk it came from");
        let notices = into.document(cx).update(cx, |document, _| document.notices());
        let drawn = if state == ComponentState::UpToDate { notices.is_empty() } else { notices.iter().all(|notice| notice.contains("last build")) };
        assert!(drawn, "it draws: {notices:?}");
        save(&into, cx);
        cx.executor().advance_clock(Duration::from_secs(5));
        cx.run_until_parked();
        let folder = into.file.parent().unwrap().to_path_buf();
        assert_eq!(std::fs::read_to_string(folder.join("components/score.rs")).unwrap(), SCORE_SOURCE);
        assert_eq!(std::fs::read(folder.join("build/score.wasm")).unwrap(), SCORE_BUILD);
        let index: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("build/index.json")).unwrap()).unwrap();
        assert_eq!(index["components"][0]["tag"], "Score");
        assert_eq!(index["components"][0]["source_hash"].as_str(), Some(built_from.as_str()));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing is built");
        assert!(!permissions.exists(), "the permissions are as they were");
        assert!(!BuildPermissions::load(permissions).always(&folder));
        // Opened again, it's as it was.
        let reopened = Document::open(&folder, Fonts::bundled_only()).unwrap();
        assert_eq!(reopened.components().iter().find(|info| info.tag == "Score").map(|info| info.state), Some(state));
    }
}

#[gpui_kit::test]
fn a_component_that_differs_is_not_replaced_and_says_so(cx: &mut TestAppContext) {
    let from = open_folder(cx, score_talk(&slides_components::source_hash(SCORE_SOURCE.as_bytes())), None);
    let theirs = format!("{SCORE_SOURCE}\n// This talk's own.\n");
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), "# Into\n").unwrap();
    std::fs::create_dir(folder.path().join("components")).unwrap();
    std::fs::write(folder.path().join("components/score.rs"), &theirs).unwrap();
    let into = open_folder(cx, folder, None);
    pick(&from, cx, "<Score");
    from.press(cx, "ctrl-c");
    click_row(&into, cx, 0);
    into.press(cx, "ctrl-v");
    assert_eq!(into.source(cx), format!("# Into\n\n{SCORE}\n"), "the thing is pasted");
    assert_eq!(
        into.status(cx).as_deref(),
        Some("This talk already has a different component called Score. The pasted one uses this talk's.")
    );
    assert!(into.document(cx).read_with(cx, |document, _| document.unsaved_files().is_empty()), "nothing is added");
    save(&into, cx);
    let saved = into.file.parent().unwrap().to_path_buf();
    assert_eq!(std::fs::read_to_string(saved.join("components/score.rs")).unwrap(), theirs);
    assert!(!saved.join("build").exists());
    // The same source is used as it is, and nothing's said.
    let same = tempfile::tempdir().unwrap();
    std::fs::write(same.path().join("slides.md"), "# Into\n").unwrap();
    std::fs::create_dir(same.path().join("components")).unwrap();
    std::fs::write(same.path().join("components/score.rs"), SCORE_SOURCE).unwrap();
    let into = open_folder(cx, same, None);
    click_row(&into, cx, 0);
    into.press(cx, "ctrl-v");
    assert!(into.source(cx).contains(SCORE));
    assert!(into.status(cx).is_none_or(|status| !status.contains("different component")));
    assert!(into.document(cx).read_with(cx, |document, _| document.unsaved_files().is_empty()));
}

#[gpui_kit::test]
fn an_element_pasted_while_typing_is_refused_in_words(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    pick(&typing, cx, "<Icon");
    typing.press(cx, "ctrl-c");
    typing.press(cx, "escape");
    for _ in 0..THINGS {
        typing.press(cx, "pageup");
    }
    assert_eq!(typing.current(cx), 0);
    typing.click_text(cx, 0, pos(0, 1));
    assert!(typing.editing(cx).is_some());
    typing.press(cx, "ctrl-v");
    assert_eq!(typing.source(cx), SHOWCASE, "nothing is pasted, nor typed in as Markdown");
    assert_eq!(typing.status(cx).as_deref(), Some("That can't be pasted into text."));
    assert!(typing.editing(cx).is_some(), "typing goes on");
    // A new text box waiting for its first letter is dropped, and the thing pasted.
    typing.press(cx, "escape");
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(InsertTextBox), cx));
    typing.settle(cx);
    assert!(typing.document(cx).read_with(cx, |document, _| document.new_box().is_some()));
    typing.press(cx, "ctrl-v");
    assert!(typing.document(cx).read_with(cx, |document, _| document.new_box().is_none()));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(ICON));
    assert_eq!(typing.source(cx).matches("<Icon").count(), SHOWCASE.matches("<Icon").count() + 1);
}

#[gpui_kit::test]
fn a_table_can_be_copied_and_pasted(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let table = pick(&typing, cx, "| Layout");
    let text = typing.source(cx)[table.source.clone()].to_string();
    typing.press(cx, "ctrl-c");
    assert_eq!(on_clipboard(cx).0.as_deref(), Some(text.as_str()));
    typing.press(cx, "ctrl-v");
    assert_eq!(typing.source(cx).matches(text.as_str()).count(), 2);
    let (slide, range) = selected(&typing, cx).expect("the copy is selected");
    assert_eq!(slide, THINGS);
    assert_ne!(range, table.source);
    assert_eq!(&typing.source(cx)[range], text);
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE);
}

#[gpui_kit::test]
fn the_home_tabs_copy_and_cut_act_on_the_thing_selected(cx: &mut TestAppContext) {
    let only = "# Only\n\n<Icon name=\"star\" x=\"100\" y=\"100\" size=\"96\" />\n";
    let typing = open(cx, only);
    pick(&typing, cx, "<Icon");
    let disabled = |typing: &Typing, cx: &mut TestAppContext, id: &'static str| {
        typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.find(id).disabled() == Some(true)
        })
    };
    assert!(!disabled(&typing, cx, CUT_ID), "the only slide's thing can be cut");
    typing.with_window(cx, |window, cx| window.click(COPY_ID, cx));
    typing.settle(cx);
    assert_eq!(on_clipboard(cx).0.as_deref(), Some("<Icon name=\"star\" x=\"100\" y=\"100\" size=\"96\" />"));
    typing.with_window(cx, |window, cx| window.click(CUT_ID, cx));
    typing.settle(cx);
    assert_eq!(typing.source(cx), "# Only\n");
    assert_eq!(undo_label(&typing, cx), Some("Cut"));
}

// What a paste brings, through every way of saving and through a crash.

/// A talk of one slide, `# Into`, in a folder, or packed if `packed`, and where it is.
fn into_talk(packed: bool) -> (tempfile::TempDir, std::path::PathBuf, Document) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("into");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), "# Into\n").unwrap();
    if !packed {
        let document = Document::open(&folder, Fonts::bundled_only()).unwrap();
        return (dir, folder, document);
    }
    let file = dir.path().join("into.sptx");
    slides_engine::pack_folder(&folder, &file, "0.1.0").unwrap();
    let document = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    (dir, file, document)
}

fn reopen(at: &std::path::Path) -> Document {
    if at.extension().is_some_and(|ext| ext == "sptx") {
        Document::open_packed(at, Fonts::bundled_only()).unwrap()
    } else {
        Document::open(at, Fonts::bundled_only()).unwrap()
    }
}

/// What copying the thing in `from` whose text starts with `start` puts on the clipboard, as a
/// paste reads it.
fn copied(from: &mut Document, start: &str) -> (String, PastedFiles) {
    let at = from.source().find(start).unwrap();
    let end = at + from.source()[at..].find("/>").unwrap() + 2;
    assert!(from.select(0, at..end), "{start} is selected");
    let (text, files) = from.copy_selected().unwrap();
    let entry = clipboard::element_entry(&text, &files).unwrap();
    match clipboard::read_app_data(entry.metadata.as_deref()) {
        AppData::Element(carried) => (carried.markdown, carried.files),
        _ => panic!("element data"),
    }
}

fn score_state_of(document: &Document) -> Option<ComponentState> {
    document.components().iter().find(|info| info.tag == "Score").map(|info| info.state)
}

/// Whether `document` shows Score up to date, drawn on its first slide.
fn draws_score(document: &mut Document) -> bool {
    document.go_to(0);
    score_state_of(document) == Some(ComponentState::UpToDate) && document.notices().is_empty()
}

fn up_to_date_score() -> Document {
    let folder = score_talk(&slides_components::source_hash(SCORE_SOURCE.as_bytes()));
    let document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    std::mem::forget(folder);
    document
}

fn save_document(document: &mut Document, target: Option<SaveTarget>) -> Vec<String> {
    let job = match target {
        Some(target) => document.save_as_job(target),
        None => document.save_job(),
    };
    let saved = job.run().unwrap_or_else(|error| panic!("{error}"));
    let notices = saved.notices().to_vec();
    document.saved(saved);
    notices
}

/// Pastes Score into a talk of one slide, packed if `packed`, saves it where `target` says,
/// "pack" or "folder", or in place, and opens it again: it's up to date and draws throughout.
fn pasted_score_survives(packed: bool, target: Option<&str>) {
    let mut from = up_to_date_score();
    let (markdown, files) = copied(&mut from, "<Score");
    let what = format!("packed {packed}, to {target:?}");
    let (dir, at, mut into) = into_talk(packed);
    into.paste_element(&markdown, files).unwrap();
    assert!(draws_score(&mut into), "{what}: right after the paste");
    let saved_at = match target {
        None => at.clone(),
        Some("pack") => dir.path().join("out.sptx"),
        Some(_) => dir.path().join("out"),
    };
    let target = target.map(|_| if saved_at.extension().is_some() { SaveTarget::Packed(saved_at.clone()) } else { SaveTarget::Folder(saved_at.clone()) });
    let notices = save_document(&mut into, target);
    assert!(notices.is_empty(), "{what}: {notices:?}");
    assert!(draws_score(&mut into), "{what}: once saved");
    // Saved again, the index still holds it.
    save_document(&mut into, None);
    let mut reopened = reopen(&saved_at);
    assert!(draws_score(&mut reopened), "{what}: opened again: {:?} {:?}", score_state_of(&reopened), reopened.notices());
}

#[test]
fn a_pasted_component_works_after_a_save_in_its_folder() {
    pasted_score_survives(false, None);
}

#[test]
fn a_pasted_component_works_after_a_save_in_its_pack() {
    pasted_score_survives(true, None);
}

#[test]
fn a_pasted_component_works_after_save_as_from_folder_to_pack() {
    pasted_score_survives(false, Some("pack"));
}

#[test]
fn a_pasted_component_works_after_save_as_from_pack_to_folder() {
    pasted_score_survives(true, Some("folder"));
}

#[test]
fn a_pasted_component_works_after_save_as_from_folder_to_folder() {
    pasted_score_survives(false, Some("folder"));
}

#[test]
fn a_pasted_component_works_after_save_as_from_pack_to_pack() {
    pasted_score_survives(true, Some("pack"));
}

#[test]
fn a_pasted_component_comes_back_from_a_crash_and_saves() {
    let mut from = up_to_date_score();
    let (markdown, files) = copied(&mut from, "<Score");
    let (_dir, folder, mut into) = into_talk(false);
    into.paste_element(&markdown, files).unwrap();
    let kept = tempfile::tempdir().unwrap();
    let recovery = Recovery::new(kept.path().to_path_buf());
    let place = into.place().clone();
    recovery
        .keep_with_builds(&place, &hash_text(into.stamp_on_disk()), into.source(), &into.added_files(), &into.builds_to_enter())
        .unwrap();
    drop(into);
    // The app stopped; the talk is opened again and its changes brought back.
    let mut back = Document::open(&folder, Fonts::bundled_only()).unwrap();
    let found = recovery.find(back.place()).expect("a working copy");
    back.recover_with_builds(found.text, found.files, found.builds).unwrap();
    assert!(draws_score(&mut back), "brought back as it was");
    save_document(&mut back, None);
    assert!(draws_score(&mut reopen(&folder)), "saved and opened again");

    // Builds kept that weren't a paste's, or name what isn't there, are let go.
    let (_dir, folder, mut into) = into_talk(false);
    into.paste_element(&markdown, copied(&mut from, "<Score").1).unwrap();
    let mut builds = into.builds_to_enter();
    builds[0].tag = "Chart".into();
    let forged = vec![builds[0].clone(), slides_components::BuildEntry { tag: "Score".into(), file: "../x".into(), source_hash: "a".repeat(64) }];
    let mut back = Document::open(&folder, Fonts::bundled_only()).unwrap();
    back.recover_with_builds(into.source().to_string(), into.added_files(), forged).unwrap();
    assert_eq!(score_state_of(&back), Some(ComponentState::NeedsBuilding));
}

#[test]
fn undoing_a_paste_saves_none_of_its_files_and_redoing_it_saves_them() {
    let mut from = up_to_date_score();
    let (markdown, files) = copied(&mut from, "<Score");
    let (_dir, folder, mut into) = into_talk(false);
    into.paste_element(&markdown, files).unwrap();
    into.undo().unwrap();
    save_document(&mut into, None);
    for gone in ["components/score.rs", "build/score.wasm", "build/index.json"] {
        assert!(!folder.join(gone).exists(), "{gone}");
    }
    into.redo().unwrap();
    save_document(&mut into, None);
    for there in ["components/score.rs", "build/score.wasm", "build/index.json"] {
        assert!(folder.join(there).exists(), "{there}");
    }
    assert!(draws_score(&mut reopen(&folder)));
}

/// Element data holding `markdown` and the files `files`, each `(path, bytes, built_from)`.
fn element_with(markdown: &str, files: &[(&str, &[u8], Option<String>)]) -> PastedFiles {
    let copied: Vec<CopiedFile> = files
        .iter()
        .map(|(path, bytes, built_from)| CopiedFile { path: path.to_string(), bytes: (*bytes).into(), built_from: built_from.clone() })
        .collect();
    let entry = clipboard::element_entry(markdown, &copied).unwrap();
    match clipboard::read_app_data(entry.metadata.as_deref()) {
        AppData::Element(carried) => carried.files,
        _ => panic!("element data"),
    }
}

#[test]
fn files_the_pasted_thing_does_not_use_are_not_added() {
    let hash = Some(slides_components::source_hash(SCORE_SOURCE.as_bytes()));
    let shape = "<Shape kind=\"rect\" x=\"10\" y=\"10\" w=\"100\" h=\"100\" />";
    let files = element_with(
        shape,
        &[("components/score.rs", SCORE_SOURCE.as_bytes(), None), ("build/score.wasm", SCORE_BUILD, hash), ("assets/photo.png", &photo(), None)],
    );
    assert_eq!(files.files.len(), 3);
    let (_dir, _folder, mut into) = into_talk(false);
    let (_, brought) = into.paste_element(shape, files).unwrap();
    assert_eq!(brought.said(), None);
    assert_eq!(score_state_of(&into), None, "no component is added");
    assert!(into.unsaved_files().is_empty(), "nothing is added, named or kept");
    assert!(into.builds_to_enter().is_empty());
    assert!(into.palette().iter().all(|entry| entry.tag != "Score"), "nor offered");
}

#[test]
fn a_pasted_component_with_a_built_in_name_is_not_added() {
    let chart = SCORE_SOURCE.replace("fn Score(", "fn Chart(");
    let markdown = "<Chart kind=\"bar\" data=\"a,1\" x=\"10\" y=\"10\" w=\"100\" h=\"100\" />";
    let files = element_with(markdown, &[("components/chart.rs", chart.as_bytes(), None)]);
    assert_eq!(files.files.len(), 1);
    let (_dir, _folder, mut into) = into_talk(false);
    let (_, brought) = into.paste_element(markdown, files).unwrap();
    assert_eq!(brought.said().as_deref(), Some("The pasted component has the name of a built-in one, so it wasn't added."));
    assert!(into.unsaved_files().is_empty());
    assert!(into.source().contains("<Chart"), "the thing is pasted, as the built-in one");
}

#[test]
fn a_build_that_came_without_its_source_is_said_as_a_component() {
    let files = element_with(SCORE, &[("build/score.wasm", SCORE_BUILD, Some("a".repeat(64)))]);
    let (_dir, _folder, mut into) = into_talk(false);
    let (_, brought) = into.paste_element(SCORE, files).unwrap();
    assert_eq!(brought.said().as_deref(), Some("A component on the pasted slide isn't in this talk."));
    assert!(into.unsaved_files().is_empty());
}

#[test]
fn a_paste_never_replaces_an_entry_in_the_talks_index() {
    // A build-only component, Other, whose build has the same bytes as the pasted Score's.
    let (_dir, folder, _) = into_talk(false);
    std::fs::create_dir(folder.join("build")).unwrap();
    std::fs::write(folder.join("build/score.wasm"), SCORE_BUILD).unwrap();
    let index = format!(r#"{{"format":1,"components":[{{"tag":"Other","file":"score","source_hash":"{}"}}]}}"#, "b".repeat(64));
    std::fs::write(folder.join("build/index.json"), &index).unwrap();
    let mut into = Document::open(&folder, Fonts::bundled_only()).unwrap();
    let mut from = up_to_date_score();
    let (markdown, files) = copied(&mut from, "<Score");
    into.paste_element(&markdown, files).unwrap();
    assert!(into.builds_to_enter().is_empty(), "nothing is entered");
    assert_eq!(score_state_of(&into), Some(ComponentState::NeedsBuilding), "it follows the rules there are");
    save_document(&mut into, None);
    assert_eq!(std::fs::read_to_string(folder.join("build/index.json")).unwrap(), index, "the talk's entry is kept");
}

#[test]
fn an_index_that_cannot_be_read_is_left_and_said_on_save() {
    let (_dir, folder, _) = into_talk(false);
    std::fs::create_dir(folder.join("build")).unwrap();
    std::fs::write(folder.join("build/index.json"), "not an index").unwrap();
    let mut into = Document::open(&folder, Fonts::bundled_only()).unwrap();
    let mut from = up_to_date_score();
    let (markdown, files) = copied(&mut from, "<Score");
    into.paste_element(&markdown, files).unwrap();
    let notices = save_document(&mut into, None);
    assert_eq!(notices, ["Couldn't save index.json: it can't be read, so a pasted component needs building."]);
    assert_eq!(std::fs::read_to_string(folder.join("build/index.json")).unwrap(), "not an index");
    assert!(std::fs::read_to_string(folder.join("slides.md")).unwrap().contains("<Score"), "the rest is saved");
}

#[test]
fn a_reference_that_was_not_pasted_never_comes_to_name_a_pasted_file() {
    let markdown = "<Picture src=\"assets/Photo.png\" data=\"assets/photo.png\" x=\"10\" y=\"10\" w=\"100\" h=\"100\" />";
    let files = element_with(markdown, &[("assets/Photo.png", &photo(), None)]);
    let (_dir, _folder, mut into) = into_talk(false);
    into.paste_element(markdown, files).unwrap();
    assert!(into.source().contains("src=\"assets/photo-2.png\" data=\"assets/photo.png\""), "{}", into.source());
    assert!(!into.has_file("assets/photo.png"));
}

// Fix round 2: a save adds to the index it finds, and never takes from it.

/// Score's source and build as another component, `name`, whose file is `name` in lower case.
fn component_named(name: &str) -> (String, PastedFiles) {
    let source = SCORE_SOURCE.replace("fn Score(", &format!("fn {name}("));
    let markdown = format!("<{name} team=\"A\" points=\"1\" x=\"10\" y=\"10\" w=\"100\" h=\"100\" />");
    let stem = name.to_lowercase();
    let hash = Some(slides_components::source_hash(source.as_bytes()));
    let files = element_with(
        &markdown,
        &[(&format!("components/{stem}.rs"), source.as_bytes(), None), (&format!("build/{stem}.wasm"), SCORE_BUILD, hash)],
    );
    (markdown, files)
}

fn state_in(document: &Document, tag: &str) -> Option<ComponentState> {
    document.components().iter().find(|info| info.tag == tag).map(|info| info.state)
}

/// Takes the thing whose text starts with `start` off the first slide.
fn delete_thing(document: &mut Document, start: &str) {
    let at = document.source().find(start).unwrap();
    let end = at + document.source()[at..].find("/>").unwrap() + 2;
    assert!(document.select(0, at..end));
    document.delete_selected().unwrap();
}

#[test]
fn a_pack_saved_in_place_keeps_an_entry_once_its_component_is_taken_off() {
    let (_dir, at, mut into) = into_talk(true);
    let (score, files) = component_named("Score");
    into.paste_element(&score, files).unwrap();
    save_document(&mut into, None);
    delete_thing(&mut into, "<Score");
    let (gauge, files) = component_named("Gauge");
    into.paste_element(&gauge, files).unwrap();
    save_document(&mut into, None);
    let reopened = reopen(&at);
    assert!(reopened.has_file("components/score.rs") && reopened.has_file("build/score.wasm"));
    assert_eq!(state_in(&reopened, "Score"), Some(ComponentState::UpToDate), "its entry is kept");
    assert_eq!(state_in(&reopened, "Gauge"), Some(ComponentState::UpToDate));
}

#[test]
fn a_folder_save_never_replaces_an_entry_that_came_after_the_paste() {
    let (_dir, folder, mut into) = into_talk(false);
    let (score, files) = component_named("Score");
    into.paste_element(&score, files).unwrap();
    // A build, or another copy of the app, enters Score meanwhile.
    std::fs::create_dir(folder.join("build")).unwrap();
    let theirs = format!(r#"{{"format":1,"components":[{{"tag":"Score","file":"score","source_hash":"{}"}}]}}"#, "c".repeat(64));
    std::fs::write(folder.join("build/index.json"), &theirs).unwrap();
    let notices = save_document(&mut into, None);
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(std::fs::read_to_string(folder.join("build/index.json")).unwrap(), theirs);
}

#[test]
fn what_the_pasted_thing_does_not_use_says_nothing() {
    let shape = "<Shape kind=\"rect\" x=\"10\" y=\"10\" w=\"100\" h=\"100\" />";
    for (what, files) in [
        ("a build alone", element_with(shape, &[("build/gauge.wasm", SCORE_BUILD, Some("a".repeat(64)))])),
        ("a picture that isn't one", element_with(shape, &[("assets/x.png", b"not a picture", None)])),
    ] {
        let (_dir, _folder, mut into) = into_talk(false);
        let (_, brought) = into.paste_element(shape, files).unwrap();
        assert_eq!(brought.said(), None, "{what}");
        assert!(into.notices().is_empty(), "{what}");
    }
}

/// What can be done to a talk, for the property below.
#[derive(Clone, Copy, Debug)]
enum Step {
    Paste(usize),
    Delete(usize),
    Undo,
    Redo,
    Save,
}

const NAMES: [&str; 3] = ["Alpha", "Beta", "Gamma"];

fn step() -> impl proptest::strategy::Strategy<Value = Step> {
    use proptest::prelude::*;
    prop_oneof![
        (0..3usize).prop_map(Step::Paste),
        (0..3usize).prop_map(Step::Delete),
        Just(Step::Undo),
        Just(Step::Redo),
        Just(Step::Save),
        Just(Step::Save),
    ]
}

/// The files of the talk saved at `at`, read fresh from where it's kept.
fn files_at(at: &std::path::Path) -> std::rc::Rc<dyn slides_engine::TalkFiles> {
    if at.extension().is_some_and(|ext| ext == "sptx") {
        std::rc::Rc::new(slides_engine::open_packed(at).unwrap())
    } else {
        std::rc::Rc::new(slides_engine::FolderFiles::new(at.to_path_buf()))
    }
}

/// Runs `steps` on a talk, packed if `packed`, and after each save reads it fresh from where it's
/// kept, pairing its builds as the engine does when it's opened: a component whose files are in
/// it and that was once saved with an entry still has one, and no entry is for files it hasn't
/// got.
fn saves_only_add_entries(packed: bool, steps: &[Step]) -> Result<(), proptest::test_runner::TestCaseError> {
    let pasted: Vec<(String, PastedFiles)> = NAMES.iter().map(|name| component_named(name)).collect();
    let (_dir, at, mut talk) = into_talk(packed);
    let mut entered: BTreeSet<&str> = BTreeSet::new();
    for step in steps {
        match *step {
            Step::Paste(which) => {
                talk.paste_element(&pasted[which].0, pasted[which].1.clone()).ok();
            }
            Step::Delete(which) => {
                let start = format!("<{}", NAMES[which]);
                if let Some(at) = talk.source().find(&start) {
                    let end = at + talk.source()[at..].find("/>").unwrap() + 2;
                    if talk.select(0, at..end) {
                        talk.delete_selected().ok();
                    }
                }
            }
            Step::Undo => {
                talk.undo().ok();
            }
            Step::Redo => {
                talk.redo().ok();
            }
            Step::Save => {
                save_document(&mut talk, None);
                let files = files_at(&at);
                let scanned = slides_engine::TalkComponents::scan(files.clone());
                for name in NAMES {
                    let stem = name.to_lowercase();
                    let has_files = files.len(&format!("components/{stem}.rs")).is_some() && files.len(&format!("build/{stem}.wasm")).is_some();
                    let state = scanned.infos().iter().find(|info| info.tag == name).map(|info| info.state);
                    if state == Some(ComponentState::UpToDate) {
                        entered.insert(name);
                    }
                    if has_files && entered.contains(name) {
                        proptest::prop_assert_eq!(state, Some(ComponentState::UpToDate), "{} after {:?}", name, steps);
                    }
                }
                let index: Option<serde_json::Value> = files.read("build/index.json").map(|bytes| serde_json::from_slice(&bytes).unwrap());
                for entry in index.iter().flat_map(|index| index["components"].as_array().cloned().unwrap_or_default()) {
                    let file = entry["file"].as_str().unwrap();
                    proptest::prop_assert!(files.len(&format!("build/{file}.wasm")).is_some(), "{} without its build after {:?}", file, steps);
                    proptest::prop_assert!(files.len(&format!("components/{file}.rs")).is_some(), "{} without its source after {:?}", file, steps);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn any_run_of_pastes_deletes_undos_and_saves_only_ever_adds_index_entries() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};
    // The run that once lost an entry, and runs with an undo or a redo across a save, then others.
    let known: [&[Step]; 3] = [
        &[Step::Paste(0), Step::Save, Step::Delete(0), Step::Paste(1), Step::Save],
        &[Step::Paste(0), Step::Delete(0), Step::Save, Step::Undo, Step::Save],
        &[Step::Paste(0), Step::Save, Step::Undo, Step::Save, Step::Redo, Step::Save],
    ];
    for packed in [false, true] {
        for steps in known {
            saves_only_add_entries(packed, steps).unwrap_or_else(|failure| panic!("packed {packed}: {failure}"));
        }
        let mut runner = TestRunner::new(Config { cases: 4, ..Config::default() });
        runner
            .run(&prop::collection::vec(step(), 1..8), |steps| saves_only_add_entries(packed, &steps))
            .unwrap_or_else(|failure| panic!("packed {packed}: {failure}"));
    }
}

#[test]
fn a_pack_saved_but_not_read_back_is_saved_and_keeps_its_entries() {
    // From review: once the pack was written, a failure to open it again failed the save, and
    // a later save started from the index before it and lost an entry.
    let (_dir, file, mut talk) = into_talk(true);
    for name in ["Alpha", "Beta"] {
        let (markdown, files) = component_named(name);
        talk.paste_element(&markdown, files).unwrap();
    }
    let saved = talk
        .save_job()
        .run_reading_back_with(|_| Err("it couldn't be read".to_string()))
        .unwrap_or_else(|error| panic!("the save was made: {error}"));
    assert_eq!(saved.notices(), [stagepack_studio::saving::NOT_READ_BACK]);
    talk.saved(saved);
    assert!(!talk.has_unsaved_changes(), "it's saved");
    for name in ["Alpha", "Beta"] {
        assert_eq!(state_in(&talk, name), Some(ComponentState::UpToDate), "{name} as it was");
    }
    // Beta is taken off the slide, Gamma pasted, and the talk saved again, read back this time:
    // the index written is the one in the pack with Gamma's entry added.
    let start = talk.source().find("<Beta").unwrap();
    let end = start + talk.source()[start..].find("/>").unwrap() + 2;
    assert!(talk.select(0, start..end));
    talk.delete_selected().unwrap();
    let (markdown, files) = component_named("Gamma");
    talk.paste_element(&markdown, files).unwrap();
    assert!(save_document(&mut talk, None).is_empty());
    let fresh = reopen(&file);
    assert_eq!(fresh.source(), talk.source());
    for name in ["Alpha", "Beta", "Gamma"] {
        assert_eq!(state_in(&fresh, name), Some(ComponentState::UpToDate), "{name}'s entry is kept");
    }
}
