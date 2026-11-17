//! The component palette: the Insert tab's Component tool, which puts a chart or one of the
//! talk's own components on the slide, set up to draw at once.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{App, AppContext as _, Bounds, ElementId, Entity, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use slides_components::source_hash;
use slides_engine::{ComponentState, Fonts};
use slides_format::{FlowPlace, TextEdit, find_tag, parse_talk};
use stagepack_studio::builds::Compile;
use stagepack_studio::document::{Document, PaletteEntry};
use stagepack_studio::insert::hex;
use stagepack_studio::insert_tools::INSERT_COMPONENT_ID;
use stagepack_studio::inspector::{INSPECTOR_HEADING_ID, Inspector};
use stagepack_studio::palette::{
    self, NEEDS_BUILDING_FIRST, NO_COMPONENTS_ID, NO_COMPONENTS_OF_ITS_OWN, OUT_OF_DATE, STILL_LOADING, default_attrs, entry_id,
    note_id,
};
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::ribbon::{MORE_RIGHT_ID, TAB_INSERT_ID};
use stagepack_studio::slide_strip::strip_row_id;
use stagepack_studio::workspace_view::{self, WorkspaceView};
use toolkit::{ComponentSchema, PropField, PropKind, Theme};

const SHOW: &str = include_str!("../../../fixtures/show/slides.md");

/// Where a new component goes on an empty slide, and how big it is.
const PLACE: &str = "x=\"640\" y=\"360\" w=\"640\" h=\"360\"";

// The talk's own components, as modules that answer with fixed JSON.

