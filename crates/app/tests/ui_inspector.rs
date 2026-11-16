//! The inspector: the form at the window's right for the thing selected on the slide.

use std::ops::Range;
use std::path::Path;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::component::color_picker::ColorPickerState;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext as _, Entity, Hsla, TestAppContext, rgb};
use slides_engine::ElementBox;
use slides_format::{find_tag, parse_talk};
use stagepack_studio::inspector::{
    INSPECTOR_CHOOSE_FILE_ID, INSPECTOR_HEADING_ID, INSPECTOR_ID, INSPECTOR_MESSAGE_ID, Inspector, field_id, none_id,
    problem_id,
};
use stagepack_studio::recovery::Recovery;
use stagepack_studio::workspace_view::RECOVER_ID;

const SHOWCASE: &str = include_str!("../../../fixtures/showcase/slides.md");
/// The showcase's slide of shapes, an icon, a picture and a table.
const THINGS: usize = 5;
/// The showcase's slide with a box of words and nothing else.
const BOX: usize = 4;
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";
const ICON: &str = "<Icon";

const BOTH_QUOTES: &str = "That can't hold both kinds of quote mark or a new line.";
const NOT_WHOLE: &str = "That has to be a whole number.";
const NEEDS_BUILD: &str = "Build this talk's components to set this up.";
const NO_SUCH_COMPONENT: &str = "This talk has no component of that name.";

/// A chart on a slide of its own, placed.
const CHART: &str = "# Sales\n\n<Chart kind=\"bar\" labels=\"A, B\" values=\"1, 2\" title=\"Sales\" x=\"300\" y=\"300\" w=\"800\" h=\"500\" />\n";

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

/// Clicks the thing on the current slide whose source starts with `start`, which selects it.
fn select(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Range<usize> {
    let element = thing(typing, cx, start);
    let rect = element.rect;
    let at = typing.at(cx, rect.x + rect.w / 2., rect.y + rect.h.max(1.) / 2.);
    typing.click(cx, at);
    let selected = selected(typing, cx);
    assert_eq!(selected.as_ref(), Some(&element.source), "{start} is selected");
    element.source
}

fn selected(typing: &Typing, cx: &TestAppContext) -> Option<Range<usize>> {
    typing
        .document(cx)
        .read_with(cx, |document, _| document.selected().map(|selected| selected.source.clone()))
}

fn inspector(typing: &Typing, cx: &TestAppContext) -> Entity<Inspector> {
    cx.read(|cx| typing.view.read(cx).inspector().clone())
}

fn panel_shows(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(INSPECTOR_ID).is_some()
    })
}

/// The words of the element `id` of the panel, if it's there.
fn label_of(typing: &Typing, cx: &mut TestAppContext, id: &str) -> Option<String> {
    let id = id.to_string();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window
            .try_find(gpui_kit::SharedString::from(id))
            .and_then(|found| found.label().map(str::to_string))
    })
}

/// What field `name` shows.
fn shown(typing: &Typing, cx: &mut TestAppContext, name: &str) -> Option<String> {
    typing.settle(cx);
    let inspector = inspector(typing, cx);
    cx.read(|cx| inspector.read(cx).shown(name, cx))
}

fn labels(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    typing.settle(cx);
    let inspector = inspector(typing, cx);
    cx.read(|cx| inspector.read(cx).labels())
}

/// Gives field `name` the keyboard, as a click on it does.
fn click_field(typing: &Typing, cx: &mut TestAppContext, name: &str) {
    let id = gpui_kit::SharedString::from(field_id(name));
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    });
    typing.settle(cx);
}

/// Types `text` over what field `name` holds, without committing it.
fn type_over(typing: &Typing, cx: &mut TestAppContext, name: &str, text: &str) {
    click_field(typing, cx, name);
    typing.press(cx, "ctrl-a");
    if text.is_empty() {
        typing.press(cx, "backspace");
    } else {
        typing.type_text(cx, text);
    }
}

/// Types `text` over what field `name` holds, and presses Enter.
fn enter(typing: &Typing, cx: &mut TestAppContext, name: &str, text: &str) {
    type_over(typing, cx, name, text);
    typing.press(cx, "enter");
}

fn picker(typing: &Typing, cx: &mut TestAppContext, name: &str) -> Entity<ColorPickerState> {
    typing.settle(cx);
    let inspector = inspector(typing, cx);
    cx.read(|cx| inspector.read(cx).color_picker(name).expect("a colour field"))
}

