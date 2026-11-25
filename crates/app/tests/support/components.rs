//! The talk's own components of `fixtures/components`, as modules that answer with fixed JSON,
//! for tests that need them built and drawing without compiling them.

#![allow(dead_code)]

use std::path::Path;

use slides_components::source_hash;

// The talk's own components, as modules that answer with fixed JSON.

/// Whether `$len` bytes at `$ptr` hold the `$nlen` bytes at `$needle`.
pub const CONTAINS: &str = r#"
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

pub fn data(at: u32, text: &str) -> String {
    let escaped: String = text.bytes().map(|byte| format!("\\{byte:02x}")).collect();
    format!(r#"(data (i32.const {at}) "{escaped}")"#)
}

pub fn packed(at: u32, text: &str) -> i64 {
    ((u64::from(at) << 32) | text.len() as u64) as i64
}

/// A module whose schema is `schema`, which draws `element` when what it's given holds each of
/// `needles`, and a problem when it doesn't, as a component whose arguments can't be read does.
pub fn module(schema: &str, needles: &[&str], element: &str) -> Vec<u8> {
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

pub fn words(text: &str) -> String {
    format!(r#"{{"Text":{{"text":"{text}","size":null,"bold":false,"color":null}}}}"#)
}

/// Each of the fixture's components: its file, its tag, its schema, what it needs to be given
/// to draw, and what it draws.
pub fn fixture_modules() -> Vec<(&'static str, &'static str, Vec<u8>)> {
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
pub enum Built {
    UpToDate,
    OutOfDate,
    Not,
}

/// A scratch copy of `fixtures/components`, with a build of each of its components, but as
/// `except` says for the one it names.
pub fn components_talk(except: Option<(&str, Built)>) -> tempfile::TempDir {
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

/// A module of interface version 2 whose schema is `schema`, which draws `element`, and stops
/// working, as a trap, when what it's given holds `trap_on`.
pub fn interactive_module(schema: &str, element: &str, trap_on: Option<&str>) -> Vec<u8> {
    let answer = format!(r#"{{"element":{element}}}"#);
    let trap = trap_on.map_or(String::new(), |needle| {
        format!("(if (call $contains (local.get $ptr) (local.get $len) (i32.const 4096) (i32.const {})) (then unreachable))", needle.len())
    });
    let text = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const 262144))
  {}
  {}
  {}
  {CONTAINS}
  (func (export "abi_version") (result i32) (i32.const 2))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64)
    {trap}
    (i64.const {})))"#,
        data(1024, schema),
        data(4096, trap_on.unwrap_or_default()),
        data(8192, &answer),
        packed(1024, schema),
        packed(8192, &answer),
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}