/// Whether `$len` bytes at `$ptr` hold the `$nlen` bytes at `$needle`.
const CONTAINS: &str = r#"
  (func $contains (param $ptr i32) (param $len i32) (param $needle i32) (param $nlen i32) (result i32)
    (local $i i32) (local $j i32)
    (block $absent
      (loop $outer
        (br_if $absent (i32.gt_u (i32.add (local.get $i) (local.get $nlen)) (local.get $len)))
        (local.set $j (i32.const 0))
        (block $differs
          (loop $inner
            (if (i32.eq (local.get $j) (local.get $nlen)) (then (return (i32.const 1))))
            (br_if $differs (i32.ne
              (i32.load8_u (i32.add (local.get $ptr) (i32.add (local.get $i) (local.get $j))))
              (i32.load8_u (i32.add (local.get $needle) (local.get $j)))))
            (local.set $j (i32.add (local.get $j) (i32.const 1)))
            (br $inner)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $outer)))
    (i32.const 0))"#;

fn data(at: u32, text: &str) -> String {
    let escaped: String = text.bytes().map(|byte| format!("\\{byte:02x}")).collect();
    format!(r#"(data (i32.const {at}) "{escaped}")"#)
}

fn packed(at: u32, text: &str) -> i64 {
    ((u64::from(at) << 32) | text.len() as u64) as i64
}

/// A module whose schema is `schema`, which draws `element` when what it's given holds each of
/// `needles`, and a problem when it doesn't, as a component whose arguments can't be read does.
fn module(schema: &str, needles: &[&str], element: &str) -> Vec<u8> {
    let good = format!(r#"{{"element":{element}}}"#);
    let bad = r#"{"element":{"Problem":{"message":"It was given what it can't read"}}}"#;
    let needle_at = |index: usize| 4096 + 256 * index as u32;
    let checks: String = needles
        .iter()
        .enumerate()
        .map(|(index, needle)| {
            format!(
                "(if (i32.eqz (call $contains (local.get $ptr) (local.get $len) (i32.const {}) (i32.const {}))) (then (return (i64.const {}))))\n",
                needle_at(index),
                needle.len(),
                packed(12288, bad)
            )
        })
        .collect();
    let needle_data: String = needles.iter().enumerate().map(|(index, needle)| data(needle_at(index), needle)).collect();
    let text = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const 262144))
  {}
  {needle_data}
  {}
  {}
  {CONTAINS}
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64)
    {checks}
    (i64.const {})))"#,
        data(1024, schema),
        data(8192, &good),
        data(12288, bad),
        packed(1024, schema),
        packed(8192, &good),
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn words(text: &str) -> String {
    format!(r#"{{"Text":{{"text":"{text}","size":null,"bold":false,"color":null}}}}"#)
}

/// Each of the fixture's components: its file, its tag, its schema, what it needs to be given
/// to draw, and what it draws.
fn fixture_modules() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    let text = |name: &str| format!(r#"{{"name":"{name}","kind":"Text","optional":false}}"#);
    let schema = |tag: &str, props: &[String], children: bool| format!(r#"{{"name":"{tag}","props":[{}],"children":{children}}}"#, props.join(","));
    let callout = format!(r#"{{"Stack":{{"horizontal":false,"gap":16.0,"children":[{},"Children"]}}}}"#, words("Callout"));
    vec![
        ("bars", "Bars", module(&schema("Bars", &[r#"{"name":"data","kind":{"File":["csv"]},"optional":false}"#.to_string()], false), &[], &words("Bars"))),
        ("callout", "Callout", module(&schema("Callout", &[text("title")], true), &[], &callout)),
        ("clock", "Clock", module(&schema("Clock", &[r#"{"name":"size","kind":"Number","optional":false}"#.to_string()], false), &[], &words("Clock"))),
        ("reveal", "Reveal", module(&schema("Reveal", &[text("first"), text("second"), text("third")], false), &[], &words("Reveal"))),
        (
            "score",
            "Score",
            module(
                &schema(
                    "Score",
                    &[
                        text("team"),
                        r#"{"name":"points","kind":"Whole","optional":false}"#.to_string(),
                        r#"{"name":"colour","kind":"Color","optional":false}"#.to_string(),
                    ],
                    false,
                ),
                &[r#""team":"""#, r#""points":"0""#, r##""colour":"#"##],
                &words("Score"),
            ),
        ),
    ]
}

/// How a component of the fixture is to be found in the scratch copy.
#[derive(Clone, Copy, PartialEq)]
enum Built {
    UpToDate,
    OutOfDate,
    Not,
}

/// A scratch copy of `fixtures/components`, with a build of each of its components, but as
/// `except` says for the one it names.
fn components_talk(except: Option<(&str, Built)>) -> tempfile::TempDir {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/components");
    let dir = tempfile::tempdir().unwrap();
    for folder in ["components", "build", "assets"] {
        std::fs::create_dir(dir.path().join(folder)).unwrap();
    }
    std::fs::copy(fixture.join("slides.md"), dir.path().join("slides.md")).unwrap();
    std::fs::copy(fixture.join("assets/sales.csv"), dir.path().join("assets/sales.csv")).unwrap();
    let mut entries = Vec::new();
    for (file, tag, wasm) in fixture_modules() {
        let source = std::fs::read(fixture.join(format!("components/{file}.rs"))).unwrap();
        std::fs::write(dir.path().join(format!("components/{file}.rs")), &source).unwrap();
        let built = except.filter(|(named, _)| *named == tag).map_or(Built::UpToDate, |(_, built)| built);
        if built == Built::Not {
            continue;
        }
        std::fs::write(dir.path().join(format!("build/{file}.wasm")), wasm).unwrap();
        let hash = if built == Built::OutOfDate { "built from something else".to_string() } else { source_hash(&source) };
        entries.push(format!(r#"{{"tag":"{tag}","file":"{file}","source_hash":"{hash}"}}"#));
    }
    let index = format!(r#"{{"format":1,"components":[{}]}}"#, entries.join(","));
    std::fs::write(dir.path().join("build/index.json"), index).unwrap();
    dir
}

fn show_talk() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), SHOW).unwrap();
    dir
}

fn palette_of(typing: &Typing, cx: &mut TestAppContext) -> Vec<PaletteEntry> {
    typing.document(cx).update(cx, |document, _| document.palette())
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

/// Opens the Insert tab's list of components, scrolling the tab along until its tool is in view.
fn open_list(typing: &Typing, cx: &mut TestAppContext) {
    click_on(typing, cx, TAB_INSERT_ID);
    for _ in 0..10 {
        let in_view = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            let tool = window.try_find(INSERT_COMPONENT_ID).map(|found| found.bounds());
            tool.is_some_and(|bounds| bounds.origin.x + bounds.size.width <= px(1280.))
        });
        if in_view {
            break;
        }
        click_on(typing, cx, MORE_RIGHT_ID);
    }
    click_on(typing, cx, INSERT_COMPONENT_ID);
}

/// Puts `tag` on the slide from the list.
fn insert(typing: &Typing, cx: &mut TestAppContext, tag: &str) {
    open_list(typing, cx);
    click_on(typing, cx, entry_id(tag));
}

fn selected(typing: &Typing, cx: &TestAppContext) -> Option<Range<usize>> {
    typing.document(cx).read_with(cx, |document, _| document.selected().map(|selected| selected.source.clone()))
}

fn selected_text(typing: &Typing, cx: &TestAppContext) -> Option<String> {
    let source = typing.source(cx);
    selected(typing, cx).map(|range| source[range].to_string())
}

fn attr(typing: &Typing, cx: &TestAppContext, name: &str) -> Option<String> {
    let source = selected(typing, cx).expect("a thing is selected");
    let talk = parse_talk(&typing.source(cx));
    find_tag(&talk, typing.current(cx), &source).and_then(|tag| tag.attr(name).map(str::to_string))
}

fn notices(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    typing.document(cx).update(cx, |document, _| document.notices())
}

fn accent(typing: &Typing, cx: &TestAppContext) -> String {
    hex(typing.document(cx).read_with(cx, |document, _| document.theme()).accent)
}

fn layers(typing: &Typing, cx: &mut TestAppContext) -> Vec<Vec<u8>> {
    let layers = typing.document(cx).update(cx, |document, _| document.layers(0.5));
    layers.iter().map(|layer| layer.pixmap.data().to_vec()).collect()
}

fn inspector(typing: &Typing, cx: &TestAppContext) -> Entity<Inspector> {
    cx.read(|cx| typing.view.read(cx).inspector().clone())
}

fn form_labels(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    typing.settle(cx);
    let inspector = inspector(typing, cx);
    cx.read(|cx| inspector.read(cx).labels())
}

/// What the talk is once `inserted` is put at the end of the first slide's content.
fn with_inserted(source: &str, after: &str, inserted: &str) -> String {
    source.replacen(after, &format!("{after}\n\n{inserted}"), 1)
}

fn schema(name: &str, props: Vec<PropField>, children: bool) -> ComponentSchema {
    ComponentSchema::new(name, props, children)
}

#[gpui_kit::test]
fn the_palette_lists_chart_then_the_talks_own(cx: &mut TestAppContext) {
    let typing = open_folder(cx, components_talk(None), None);
    open_list(&typing, cx);
    let palette = palette_of(&typing, cx);
    let tags: Vec<&str> = palette.iter().map(|entry| entry.tag.as_str()).collect();
    assert_eq!(tags, ["Chart", "Bars", "Callout", "Clock", "Reveal", "Score"]);
    assert!(palette.iter().all(|entry| !entry.loading), "{palette:?}");
    let labels: Vec<&str> = palette.iter().map(|entry| entry.label.as_str()).collect();
    assert_eq!(labels, ["Chart", "Bars", "Callout", "Clock", "Reveal", "Score"]);
    assert!(!palette[0].ours && palette[0].state.is_none());
    assert!(palette[1..].iter().all(|entry| entry.ours && entry.state == Some(ComponentState::UpToDate)));
    let content: Vec<&str> = palette.iter().filter(|entry| entry.takes_content).map(|entry| entry.tag.as_str()).collect();
    assert_eq!(content, ["Callout"]);
    assert_eq!(palette::label("ScoreCard"), "Score Card");
    assert_eq!(palette::label("Chart"), "Chart");

    for entry in &palette {
        assert_eq!(label_of(&typing, cx, entry_id(&entry.tag)).as_deref(), Some(entry.label.as_str()), "{}", entry.tag);
        assert!(!is_shown(&typing, cx, note_id(&entry.tag)), "nothing to say of {}", entry.tag);
    }
    assert!(!is_shown(&typing, cx, NO_COMPONENTS_ID));
    for left_out in ["Box", "Shape", "Icon", "Picture"] {
        assert!(!is_shown(&typing, cx, entry_id(left_out)), "{left_out} has a tool of its own");
    }
}

#[gpui_kit::test]
fn a_talk_with_none_says_so(cx: &mut TestAppContext) {
    let typing = open_folder(cx, show_talk(), None);
    let tags: Vec<String> = palette_of(&typing, cx).into_iter().map(|entry| entry.tag).collect();
    assert_eq!(tags, ["Chart"]);
    open_list(&typing, cx);
    assert_eq!(label_of(&typing, cx, entry_id("Chart")).as_deref(), Some("Chart"));
    assert_eq!(label_of(&typing, cx, NO_COMPONENTS_ID).as_deref(), Some(NO_COMPONENTS_OF_ITS_OWN));
    assert_eq!(NO_COMPONENTS_OF_ITS_OWN, "This talk has no components of its own.");
}

#[gpui_kit::test]
fn inserting_a_chart_draws_bars_at_once(cx: &mut TestAppContext) {
    let typing = open_folder(cx, show_talk(), None);
    let before = layers(&typing, cx).len();
    insert(&typing, cx, "Chart");
    let chart = format!("<Chart kind=\"bar\" labels=\"A, B, C\" values=\"3, 5, 2\" {PLACE} />");
    assert_eq!(typing.source(cx), with_inserted(SHOW, "</Box>", &chart));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(chart.as_str()), "selected");
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Chart"), "its form shows");
    let labels = form_labels(&typing, cx);
    assert!(labels.iter().any(|label| label == "Kind"), "{labels:?}");
    assert!(notices(&typing, cx).is_empty(), "no labelled box: {:?}", notices(&typing, cx));
    assert_eq!(layers(&typing, cx).len(), before + 1, "a layer of its own");
    assert!(!is_shown(&typing, cx, entry_id("Chart")), "the list closes");
    let undo = typing.document(cx).read_with(cx, |document, _| document.undo_label());
    assert_eq!(undo, Some("Insert component"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), SHOW, "one step");
}

#[gpui_kit::test]
fn a_chart_put_on_the_slide_from_the_list_of_slides_is_taken_away_by_delete(cx: &mut TestAppContext) {
    // Seen in the window: with the keyboard in the list of slides, a thing put on the slide
    // didn't have it, and Delete took the whole slide away.
    let typing = open_folder(cx, show_talk(), None);
    click_on(&typing, cx, strip_row_id(0));
    insert(&typing, cx, "Chart");
    assert_ne!(typing.source(cx), SHOW, "the chart is on the slide");
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), SHOW, "Delete takes the chart away, and only it");
}

#[gpui_kit::test]
fn inserting_a_talks_component_writes_its_required_arguments(cx: &mut TestAppContext) {
    let typing = open_folder(cx, components_talk(None), None);
    let source = typing.source(cx);
    assert!(notices(&typing, cx).is_empty(), "{:?}", notices(&typing, cx));
    insert(&typing, cx, "Score");
    let score = format!("<Score team=\"\" points=\"0\" colour=\"{}\" {PLACE} />", accent(&typing, cx));
    assert_eq!(typing.source(cx), with_inserted(&source, "The examples from the guide for coders", &score));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(score.as_str()));
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Score"));
    let labels = form_labels(&typing, cx);
    assert!(labels.iter().any(|label| label == "Team") && labels.iter().any(|label| label == "Colour"), "{labels:?}");
    assert!(notices(&typing, cx).is_empty(), "it draws, with no failed-component box: {:?}", notices(&typing, cx));
}

#[test]
fn default_attrs_writes_each_required_argument_at_a_starting_value() {
    let theme = Theme::default();
    let accent = hex(theme.accent);
    let required = |name: &str, kind: PropKind| PropField::new(name, kind, false);
    let optional = |name: &str, kind: PropKind| PropField::new(name, kind, true);
    let cases: Vec<(PropField, Option<String>)> = vec![
        (required("team", PropKind::Text), Some(String::new())),
        (required("points", PropKind::Whole), Some("0".to_string())),
        (required("size", PropKind::Number), Some("0".to_string())),
        (required("ready", PropKind::Flag), None),
        (required("colour", PropKind::Color), Some(accent.clone())),
        (required("logo", PropKind::File(vec!["png".to_string()])), None),
        (required("bar_kind", PropKind::Choice(vec!["line-chart".to_string(), "bar".to_string()])), Some("line-chart".to_string())),
        (required("empty_choice", PropKind::Choice(Vec::new())), None),
        (optional("note", PropKind::Text), None),
        (optional("count", PropKind::Whole), None),
        (optional("tint", PropKind::Color), None),
        (optional("shape", PropKind::Choice(vec!["round".to_string()])), None),
    ];
    for (prop, wanted) in cases {
        let written = default_attrs(&schema("Thing", vec![prop.clone()], false), &theme);
        // Written with `-` for `_`, as the toolkit reads an argument's attribute.
        let name = match prop.name.as_str() {
            "bar_kind" => "bar-kind".to_string(),
            "empty_choice" => "empty-choice".to_string(),
            other => other.to_string(),
        };
        let wanted: Vec<(String, String)> = wanted.into_iter().map(|value| (name.clone(), value)).collect();
        assert_eq!(written, wanted, "{prop:?}");
    }
    // In the order the component gives them.
    let score = schema(
        "Score",
        vec![required("team", PropKind::Text), required("points", PropKind::Whole), required("colour", PropKind::Color)],
        false,
    );
    let pairs = |pairs: &[(&str, &str)]| pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect::<Vec<_>>();
    assert_eq!(default_attrs(&score, &theme), pairs(&[("team", ""), ("points", "0"), ("colour", &accent)]));
    // A name or a value the talk can't hold isn't written.
    let odd = schema(
        "Odd",
        vec![required("Big", PropKind::Text), required("quote", PropKind::Choice(vec!["it's \"both\"".to_string()]))],
        false,
    );
    assert!(default_attrs(&odd, &theme).is_empty());
    // A chart shows something at once.
    let chart = schema(
        "Chart",
        vec![
            required("kind", PropKind::Choice(vec!["bar".to_string(), "line".to_string(), "pie".to_string()])),
            required("labels", PropKind::Text),
            required("values", PropKind::Text),
            optional("title", PropKind::Text),
            required("build", PropKind::Flag),
        ],
        false,
    );
    assert_eq!(default_attrs(&chart, &theme), pairs(&[("kind", "bar"), ("labels", "A, B, C"), ("values", "3, 5, 2")]));
}

#[gpui_kit::test]
fn a_component_that_needs_building_is_greyed_and_inserts_nothing(cx: &mut TestAppContext) {
    let typing = open_folder(cx, components_talk(Some(("Score", Built::Not))), None);
    let source = typing.source(cx);
    let score = palette_of(&typing, cx).into_iter().find(|entry| entry.tag == "Score").expect("listed");
    assert_eq!(score.state, Some(ComponentState::NeedsBuilding));
    assert_eq!(palette::tooltip(&score), Some(NEEDS_BUILDING_FIRST));
    assert_eq!(NEEDS_BUILDING_FIRST, "Build this talk's components first.");
    insert(&typing, cx, "Score");
    assert_eq!(typing.source(cx), source, "nothing is written");
    assert!(!typing.document(cx).read_with(cx, |document, _| document.can_undo()));
    let refused = typing.document(cx).update(cx, |document, _| document.insert_component("Score"));
    assert_eq!(refused, Ok(Vec::new()));
    assert_eq!(typing.source(cx), source);
    // One that's built has no tooltip of that kind.
    let callout = palette_of(&typing, cx).into_iter().find(|entry| entry.tag == "Callout").unwrap();
    assert_eq!(palette::tooltip(&callout), None);
}

#[gpui_kit::test]
fn an_out_of_date_component_is_marked_and_still_inserts(cx: &mut TestAppContext) {
    let typing = open_folder(cx, components_talk(Some(("Score", Built::OutOfDate))), None);
    open_list(&typing, cx);
    let score = palette_of(&typing, cx).into_iter().find(|entry| entry.tag == "Score").expect("listed");
    assert_eq!(score.state, Some(ComponentState::OutOfDate));
    assert_eq!(palette::note(&score), Some(OUT_OF_DATE));
    assert_eq!(OUT_OF_DATE, "out of date");
    assert_eq!(label_of(&typing, cx, note_id("Score")).as_deref(), Some(OUT_OF_DATE));
    assert!(!is_shown(&typing, cx, note_id("Callout")));
    click_on(&typing, cx, entry_id("Score"));
    let written = selected_text(&typing, cx).expect("inserted and selected");
    assert!(written.starts_with("<Score team=\"\" points=\"0\" colour=\"#"), "{written}");
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Score"));
}

#[gpui_kit::test]
fn a_component_that_takes_content_can_be_typed_in(cx: &mut TestAppContext) {
    let typing = open_folder(cx, components_talk(None), None);
    let source = typing.source(cx);
    insert(&typing, cx, "Callout");
    let callout = format!("<Callout title=\"\" {PLACE}>\n\n</Callout>");
    assert_eq!(typing.source(cx), with_inserted(&source, "The examples from the guide for coders", &callout));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(callout.as_str()));
    typing.type_text(cx, "Hi");
    let typed = format!("<Callout title=\"\" {PLACE}>\n\nHi\n\n</Callout>");
    assert_eq!(typing.source(cx), with_inserted(&source, "The examples from the guide for coders", &typed));
    assert!(typing.editing(cx).is_some(), "typing goes on inside it");
    typing.type_text(cx, " there");
    assert!(typing.source(cx).contains("\n\nHi there\n\n</Callout>"), "{}", typing.source(cx));
    assert!(typing.caret(cx).is_some(), "with a caret where it's drawn");

    // One that takes no content doesn't take typing.
    let typing = open_folder(cx, components_talk(None), None);
    insert(&typing, cx, "Score");
    let inserted = typing.source(cx);
    typing.type_text(cx, "Hi");
    assert_eq!(typing.source(cx), inserted);
    assert!(typing.editing(cx).is_none());
}

/// Opens the talk in `folder` with a compiler that counts its calls and permissions kept in
/// `permissions`.
fn open_counting(cx: &mut TestAppContext, folder: tempfile::TempDir, permissions: &Path) -> (Typing, Arc<AtomicUsize>) {
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

#[gpui_kit::test]
fn inserting_builds_nothing(cx: &mut TestAppContext) {
    let app_data = tempfile::tempdir().unwrap();
    let permissions = app_data.path().join("permissions.json");
    let (typing, calls) = open_counting(cx, components_talk(Some(("Score", Built::OutOfDate))), &permissions);
    let folder = typing.file.parent().unwrap().to_path_buf();
    let files_before = std::fs::read_dir(app_data.path()).unwrap().count();
    let build_before = std::fs::read(folder.join("build/index.json")).unwrap();
    for tag in ["Chart", "Score", "Callout", "Clock"] {
        insert(&typing, cx, tag);
    }
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "no build");
    assert_eq!(std::fs::read_dir(app_data.path()).unwrap().count(), files_before, "the permissions are as they were");
    assert!(!BuildPermissions::load(permissions).always(&folder));
    assert_eq!(std::fs::read(folder.join("build/index.json")).unwrap(), build_before, "nothing is marked for building");
    let states: Vec<ComponentState> = typing.document(cx).read_with(cx, |document, _| document.components().iter().map(|info| info.state).collect());
    assert!(states.contains(&ComponentState::OutOfDate), "{states:?}");
}

#[gpui_kit::test]
fn the_form_changes_the_new_chart(cx: &mut TestAppContext) {
    let typing = open_folder(cx, show_talk(), None);
    insert(&typing, cx, "Chart");
    let bars = layers(&typing, cx);
    let inspector = inspector(&typing, cx);
    typing.with_window(cx, |window, cx| inspector.update(cx, |inspector, cx| inspector.choose("kind", "pie", window, cx)));
    typing.settle(cx);
    assert_eq!(attr(&typing, cx, "kind").as_deref(), Some("pie"));
    assert!(notices(&typing, cx).is_empty(), "{:?}", notices(&typing, cx));
    let pie = layers(&typing, cx);
    assert_eq!(pie.len(), bars.len());
    assert_ne!(pie, bars, "the slide's layers change");
}

#[gpui_kit::test]
fn a_packed_talk_lists_and_inserts_its_components(cx: &mut TestAppContext) {
    let folder = components_talk(Some(("Clock", Built::Not)));
    let elsewhere = tempfile::tempdir().unwrap();
    let file = elsewhere.path().join("talk.sptx");
    slides_engine::pack_folder(folder.path(), &file, "test").unwrap();
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds { origin: Point::default(), size: size(px(1280.), px(800.)) };
        let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| WorkspaceView::open(file, window, cx))).expect("open test window")
    });
    let typing = Typing { _folder: elsewhere, file: folder.path().join("slides.md"), handle, view };
    typing.settle(cx);
    assert!(typing.document(cx).read_with(cx, |document, _| document.is_packed()));
    let palette = palette_of(&typing, cx);
    let tags: Vec<&str> = palette.iter().map(|entry| entry.tag.as_str()).collect();
    assert_eq!(tags, ["Chart", "Bars", "Callout", "Clock", "Reveal", "Score"]);
    assert_eq!(palette.iter().find(|entry| entry.tag == "Clock").unwrap().state, Some(ComponentState::NeedsBuilding));
    let source = typing.source(cx);
    insert(&typing, cx, "Clock");
    assert_eq!(typing.source(cx), source, "nothing builds in a packed talk, so it can't go in");
    insert(&typing, cx, "Score");
    let written = selected_text(&typing, cx).expect("inserted and selected");
    assert!(written.starts_with("<Score team=\"\" points=\"0\""), "{written}");
    assert!(notices(&typing, cx).is_empty(), "{:?}", notices(&typing, cx));
}

#[test]
fn opening_a_talk_loads_no_component() {
    let folder = components_talk(None);
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    assert_eq!(document.modules_loaded(), 0, "opening loads nothing");
    let palette = document.palette();
    assert_eq!(palette.len(), 6, "listed, without anything loaded");
    assert!(palette[1..].iter().all(|entry| entry.loading && !entry.takes_content), "{palette:?}");
    assert_eq!(palette::note(&palette[1]), Some(STILL_LOADING));
    assert_eq!(document.modules_loaded(), 0);
    // Drawing the slide of scores loads the one it uses.
    let _ = document.layers_of(1, 0.25);
    assert_eq!(document.modules_loaded(), 1);
    // Opening the palette loads the rest, a step at a time.
    document.begin_palette();
    while document.palette_step() {}
    let palette = document.palette();
    assert!(palette.iter().all(|entry| !entry.loading), "{palette:?}");
    let content: Vec<&str> = palette.iter().filter(|entry| entry.takes_content).map(|entry| entry.tag.as_str()).collect();
    assert_eq!(content, ["Callout"]);
    assert_eq!(document.modules_loaded(), 5);
}

/// A module whose `schema` spins for as long as it's let.
fn spinning_schema() -> Vec<u8> {
    let text = r#"(module
  (memory (export "memory") 1)
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param i32) (result i32) (i32.const 0))
  (func (export "schema") (result i64) (loop br 0) (i64.const 0))
  (func (export "render") (param i32 i32) (result i64) (i64.const 0)))"#;
    wat::parse_str(text).unwrap()
}

