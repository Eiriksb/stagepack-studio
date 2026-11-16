//! The icon picker: a search field and a grid of the bundled icons, from the Insert tab's Icon
//! tool and from an icon's Icon field in the inspector.

use std::ops::Range;
use std::time::Instant;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Entity, ElementId, Rgba, ScrollDelta, TestAppContext, point, px};
use slides_engine::icon_names;
use stagepack_studio::icon_picker::{ICON_GRID_ID, ICON_SEARCH_ID, IconPicker, NO_ICONS_ID, NO_MATCHES, icon_id, matching};
use stagepack_studio::insert_tools::{INSERT_ICON_ID, INSERT_SHAPES_ID, shape_id};
use stagepack_studio::inspector::field_id;
use stagepack_studio::ribbon::TAB_INSERT_ID;
use stagepack_studio::slide_editor::NEW_BOX_ID;

const TALK: &str = "---\ntitle: T\n---\n\n# One\n\nSome words\n\n---\n\n# Two\n";
/// Where the first slide's content ends, and where what's inserted on it goes.
const END_OF_ONE: &str = "Some words";
const SHOWCASE: &str = include_str!("../../../fixtures/showcase/slides.md");
/// The showcase's slide of shapes, an icon, a picture and a table.
const THINGS: usize = 5;

/// An icon named `name` as the picker writes it, in the middle of the slide.
fn icon_line(name: &str) -> String {
    format!("<Icon name=\"{name}\" size=\"128\" x=\"896\" y=\"476\" w=\"128\" h=\"128\" />")
}

/// What the talk says once `inserted` is put after the first slide's content.
fn with_inserted(inserted: &str) -> String {
    TALK.replacen(END_OF_ONE, &format!("{END_OF_ONE}\n\n{inserted}"), 1)
}

fn selected(typing: &Typing, cx: &TestAppContext) -> Option<Range<usize>> {
    typing.document(cx).read_with(cx, |document, _| document.selected().map(|selected| selected.source.clone()))
}

fn selected_text(typing: &Typing, cx: &TestAppContext) -> Option<String> {
    let source = typing.source(cx);
    selected(typing, cx).map(|range| source[range].to_string())
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn new_box(typing: &Typing, cx: &TestAppContext) -> Option<(usize, (f32, f32, f32, f32))> {
    typing.document(cx).read_with(cx, |document, _| document.new_box())
}

fn click_on(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    });
    typing.settle(cx);
}

fn is_shown(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

fn label_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

/// The Insert tab's picker.
fn picker(typing: &Typing, cx: &TestAppContext) -> Entity<IconPicker> {
    cx.read(|cx| typing.view.read(cx).ribbon().read(cx).icon_picker().clone())
}

fn is_open(picker: &Entity<IconPicker>, cx: &TestAppContext) -> bool {
    picker.read_with(cx, |picker, _| picker.is_open())
}

fn made(picker: &Entity<IconPicker>, cx: &TestAppContext) -> usize {
    picker.read_with(cx, |picker, _| picker.previews_made())
}

/// Opens the picker from the Insert tab's Icon tool.
fn open_picker(typing: &Typing, cx: &mut TestAppContext) -> Entity<IconPicker> {
    click_on(typing, cx, TAB_INSERT_ID);
    click_on(typing, cx, INSERT_ICON_ID);
    let picker = picker(typing, cx);
    assert!(is_open(&picker, cx), "the picker opens");
    picker
}

fn search_has_keyboard(typing: &Typing, cx: &mut TestAppContext, picker: &Entity<IconPicker>) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        picker.read(cx).has_keyboard(window, cx)
    })
}

fn insert_shape(typing: &Typing, cx: &mut TestAppContext, kind: &str) {
    click_on(typing, cx, TAB_INSERT_ID);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(INSERT_SHAPES_ID, cx);
        window.render_frame(cx);
        window.within("popup-menu").click(shape_id(kind), cx);
    });
    typing.settle(cx);
}

