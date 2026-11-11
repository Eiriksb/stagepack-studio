use slides_components::{Assets, ComponentError, WasmComponent, MAX_ELEMENTS, MAX_ELEMENT_DEPTH, MAX_MODULE_BYTES};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use toolkit::{Color, ComponentSchema, CtxData, Element, PropField, PropKind, RenderInput, RenderOutput, ShapeKind, Theme};

const SCHEMA: &str = r#"{"name":"Test","props":[],"children":false}"#;
const SCHEMA_AT: u32 = 1024;
/// Where a test's answer sits in the module's memory.
const ANSWER_AT: u32 = 8192;
/// Where `alloc` starts handing out room, past every answer.
const HEAP_AT: u32 = 4 * 65536;
const READ_ASSET: &str = r#"(import "toolkit" "read_asset" (func $read_asset (param i32 i32) (result i64)))"#;

/// The parts of a test module. Whatever is left out is filled in with a working default.
#[derive(Default)]
struct Wat<'a> {
    abi_version: Option<u32>,
    imports: &'a str,
    /// Globals, data and anything else at the module's top level.
    extra: &'a str,
    /// The body of `schema`, if it isn't the usual one.
    schema: Option<&'a str>,
    /// The body of `render(ptr, len) -> i64`.
    render: &'a str,
    /// Pages of memory, if not the usual 8.
    pages: Option<u32>,
}

fn wat(parts: Wat) -> Vec<u8> {
    let Wat { abi_version, imports, extra, schema, render, pages } = parts;
    let pages = pages.unwrap_or(8);
    let version = abi_version.unwrap_or(1);
    let usual_schema = format!("(i64.const {})", packed(SCHEMA_AT, SCHEMA.len()));
    let schema = schema.unwrap_or(&usual_schema);
    let schema_data = data(SCHEMA_AT, SCHEMA.as_bytes());
    let text = format!(
        r#"(module
  {imports}
  (memory (export "memory") {pages})
  (global $heap (mut i32) (i32.const {HEAP_AT}))
  {schema_data}
  {extra}
  (func (export "abi_version") (result i32) (i32.const {version}))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) {schema})
  (func (export "render") (param $ptr i32) (param $len i32) (result i64) {render}))"#
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// A data segment holding `bytes` at `at`.
fn data(at: u32, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|byte| format!("\\{byte:02x}")).collect();
    format!(r#"(data (i32.const {at}) "{escaped}")"#)
}

fn packed(at: u32, len: usize) -> i64 {
    ((u64::from(at) << 32) | len as u64) as i64
}

/// A module whose `render` gives `answer`.
fn answering(answer: &str) -> Vec<u8> {
    let extra = data(ANSWER_AT, answer.as_bytes());
    let render = format!("(i64.const {})", packed(ANSWER_AT, answer.len()));
    wat(Wat { extra: &extra, render: &render, ..Wat::default() })
}

/// A module whose `render` reads the asset at `path` once and then gives `answer`.
fn reading(path: &str, answer: &str) -> Vec<u8> {
    let path_at = ANSWER_AT - 1024;
    let extra = format!("{}\n{}", data(path_at, path.as_bytes()), data(ANSWER_AT, answer.as_bytes()));
    let render = format!(
        "(drop (call $read_asset (i32.const {path_at}) (i32.const {}))) (i64.const {})",
        path.len(),
        packed(ANSWER_AT, answer.len())
    );
    wat(Wat { imports: READ_ASSET, extra: &extra, render: &render, ..Wat::default() })
}