/// Picks `color` in field `name`'s picker, as a click on one of its swatches does.
fn pick(typing: &Typing, cx: &mut TestAppContext, name: &str, color: Hsla) {
    let picker = picker(typing, cx, name);
    typing.with_window(cx, |window, cx| picker.update(cx, |picker, cx| picker.select_color(color, window, cx)));
    typing.settle(cx);
}

/// Chooses `value` in the choice field `name`, as picking it from the list does.
fn choose(typing: &Typing, cx: &mut TestAppContext, name: &str, value: &str) {
    let inspector = inspector(typing, cx);
    typing.with_window(cx, |window, cx| inspector.update(cx, |inspector, cx| inspector.choose(name, value, window, cx)));
    typing.settle(cx);
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

/// Whether field `name` has the keyboard.
fn has_keyboard(typing: &Typing, cx: &mut TestAppContext, name: &str) -> bool {
    let inspector = inspector(typing, cx);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        inspector.read(cx).has_keyboard(name, window, cx)
    })
}

/// The value of attribute `name` of the tag selected.
fn attr(typing: &Typing, cx: &TestAppContext, name: &str) -> Option<String> {
    let source = selected(typing, cx).expect("a thing is selected");
    let slide = typing.current(cx);
    let talk = parse_talk(&typing.source(cx));
    find_tag(&talk, slide, &source).and_then(|tag| tag.attr(name).map(str::to_string))
}

/// A folder holding `source`, with the talk's own `Score` built into it: a team's name, its
/// points, whether it's ready, its colour, a note, a kind of bar and a logo.
fn talk_with_score(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("build/score.wasm"), score_module()).unwrap();
    let index = r#"{"format":1,"components":[{"tag":"Score","file":"score","source_hash":"built elsewhere"}]}"#;
    std::fs::write(dir.path().join("build/index.json"), index).unwrap();
    dir
}

/// A component called `Score` that draws the word "score", whatever it's given.
fn score_module() -> Vec<u8> {
    let schema = r#"{"name":"Score","props":[{"name":"team","kind":"Text","optional":false},{"name":"points","kind":"Whole","optional":false},{"name":"ready","kind":"Flag","optional":false},{"name":"colour","kind":"Color","optional":false},{"name":"note","kind":"Text","optional":true},{"name":"bar_kind","kind":{"Choice":["bar","line-chart"]},"optional":false},{"name":"logo","kind":{"File":["png"]},"optional":true}],"children":false}"#;
    let answer = r#"{"element":{"Text":{"text":"score","size":null,"bold":false,"color":null}}}"#;
    let packed = |at: u64, text: &str| ((at << 32) | text.len() as u64) as i64;
    let data = |at: u32, text: &str| {
        let escaped: String = text.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        format!(r#"(data (i32.const {at}) "{escaped}")"#)
    };
    let text = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const 262144))
  {}
  {}
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64) (i64.const {})))"#,
        data(1024, schema),
        data(8192, answer),
        packed(1024, schema),
        packed(8192, answer),
    );
    wat::parse_str(&text).unwrap()
}

const SCORE: &str = "# Scores\n\n<Score team=\"Oslo\" points=\"3\" ready=\"true\" colour=\"#16a34a\" bar-kind=\"bar\" x=\"100\" y=\"300\" w=\"600\" h=\"300\" />\n";

#[gpui_kit::test]
fn the_panel_shows_only_with_a_selection(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    assert!(!panel_shows(&typing, cx), "nothing is selected");
    let wide = typing.slide_bounds(cx);
    select(&typing, cx, ELLIPSE);
    assert!(panel_shows(&typing, cx));
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Shape"));
    let narrow = typing.slide_bounds(cx);
    assert!(narrow.size.width < wide.size.width, "the slide makes room: {wide:?} then {narrow:?}");
    typing.press(cx, "escape");
    assert!(!panel_shows(&typing, cx), "let go of");
    assert_eq!(typing.slide_bounds(cx), wide, "and the slide has its room back");
    select(&typing, cx, ICON);
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Icon"));
    typing.press(cx, "pagedown");
    assert!(!panel_shows(&typing, cx), "the thing selected isn't on the slide shown");
    assert_eq!(typing.source(cx), SHOWCASE);
}