// ---------------------------------------------------------------------------------------
// Finding an icon

#[test]
fn matching_finds_names_holding_every_word() {
    assert_eq!(matching("").len(), 2776, "every icon");
    assert_eq!(matching("  ").len(), 2776);
    assert_eq!(matching(""), icon_names().to_vec(), "in name order");

    // Names that start with what's asked for come first, then the rest, each lot in name order.
    assert_eq!(matching("arrow right"), ["arrow-right", "arrow-down-right", "arrow-up-right"]);
    let star = matching("star");
    assert_eq!(&star[..4], ["star", "star-filled", "star-half", "star-review"]);
    assert!(star[4..].is_sorted() && star.iter().all(|name| name.contains("star")), "{star:?}");

    // Case and dashes don't matter.
    let asked = matching("ARROW-R");
    assert_eq!(asked, matching("arrow-r"));
    assert_eq!(asked, matching("arrowr"));
    assert_eq!(asked.first(), Some(&"arrow-right"), "{asked:?}");
    assert!(matching("Arrow R").starts_with(&["arrow-right"]));
    assert!(matching("arrow r").iter().all(|name| name.contains("arrow")));

    assert!(matching("zzzz").is_empty());

    let long = "a".repeat(10_000);
    let started = Instant::now();
    assert!(matching(&long).is_empty());
    let words = "ab ".repeat(3_333);
    assert!(matching(&words).len() < 2776);
    assert!(started.elapsed().as_millis() < 200, "at once: {:?}", started.elapsed());
}

// ---------------------------------------------------------------------------------------
// The picker from the Insert tab

#[gpui_kit::test]
fn the_picker_opens_with_the_keyboard_in_search(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    click_on(&typing, cx, TAB_INSERT_ID);
    assert_eq!(label_of(&typing, cx, "group-Icons").as_deref(), Some("Icons"));
    let picker = open_picker(&typing, cx);
    assert!(is_shown(&typing, cx, ICON_SEARCH_ID));
    assert!(is_shown(&typing, cx, ICON_GRID_ID));
    assert!(search_has_keyboard(&typing, cx, &picker));
    assert!(is_shown(&typing, cx, icon_id(icon_names()[0])), "the grid starts at the first icon");
    let first = label_of(&typing, cx, icon_id(icon_names()[0]));
    assert_eq!(first.as_deref(), Some(icon_names()[0]), "each icon is named by its name");

    // Keys typed there are the search's, not the slide's.
    for key in ["backspace", "delete", "left", "right", "pagedown", "pageup", "ctrl-m", "ctrl-d"] {
        typing.press(cx, key);
    }
    typing.type_text(cx, "st");
    assert_eq!(typing.source(cx), TALK, "nothing is typed on the slide");
    assert_eq!(typing.current(cx), 0);
    assert!(typing.editing(cx).is_none());
    assert_eq!(picker.read_with(cx, |picker, cx| picker.query(cx)), "st");
}

#[gpui_kit::test]
fn typing_narrows_the_grid(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    typing.type_text(cx, "arrow right");
    let shown = picker.read_with(cx, |picker, _| picker.matches().to_vec());
    assert_eq!(shown, matching("arrow right"));
    assert!(is_shown(&typing, cx, icon_id("arrow-right")));
    assert!(!is_shown(&typing, cx, icon_id(icon_names()[0])), "what doesn't match is gone");
    // Narrowing again, the grid shows the new first ones.
    typing.press(cx, "ctrl-a");
    typing.type_text(cx, "star");
    assert!(is_shown(&typing, cx, icon_id("star")));
    assert!(!is_shown(&typing, cx, icon_id("arrow-right")));
}