/// A talk of one slide with `count` components whose builds spin when asked what they take.
fn spinning_talk(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "# T\n").unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    let mut entries = Vec::new();
    for n in 0..count {
        let tag = format!("Spin{}", (b'a' + (n % 26) as u8) as char) + &"x".repeat(n / 26);
        let file = tag.to_lowercase();
        let source = format!("use toolkit::prelude::*;\n\n#[component]\nfn {tag}() -> Element {{\n    text(\"{tag}\")\n}}\n");
        std::fs::write(dir.path().join(format!("components/{file}.rs")), &source).unwrap();
        std::fs::write(dir.path().join(format!("build/{file}.wasm")), spinning_schema()).unwrap();
        entries.push(format!(r#"{{"tag":"{tag}","file":"{file}","source_hash":"{}"}}"#, source_hash(source.as_bytes())));
    }
    std::fs::write(dir.path().join("build/index.json"), format!(r#"{{"format":1,"components":[{}]}}"#, entries.join(","))).unwrap();
    dir
}

#[gpui_kit::test]
fn the_palette_fills_in_within_its_budget(cx: &mut TestAppContext) {
    let typing = open_folder(cx, spinning_talk(40), None);
    assert_eq!(typing.document(cx).read_with(cx, |document, _| document.modules_loaded()), 0);
    click_on(&typing, cx, TAB_INSERT_ID);
    for _ in 0..10 {
        let in_view = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            let tool = window.try_find(INSERT_COMPONENT_ID).map(|found| found.bounds());
            tool.is_some_and(|bounds| bounds.origin.x + bounds.size.width <= px(1280.))
        });
        if in_view {
            break;
        }
        click_on(&typing, cx, MORE_RIGHT_ID);
    }
    // The list opens at once, every one of the talk's own still loading.
    let started = Instant::now();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(INSERT_COMPONENT_ID, cx);
        window.render_frame(cx);
    });
    // Loading the 40 would take seconds; opening is drawing the list, which a loaded machine can
    // make take a few hundred milliseconds in a debug build.
    assert!(started.elapsed() < Duration::from_millis(1000), "opening took {:?}", started.elapsed());
    let palette = palette_of(&typing, cx);
    assert_eq!(palette.len(), 41);
    assert!(palette[1..].iter().all(|entry| entry.loading), "{palette:?}");
    assert_eq!(label_of(&typing, cx, note_id("Spina")).as_deref(), Some(STILL_LOADING));
    assert_eq!(label_of(&typing, cx, entry_id("Chart")).as_deref(), Some("Chart"), "a chart can go in already");

    // Each is loaded in a step of its own, and the budget runs out long before all 40 are.
    let started = Instant::now();
    cx.run_until_parked();
    typing.settle(cx);
    assert!(started.elapsed() < Duration::from_secs(4), "loading took {:?}", started.elapsed());
    let longest = typing.document(cx).read_with(cx, |document, _| document.palette_longest_step());
    assert!(longest < Duration::from_millis(600), "a step took {longest:?}");
    let loaded = typing.document(cx).read_with(cx, |document, _| document.modules_loaded());
    assert!((1..10).contains(&loaded), "{loaded} loaded");
    let palette = palette_of(&typing, cx);
    assert!(palette.iter().all(|entry| !entry.loading), "nothing is left loading: {palette:?}");
    assert!(!is_shown(&typing, cx, note_id("Spinf")));
    // Left alone, it does nothing more.
    cx.run_until_parked();
    assert_eq!(typing.document(cx).read_with(cx, |document, _| document.modules_loaded()), loaded);
    // One past the budget is loaded when it's chosen, and one that can't be is refused.
    click_on(&typing, cx, entry_id("Spinf"));
    assert_eq!(typing.source(cx), "# T\n", "nothing is added");
    assert_eq!(typing.status(cx).as_deref(), Some(COULD_NOT_LOAD));
}