#[gpui_kit::test]
fn a_shape_shows_its_fields_with_its_values(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ELLIPSE);
    assert_eq!(
        labels(&typing, cx),
        ["Shape", "Fill", "Outline", "Outline width", "Left", "Top", "Width", "Height", "Turn"]
    );
    for (name, value) in [
        ("kind", "Ellipse"),
        ("fill", "#ffa500"),
        ("outline", "#000000"),
        ("outline-width", "4"),
        ("x", "700"),
        ("y", "560"),
        ("w", "240"),
        ("h", "200"),
        ("rotate", ""),
    ] {
        assert_eq!(shown(&typing, cx, name).as_deref(), Some(value), "{name}");
    }
    select(&typing, cx, ICON);
    let id = gpui_kit::SharedString::from(field_id("name"));
    let in_field = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(id).value().map(str::to_string)
    });
    assert_eq!(in_field.as_deref(), Some("arrow-right"), "the field itself shows it");

    // A kind the shape doesn't have draws a rectangle, and shows as one.
    let odd = SHOWCASE.replace("kind=\"ellipse\"", "kind=\"star\"");
    let typing = open(cx, &odd);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, "<Shape kind=\"star\"");
    assert_eq!(shown(&typing, cx, "kind").as_deref(), Some("Rectangle"));
    click_field(&typing, cx, "x");
    typing.press(cx, "tab");
    assert_eq!(typing.source(cx), odd, "nothing is written until it's changed");

    // The box on its own slide.
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, BOX);
    select(&typing, cx, "<Box");
    assert_eq!(
        labels(&typing, cx),
        ["Fill", "Outline", "Outline width", "Corner radius", "Padding", "Left", "Top", "Width", "Height", "Turn"]
    );
    assert_eq!(shown(&typing, cx, "radius").as_deref(), Some("32"));
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Text box"));
}

#[gpui_kit::test]
fn changing_a_fill_writes_only_that_attribute(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ELLIPSE);
    pick(&typing, cx, "fill", rgb(0xff0000).into());
    assert_eq!(typing.source(cx), SHOWCASE.replace("fill=\"orange\"", "fill=\"#ff0000\""));
    assert_eq!(shown(&typing, cx, "fill").as_deref(), Some("#ff0000"));
    assert_eq!(undo_label(&typing, cx), Some("Change setting"));

    // A drag of the sliders is one change, given once the picker closes.
    let picker = picker(&typing, cx, "outline");
    typing.with_window(cx, |window, cx| {
        picker.update(cx, |picker, cx| {
            picker.set_open(true, cx);
            picker.update_color(rgb(0x00ff00).into(), window, cx);
            picker.update_color(rgb(0x0000ff).into(), window, cx);
        })
    });
    typing.settle(cx);
    let changed = SHOWCASE.replace("fill=\"orange\"", "fill=\"#ff0000\"");
    assert_eq!(typing.source(cx), changed, "nothing yet, while it's open");
    typing.with_window(cx, |_, cx| picker.update(cx, |picker, cx| picker.set_open(false, cx)));
    typing.settle(cx);
    assert_eq!(typing.source(cx), changed.replace("outline=\"black\"", "outline=\"#0000ff\""));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), changed, "one step for the whole drag");
}

#[gpui_kit::test]
fn emptying_an_optional_field_removes_it(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    enter(&typing, cx, "size", "");
    let no_size = SHOWCASE.replace(" size=\"96\"", "");
    assert_eq!(typing.source(cx), no_size);
    assert_eq!(shown(&typing, cx, "size").as_deref(), Some(""));

    // An optional colour has a button to take it away.
    select(&typing, cx, ELLIPSE);
    let id = gpui_kit::SharedString::from(none_id("fill"));
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    });
    typing.settle(cx);
    assert_eq!(typing.source(cx), no_size.replace(" fill=\"orange\"", ""));

    // A required text field emptied is written empty.
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    enter(&typing, cx, "name", "");
    assert_eq!(typing.source(cx), SHOWCASE.replace("name=\"arrow-right\"", "name=\"\""));
}

