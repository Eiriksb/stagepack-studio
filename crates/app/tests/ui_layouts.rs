//! The layout picker: the Home tab's Layout tool and the New slide arrow show a tile for each
//! layout, and the list of slides' menu names them; a pick gives the slide that layout.

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{TestAppContext, px, size};
use slides_engine::layouts;
use stagepack_studio::layout_picker::{layout_area_id, layout_tile_id};
use stagepack_studio::ribbon::{LAYOUT_ID, NEW_SLIDE_LAYOUTS_ID};
use stagepack_studio::selection::Thing;
use stagepack_studio::slide_strip::strip_row_id;

const TALK: &str = "---\ntitle: T\n---\n\n# One\n\nFirst words\n\n---\nlayout: two-columns\n---\n\n# Two\n\nleft text\n\n\
<Region name=\"right\" />\n\nright side\n\n---\n\n# Three\n";

/// Each layout, by the engine's name, and the name a tile shows.
const NAMES: [(&str, &str); 5] = [
    ("title", "Title"),
    ("content", "Title and content"),
    ("two-columns", "Two columns"),
    ("section", "Section"),
    ("blank", "Blank"),
];

/// Opens `source` in a window wide enough that the Home tab's Slides tools all show.
fn open_wide(cx: &mut TestAppContext, source: &str) -> Typing {
    let typing = open(cx, source);
    cx.simulate_window_resize(typing.handle, size(px(1800.), px(900.)));
    typing.settle(cx);
    typing
}

/// Opens the Layout tool's tiles.
fn open_picker(typing: &Typing, cx: &mut TestAppContext) {
    click_on(typing, cx, LAYOUT_ID);
}

/// Gives the current slide the layout `layout` from the Layout tool's tiles.
fn pick(typing: &Typing, cx: &mut TestAppContext, layout: &str) {
    open_picker(typing, cx);
    click_on(typing, cx, layout_tile_id(layout));
}

fn shown(typing: &Typing, cx: &mut TestAppContext, layout: &str) -> bool {
    bounds_of(typing, cx, layout_tile_id(layout)).is_some()
}

/// Which tiles are marked as the current layout.
fn marked(typing: &Typing, cx: &mut TestAppContext) -> Vec<&'static str> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        NAMES
            .iter()
            .filter(|(layout, _)| window.find(layout_tile_id(layout)).selected() == Some(true))
            .map(|(layout, _)| *layout)
            .collect()
    })
}

/// The layout slide `slide`'s settings name, if any.
fn layout_of(typing: &Typing, cx: &TestAppContext, slide: usize) -> Option<String> {
    typing.document(cx).read_with(cx, |document, _| {
        document.talk().slides[slide]
            .settings
            .as_ref()
            .and_then(|settings| settings.get("layout"))
            .map(str::to_string)
    })
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn picked(typing: &Typing, cx: &TestAppContext) -> Option<Vec<Thing>> {
    typing.document(cx).read_with(cx, |document, _| document.selection().map(|selection| selection.things.clone()))
}

fn list_has_keyboard(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        let view = typing.view.clone();
        view.read(cx).slide_strip().read(cx).focus_handle().contains_focused(window, cx)
    })
}

/// The flow of the current slide that holds `words`, and the area it's drawn in.
fn area_holding(typing: &Typing, cx: &mut TestAppContext, words: &str) -> Option<String> {
    let slide = typing.current(cx);
    let layout = typing.layout(cx);
    let flows = typing.flows(cx, slide).len();
    (0..flows)
        .find(|&flow| typing.text_of(cx, slide, flow).iter().any(|line| line.contains(words)))
        .and_then(|flow| layout.region_of_flow(flow).map(str::to_string))
}

/// Selects the main text's area with a press on its frame, once typing in it shows the frame.
fn select_body(typing: &Typing, cx: &mut TestAppContext) {
    let slide = typing.current(cx);
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, slide).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text");
    typing.click_text(cx, body, pos(0, 3));
    let rect = typing.layout(cx).region("body").expect("the area").rect;
    let at = typing.at(cx, rect.x, rect.y + rect.h / 2.);
    typing.click(cx, at);
    assert_eq!(picked(typing, cx), Some(vec![Thing::Region("body".to_string())]));
}

