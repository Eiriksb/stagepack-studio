//! Aligning, spacing out and reordering the things selected, from the Home tab's Arrange menu
//! and from the menu a right-click on a thing opens; and hiding the form beside the slide.

use std::ops::Range;
use std::path::{Path, PathBuf};

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    InputEvent as _, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, TestAppContext, px, size,
};
use slides_engine::{Deck, FolderFiles, Fonts, NativeContent};
use stagepack_studio::arrange_tools::{ARRANGE_ID, ReorderThing};
use stagepack_studio::inspector::INSPECTOR_ID;
use stagepack_studio::ribbon::{TAB_VIEW_ID, TOGGLE_INSPECTOR_ID};
use stagepack_studio::selection::{Selection, Thing};
use stagepack_studio::slide_strip::strip_row_id;
use toolkit::ShapeKind;

const TALK: &str = "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n---\n\n# Two\n";
const RECT: &str = "<Shape kind=\"rect\"";
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";
const ICON: &str = "<Icon";
/// How [`picked`] shows the icon.
const ICON_PICKED: &str = "<Icon name=\"arrow-";
const NEEDS_THREE: &str = "Select three or more things to space them out.";
const IN_THE_TEXT: &str = "Drag it out of the text first.";
const AREA_ORDER: &str = "A layout's area always sits behind the things on the slide.";

/// The Arrange menu's items, a line between each group.
const ARRANGE_MENU: [&str; 14] = [
    "Bring to front",
    "Bring forward",
    "Send backward",
    "Send to back",
    "",
    "Align left",
    "Align centre",
    "Align right",
    "Align top",
    "Align middle",
    "Align bottom",
    "",
    "Space out across",
    "Space out down",
];

/// The menu a right-click on a thing opens.
const THING_MENU: [&str; 10] =
    ["Cut", "Copy", "Paste", "Duplicate", "Delete", "", "Bring to front", "Bring forward", "Send backward", "Send to back"];

/// Opens `source` in a window wide enough that the Home tab's tools all show.
fn open_wide(cx: &mut TestAppContext, source: &str) -> Typing {
    let typing = open(cx, source);
    cx.simulate_window_resize(typing.handle, size(px(1800.), px(900.)));
    typing.settle(cx);
    typing
}

fn selection(typing: &Typing, cx: &TestAppContext) -> Option<Selection> {
    typing.document(cx).read_with(cx, |document, _| document.selection().cloned())
}

/// What's selected, each block by how its text starts and each area by its name.
fn picked(typing: &Typing, cx: &TestAppContext) -> Vec<String> {
    let source = typing.source(cx);
    selection(typing, cx)
        .map(|selection| {
            selection
                .things
                .into_iter()
                .map(|thing| match thing {
                    Thing::Block(range) => source[range].chars().take(18).collect(),
                    Thing::Region(name) => name,
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

/// The value of `name` written on the tag that starts with `start`.
fn value_of(typing: &Typing, cx: &mut TestAppContext, start: &str, name: &str) -> String {
    let source = typing.source(cx);
    let tag = &source[of(typing, cx, start)];
    let from = tag.find(&format!(" {name}=\"")).unwrap_or_else(|| panic!("{tag} has no {name}")) + name.len() + 3;
    tag[from..].split('"').next().unwrap().to_string()
}

/// Opens the Arrange menu.
fn open_arrange(typing: &Typing, cx: &mut TestAppContext) {
    click_on(typing, cx, ARRANGE_ID);
}

/// Whether item `label` of the open Arrange menu is off: a click on it does nothing, and the
/// menu stays open.
fn arrange_item_off(typing: &Typing, cx: &mut TestAppContext, label: &str) -> bool {
    let at = ARRANGE_MENU.iter().position(|item| *item == label).expect("an item of the menu");
    let before = typing.source(cx);
    click_item(typing, cx, at);
    menu_open(typing, cx) && typing.source(cx) == before
}

/// Whether the open menu says `words`.
fn menu_says(typing: &Typing, cx: &mut TestAppContext, words: &str) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").try_find_by_label(words).is_some()
    })
}

fn menu_open(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find("popup-menu").is_some()
    })
}

/// Clicks item `at` of the open menu.
fn click_item(typing: &Typing, cx: &mut TestAppContext, at: usize) {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(at, cx);
    });
    typing.settle(cx);
}