#[gpui_kit::test]
fn a_number_out_of_range_is_held_to_the_range(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ELLIPSE);
    enter(&typing, cx, "x", "9000");
    assert_eq!(attr(&typing, cx, "x").as_deref(), Some("7680"));
    assert_eq!(shown(&typing, cx, "x").as_deref(), Some("7680"), "the field shows what was written");
    enter(&typing, cx, "w", "0");
    assert_eq!(attr(&typing, cx, "w").as_deref(), Some("1"));
    enter(&typing, cx, "rotate", "-400");
    assert_eq!(attr(&typing, cx, "rotate").as_deref(), Some("-360"));
    enter(&typing, cx, "h", "12.5");
    assert_eq!(attr(&typing, cx, "h").as_deref(), Some("12.5"), "a fraction is a number");

    // The arrow keys step a number.
    click_field(&typing, cx, "y");
    typing.press(cx, "up");
    assert_eq!(attr(&typing, cx, "y").as_deref(), Some("561"));
}

#[gpui_kit::test]
fn a_fraction_in_a_whole_number_is_refused(cx: &mut TestAppContext) {
    let typing = open_folder(cx, talk_with_score(SCORE), None);
    select(&typing, cx, "<Score");
    assert_eq!(labels(&typing, cx)[..7], ["Team", "Points", "Ready", "Colour", "Note", "Bar kind", "Logo"]);
    enter(&typing, cx, "points", "2.5");
    assert_eq!(typing.source(cx), SCORE, "nothing written");
    assert_eq!(label_of(&typing, cx, &problem_id("points")).as_deref(), Some(NOT_WHOLE));
    assert_eq!(shown(&typing, cx, "points").as_deref(), Some("2.5"), "what was typed stays");
    enter(&typing, cx, "points", "4");
    assert_eq!(typing.source(cx), SCORE.replace("points=\"3\"", "points=\"4\""));
    assert_eq!(label_of(&typing, cx, &problem_id("points")), None);
}

#[gpui_kit::test]
fn a_choice_writes_its_kebab_name(cx: &mut TestAppContext) {
    let typing = open_folder(cx, talk_with_score(SCORE), None);
    select(&typing, cx, "<Score");
    assert_eq!(shown(&typing, cx, "bar-kind").as_deref(), Some("Bar"));
    choose(&typing, cx, "bar-kind", "line-chart");
    assert_eq!(typing.source(cx), SCORE.replace("bar-kind=\"bar\"", "bar-kind=\"line-chart\""));
    assert_eq!(shown(&typing, cx, "bar-kind").as_deref(), Some("Line chart"));

    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ELLIPSE);
    choose(&typing, cx, "kind", "rounded");
    assert_eq!(typing.source(cx), SHOWCASE.replace("kind=\"ellipse\"", "kind=\"rounded\""));
}

#[gpui_kit::test]
fn a_checkbox_writes_at_once_and_undoes_in_one_step(cx: &mut TestAppContext) {
    let typing = open(cx, CHART);
    select(&typing, cx, "<Chart");
    assert_eq!(shown(&typing, cx, "build").as_deref(), Some("false"));
    click_field(&typing, cx, "build");
    let built = CHART.replace(" />", " build=\"true\" />");
    assert_eq!(typing.source(cx), built);
    assert_eq!(shown(&typing, cx, "build").as_deref(), Some("true"));
    click_field(&typing, cx, "build");
    assert_eq!(typing.source(cx), CHART.replace(" />", " build=\"false\" />"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), built);
    assert_eq!(shown(&typing, cx, "build").as_deref(), Some("true"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), CHART);
    assert!(!can_undo(&typing, cx));
    assert!(selected(&typing, cx).is_some(), "the chart stays selected");
}

#[gpui_kit::test]
fn escape_puts_the_old_value_back(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    type_over(&typing, cx, "name", "star");
    assert_eq!(shown(&typing, cx, "name").as_deref(), Some("star"));
    typing.press(cx, "escape");
    assert_eq!(shown(&typing, cx, "name").as_deref(), Some("arrow-right"));
    assert!(selected(&typing, cx).is_some(), "Escape in a field lets go of nothing");
    typing.press(cx, "tab");
    assert_eq!(typing.source(cx), SHOWCASE, "and leaving the field writes nothing");
}