fn text_answer(text: &str) -> String {
    format!(r#"{{"element":{{"Text":{{"text":"{text}","size":null,"bold":false,"color":null}}}}}}"#)
}

fn input() -> RenderInput {
    RenderInput::new(BTreeMap::new(), CtxData::new(0, 0.0, 1920.0, 1080.0, 800.0, 0.0, Theme::default()))
}

fn no_assets() -> Assets {
    Rc::new(|_: &str| None)
}

/// Assets that answer every path with `bytes`, and the paths they were asked for.
fn counted_assets(bytes: &'static [u8]) -> (Assets, Rc<RefCell<Vec<String>>>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let noted = asked.clone();
    let assets: Assets = Rc::new(move |path: &str| {
        noted.borrow_mut().push(path.to_string());
        Some(bytes.to_vec())
    });
    (assets, asked)
}

fn load(wasm: &[u8]) -> WasmComponent {
    WasmComponent::load(wasm).unwrap_or_else(|error| panic!("the module didn't load: {error:?}"))
}

fn render(wasm: &[u8]) -> Result<RenderOutput, ComponentError> {
    load(wasm).render(&input(), no_assets())
}

#[track_caller]
fn assert_bad_output(result: Result<RenderOutput, ComponentError>) {
    match result {
        Err(ComponentError::BadOutput(reason)) => assert!(!reason.is_empty()),
        other => panic!("expected BadOutput, got {other:?}"),
    }
}

#[track_caller]
fn assert_malformed(result: Result<WasmComponent, ComponentError>) {
    match result {
        Err(ComponentError::Malformed(reason)) => assert!(!reason.is_empty()),
        Err(other) => panic!("expected Malformed, got {other:?}"),
        Ok(_) => panic!("expected Malformed, but it loaded"),
    }
}

#[test]
fn a_working_module_renders() {
    let output = render(&answering(&text_answer("hello"))).unwrap();
    assert_eq!(output.element, toolkit::text("hello"));
    assert_eq!(load(&answering(&text_answer("hello"))).schema().name, "Test");
}

#[test]
fn a_component_that_never_returns_is_stopped() {
    let component = load(&wat(Wat { render: "(loop br 0) (i64.const 0)", ..Wat::default() }));
    let started = Instant::now();
    assert_eq!(component.render(&input(), no_assets()), Err(ComponentError::TookTooLong));
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}

#[test]
fn a_schema_that_never_returns_is_stopped() {
    let started = Instant::now();
    let result = WasmComponent::load(&wat(Wat { schema: Some("(loop br 0) (i64.const 0)"), render: "(i64.const 0)", ..Wat::default() }));
    assert_eq!(result.err(), Some(ComponentError::TookTooLong));
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}

#[test]
fn a_component_that_eats_memory_is_stopped() {
    let render = "(loop (br_if 0 (i32.ne (memory.grow (i32.const 16)) (i32.const -1)))) unreachable";
    assert_eq!(render_of(render), Err(ComponentError::OutOfMemory));
}

fn render_of(body: &str) -> Result<RenderOutput, ComponentError> {
    render(&wat(Wat { render: body, ..Wat::default() }))
}

#[test]
fn a_component_that_panics_is_reported() {
    assert_eq!(render_of("unreachable"), Err(ComponentError::Crashed));
}

#[test]
fn a_component_that_recurses_without_end_is_reported() {
    let extra = "(func $deeper (result i64) (call $deeper))";
    let wasm = wat(Wat { extra, render: "(call $deeper)", ..Wat::default() });
    assert_eq!(render(&wasm), Err(ComponentError::Crashed));
}

#[test]
fn a_pointer_out_of_bounds_is_reported() {
    assert_bad_output(render_of(&format!("(i64.const {})", packed(0xffff_0000, 100))));
}

#[test]
fn an_enormous_answer_is_refused() {
    assert_bad_output(render_of(&format!("(i64.const {})", packed(0, 1 << 31))));
}

#[test]
fn an_answer_that_is_not_json_is_refused() {
    assert_bad_output(render(&answering("this is not JSON")));
}

#[test]
fn an_answer_with_null_for_a_number_is_refused() {
    let answer = r#"{"element":{"Text":{"text":"a","size":null,"bold":false,"color":null}},"steps":null}"#;
    assert_bad_output(render(&answering(answer)));
}

fn nested(depth: usize) -> String {
    let mut element = String::from(r#""Empty""#);
    for _ in 1..depth {
        element = format!(r#"{{"Stack":{{"horizontal":true,"gap":0,"children":[{element}]}}}}"#);
    }
    format!(r#"{{"element":{element}}}"#)
}

#[test]
fn an_answer_nested_too_deeply_is_refused() {
    assert!(render(&answering(&nested(MAX_ELEMENT_DEPTH))).is_ok());
    assert_bad_output(render(&answering(&nested(MAX_ELEMENT_DEPTH + 1))));
    // Deeper than the JSON reader goes at all.
    assert_bad_output(render(&answering(&nested(200))));
}

fn wide(count: usize) -> String {
    let children = vec![r#""Empty""#; count].join(",");
    format!(r#"{{"element":{{"Stack":{{"horizontal":true,"gap":0,"children":[{children}]}}}}}}"#)
}

#[test]
fn an_answer_with_too_many_elements_is_refused() {
    // The stack itself is one of them.
    assert!(render(&answering(&wide(MAX_ELEMENTS - 1))).is_ok());
    assert_bad_output(render(&answering(&wide(MAX_ELEMENTS))));
}

#[test]
fn a_module_with_any_other_import_is_refused() {
    let imports = r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#;
    let wasm = wat(Wat { imports, render: "(i64.const 0)", ..Wat::default() });
    assert_malformed(WasmComponent::load(&wasm));
}

#[test]
fn read_asset_of_the_wrong_kind_is_refused() {
    let imports = r#"(import "toolkit" "read_asset" (func (param i32) (result i64)))"#;
    let wasm = wat(Wat { imports, render: "(i64.const 0)", ..Wat::default() });
    assert_malformed(WasmComponent::load(&wasm));
}

#[test]
fn a_module_missing_an_export_is_refused() {
    let without_schema = r#"(module
  (memory (export "memory") 1)
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param i32) (result i32) (i32.const 0))
  (func (export "render") (param i32 i32) (result i64) (i64.const 0)))"#;
    assert_malformed(WasmComponent::load(&wat::parse_str(without_schema).unwrap()));
    let without_memory = r#"(module
  (memory 1)
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param i32) (result i32) (i32.const 0))
  (func (export "schema") (result i64) (i64.const 0))
  (func (export "render") (param i32 i32) (result i64) (i64.const 0)))"#;
    assert_malformed(WasmComponent::load(&wat::parse_str(without_memory).unwrap()));
}

