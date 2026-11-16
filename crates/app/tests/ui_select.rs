//! Picking a thing on the slide with a click, the outline that shows it, and taking it away.

use std::ops::Range;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{Bounds, Modifiers, Pixels, Point, TestAppContext, px, size};
use slides_engine::{ElementBox, Hit};
use slides_format::FlowPlace;
use stagepack_studio::slide_editor::SELECTION_OUTLINE_ID;

const SHOWCASE: &str = include_str!("../../../fixtures/showcase/slides.md");
/// The showcase's slide of shapes, an icon, a picture and a table.
const THINGS: usize = 5;
/// The showcase's slide with a box of words and nothing else.
const BOX: usize = 4;
const ICON: &str = "<Icon name=\"arrow-right\" x=\"100\" y=\"840\" size=\"96\" />";
const CANNOT_REMOVE: &str = "That couldn't be taken away without changing what's around it.";

/// Goes on to slide `index` with the keys, from the first.
fn go_to(typing: &Typing, cx: &mut TestAppContext, index: usize) {
    for _ in 0..index {
        typing.press(cx, "pagedown");
    }
    assert_eq!(typing.current(cx), index);
}

/// The thing on the current slide whose source starts with `start`.
fn thing(typing: &Typing, cx: &mut TestAppContext, start: &str) -> ElementBox {
    let source = typing.source(cx);
    typing
        .layout(cx)
        .elements()
        .iter()
        .find(|element| source[element.source.clone()].starts_with(start))
        .cloned()
        .unwrap_or_else(|| panic!("no thing starts with {start:?}"))
}

/// The middle of `element` in the window.
fn middle(typing: &Typing, cx: &TestAppContext, element: &ElementBox) -> Point<Pixels> {
    let rect = element.rect;
    typing.at(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.)
}

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

fn selected(typing: &Typing, cx: &TestAppContext) -> Option<(usize, Range<usize>)> {
    typing.document(cx).read_with(cx, |document, _| {
        document
            .selected()
            .map(|selected| (selected.slide, selected.source.clone()))
    })
}

/// Where the selection's outline is drawn, if it is.
fn outline(typing: &Typing, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window
            .try_find(SELECTION_OUTLINE_ID)
            .map(|outline| outline.bounds())
    })
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
    let in_order = ["<Box", "<Shape kind=\"ellipse\"", "<Shape kind=\"arrow\"", "<Icon", "<Picture", "| Layout"]
        .map(|start| thing(&typing, cx, start).source);
    let first = thing(&typing, cx, "<Box");
    let at = middle(&typing, cx, &first);
    typing.click(cx, at);
    for next in in_order.iter().skip(1).chain([&in_order[0]]) {
        typing.press(cx, "tab");
        assert_eq!(selected(&typing, cx), Some((THINGS, next.clone())));
    }
    typing.press(cx, "shift-tab");
    assert_eq!(selected(&typing, cx), Some((THINGS, in_order[5].clone())), "shift-tab goes back round");
    typing.press(cx, "shift-tab");
    assert_eq!(selected(&typing, cx), Some((THINGS, in_order[4].clone())));
    assert_eq!(typing.status(cx).as_deref(), Some("Picture selected"));
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