#[gpui_kit::test]
fn a_value_with_both_quotes_says_so_and_writes_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, CHART);
    select(&typing, cx, "<Chart");
    enter(&typing, cx, "title", "Bob's \"best\"");
    assert_eq!(typing.source(cx), CHART);
    assert_eq!(shown(&typing, cx, "title").as_deref(), Some("Bob's \"best\""));
    assert_eq!(label_of(&typing, cx, &problem_id("title")).as_deref(), Some(BOTH_QUOTES));
    enter(&typing, cx, "title", "Bob's best");
    assert_eq!(attr(&typing, cx, "title").as_deref(), Some("Bob's best"));
    assert_eq!(label_of(&typing, cx, &problem_id("title")), None, "the sentence goes");
}

#[gpui_kit::test]
fn anything_typed_in_a_text_field_reads_back_or_is_refused(cx: &mut TestAppContext) {
    let source = format!("{CHART}\n---\n\n# After\n\nWords\n");
    let typing = open(cx, &source);
    select(&typing, cx, "<Chart");
    let after = |text: &str| parse_talk(text).slides[1].range.clone();
    for typed in [
        "\"", "'", "\"'", "<", "/>", "{", "|", "a\nb", "---", "`", "&quot;", "a\u{0}b", "<b>x</b>", "&amp; &", "}{",
    ] {
        let before = typing.source(cx);
        type_over(&typing, cx, "title", typed);
        let in_field = shown(&typing, cx, "title").unwrap();
        typing.press(cx, "enter");
        let now = typing.source(cx);
        let talk = parse_talk(&now);
        assert_eq!(talk.slides.len(), 2, "{typed:?}");
        assert!(talk.slides.iter().all(|slide| slide.error.is_none()), "{typed:?}");
        assert_eq!(now[after(&now)], before[after(&before)], "{typed:?}: the other slide");
        if now == before {
            assert_eq!(label_of(&typing, cx, &problem_id("title")).as_deref(), Some(BOTH_QUOTES), "{typed:?}");
        } else {
            assert_eq!(attr(&typing, cx, "title").as_deref(), Some(in_field.as_str()), "{typed:?}");
        }
    }
}

#[gpui_kit::test]
fn undo_updates_the_field(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    enter(&typing, cx, "size", "120");
    assert_eq!(attr(&typing, cx, "size").as_deref(), Some("120"));
    // The keyboard goes back to the slide, where Ctrl+Z is the talk's.
    select(&typing, cx, ICON);
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOWCASE);
    assert_eq!(shown(&typing, cx, "size").as_deref(), Some("96"));
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(shown(&typing, cx, "size").as_deref(), Some("120"));
}

#[gpui_kit::test]
fn a_field_with_the_keyboard_is_not_overwritten_by_an_outside_change(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    type_over(&typing, cx, "name", "sta");
    let outside = SHOWCASE.replace("size=\"96\"", "size=\"80\"");
    typing.view.update(cx, |view, cx| view.reload_for_test(Some(outside.clone()), cx));
    typing.settle(cx);
    assert_eq!(typing.source(cx), outside);
    assert_eq!(shown(&typing, cx, "name").as_deref(), Some("sta"), "what's being typed stays");
    assert_eq!(shown(&typing, cx, "size").as_deref(), Some("80"), "the other fields follow");
    assert!(has_keyboard(&typing, cx, "name"));
    typing.press(cx, "enter");
    assert_eq!(typing.source(cx), outside.replace("name=\"arrow-right\"", "name=\"sta\""));
}

#[gpui_kit::test]
fn keys_in_a_field_stay_in_the_field(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    let icon = select(&typing, cx, ICON);
    click_field(&typing, cx, "name");
    for key in ["backspace", "delete", "left", "right", "pagedown", "ctrl-m", "ctrl-d"] {
        typing.press(cx, key);
    }
    typing.type_text(cx, "x");
    assert_eq!(typing.source(cx), SHOWCASE, "nothing is typed on the slide or taken away");
    assert_eq!(selected(&typing, cx), Some(icon), "and the selection stays");
    assert_eq!(typing.current(cx), THINGS);
    assert!(typing.editing(cx).is_none());
    typing.press(cx, "tab");
    assert!(has_keyboard(&typing, cx, "size"), "Tab goes on to the next field");
    typing.press(cx, "shift-tab");
    assert!(has_keyboard(&typing, cx, "name"), "and Shift+Tab back");
}