#[test]
fn an_export_of_the_wrong_kind_is_refused() {
    let wasm = wat::parse_str(
        r#"(module
  (memory (export "memory") 1)
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param i32) (result i32) (i32.const 0))
  (func (export "schema") (result i64) (i64.const 0))
  (func (export "render") (param i32) (result i32) (i32.const 0)))"#,
    )
    .unwrap();
    assert_malformed(WasmComponent::load(&wasm));
}

#[test]
fn something_that_is_not_webassembly_is_refused() {
    assert_malformed(WasmComponent::load(b"not a module"));
}

#[test]
fn a_newer_version_is_refused_with_its_own_message() {
    let wasm = wat(Wat { abi_version: Some(2), render: "(i64.const 0)", ..Wat::default() });
    let error = WasmComponent::load(&wasm).err().unwrap();
    assert_eq!(error, ComponentError::NewerVersion(2));
    assert_eq!(error.short(), "built by a newer version of this app");
}

#[test]
fn a_module_too_large_is_refused_before_it_is_compiled() {
    // Not WebAssembly at all, so only a size check could give TooLarge.
    let bytes = vec![0u8; MAX_MODULE_BYTES + 1];
    assert_eq!(WasmComponent::load(&bytes).err(), Some(ComponentError::TooLarge));
}

#[test]
fn each_render_starts_fresh() {
    let answer = text_answer("0");
    let digit_at = ANSWER_AT + answer.find("\"0\"").unwrap() as u32 + 1;
    let extra = format!("(global $count (mut i32) (i32.const 0))\n{}", data(ANSWER_AT, answer.as_bytes()));
    let render = format!(
        "(global.set $count (i32.add (global.get $count) (i32.const 1)))
         (i32.store8 (i32.const {digit_at}) (i32.add (i32.const 48) (global.get $count)))
         (i64.const {})",
        packed(ANSWER_AT, answer.len())
    );
    let component = load(&wat(Wat { extra: &extra, render: &render, ..Wat::default() }));
    let first = component.render(&input(), no_assets()).unwrap();
    let second = component.render(&input(), no_assets()).unwrap();
    assert_eq!(first.element, toolkit::text("1"));
    assert_eq!(second, first);
}