#[gpui_kit::test]
fn enter_inserts_the_first_match(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    typing.type_text(cx, "arrow right");
    typing.press(cx, "enter");
    assert_eq!(typing.source(cx), with_inserted(&icon_line("arrow-right")));
    assert!(!is_open(&picker, cx), "the picker closes");
    assert!(!is_shown(&typing, cx, ICON_SEARCH_ID));
    assert_eq!(selected_text(&typing, cx), Some(icon_line("arrow-right")), "and the icon is selected");
    assert_eq!(undo_label(&typing, cx), Some("Insert icon"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "one undo step");
    assert_eq!(selected(&typing, cx), None);
}

#[gpui_kit::test]
fn a_click_inserts_that_icon_selected(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    typing.type_text(cx, "star");
    click_on(&typing, cx, icon_id("star"));
    assert_eq!(typing.source(cx), with_inserted(&icon_line("star")));
    assert!(!is_open(&picker, cx));
    assert_eq!(selected_text(&typing, cx), Some(icon_line("star")));
    // The icon is drawn on the slide, where it was put.
    let layout = typing.layout(cx);
    let source = typing.source(cx);
    let icon = layout.elements().iter().find(|element| source[element.source.clone()].starts_with("<Icon")).cloned();
    let icon = icon.expect("the icon is on the slide");
    assert_eq!((icon.rect.x, icon.rect.y, icon.rect.w, icon.rect.h), (896., 476., 128., 128.));
    // The picker opens afresh.
    open_picker(&typing, cx);
    assert_eq!(picker.read_with(cx, |picker, cx| picker.query(cx)), "");
    assert_eq!(picker.read_with(cx, |picker, _| picker.matches().len()), 2776);
}

#[gpui_kit::test]
fn escape_closes_and_inserts_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "rect");
    let shape = selected(&typing, cx).expect("the shape is selected");
    let source = typing.source(cx);
    let picker = open_picker(&typing, cx);
    typing.type_text(cx, "star");
    typing.press(cx, "escape");
    assert!(!is_open(&picker, cx));
    assert!(!is_shown(&typing, cx, ICON_SEARCH_ID));
    assert_eq!(typing.source(cx), source, "nothing is inserted");
    assert_eq!(selected(&typing, cx), Some(shape), "and the selection stays");

    // A text box waiting for its first character waits on.
    let typing = open(cx, TALK);
    click_on(&typing, cx, TAB_INSERT_ID);
    click_on(&typing, cx, stagepack_studio::insert_tools::INSERT_TEXT_BOX_ID);
    let waiting = new_box(&typing, cx).expect("a box waits");
    let picker = open_picker(&typing, cx);
    assert!(search_has_keyboard(&typing, cx, &picker));
    assert_eq!(new_box(&typing, cx), Some(waiting), "opening the picker keeps it");
    typing.type_text(cx, "st");
    assert_eq!(typing.source(cx), TALK, "typing in the search writes no box");
    typing.press(cx, "escape");
    assert!(!is_open(&picker, cx));
    assert_eq!(new_box(&typing, cx), Some(waiting), "Escape keeps it");
    assert!(is_shown(&typing, cx, NEW_BOX_ID));
    typing.type_text(cx, "Hi");
    assert!(typing.source(cx).contains("\n\nHi\n\n</Box>"), "typing goes on in it: {}", typing.source(cx));

    // Inserting an icon is another insert, which drops a waiting box.
    let typing = open(cx, TALK);
    click_on(&typing, cx, TAB_INSERT_ID);
    click_on(&typing, cx, stagepack_studio::insert_tools::INSERT_TEXT_BOX_ID);
    assert!(new_box(&typing, cx).is_some());
    open_picker(&typing, cx);
    typing.type_text(cx, "star");
    typing.press(cx, "enter");
    assert_eq!(new_box(&typing, cx), None);
    assert_eq!(typing.source(cx), with_inserted(&icon_line("star")));
}