/// Picks `label` from the Arrange menu.
fn arrange(typing: &Typing, cx: &mut TestAppContext, label: &str) {
    open_arrange(typing, cx);
    let at = ARRANGE_MENU.iter().position(|item| *item == label).expect("an item of the menu");
    click_item(typing, cx, at);
}

/// The open menu's items, a line as nothing.
fn menu_items(typing: &Typing, cx: &mut TestAppContext, count: usize) -> Vec<String> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        (0..count).map(|item| menu.find(item).label().unwrap_or_default().to_string()).collect()
    })
}

/// A right-click at `at`.
fn right_click(typing: &Typing, cx: &mut TestAppContext, at: Point<Pixels>) {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let modifiers = Modifiers::default();
        window.dispatch_event(MouseMoveEvent { position: at, pressed_button: None, modifiers }.to_platform_input(), cx);
        window.dispatch_event(
            MouseDownEvent { button: MouseButton::Right, position: at, modifiers, click_count: 1, first_mouse: false }.to_platform_input(),
            cx,
        );
        window.dispatch_event(MouseUpEvent { button: MouseButton::Right, position: at, modifiers, click_count: 1 }.to_platform_input(), cx);
    });
    typing.settle(cx);
}

fn right_click_thing(typing: &Typing, cx: &mut TestAppContext, start: &str) {
    let element = thing(typing, cx, start);
    let at = middle(typing, cx, &element);
    right_click(typing, cx, at);
}

fn slide_has_keyboard(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        let view = typing.view.clone();
        view.read(cx).slide_editor().read(cx).focus_handle().is_focused(window)
    })
}

fn list_has_keyboard(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        let view = typing.view.clone();
        view.read(cx).slide_strip().read(cx).focus_handle().contains_focused(window, cx)
    })
}

/// The rule that keeps Delete from taking a slide while a thing on it shows as selected: never
/// both at once.
fn holds_the_rule(typing: &Typing, cx: &mut TestAppContext, step: &str) {
    let both = list_has_keyboard(typing, cx) && selection(typing, cx).is_some();
    assert!(!both, "after {step}, the list has the keyboard while a thing is selected");
}