#[test]
fn an_asset_path_outside_assets_reads_nothing() {
    let calls = Rc::new(Cell::new(0));
    let counted = calls.clone();
    let assets: Assets = Rc::new(move |_: &str| {
        counted.set(counted.get() + 1);
        Some(b"secret".to_vec())
    });
    let output = load(&reading("assets/../slides.md", &text_answer("done"))).render(&input(), assets).unwrap();
    assert_eq!(output.element, toolkit::text("done"));
    assert_eq!(calls.get(), 0);
}

#[test]
fn a_path_out_of_bounds_reads_nothing() {
    let (assets, asked) = counted_assets(b"x");
    let answer = text_answer("done");
    let extra = data(ANSWER_AT, answer.as_bytes());
    let render = format!("(drop (call $read_asset (i32.const -16) (i32.const 64))) (i64.const {})", packed(ANSWER_AT, answer.len()));
    let wasm = wat(Wat { imports: READ_ASSET, extra: &extra, render: &render, ..Wat::default() });
    assert!(load(&wasm).render(&input(), assets).is_ok());
    assert!(asked.borrow().is_empty());
}

#[test]
fn an_asset_reaches_the_component() {
    // The asset is itself the answer, so the module hands back exactly what it was given.
    let answer = text_answer("from the asset");
    let (assets, asked) = counted_assets(Box::leak(answer.into_bytes().into_boxed_slice()));
    let path = "assets/answer.json";
    let path_at = ANSWER_AT;
    let extra = data(path_at, path.as_bytes());
    let render = format!("(call $read_asset (i32.const {path_at}) (i32.const {}))", path.len());
    let wasm = wat(Wat { imports: READ_ASSET, extra: &extra, render: &render, ..Wat::default() });
    let output = load(&wasm).render(&input(), assets).unwrap();
    assert_eq!(output.element, toolkit::text("from the asset"));
    assert_eq!(*asked.borrow(), [path]);
}

#[test]
fn at_most_64_assets_are_read_per_render() {
    let (assets, asked) = counted_assets(b"x");
    let path = "assets/a.txt";
    let answer = text_answer("done");
    let extra = format!("{}\n{}", data(ANSWER_AT - 1024, path.as_bytes()), data(ANSWER_AT, answer.as_bytes()));
    let render = format!(
        "(local $i i32)
         (loop $again
           (drop (call $read_asset (i32.const {}) (i32.const {})))
           (local.set $i (i32.add (local.get $i) (i32.const 1)))
           (br_if $again (i32.lt_u (local.get $i) (i32.const 100))))
         (i64.const {})",
        ANSWER_AT - 1024,
        path.len(),
        packed(ANSWER_AT, answer.len())
    );
    let component = load(&wat(Wat { imports: READ_ASSET, extra: &extra, render: &render, ..Wat::default() }));
    assert!(component.render(&input(), assets.clone()).is_ok());
    assert_eq!(asked.borrow().len(), 64);
    // The count is the render's, not the component's.
    assert!(component.render(&input(), assets).is_ok());
    assert_eq!(asked.borrow().len(), 128);
}

#[test]
fn a_picture_outside_assets_becomes_a_problem() {
    let answer = r#"{"element":{"Stack":{"horizontal":false,"gap":0,"children":[
        {"Picture":{"src":"assets/logo.png","width":10,"height":10}},
        {"Picture":{"src":"../secret.png","width":10,"height":10}},
        {"Shape":{"kind":"Rect","width":10,"height":10,"fill":null,"outline":null,"outline_width":0,"radius":0,"padding":0,
            "child":{"Picture":{"src":"/etc/passwd","width":10,"height":10}}}}]}}}"#;
    let output = render(&answering(answer)).unwrap();
    let Element::Stack { children, .. } = output.element else { panic!("not a stack: {:?}", output.element) };
    assert_eq!(children[0], toolkit::picture("assets/logo.png", 10.0, 10.0));
    assert!(matches!(children[1], Element::Problem { .. }), "{:?}", children[1]);
    let Element::Shape { child: Some(child), .. } = &children[2] else { panic!("not a shape: {:?}", children[2]) };
    assert!(matches!(**child, Element::Problem { .. }), "{child:?}");
}

