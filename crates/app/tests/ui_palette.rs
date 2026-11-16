//! The component palette: the Insert tab's Component tool, which puts a chart or one of the
//! talk's own components on the slide, set up to draw at once.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{App, AppContext as _, Bounds, ElementId, Entity, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use slides_components::source_hash;
use slides_engine::{ComponentState, Fonts};
use slides_format::{find_tag, parse_talk};
use stagepack_studio::builds::Compile;
use stagepack_studio::document::{Document, PaletteEntry};
use stagepack_studio::insert::hex;
use stagepack_studio::insert_tools::INSERT_COMPONENT_ID;
use stagepack_studio::inspector::{INSPECTOR_HEADING_ID, Inspector};
use stagepack_studio::palette::{
    self, NEEDS_BUILDING_FIRST, NO_COMPONENTS_ID, NO_COMPONENTS_OF_ITS_OWN, OUT_OF_DATE, default_attrs, entry_id, note_id,
};
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::ribbon::{MORE_RIGHT_ID, TAB_INSERT_ID};
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
    let palette = palette_of(&typing, cx);
    let tags: Vec<&str> = palette.iter().map(|entry| entry.tag.as_str()).collect();
    assert_eq!(tags, ["Chart", "Bars", "Callout", "Clock", "Reveal", "Score"]);
    let labels: Vec<&str> = palette.iter().map(|entry| entry.label.as_str()).collect();
    assert_eq!(labels, ["Chart", "Bars", "Callout", "Clock", "Reveal", "Score"]);
    assert!(!palette[0].ours && palette[0].state.is_none());
    assert!(palette[1..].iter().all(|entry| entry.ours && entry.state == Some(ComponentState::UpToDate)));
    let content: Vec<&str> = palette.iter().filter(|entry| entry.takes_content).map(|entry| entry.tag.as_str()).collect();
    assert_eq!(content, ["Callout"]);
    assert_eq!(palette::label("ScoreCard"), "Score Card");
    assert_eq!(palette::label("Chart"), "Chart");

    open_list(&typing, cx);
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
        let name = prop.name.replace('_', "-");
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
    let score = palette_of(&typing, cx).into_iter().find(|entry| entry.tag == "Score").expect("listed");
    assert_eq!(score.state, Some(ComponentState::OutOfDate));
    assert_eq!(palette::note(&score), Some(OUT_OF_DATE));
    assert_eq!(OUT_OF_DATE, "out of date");
    open_list(&typing, cx);
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
fn a_document_opened_without_a_window_has_its_palette() {
    let folder = components_talk(None);
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    assert_eq!(document.palette().len(), 6);
}