#[gpui_kit::test]
fn a_component_with_no_build_asks_for_a_build(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/components");
    std::fs::write(dir.path().join("slides.md"), std::fs::read(fixture.join("slides.md")).unwrap()).unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    std::fs::copy(fixture.join("components/score.rs"), dir.path().join("components/score.rs")).unwrap();
    let typing = open_folder(cx, dir, None);
    go_to(&typing, cx, 1);
    select(&typing, cx, "<Score team=\"Oslo\"");
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Score"));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some(NEEDS_BUILD));
    assert!(labels(&typing, cx).is_empty(), "no fields");
}

#[gpui_kit::test]
fn an_unknown_tag_says_so(cx: &mut TestAppContext) {
    let source = "# T\n\n<StatCard x=\"100\" y=\"300\" w=\"300\" h=\"200\" />\n";
    let typing = open(cx, source);
    select(&typing, cx, "<StatCard");
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Stat Card"));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some(NO_SUCH_COMPONENT));
}

#[gpui_kit::test]
fn a_table_and_a_written_picture_say_what_they_are(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, "| Layout");
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Table"));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("2 columns, 3 rows"));

    let source = "# T\n\n![](assets/photo.png)\n";
    let typing = open(cx, source);
    let folder = typing.file.parent().unwrap().to_path_buf();
    std::fs::create_dir(folder.join("assets")).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase/assets/photo.png"), folder.join("assets/photo.png")).unwrap();
    let typing = open_folder(cx, { let Typing { _folder, .. } = typing; _folder }, None);
    select(&typing, cx, "![]");
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Picture"));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("photo.png"));
    assert!(labels(&typing, cx).is_empty(), "nothing to change");
}

#[gpui_kit::test]
fn place_fields_move_the_thing(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ELLIPSE);
    enter(&typing, cx, "x", "800");
    enter(&typing, cx, "w", "300");
    assert_eq!(typing.source(cx), SHOWCASE.replace("x=\"700\" y=\"560\" w=\"240\"", "x=\"800\" y=\"560\" w=\"300\""));
    let outline = typing.document(cx).update(cx, |document, _| document.selected_box()).expect("still selected");
    assert_eq!((outline.rect.x, outline.rect.w), (800., 300.), "the outline follows");
    let drawn = thing(&typing, cx, ELLIPSE);
    assert_eq!(drawn.rect, outline.rect);
}

#[gpui_kit::test]
fn a_chart_shows_plain_labels(cx: &mut TestAppContext) {
    let source = "# T\n\n<Chart kind=\"pie\" labels=\"A, B\" values=\"1, 2\" />\n";
    let typing = open(cx, source);
    select(&typing, cx, "<Chart");
    assert_eq!(labels(&typing, cx), ["Kind", "Title", "Labels", "Values", "Data file", "Build up by clicks"]);
    assert_eq!(shown(&typing, cx, "kind").as_deref(), Some("Pie"));
    assert_eq!(shown(&typing, cx, "labels").as_deref(), Some("A, B"));
    assert_eq!(shown(&typing, cx, "data").as_deref(), Some(""));
    assert_eq!(label_of(&typing, cx, INSPECTOR_CHOOSE_FILE_ID).as_deref(), Some("Choose file…"));
}

#[gpui_kit::test]
fn the_same_value_makes_no_undo_step(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    enter(&typing, cx, "size", "96");
    enter(&typing, cx, "x", "100.0");
    assert_eq!(typing.source(cx), SHOWCASE);
    select(&typing, cx, ELLIPSE);
    choose(&typing, cx, "kind", "ellipse");
    pick(&typing, cx, "outline", rgb(0x000000).into());
    assert_eq!(typing.source(cx), SHOWCASE, "a colour named by hand stays as it's written");
    assert!(!can_undo(&typing, cx));
    pick(&typing, cx, "outline", rgb(0x111111).into());
    assert_eq!(typing.source(cx), SHOWCASE.replace("outline=\"black\"", "outline=\"#111111\""), "until another is picked");
}