#[test]
fn each_error_has_words_for_its_box() {
    let all = [
        ComponentError::TooLarge,
        ComponentError::Malformed("x".into()),
        ComponentError::NewerVersion(2),
        ComponentError::TookTooLong,
        ComponentError::OutOfMemory,
        ComponentError::Crashed,
        ComponentError::BadOutput("x".into()),
    ];
    let words: Vec<_> = all.iter().map(ComponentError::short).collect();
    assert_eq!(
        words,
        [
            "too large to run",
            "can't be loaded",
            "built by a newer version of this app",
            "took too long",
            "used too much memory",
            "stopped working",
            "gave an answer that can't be read",
        ]
    );
}

/// Built once from `fixtures/v1/score.rs` and never again: see `fixtures/v1/README.md`.
const SCORE_V1: &[u8] = include_bytes!("fixtures/v1/score.wasm");

#[test]
fn a_component_built_for_version_one_still_runs() {
    let component = load(SCORE_V1);
    let field = |name: &str, kind: PropKind| PropField::new(name, kind, false);
    assert_eq!(
        *component.schema(),
        ComponentSchema::new("Score", vec![field("team", PropKind::Text), field("points", PropKind::Whole), field("colour", PropKind::Color)], false)
    );

    let mut input = input();
    input.props = [("team", "Oslo"), ("points", "3"), ("colour", "green")].map(|(k, v)| (k.to_string(), v.to_string())).into();
    let output = component.render(&input, no_assets()).unwrap();
    let green = Color::rgb(0, 128, 0);
    let points = Element::Text { text: "3".into(), size: Some(40.0), bold: true, color: None };
    let badge = Element::Shape {
        kind: ShapeKind::Rounded,
        width: 120.0,
        height: 80.0,
        fill: Some(green),
        outline: None,
        outline_width: 0.0,
        radius: 0.0,
        padding: 8.0,
        child: Some(Box::new(points)),
    };
    let team = Element::Text { text: "Oslo".into(), size: Some(48.0), bold: false, color: None };
    assert_eq!(
        output,
        RenderOutput::new(Element::Stack { horizontal: true, gap: 24.0, children: vec![team, badge] }, 0, false, false)
    );
}

/// Where a large answer sits, past the heap the input is written to.
const FAR_AT: u32 = 1024 * 1024;

/// A module whose `render` gives `answer`, which can be up to 3 MiB.
fn answering_at_length(answer: &str) -> Vec<u8> {
    let extra = data(FAR_AT, answer.as_bytes());
    let render = format!("(i64.const {})", packed(FAR_AT, answer.len()));
    wat(Wat { extra: &extra, render: &render, pages: Some(64), ..Wat::default() })
}

#[test]
fn an_answer_placing_children_twice_is_refused() {
    let once = r#"{"element":{"Stack":{"horizontal":false,"gap":0,"children":["Children","Empty"]}}}"#;
    assert!(render(&answering(once)).is_ok());
    let twice = r#"{"element":{"Stack":{"horizontal":false,"gap":0,"children":["Children",{"Shape":{"kind":"Rect","width":1,"height":1,"fill":null,"outline":null,"outline_width":0,"radius":0,"padding":0,"child":"Children"}}]}}}"#;
    assert_bad_output(render(&answering(twice)));
}