#[gpui_kit::test]
fn the_picker_shows_five_tiles_and_marks_the_current_one(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    for (layout, _) in NAMES {
        assert!(!shown(&typing, cx, layout), "no tiles before the tool is pressed");
    }
    open_picker(&typing, cx);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        for (layout, name) in NAMES {
            let tile = window.find(layout_tile_id(layout));
            assert_eq!(tile.label(), Some(name), "{layout}");
            assert!(tile.visible(), "{layout}");
        }
    });
    // Left to right in the engine's order.
    let lefts: Vec<_> = NAMES.iter().map(|(layout, _)| bounds_of(&typing, cx, layout_tile_id(layout)).unwrap().left()).collect();
    assert!(lefts.windows(2).all(|pair| pair[0] < pair[1]), "{lefts:?}");
    assert_eq!(marked(&typing, cx), ["content"]);
    typing.press(cx, "escape");
    assert!(!shown(&typing, cx, "content"), "Escape closes the tiles");

    go_to(&typing, cx, 1);
    open_picker(&typing, cx);
    assert_eq!(marked(&typing, cx), ["two-columns"]);
}

#[gpui_kit::test]
fn picking_two_columns_writes_the_setting_and_nothing_else(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    pick(&typing, cx, "two-columns");
    assert_eq!(typing.source(cx), TALK.replacen("title: T\n", "title: T\nlayout: two-columns\n", 1));
    assert!(!shown(&typing, cx, "two-columns"), "a pick closes the tiles");
    assert_eq!(typing.status(cx).as_deref(), Some("Layout changed to Two columns."));
    assert_eq!(typing.current(cx), 0);
}

#[gpui_kit::test]
fn picking_title_and_content_removes_the_setting(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    go_to(&typing, cx, 1);
    pick(&typing, cx, "content");
    assert_eq!(typing.source(cx), TALK.replacen("---\nlayout: two-columns\n---\n", "---\n", 1));
    assert_eq!(layout_of(&typing, cx, 1), None);
    assert_eq!(typing.status(cx).as_deref(), Some("Layout changed to Title and content."));
}

#[gpui_kit::test]
fn the_current_layout_again_makes_no_step(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    pick(&typing, cx, "content");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
    assert_eq!(typing.status(cx), None);

    go_to(&typing, cx, 1);
    pick(&typing, cx, "two-columns");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn it_is_one_undo_step(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    pick(&typing, cx, "section");
    assert_eq!(undo_label(&typing, cx), Some("Change layout"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx), "one step took it all back");
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(layout_of(&typing, cx, 0).as_deref(), Some("section"));
}

#[gpui_kit::test]
fn a_moved_area_keeps_its_place_through_a_change_and_back(cx: &mut TestAppContext) {
    let talk = TALK.replacen("title: T\n", "title: T\nregion.body: x=300 y=400\n", 1);
    let typing = open_wide(cx, &talk);
    let before = typing.layout(cx).region("body").expect("the area").rect;
    assert_eq!((before.x, before.y), (300., 400.));
    pick(&typing, cx, "two-columns");
    assert!(typing.source(cx).contains("region.body: x=300 y=400\n"), "{}", typing.source(cx));
    assert!(typing.layout(cx).region("body").is_none(), "two columns has no such area");
    pick(&typing, cx, "content");
    assert_eq!(typing.source(cx), talk);
    assert_eq!(typing.layout(cx).region("body").expect("the area").rect, before);
}

#[gpui_kit::test]
fn text_in_the_right_column_shows_in_the_main_area_under_title_and_content(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    go_to(&typing, cx, 1);
    assert_eq!(area_holding(&typing, cx, "right side").as_deref(), Some("right"));
    pick(&typing, cx, "content");
    let source = typing.source(cx);
    assert!(source.contains("<Region name=\"right\" />\n\nright side\n"), "nothing lost: {source}");
    assert_eq!(area_holding(&typing, cx, "right side").as_deref(), Some("body"));
    assert_eq!(area_holding(&typing, cx, "left text").as_deref(), Some("body"));
}

#[gpui_kit::test]
fn new_slide_from_a_tile_adds_that_layout(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(NEW_SLIDE_LAYOUTS_ID).click("popup", cx);
    });
    typing.settle(cx);
    assert!(NAMES.iter().all(|(layout, _)| shown(&typing, cx, layout)));
    assert!(marked(&typing, cx).is_empty(), "a new slide has no layout yet");
    click_on(&typing, cx, layout_tile_id("section"));
    assert_eq!(typing.slide_count(cx), 4);
    assert_eq!(typing.current(cx), 1);
    assert_eq!(layout_of(&typing, cx, 1).as_deref(), Some("section"));
    assert!(!shown(&typing, cx, "section"), "a pick closes the tiles");
}