#[gpui_kit::test]
fn a_click_elsewhere_commits_a_field_to_its_own_thing(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, THINGS);
    select(&typing, cx, ICON);
    type_over(&typing, cx, "size", "120");
    let ellipse = select(&typing, cx, ELLIPSE);
    let sized = SHOWCASE.replace("size=\"96\"", "size=\"120\"");
    assert_eq!(typing.source(cx), sized, "the icon's size, not the ellipse's");
    assert_eq!(selected(&typing, cx), Some(ellipse));
    assert_eq!(labels(&typing, cx)[0], "Shape", "the form is the ellipse's");

    // A click on the desk around the slide commits too, and lets go.
    type_over(&typing, cx, "outline-width", "6");
    let desk = typing.slide_bounds(cx).origin - gpui_kit::point(gpui_kit::px(4.), gpui_kit::px(4.));
    typing.click(cx, desk);
    assert_eq!(typing.source(cx), sized.replace("outline-width=\"4\"", "outline-width=\"6\""));
    assert!(!panel_shows(&typing, cx));
}

/// A CSV file outside any talk.
fn outside_csv() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Q3 Sales.csv");
    std::fs::write(&path, "month,sales\nJuly,10\nAugust,12\n").unwrap();
    (dir, path)
}

/// Chooses `path` for the file field `name` through its button and the system's dialog.
fn choose_file(typing: &Typing, cx: &mut TestAppContext, name: &str, path: &Path) {
    let field = gpui_kit::SharedString::from(field_id(name));
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(field).click(INSPECTOR_CHOOSE_FILE_ID, cx);
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths(), "the dialog is asked for");
    let path = path.to_path_buf();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![path])
    });
    cx.run_until_parked();
    typing.settle(cx);
}

#[gpui_kit::test]
fn choosing_a_data_file_adds_it_and_writes_its_path(cx: &mut TestAppContext) {
    let (_outside, csv) = outside_csv();
    let typing = open(cx, CHART);
    select(&typing, cx, "<Chart");
    choose_file(&typing, cx, "data", &csv);
    assert_eq!(attr(&typing, cx, "data").as_deref(), Some("assets/q3-sales.csv"));
    assert_eq!(shown(&typing, cx, "data").as_deref(), Some("q3-sales.csv"), "its name, not where it's kept");
    let has = typing.document(cx).read_with(cx, |document, _| document.has_file("assets/q3-sales.csv"));
    assert!(has);
    assert_eq!(undo_label(&typing, cx), Some("Change setting"));

    // A file of another kind is refused, and says so.
    let picture = csv.with_file_name("photo.png");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase/assets/photo.png"), &picture).unwrap();
    let before = typing.source(cx);
    let inspector = inspector(&typing, cx);
    typing.with_window(cx, |_, cx| inspector.update(cx, |inspector, cx| inspector.chosen_file("data", &picture, cx)));
    typing.settle(cx);
    assert_eq!(typing.source(cx), before);
    assert_eq!(label_of(&typing, cx, &problem_id("data")).as_deref(), Some("That kind of file can't be used here."));
}

#[gpui_kit::test]
fn a_file_chosen_in_the_inspector_survives_a_crash(cx: &mut TestAppContext) {
    let app_data = tempfile::tempdir().unwrap();
    let recovery = Recovery::new(app_data.path().join("recovery"));
    let (_outside, csv) = outside_csv();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), CHART).unwrap();
    let typing = open_folder(cx, dir, Some(recovery.clone()));
    select(&typing, cx, "<Chart");
    choose_file(&typing, cx, "data", &csv);
    wait(cx, 1_000);
    let edited = typing.source(cx);
    let Typing { _folder: folder, handle, view, .. } = typing;
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    assert!(!folder.path().join("assets").exists(), "nothing on disk until Save");

    let typing = open_folder(cx, folder, Some(recovery));
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(RECOVER_ID, cx);
    });
    typing.settle(cx);
    assert_eq!(typing.source(cx), edited);
    let has = typing.document(cx).read_with(cx, |document, _| document.has_file("assets/q3-sales.csv"));
    assert!(has, "the file came back with the text");
}

#[gpui_kit::test]
fn a_double_click_still_types_in_a_box_with_the_panel_appearing(cx: &mut TestAppContext) {
    let typing = open(cx, SHOWCASE);
    go_to(&typing, cx, BOX);
    let flow = 0;
    let at = pos(0, 5);
    let before = typing.slide_bounds(cx);
    typing.double_click_text(cx, flow, at);
    assert_ne!(typing.slide_bounds(cx), before, "the panel came, and the slide made room");
    let editing = typing.editing(cx).expect("the second click types in the box");
    assert_eq!(editing.edit.selection, caret_at(at), "where it was clicked");
    assert!(panel_shows(&typing, cx), "the box stays selected while it's typed in");
}