#[test]
fn an_answer_with_too_much_text_is_refused() {
    let limit = 256 * 1024;
    assert!(render(&answering_at_length(&text_answer(&"a".repeat(limit)))).is_ok());
    assert_bad_output(render(&answering_at_length(&text_answer(&"a".repeat(limit + 1)))));
    // Spread over many elements, it still counts.
    let half = format!(r#"{{"Problem":{{"message":"{}"}}}}"#, "b".repeat(limit / 2));
    let icon = r#"{"Icon":{"name":"star","size":10,"color":null}}"#;
    let spread = format!(r#"{{"element":{{"Stack":{{"horizontal":false,"gap":0,"children":[{half},{half},{icon}]}}}}}}"#);
    assert_bad_output(render(&answering_at_length(&spread)));
}

/// A module whose schema is `schema`.
fn with_schema(schema: &str) -> Vec<u8> {
    let at = ANSWER_AT + 4096;
    let extra = data(at, schema.as_bytes());
    let body = format!("(i64.const {})", packed(at, schema.len()));
    wat(Wat { extra: &extra, schema: Some(&body), render: "(i64.const 0)", ..Wat::default() })
}

fn schema_json(name: &str, props: &[(&str, &str)]) -> String {
    let props: Vec<String> = props.iter().map(|(name, kind)| format!(r#"{{"name":"{name}","kind":{kind},"optional":true}}"#)).collect();
    format!(r#"{{"name":"{name}","props":[{}],"children":false}}"#, props.join(","))
}

#[test]
fn a_schema_holds_its_strings_to_64_bytes() {
    let long = "N".repeat(65);
    assert!(WasmComponent::load(&with_schema(&schema_json(&"N".repeat(64), &[]))).is_ok());
    assert_malformed(WasmComponent::load(&with_schema(&schema_json(&long, &[]))));
    assert_malformed(WasmComponent::load(&with_schema(&schema_json("Test", &[(&long, r#""Text""#)]))));
    let choice = format!(r#"{{"Choice":["a","{long}"]}}"#);
    assert_malformed(WasmComponent::load(&with_schema(&schema_json("Test", &[("look", &choice)]))));
    let file = format!(r#"{{"File":["{long}"]}}"#);
    assert_malformed(WasmComponent::load(&with_schema(&schema_json("Test", &[("data", &file)]))));
}

#[test]
fn a_schema_has_at_most_64_props() {
    let names: Vec<String> = (0..65).map(|n| format!("p{n}")).collect();
    let props: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), r#""Text""#)).collect();
    assert!(WasmComponent::load(&with_schema(&schema_json("Test", &props[..64]))).is_ok());
    assert_malformed(WasmComponent::load(&with_schema(&schema_json("Test", &props))));
}

#[test]
fn a_module_over_four_mebibytes_is_too_large() {
    assert_eq!(MAX_MODULE_BYTES, 4 * 1024 * 1024);
}

#[test]
fn a_schema_with_a_name_kept_for_the_slide_is_refused() {
    for name in ["x", "click", "slot"] {
        assert_malformed(WasmComponent::load(&with_schema(&schema_json("Test", &[(name, r#""Text""#)]))));
    }
}

const LATER: &str = r#"(import "toolkit" "draw_later" (func $later (param i32) (result i32)))"#;

#[test]
fn a_toolkit_function_this_version_does_not_know_loads_and_traps_if_called() {
    let answer = text_answer("hi");
    let extra = data(ANSWER_AT, answer.as_bytes());
    let render = format!("(i64.const {})", packed(ANSWER_AT, answer.len()));
    let wasm = wat(Wat { imports: LATER, extra: &extra, render: &render, ..Wat::default() });
    assert_eq!(render_with(&wasm).unwrap().element, toolkit::text("hi"));
    let calling = wat(Wat { imports: LATER, render: "(drop (call $later (i32.const 1))) (i64.const 0)", ..Wat::default() });
    assert_eq!(render_with(&calling), Err(ComponentError::Crashed));
}

fn render_with(wasm: &[u8]) -> Result<RenderOutput, ComponentError> {
    WasmComponent::load(wasm)?.render(&input(), no_assets())
}

#[test]
fn a_toolkit_function_of_a_newer_version_says_so() {
    let wasm = wat(Wat { abi_version: Some(2), imports: LATER, render: "(i64.const 0)", ..Wat::default() });
    assert_eq!(WasmComponent::load(&wasm).err(), Some(ComponentError::NewerVersion(2)));
}

#[test]
fn a_toolkit_import_that_is_not_a_function_is_refused() {
    let imports = r#"(import "toolkit" "later" (global i32))"#;
    assert_malformed(WasmComponent::load(&wat(Wat { imports, render: "(i64.const 0)", ..Wat::default() })));
}