#[gpui_kit::test]
fn the_slide_lists_menu_changes_the_layout(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.right_click(strip_row_id(2), cx);
    });
    typing.settle(cx);
    assert_eq!(typing.current(cx), 2, "the row's slide is the one in hand");
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(11usize).label(), Some("Layout"));
        menu.click(11usize, cx);
        window.render_frame(cx);
        let mut open = window.within("submenu");
        let submenu = open.within("popup-menu");
        let names: Vec<_> = (0..NAMES.len()).map(|item| submenu.find(item).label().unwrap_or_default().to_string()).collect();
        assert_eq!(names, NAMES.map(|(_, name)| name));
        assert!(submenu.try_find(NAMES.len()).is_none(), "nothing else");
    });
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within("submenu").within("popup-menu").click(4usize, cx);
    });
    typing.settle(cx);
    assert_eq!(layout_of(&typing, cx, 2).as_deref(), Some("blank"));
    assert_eq!(typing.status(cx).as_deref(), Some("Layout changed to Blank."));
    assert!(list_has_keyboard(&typing, cx), "the keyboard stays with the list");
    assert_eq!(picked(&typing, cx), None);
}

#[gpui_kit::test]
fn a_talk_with_an_unknown_layout_marks_no_tile(cx: &mut TestAppContext) {
    let talk = TALK.replacen("title: T\n", "title: T\nlayout: fancy\n", 1);
    let typing = open_wide(cx, &talk);
    open_picker(&typing, cx);
    assert!(marked(&typing, cx).is_empty());
    click_on(&typing, cx, layout_tile_id("content"));
    assert_eq!(typing.source(cx), TALK, "the name that isn't a layout's goes");
    assert_eq!(undo_label(&typing, cx), Some("Change layout"));
}

#[gpui_kit::test]
fn the_tiles_draw_the_layouts_areas(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    open_picker(&typing, cx);
    for layout in layouts() {
        let tile = bounds_of(&typing, cx, layout_tile_id(layout.name)).expect("the tile");
        let drawn: Vec<_> = (0..)
            .map_while(|index| bounds_of(&typing, cx, layout_area_id(layout.name, index)))
            .collect();
        assert_eq!(drawn.len(), layout.regions.len(), "{}", layout.name);
        for area in &drawn {
            assert!(tile.contains(&area.origin) && area.size.width > px(0.) && area.size.height > px(0.), "{}: {area:?} in {tile:?}", layout.name);
        }
    }
    // Two columns side by side, under the title.
    let mut area = |index| bounds_of(&typing, cx, layout_area_id("two-columns", index)).unwrap();
    let (title, left, right) = (area(0), area(1), area(2));
    assert!(title.bottom() <= left.top() && left.right() <= right.left(), "{title:?} {left:?} {right:?}");
    assert!((left.size.width - right.size.width).abs() < px(1.));
}

#[gpui_kit::test]
fn a_selected_area_stays_selected_where_the_new_layout_has_it(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    select_body(&typing, cx);
    pick(&typing, cx, "blank");
    assert_eq!(layout_of(&typing, cx, 0).as_deref(), Some("blank"));
    assert_eq!(picked(&typing, cx), Some(vec![Thing::Region("body".to_string())]), "blank has a main area too");
    pick(&typing, cx, "two-columns");
    assert_eq!(layout_of(&typing, cx, 0).as_deref(), Some("two-columns"));
    assert_eq!(picked(&typing, cx), None, "two columns has no area of that name");
}

#[gpui_kit::test]
fn typing_in_an_area_the_new_layout_lacks_ends_and_loses_nothing(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    let layout = typing.layout(cx);
    let body = (0..typing.flows(cx, 0).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text");
    typing.click_text(cx, body, pos(0, 5));
    typing.type_text(cx, " typed");
    assert!(typing.editing(cx).is_some());
    pick(&typing, cx, "two-columns");
    assert!(typing.editing(cx).is_none(), "the typing ended");
    let source = typing.source(cx);
    assert!(source.contains("First typed words"), "{source}");
    assert_eq!(layout_of(&typing, cx, 0).as_deref(), Some("two-columns"));
    assert_eq!(area_holding(&typing, cx, "First typed words").as_deref(), Some("left"));
}

#[gpui_kit::test]
fn with_the_list_holding_the_keyboard_nothing_becomes_selected(cx: &mut TestAppContext) {
    let typing = open_wide(cx, TALK);
    click_on(&typing, cx, strip_row_id(0));
    assert!(list_has_keyboard(&typing, cx));
    pick(&typing, cx, "title");
    assert_eq!(layout_of(&typing, cx, 0).as_deref(), Some("title"));
    assert!(list_has_keyboard(&typing, cx), "the keyboard is where it was");
    assert_eq!(picked(&typing, cx), None);
    assert!(typing.editing(cx).is_none());
}