/// The shapes of slide `index` of `source`, from back to front, as they're drawn.
fn shapes_drawn(folder: &Path, source: &str, index: usize) -> Vec<ShapeKind> {
    let mut deck = Deck::open(Box::new(FolderFiles::new(folder.to_path_buf())), Fonts::bundled_only(), source.to_string());
    deck.specs(index)
        .iter()
        .flat_map(|spec| spec.native.iter())
        .filter_map(|native| match &native.content {
            NativeContent::Shape { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

fn folder_of(typing: &Typing) -> PathBuf {
    typing.file.parent().unwrap().to_path_buf()
}

/// Selects the main text's area with a press on its frame, once typing in it shows the frame.
fn select_body(typing: &Typing, cx: &mut TestAppContext) {
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text");
    typing.click_text(cx, body, pos(0, 3));
    let rect = typing.layout(cx).region("body").expect("the area").rect;
    let at = typing.at(cx, rect.x, rect.y + rect.h / 2.);
    typing.click(cx, at);
    assert_eq!(picked(typing, cx), ["body"]);
}

#[gpui_kit::test]
fn one_thing_aligns_to_the_slide(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    arrange(&typing, cx, "Align right");
    assert_eq!(value_of(&typing, cx, RECT, "x"), "1720", "its right edge on the slide's");
    assert_eq!(value_of(&typing, cx, RECT, "y"), "500", "and no higher or lower");
    assert_eq!(undo_label(&typing, cx), Some("Align"));
    assert_eq!(picked(&typing, cx), [&RECT[..18]], "still selected");
    assert!(slide_has_keyboard(&typing, cx), "and the slide has the keyboard");
    arrange(&typing, cx, "Align middle");
    assert_eq!(value_of(&typing, cx, RECT, "y"), "440");
    typing.press(cx, "ctrl-z");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn three_things_align_to_each_other_in_one_step(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    select_things(&typing, cx, &[ICON, RECT, ELLIPSE]);
    arrange(&typing, cx, "Align top");
    for start in [RECT, ELLIPSE, ICON] {
        assert_eq!(value_of(&typing, cx, start, "y"), "500", "{start} at the top of the three");
    }
    assert_eq!(value_of(&typing, cx, ELLIPSE, "x"), "700", "and no further across");
    assert_eq!(picked(&typing, cx).len(), 3, "all still selected");
    assert_eq!(undo_label(&typing, cx), Some("Align"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "one step");
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn space_out_across_makes_equal_gaps(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    select_things(&typing, cx, &[RECT, ELLIPSE, ICON]);
    arrange(&typing, cx, "Space out across");
    let rects = [RECT, ELLIPSE, ICON].map(|start| thing(&typing, cx, start).rect);
    let gaps = [rects[1].x - rects[0].x - rects[0].w, rects[2].x - rects[1].x - rects[1].w];
    assert!((gaps[0] - gaps[1]).abs() <= 1., "the gaps are the same: {gaps:?}");
    assert_eq!(value_of(&typing, cx, RECT, "x"), "200", "the ends stay put");
    assert_eq!(value_of(&typing, cx, ICON, "x"), "1400");
    assert_eq!(value_of(&typing, cx, ELLIPSE, "y"), "550", "and nothing goes up or down");
    assert_eq!(undo_label(&typing, cx), Some("Distribute"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn space_out_with_two_things_is_disabled_and_says_why(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    select_things(&typing, cx, &[RECT, ELLIPSE]);
    open_arrange(&typing, cx);
    assert!(arrange_item_off(&typing, cx, "Space out across"));
    assert!(arrange_item_off(&typing, cx, "Space out down"));
    assert!(menu_says(&typing, cx, NEEDS_THREE));
    typing.press(cx, "escape");
    assert!(!menu_open(&typing, cx));
    assert_eq!(picked(&typing, cx).len(), 2, "both still selected");
    assert!(slide_has_keyboard(&typing, cx));
    // Asked all the same, as by a key, it's said why.
    typing.with_window(cx, |window, cx| {
        window.dispatch_action(Box::new(stagepack_studio::arrange_tools::DistributeThings { across: true }), cx)
    });
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(NEEDS_THREE));
    assert_eq!(typing.source(cx), TALK);
    // Two can be aligned.
    arrange(&typing, cx, "Align left");
    assert_eq!(value_of(&typing, cx, ELLIPSE, "x"), "200");
}

#[gpui_kit::test]
fn bring_forward_moves_its_lines_and_draws_it_above(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    let folder = folder_of(&typing);
    assert_eq!(shapes_drawn(&folder, TALK, 0), [ShapeKind::Rect, ShapeKind::Ellipse]);
    let (rect, ellipse) = (of(&typing, cx, RECT), of(&typing, cx, ELLIPSE));
    click_thing(&typing, cx, RECT, Modifiers::default());
    arrange(&typing, cx, "Bring forward");
    let source = typing.source(cx);
    assert_eq!(shapes_drawn(&folder, &source, 0), [ShapeKind::Ellipse, ShapeKind::Rect], "drawn above it now");
    assert!(source.find(&TALK[ellipse.clone()]).unwrap() < source.find(&TALK[rect.clone()]).unwrap(), "written after it");
    assert!(source.contains(&TALK[rect.clone()]) && source.contains(&TALK[ellipse]), "each tag's bytes are as they were");
    assert_eq!(source.len(), TALK.len());
    assert_eq!(picked(&typing, cx), [&RECT[..18]], "it's still the one selected");
    assert!(slide_has_keyboard(&typing, cx));
    assert_eq!(undo_label(&typing, cx), Some("Change order"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert_eq!(selection(&typing, cx).map(|selection| selection.things), Some(vec![Thing::Block(rect)]), "selected where it was");
}

#[gpui_kit::test]
fn to_front_and_to_back(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    arrange(&typing, cx, "Bring to front");
    let source = typing.source(cx);
    let at = |start: &str| source.find(start).unwrap();
    assert!(at(ELLIPSE) < at(ICON) && at(ICON) < at(RECT), "in front of both");
    assert_eq!(picked(&typing, cx), [&RECT[..18]]);
    arrange(&typing, cx, "Send to back");
    assert_eq!(typing.source(cx), TALK, "behind both again, where it began");
    assert_eq!(picked(&typing, cx), [&RECT[..18]]);
}

#[gpui_kit::test]
fn the_front_thing_cannot_come_forward(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_thing(&typing, cx, ICON, Modifiers::default());
    open_arrange(&typing, cx);
    assert!(arrange_item_off(&typing, cx, "Bring forward"));
    assert!(arrange_item_off(&typing, cx, "Bring to front"));
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), TALK);
    // It can go back.
    arrange(&typing, cx, "Send backward");
    let source = typing.source(cx);
    assert!(source.find(ICON).unwrap() < source.find(ELLIPSE).unwrap());
    typing.press(cx, "ctrl-z");
    // And the one at the back can't go further back.
    click_thing(&typing, cx, RECT, Modifiers::default());
    open_arrange(&typing, cx);
    assert!(arrange_item_off(&typing, cx, "Send backward"));
    assert!(arrange_item_off(&typing, cx, "Send to back"));
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn an_area_cannot_be_reordered_and_says_why(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    select_body(&typing, cx);
    open_arrange(&typing, cx);
    for item in ["Bring to front", "Bring forward", "Send backward", "Send to back"] {
        assert!(arrange_item_off(&typing, cx, item), "{item}");
    }
    assert!(menu_says(&typing, cx, AREA_ORDER));
    typing.press(cx, "escape");
    typing.with_window(cx, |window, cx| {
        window.dispatch_action(Box::new(ReorderThing { order: slides_format::Order::Front }), cx)
    });
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(AREA_ORDER));
    assert_eq!(typing.source(cx), TALK);
    // An area can be aligned, by the words on its settings line.
    arrange(&typing, cx, "Align bottom");
    assert!(typing.source(cx).contains("region.body:"), "{}", typing.source(cx));
    assert_eq!(undo_label(&typing, cx), Some("Align"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    // Several aren't reordered either.
    select_things(&typing, cx, &[RECT, ELLIPSE]);
    open_arrange(&typing, cx);
    assert!(arrange_item_off(&typing, cx, "Bring forward"));
    assert!(menu_says(&typing, cx, "Select one thing to bring it forward or send it back."));
}

#[gpui_kit::test]
fn a_thing_in_the_text_is_not_arranged_and_says_why(cx: &mut TestAppContext) {
    let talk = "# T\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n<Shape kind=\"rect\" x=\"100\" y=\"500\" w=\"200\" h=\"200\" />\n";
    let typing = open_wide(cx, talk);
    let table = of(&typing, cx, "| A");
    typing.document(cx).update(cx, |document, cx| {
        document.select(0, table);
        cx.notify();
    });
    typing.settle(cx);
    open_arrange(&typing, cx);
    assert!(arrange_item_off(&typing, cx, "Align left"));
    assert!(arrange_item_off(&typing, cx, "Bring forward"));
    assert!(menu_says(&typing, cx, IN_THE_TEXT));
    typing.press(cx, "escape");
    typing.with_window(cx, |window, cx| {
        window.dispatch_action(Box::new(stagepack_studio::arrange_tools::AlignThings { to: stagepack_studio::arrange::AlignTo::Left }), cx)
    });
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(IN_THE_TEXT));
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn a_right_click_opens_the_things_menu_and_keeps_it_selected(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    select_things(&typing, cx, &[RECT, ELLIPSE]);
    right_click_thing(&typing, cx, ELLIPSE);
    assert!(menu_open(&typing, cx));
    assert_eq!(menu_items(&typing, cx, THING_MENU.len()), THING_MENU);
    assert_eq!(picked(&typing, cx).len(), 2, "a selection it's part of stays");
    typing.press(cx, "escape");
    assert!(!menu_open(&typing, cx));
    assert_eq!(picked(&typing, cx).len(), 2);
    assert!(slide_has_keyboard(&typing, cx), "the slide has the keyboard back");
    // On a thing not selected: it's selected alone.
    right_click_thing(&typing, cx, ICON);
    assert_eq!(picked(&typing, cx), [ICON_PICKED]);
    // Its order can be changed from there.
    click_item(&typing, cx, THING_MENU.iter().position(|item| *item == "Send to back").unwrap());
    assert!(typing.source(cx).find(ICON).unwrap() < typing.source(cx).find(RECT).unwrap(), "behind both");
    assert_eq!(picked(&typing, cx), [ICON_PICKED], "still selected");
    assert!(slide_has_keyboard(&typing, cx));
    // On the empty slide, there's only Paste.
    let empty = typing.at(cx, 1880., 1060.);
    right_click(&typing, cx, empty);
    assert!(menu_open(&typing, cx));
    assert_eq!(menu_items(&typing, cx, 1), ["Paste"]);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.within("popup-menu").try_find(1usize).is_none(), "nothing else");
    });
    typing.press(cx, "escape");
}

#[gpui_kit::test]
fn a_right_click_on_the_text_being_typed_leaves_it_be(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).unwrap();
    typing.click_text(cx, body, pos(0, 3));
    let at = typing.point_of(cx, body, pos(0, 6));
    right_click(&typing, cx, at);
    assert!(!menu_open(&typing, cx));
    assert!(typing.editing(cx).is_some(), "typing goes on");
    assert!(slide_has_keyboard(&typing, cx));
}

#[gpui_kit::test]
fn delete_from_the_menu_takes_the_thing_and_not_the_slide(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    // From the list of slides having the keyboard.
    click_on(&typing, cx, strip_row_id(0));
    assert!(list_has_keyboard(&typing, cx));
    right_click_thing(&typing, cx, RECT);
    holds_the_rule(&typing, cx, "a right-click");
    click_item(&typing, cx, THING_MENU.iter().position(|item| *item == "Delete").unwrap());
    let source = typing.source(cx);
    assert!(!source.contains(RECT), "the rectangle is gone");
    assert_eq!(typing.slide_count(cx), 2, "and the slide stays");
    assert_eq!(undo_label(&typing, cx), Some("Delete"));
    holds_the_rule(&typing, cx, "Delete");
}

#[gpui_kit::test]
fn the_keyboard_rule_holds_after_the_menu_closes(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    // Closed with Escape.
    right_click_thing(&typing, cx, RECT);
    typing.press(cx, "escape");
    holds_the_rule(&typing, cx, "Escape");
    assert!(slide_has_keyboard(&typing, cx));
    assert_eq!(picked(&typing, cx), [&RECT[..18]]);
    // The arrow keys move it, as the slide has the keyboard.
    typing.press(cx, "right");
    assert_eq!(value_of(&typing, cx, RECT, "x"), "201");
    // Closed by choosing.
    right_click_thing(&typing, cx, RECT);
    click_item(&typing, cx, THING_MENU.iter().position(|item| *item == "Bring forward").unwrap());
    holds_the_rule(&typing, cx, "choosing");
    assert!(slide_has_keyboard(&typing, cx));
    assert_eq!(picked(&typing, cx), [&RECT[..18]]);
    // Closed by a click on the list of slides, which takes the keyboard.
    right_click_thing(&typing, cx, RECT);
    let row = bounds_of(&typing, cx, strip_row_id(1)).expect("the second row").center();
    // The one click closes the menu and is the row's too.
    typing.click(cx, row);
    assert!(!menu_open(&typing, cx), "the menu closed");
    assert_eq!(typing.current(cx), 1, "the row's slide is shown");
    assert!(list_has_keyboard(&typing, cx), "the list has the keyboard");
    assert!(!slide_has_keyboard(&typing, cx));
    assert_eq!(selection(&typing, cx), None, "and nothing is selected");
    holds_the_rule(&typing, cx, "a click on the list");
    // Delete there takes the slide, as nothing on it is selected.
    typing.press(cx, "delete");
    assert_eq!(typing.slide_count(cx), 1);
}

#[gpui_kit::test]
fn the_form_can_be_hidden_and_shown(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    assert!(bounds_of(&typing, cx, INSPECTOR_ID).is_some(), "the form starts shown");
    let width = typing.slide_bounds(cx).size.width;
    click_on(&typing, cx, TAB_VIEW_ID);
    let toggled = |typing: &Typing, cx: &mut TestAppContext| {
        typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.find(TOGGLE_INSPECTOR_ID).checked()
        })
    };
    assert_eq!(toggled(&typing, cx), Some(true));
    click_on(&typing, cx, TOGGLE_INSPECTOR_ID);
    assert!(bounds_of(&typing, cx, INSPECTOR_ID).is_none(), "hidden");
    assert_eq!(toggled(&typing, cx), Some(false));
    assert!(typing.slide_bounds(cx).size.width > width, "the slide takes the room");
    click_on(&typing, cx, TOGGLE_INSPECTOR_ID);
    assert!(bounds_of(&typing, cx, INSPECTOR_ID).is_some(), "shown again");
    assert_eq!(typing.slide_bounds(cx).size.width, width);
}

/// A text box with a line of text, below which there's room in the box.
const BOXED: &str = "# T\n\n<Text x=\"200\" y=\"300\" w=\"800\" h=\"400\">\n\nHi there\n\n</Text>\n\n---\n\n# Two\n";

#[gpui_kit::test]
fn a_right_click_on_a_box_emptied_by_typing_never_acts_on_the_slide(cx: &mut TestAppContext) {
    // Asked in review: picking an emptied box ends its typing, which takes the box away. Every
    // point of a box being typed in is its text, though, so the press is left be.
    let typing = open_wide(cx, BOXED);
    let layout = typing.layout(cx);
    let flow = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow).is_none()).expect("the box's text");
    typing.double_click_text(cx, flow, pos(0, 2));
    assert!(typing.editing(cx).is_some(), "typing in the box");
    typing.press(cx, "ctrl-a");
    typing.press(cx, "backspace");
    // Its corners, an edge, and well below its emptied line.
    for (x, y) in [(202., 302.), (998., 698.), (202., 500.), (600., 620.)] {
        let at = typing.at(cx, x, y);
        right_click(&typing, cx, at);
        assert!(!menu_open(&typing, cx), "at {x}, {y}");
        assert!(typing.editing(cx).is_some(), "typing goes on");
    }
    typing.press(cx, "escape");
    assert_eq!(typing.slide_count(cx), 2, "the slide stays");
}

#[gpui_kit::test]
fn with_nothing_selected_no_item_of_the_things_menu_acts_on_the_slide(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    // Something of the slide's own on the clipboard, which Paste puts on the slide.
    click_thing(&typing, cx, RECT, Modifiers::default());
    typing.press(cx, "ctrl-c");
    for (at, item) in THING_MENU.iter().enumerate().filter(|(_, item)| !item.is_empty()) {
        right_click_thing(&typing, cx, ELLIPSE);
        assert!(menu_open(&typing, cx));
        // What was selected goes while the menu is open.
        typing.document(cx).update(cx, |document, cx| {
            document.deselect();
            cx.notify();
        });
        typing.settle(cx);
        let before = typing.source(cx);
        click_item(&typing, cx, at);
        if menu_open(&typing, cx) {
            typing.press(cx, "escape");
        }
        assert_eq!(typing.slide_count(cx), 2, "{item} left the slides as they were");
        if *item != "Paste" {
            assert_eq!(typing.source(cx), before, "{item} did nothing");
        } else if typing.source(cx) != before {
            typing.press(cx, "ctrl-z");
        }
    }
}

#[gpui_kit::test]
fn an_order_the_talk_cannot_take_is_off_in_both_menus(cx: &mut TestAppContext) {
    // The rectangle shares its last line with the ellipse, so its lines can't be moved alone.
    let talk = "# T\n\n<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\">\n</Shape> <Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" />\n";
    let typing = open_wide(cx, talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let (quick, tried) = typing.document(cx).read_with(cx, |document, _| (document.arrange_here().can, document.arrange_menu().can));
    assert!(quick[0], "the quick look says it could come forward");
    assert!(!tried[0], "but the talk can't take it");
    open_arrange(&typing, cx);
    assert!(arrange_item_off(&typing, cx, "Bring forward"));
    typing.press(cx, "escape");
    right_click_thing(&typing, cx, RECT);
    let at = THING_MENU.iter().position(|item| *item == "Bring forward").unwrap();
    click_item(&typing, cx, at);
    assert!(menu_open(&typing, cx), "the item is off");
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn aligning_never_takes_a_thing_off_the_slide(cx: &mut TestAppContext) {
    // Found in review: a thing mostly off the slide took another wholly off with it.
    let talk = "# T\n\n<Shape kind=\"rect\" fill=\"#2563eb\" x=\"-984\" y=\"300\" w=\"1000\" h=\"100\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"600\" y=\"600\" w=\"500\" h=\"100\" />\n";
    let typing = open_wide(cx, talk);
    let on = typing.at(cx, 8., 350.);
    typing.click(cx, on);
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(picked(&typing, cx).len(), 2);
    arrange(&typing, cx, "Align left");
    assert_eq!(value_of(&typing, cx, ELLIPSE, "x"), "-484", "sixteen units of it stay on the slide");
    assert_eq!(value_of(&typing, cx, RECT, "x"), "-984");
    assert_eq!(undo_label(&typing, cx), Some("Align"));
}

#[gpui_kit::test]
fn hiding_the_form_writes_what_was_typed_and_gives_the_slide_the_keyboard(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_on(&typing, cx, gpui_kit::SharedString::from(stagepack_studio::inspector::field_id("x")));
    typing.press(cx, "ctrl-a");
    typing.type_text(cx, "250");
    assert!(!slide_has_keyboard(&typing, cx), "the field has the keyboard");
    // Hidden while the field has the keyboard, as by the View tab's toggle.
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(stagepack_studio::workspace_view::ToggleInspector), cx));
    typing.settle(cx);
    assert!(bounds_of(&typing, cx, INSPECTOR_ID).is_none(), "the form is hidden");
    assert_eq!(value_of(&typing, cx, RECT, "x"), "250", "what was typed is written");
    assert_eq!(picked(&typing, cx), [&RECT[..18]], "the thing is still selected");
    assert!(slide_has_keyboard(&typing, cx), "and the slide has the keyboard");
    holds_the_rule(&typing, cx, "hiding the form");
    typing.press(cx, "right");
    assert_eq!(value_of(&typing, cx, RECT, "x"), "251", "the arrow keys move it");
}

fn notes_have_keyboard(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| typing.view.read(cx).notes_panel().read(cx).is_focused(window, cx))
}

#[gpui_kit::test]
fn a_right_click_on_a_thing_takes_the_keyboard_from_the_notes(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_on(&typing, cx, stagepack_studio::notes_panel::NOTES_ID);
    assert!(notes_have_keyboard(&typing, cx));
    right_click_thing(&typing, cx, RECT);
    assert!(menu_open(&typing, cx));
    assert_eq!(picked(&typing, cx), [&RECT[..18]]);
    typing.press(cx, "escape");
    assert!(slide_has_keyboard(&typing, cx), "the slide has the keyboard");
    assert!(!notes_have_keyboard(&typing, cx));
    assert_eq!(picked(&typing, cx), [&RECT[..18]], "and the thing is selected");
}

#[gpui_kit::test]
fn a_right_click_on_the_empty_slide_from_the_list_selects_nothing(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_on(&typing, cx, strip_row_id(0));
    assert!(list_has_keyboard(&typing, cx));
    let empty = typing.at(cx, 1880., 1060.);
    right_click(&typing, cx, empty);
    assert_eq!(menu_items(&typing, cx, 1), ["Paste"]);
    assert_eq!(selection(&typing, cx), None);
    typing.press(cx, "escape");
    // As a click on the slide does, it takes the keyboard from the list, with nothing selected.
    assert!(slide_has_keyboard(&typing, cx), "the slide has the keyboard");
    assert!(!list_has_keyboard(&typing, cx));
    assert_eq!(selection(&typing, cx), None, "and nothing is selected");
    holds_the_rule(&typing, cx, "Escape");
}

#[gpui_kit::test]
fn a_right_click_on_an_areas_text_offers_only_paste_and_lets_go(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).unwrap();
    let at = typing.point_of(cx, body, pos(0, 3));
    right_click(&typing, cx, at);
    assert_eq!(menu_items(&typing, cx, 1), ["Paste"]);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.within("popup-menu").try_find(1usize).is_none(), "nothing else");
    });
    assert_eq!(selection(&typing, cx), None, "what was selected is let go of");
    assert!(typing.editing(cx).is_none(), "and nothing is typed in");
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_right_click_off_any_thing_while_typing_does_nothing(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).unwrap();
    typing.click_text(cx, body, pos(0, 3));
    let typed = typing.editing(cx).expect("typing").edit.selection;
    let empty = typing.at(cx, 1880., 1060.);
    right_click(&typing, cx, empty);
    assert!(!menu_open(&typing, cx), "no menu");
    assert_eq!(typing.editing(cx).map(|editing| editing.edit.selection), Some(typed), "typing goes on where it was");
    assert!(slide_has_keyboard(&typing, cx));
    assert_eq!(typing.source(cx), TALK);
}