#[gpui_kit::test]
fn no_matches_says_so(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    typing.type_text(cx, "zzzz");
    assert_eq!(label_of(&typing, cx, NO_ICONS_ID).as_deref(), Some(NO_MATCHES));
    assert_eq!(NO_MATCHES, "No icons match.");
    typing.press(cx, "enter");
    assert_eq!(typing.source(cx), TALK, "Enter picks nothing");
    assert!(is_open(&picker, cx), "and the picker stays open");
    typing.press(cx, "backspace");
    typing.press(cx, "backspace");
    typing.press(cx, "backspace");
    typing.press(cx, "backspace");
    assert_eq!(label_of(&typing, cx, NO_ICONS_ID), None);
}

#[gpui_kit::test]
fn the_picker_fits_in_the_smallest_window(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    cx.simulate_window_resize(typing.handle, gpui_kit::size(px(1024.), px(600.)));
    typing.settle(cx);
    open_picker(&typing, cx);
    let (search, grid) = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        (window.find(ICON_SEARCH_ID).bounds(), window.find(ICON_GRID_ID).bounds())
    });
    assert!(search.top() >= px(0.) && search.left() >= px(0.), "{search:?}");
    assert!(grid.right() <= px(1024.) && grid.bottom() <= px(600.), "{grid:?}");
}

// ---------------------------------------------------------------------------------------
// Drawing the icons

#[gpui_kit::test]
fn only_rows_on_screen_are_drawn(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    let opened = made(&picker, cx);
    assert!(opened > 0 && opened < 200, "{opened} drawn");
    assert!(opened <= 9 * 8, "{opened} drawn: no more than the rows on screen");
    let kept = picker.read_with(cx, |picker, _| picker.previews_kept());
    assert_eq!(kept, opened);
    // Each icon on screen shows its picture.
    assert!(picker.read_with(cx, |picker, _| picker.has_preview(icon_names()[0])));

    // Scrolling through the whole list draws the rows come to and keeps far less than all.
    let mut most_bytes = 0;
    // A screenful at a time, eight rows of 40 pixels.
    for _ in 0..50 {
        typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.scroll(ICON_GRID_ID, ScrollDelta::Pixels(point(px(0.), px(-320.))), cx);
        });
        typing.settle(cx);
        most_bytes = most_bytes.max(picker.read_with(cx, |picker, _| picker.preview_bytes()));
    }
    let last = *icon_names().last().unwrap();
    assert!(is_shown(&typing, cx, icon_id(last)), "the end of the list was reached");
    assert!(picker.read_with(cx, |picker, _| picker.has_preview(last)));
    assert!(most_bytes < 64 * 1024 * 1024, "{most_bytes} bytes kept");
    assert!(picker.read_with(cx, |picker, _| picker.previews_kept()) < 2776 / 2, "far less than all are kept");
    assert!(made(&picker, cx) >= 2776, "every row came to was drawn");

    // Closed, the picker lets go of them all.
    typing.press(cx, "escape");
    assert_eq!(picker.read_with(cx, |picker, _| picker.previews_kept()), 0);
}

#[gpui_kit::test]
fn an_open_picker_sitting_still_does_no_work(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    typing.type_text(cx, "arrow");
    wait(cx, 1000);
    typing.settle(cx);
    let drawn = made(&picker, cx);
    let (redraws, _watching) = redraws(cx, &picker);
    wait(cx, 3000);
    typing.settle(cx);
    assert_eq!(redraws.get(), 0, "nothing asks for the picker to be drawn again");
    assert_eq!(made(&picker, cx), drawn, "and nothing is drawn again");
}