const COULD_NOT_LOAD: &str = "That component couldn't be loaded, so it wasn't added.";

#[test]
fn a_component_that_cannot_be_loaded_is_refused() {
    let folder = spinning_talk(1);
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let refused = document.insert_component("Spina").unwrap_err();
    assert_eq!(refused.to_string(), COULD_NOT_LOAD);
    assert_eq!(document.source(), "# T\n");
    assert!(!document.can_undo());
}

/// The first release's build of Score, made by the real toolchain, and its source.
const V1_SCORE: &[u8] = include_bytes!("../../components/tests/fixtures/v1/score.wasm");
const V1_SCORE_SOURCE: &str = include_str!("../../components/tests/fixtures/v1/score.rs");

#[test]
fn a_real_score_draws_with_the_palettes_starting_values() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "# T\n").unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("components/score.rs"), V1_SCORE_SOURCE).unwrap();
    std::fs::write(dir.path().join("build/score.wasm"), V1_SCORE).unwrap();
    let index = format!(r#"{{"format":1,"components":[{{"tag":"Score","file":"score","source_hash":"{}"}}]}}"#, source_hash(V1_SCORE_SOURCE.as_bytes()));
    std::fs::write(dir.path().join("build/index.json"), index).unwrap();
    let mut document = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    document.insert_component("Score").unwrap();
    let written = parse_talk(document.source());
    let tag = find_tag(&written, 0, &document.selected().expect("selected").source).expect("a tag").clone();
    let attrs: Vec<(&str, &str)> = tag.attrs.iter().map(|attr| (attr.name.as_str(), attr.value.as_str())).collect();
    assert_eq!(&attrs[..2], &[("team", ""), ("points", "0")], "{attrs:?}");
    assert_eq!(attrs[2].0, "colour");
    assert!(document.notices().is_empty(), "{:?}", document.notices());
}

#[gpui_kit::test]
fn a_click_on_a_component_that_does_not_draw_its_words_selects_it(cx: &mut TestAppContext) {
    let folder = components_talk(Some(("Clock", Built::Not)));
    let slides = "# T\n\nBody words\n\n<Score team=\"\" points=\"0\" colour=\"#123456\" x=\"100\" y=\"300\" w=\"600\" h=\"300\">\n\nHidden words\n\n</Score>\n\n\
                  <Nope x=\"800\" y=\"300\" w=\"400\" h=\"300\">\n\nUnknown words\n\n</Nope>\n\n\
                  <Clock size=\"100\" x=\"1300\" y=\"300\" w=\"400\" h=\"300\">\n\nLater words\n\n</Clock>\n";
    std::fs::write(folder.path().join("slides.md"), slides).unwrap();
    let typing = open_folder(cx, folder, None);
    for (x, y, start, name) in [(400.0, 450.0, "<Score", "Score"), (1000.0, 450.0, "<Nope", "Nope"), (1500.0, 450.0, "<Clock", "Clock")] {
        let at = slides.find(start).unwrap();
        let close = format!("</{name}>");
        let range = at..at + slides[at..].find(&close).unwrap() + close.len();
        let point = typing.at(cx, x, y);
        typing.click(cx, point);
        assert_eq!(selected(&typing, cx), Some(range), "{name} is selected");
        assert!(typing.editing(cx).is_none(), "{name}: nothing is being typed");
        typing.type_text(cx, "x");
        assert_eq!(typing.source(cx), slides, "{name}: nothing is typed into the slide");
        typing.press(cx, "escape");
    }
}