#[gpui_kit::test]
fn the_icons_are_drawn_again_when_the_theme_changes(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let picker = open_picker(&typing, cx);
    let light = typing.with_window(cx, |_, cx| Rgba::from(cx.theme().foreground));
    let drawn = made(&picker, cx);
    assert_eq!(picker.read_with(cx, |picker, _| picker.preview_color()), Some(rgba_bytes(light)));
    typing.with_window(cx, |window, cx| Theme::change(ThemeMode::Dark, Some(window), cx));
    typing.settle(cx);
    let dark = typing.with_window(cx, |_, cx| Rgba::from(cx.theme().foreground));
    assert_ne!(rgba_bytes(light), rgba_bytes(dark));
    assert_eq!(picker.read_with(cx, |picker, _| picker.preview_color()), Some(rgba_bytes(dark)));
    assert!(made(&picker, cx) >= drawn * 2, "the icons on screen were drawn again");
    assert_eq!(picker.read_with(cx, |picker, _| picker.previews_kept()), drawn);
}

fn rgba_bytes(color: Rgba) -> [u8; 4] {
    [color.r, color.g, color.b, color.a].map(|part| (part * 255.).round() as u8)
}

// ---------------------------------------------------------------------------------------
// The inspector's Icon field

fn select_icon(typing: &Typing, cx: &mut TestAppContext) -> Range<usize> {
    for _ in 0..THINGS {
        typing.press(cx, "pagedown");
    }
    let source = typing.source(cx);
    let icon = typing
        .layout(cx)
        .elements()
        .iter()
        .find(|element| source[element.source.clone()].starts_with("<Icon"))
        .cloned()
        .expect("the showcase has an icon");
    let at = typing.at(cx, icon.rect.x + icon.rect.w / 2., icon.rect.y + icon.rect.h / 2.);
    typing.click(cx, at);
    assert_eq!(selected(typing, cx), Some(icon.source.clone()));
    icon.source
}

#[gpui_kit::test]
fn the_inspector_changes_an_icons_name_in_place(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    select_icon(&typing, cx);
    let field = field_id("name");
    let button = label_of(&typing, cx, field.clone());
    assert_eq!(button.as_deref(), Some("arrow-right"), "the field shows the icon's name");
    click_on(&typing, cx, field.clone());
    assert!(is_shown(&typing, cx, ICON_SEARCH_ID), "it opens the picker");
    typing.type_text(cx, "star");
    typing.press(cx, "enter");
    let changed = SHOWCASE.replace("<Icon name=\"arrow-right\"", "<Icon name=\"star\"");
    assert_eq!(typing.source(cx), changed, "only the name's bytes change");
    assert!(!is_shown(&typing, cx, ICON_SEARCH_ID), "the picker closes");
    assert!(selected_text(&typing, cx).is_some_and(|icon| icon.starts_with("<Icon name=\"star\"")), "the icon stays selected");
    assert_eq!(label_of(&typing, cx, field.clone()).as_deref(), Some("star"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE, "one undo step");

    // A click picks too, and Escape writes nothing.
    click_on(&typing, cx, field.clone());
    typing.type_text(cx, "favorite");
    click_on(&typing, cx, icon_id("favorite"));
    assert_eq!(typing.source(cx), SHOWCASE.replace("<Icon name=\"arrow-right\"", "<Icon name=\"favorite\""));
    click_on(&typing, cx, field);
    typing.type_text(cx, "star");
    typing.press(cx, "escape");
    assert!(!is_shown(&typing, cx, ICON_SEARCH_ID));
    assert_eq!(typing.source(cx), SHOWCASE.replace("<Icon name=\"arrow-right\"", "<Icon name=\"favorite\""));
    assert!(selected(&typing, cx).is_some(), "Escape lets go of nothing");
}

#[gpui_kit::test]
fn an_icon_the_app_does_not_have_shows_its_name_alone(cx: &mut TestAppContext) {
    let unknown = SHOWCASE.replace("<Icon name=\"arrow-right\"", "<Icon name=\"no-such-icon\"");
    let typing = open(cx, &unknown);
    select_icon(&typing, cx);
    assert_eq!(label_of(&typing, cx, field_id("name")).as_deref(), Some("no-such-icon"));
    assert_eq!(typing.source(cx), unknown);
}