// A box of text is remembered by where its tag is, never by counting the slide's boxes, which
// changes as the talk's components load.

/// A Callout that takes content if `content`, and draws it under its title.
fn callout_module(content: bool) -> Vec<u8> {
    let schema = format!(r#"{{"name":"Callout","props":[{{"name":"title","kind":"Text","optional":false}}],"children":{content}}}"#);
    let element = format!(r#"{{"Stack":{{"horizontal":false,"gap":16.0,"children":[{},"Children"]}}}}"#, words("Callout"));
    module(&schema, &[], &element)
}

/// A component called `tag` whose render spins for as long as it's let.
fn slow_module(tag: &str) -> Vec<u8> {
    let schema = format!(r#"{{"name":"{tag}","props":[],"children":false}}"#);
    let text = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const 262144))
  {}
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64) (loop br 0) (i64.const 0)))"#,
        data(1024, &schema),
        packed(1024, &schema)
    );
    wat::parse_str(&text).unwrap()
}

/// A slide of `slows` components that spin as they draw, a Callout holding "Note" and a text
/// box holding "Hello", in a folder with a build of each.
fn two_boxes(slows: usize) -> (tempfile::TempDir, String) {
    let slow_tags: String = (0..slows).map(|n| format!("<Slow{} x=\"0\" y=\"{}\" w=\"10\" h=\"10\" />\n\n", (b'a' + n as u8) as char, n * 20)).collect();
    let slides = format!(
        "# T\n\n{slow_tags}<Callout title=\"\" x=\"100\" y=\"100\" w=\"600\" h=\"300\">\n\nNote\n\n</Callout>\n\n<Text x=\"100\" y=\"500\" w=\"600\" h=\"200\">\n\nHello\n\n</Text>\n"
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("slides.md"), &slides).unwrap();
    let mut builds = vec![("Callout".to_string(), callout_module(true))];
    builds.extend((0..slows).map(|n| format!("Slow{}", (b'a' + n as u8) as char)).map(|tag| { let wasm = slow_module(&tag); (tag, wasm) }));
    let mut entries = Vec::new();
    for (tag, wasm) in builds {
        let file = tag.to_lowercase();
        let source = format!("use toolkit::prelude::*;\n\n#[component]\nfn {tag}() -> Element {{\n    text(\"{tag}\")\n}}\n");
        std::fs::write(dir.path().join(format!("components/{file}.rs")), &source).unwrap();
        std::fs::write(dir.path().join(format!("build/{file}.wasm")), wasm).unwrap();
        entries.push(format!(r#"{{"tag":"{tag}","file":"{file}","source_hash":"{}"}}"#, source_hash(source.as_bytes())));
    }
    std::fs::write(dir.path().join("build/index.json"), format!(r#"{{"format":1,"components":[{}]}}"#, entries.join(","))).unwrap();
    (dir, slides)
}

/// Which of the slide's flows is the box whose tag starts with `start`.
fn box_flow(document: &Document, start: &str) -> Option<usize> {
    let source = document.source();
    document
        .text_flows(document.talk(), 0)
        .iter()
        .position(|flow| matches!(&flow.place, FlowPlace::Tag(range) if source[range.clone()].starts_with(start)))
}

/// Starts typing at the end of the box whose tag starts with `start`.
fn type_at_end(document: &mut Document, start: &str, text: &str) {
    let flow = box_flow(document, start).unwrap_or_else(|| panic!("{start} is a box of text"));
    let paras = document.text_flows(document.talk(), 0)[flow].paras.clone();
    let end = TextEdit::new(paras).select_all().selection.end();
    assert!(document.begin_text(0, flow, end));
    document.apply_text(|edit| edit.insert(text)).unwrap();
}

/// Loads every build the palette lists that isn't loaded.
fn load_all(document: &mut Document) {
    document.begin_palette();
    while document.palette_step() {}
}

/// Makes the Callout's build take content or not, as a rebuild would, and loads it.
fn rebuild_callout(document: &mut Document, folder: &Path, content: bool) {
    std::fs::write(folder.join("build/callout.wasm"), callout_module(content)).unwrap();
    document.reload_components();
    load_all(document);
    assert_eq!(box_flow(document, "<Callout").is_some(), content);
}

#[test]
fn undo_after_a_component_loads_resumes_in_the_same_box() {
    // With nothing drawn, and with the Callout run out of time on a slide too slow to draw.
    for (slows, draw) in [(0, false), (5, true)] {
        let (dir, slides) = two_boxes(slows);
        let mut document = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
        if draw {
            let _ = document.layers_of(0, 0.25);
            let _ = document.text_layout(0);
        }
        assert_eq!(box_flow(&document, "<Callout"), None, "the Callout isn't loaded");
        type_at_end(&mut document, "<Text", "X");
        document.end_text().unwrap();
        assert!(document.source().contains("HelloX"));
        load_all(&mut document);
        assert!(box_flow(&document, "<Callout").is_some(), "the Callout is loaded now");
        document.undo().unwrap();
        assert_eq!(document.source(), slides, "the X is taken away");
        assert_eq!(document.text_flow(), box_flow(&document, "<Text"), "typing goes on in the same box");
        let caret = document.text_layout(0).caret(document.text_flow().unwrap(), slides_format::Pos::default());
        assert!(caret.is_some(), "where it's drawn");
        document.apply_text(|edit| edit.insert("Y")).unwrap();
        assert!(document.source().contains("HelloY") || document.source().contains("YHello"), "{}", document.source());
        assert!(document.source().contains("\n\nNote\n\n"), "the Callout is as it was");
    }
}

#[test]
fn undo_after_a_component_stops_taking_content_types_nothing() {
    let (dir, slides) = two_boxes(0);
    let mut document = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    load_all(&mut document);
    type_at_end(&mut document, "<Callout", "X");
    document.end_text().unwrap();
    let typed = document.source().to_string();
    assert!(typed.contains("NoteX"));
    rebuild_callout(&mut document, dir.path(), false);
    document.undo().unwrap();
    assert_eq!(document.source(), slides, "undone exactly");
    assert_eq!(document.text_flow(), None, "nothing is being typed");
    assert!(document.apply_text(|edit| edit.insert("Y")).is_err() || document.source() == slides);
    assert_eq!(document.source(), slides);
    document.redo().unwrap();
    assert_eq!(document.source(), typed, "made again exactly");
    assert_eq!(document.text_flow(), None);
}

/// What a test of the property does.
#[derive(Clone, Debug)]
enum Act {
    /// Types the character at the end of the Callout's text if `callout`, or else the text box's.
    Type { callout: bool, letter: char },
    EndTyping,
    /// Makes the Callout take content, or stop taking it.
    Toggle,
    Undo,
    Redo,
}

fn act() -> impl proptest::strategy::Strategy<Value = Act> {
    use proptest::prelude::*;
    prop_oneof![
        4 => (any::<bool>(), proptest::char::range('a', 'e')).prop_map(|(callout, letter)| Act::Type { callout, letter }),
        1 => Just(Act::EndTyping),
        2 => Just(Act::Toggle),
        2 => Just(Act::Undo),
        1 => Just(Act::Redo),
    ]
}

/// The two boxes' words, as a model of the talk that never looks at what's loaded.
#[derive(Clone, Debug, PartialEq)]
struct Words {
    note: String,
    hello: String,
}

impl Words {
    fn source(&self, slides: &str) -> String {
        slides.replacen("\n\nNote\n\n", &format!("\n\n{}\n\n", self.note), 1).replacen("\n\nHello\n\n", &format!("\n\n{}\n\n", self.hello), 1)
    }
}

/// Where the box whose tag starts with `start` starts in `source`.
fn tag_start_of(source: &str, start: &str) -> usize {
    source.find(start).unwrap()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 24, ..Default::default() })]

    #[test]
    fn typing_follows_its_box_whatever_loads(acts in proptest::collection::vec(act(), 1..16)) {
        let (dir, slides) = two_boxes(0);
        let mut document = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
        let mut content = false;
        // The model: the words now, the steps to undo and redo, each with the box it was typed
        // in, and the box of the run of typing open, if any.
        let mut now = Words { note: "Note".to_string(), hello: "Hello".to_string() };
        let mut undo: Vec<(Words, bool)> = Vec::new();
        let mut redo: Vec<(Words, bool)> = Vec::new();
        let mut open: Option<bool> = None;
        for act in acts {
            let mut resumed_in: Option<bool> = None;
            match act {
                Act::Type { callout, letter } => {
                    if callout && !content {
                        continue;
                    }
                    if open != Some(callout) {
                        document.end_text().unwrap();
                        let start = if callout { "<Callout" } else { "<Text" };
                        let flow = box_flow(&document, start).unwrap();
                        let paras = document.text_flows(document.talk(), 0)[flow].paras.clone();
                        let end = TextEdit::new(paras).select_all().selection.end();
                        proptest::prop_assert!(document.begin_text(0, flow, end));
                        undo.push((now.clone(), callout));
                        redo.clear();
                        open = Some(callout);
                    }
                    document.apply_text(|edit| edit.insert(&letter.to_string())).unwrap();
                    if callout { now.note.push(letter) } else { now.hello.push(letter) }
                }
                Act::EndTyping => {
                    document.end_text().unwrap();
                    open = None;
                }
                Act::Toggle => {
                    document.end_text().unwrap();
                    open = None;
                    content = !content;
                    rebuild_callout(&mut document, dir.path(), content);
                }
                Act::Undo => {
                    open = None;
                    document.undo().unwrap();
                    if let Some((before, callout)) = undo.pop() {
                        redo.push((now.clone(), callout));
                        now = before;
                        resumed_in = Some(callout);
                    }
                }
                Act::Redo => {
                    open = None;
                    document.redo().unwrap();
                    if let Some((after, callout)) = redo.pop() {
                        undo.push((now.clone(), callout));
                        now = after;
                        resumed_in = Some(callout);
                    }
                }
            }
            let source = document.source().to_string();
            proptest::prop_assert_eq!(&source, &now.source(&slides));
            // Typing brought back is in the box the step was typed in, at its tag's start.
            if let Some(flow) = document.text_flow() {
                let flows = document.text_flows(document.talk(), 0);
                let FlowPlace::Tag(range) = &flows[flow].place else {
                    return Err(proptest::test_runner::TestCaseError::fail(format!("typing in {:?}", flows[flow].place)));
                };
                if let Some(callout) = resumed_in {
                    let wanted = tag_start_of(&source, if callout { "<Callout" } else { "<Text" });
                    proptest::prop_assert_eq!(range.start, wanted);
                }
            }
            if resumed_in == Some(true) && !content {
                proptest::prop_assert_eq!(document.text_flow(), None);
            }
        }
    }
}
