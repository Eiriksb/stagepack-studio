use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slides_components::{BuildEntry, BuildIndex, INDEX_FILE, MEMORY_LIMIT_BYTES, source_hash};
use slides_engine::*;
use toolkit::{Ctx, Element, Registry, component, text};

// Modules, written in WAT, that answer with fixed JSON.

/// Where a module's schema sits in its memory.
const SCHEMA_AT: u32 = 1024;
/// Where its answers sit.
const ANSWER_AT: u32 = 8192;
/// Where what it looks for sits.
const NEEDLE_AT: u32 = 6144;
/// Where `alloc` starts handing out room, past everything else.
const HEAP_AT: u32 = 4 * 65536;
const READ_ASSET: &str = r#"(import "toolkit" "read_asset" (func $read_asset (param i32 i32) (result i64)))"#;

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

fn data(at: u32, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|byte| format!("\\{byte:02x}")).collect();
    format!(r#"(data (i32.const {at}) "{escaped}")"#)
}

fn packed(at: u32, len: usize) -> i64 {
    ((u64::from(at) << 32) | len as u64) as i64
}

/// A module that calls itself `name`, with `extra` at its top level and `render` as the
/// body of `render(ptr, len) -> i64`.
fn module(name: &str, imports: &str, extra: &str, render: &str) -> Vec<u8> {
    let schema = format!(r#"{{"name":"{name}","props":[],"children":true}}"#);
    let schema_data = data(SCHEMA_AT, schema.as_bytes());
    let schema_answer = packed(SCHEMA_AT, schema.len());
    let text = format!(
        r#"(module
  {imports}
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const {HEAP_AT}))
  {schema_data}
  {extra}
  {CONTAINS}
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {schema_answer}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64) {render}))"#
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// A module called `name` whose every render answers `answer`.
fn answering(name: &str, answer: &str) -> Vec<u8> {
    let extra = data(ANSWER_AT, answer.as_bytes());
    module(name, "", &extra, &format!("(i64.const {})", packed(ANSWER_AT, answer.len())))
}

/// A module called `name` that answers with the text `yes` if its input holds `needle`, and `no` if not.
fn looking_for(name: &str, needle: &str) -> Vec<u8> {
    let (yes, no) = (text_answer("yes"), text_answer("no"));
    let no_at = ANSWER_AT + yes.len() as u32;
    let extra = format!("{}\n{}\n{}", data(NEEDLE_AT, needle.as_bytes()), data(ANSWER_AT, yes.as_bytes()), data(no_at, no.as_bytes()));
    let render = format!(
        "(if (result i64) (call $contains (local.get $ptr) (local.get $len) (i32.const {NEEDLE_AT}) (i32.const {})) (then (i64.const {})) (else (i64.const {})))",
        needle.len(),
        packed(ANSWER_AT, yes.len()),
        packed(no_at, no.len())
    );
    module(name, "", &extra, &render)
}

/// A module called `name` that reads the asset at `path` and answers `yes` if it got it, and `no` if not.
fn reading(name: &str, path: &str) -> Vec<u8> {
    let (yes, no) = (text_answer("yes"), text_answer("no"));
    let no_at = ANSWER_AT + yes.len() as u32;
    let extra = format!("{}\n{}\n{}", data(NEEDLE_AT, path.as_bytes()), data(ANSWER_AT, yes.as_bytes()), data(no_at, no.as_bytes()));
    let render = format!(
        "(if (result i64) (i64.eqz (call $read_asset (i32.const {NEEDLE_AT}) (i32.const {}))) (then (i64.const {})) (else (i64.const {})))",
        path.len(),
        packed(no_at, no.len()),
        packed(ANSWER_AT, yes.len())
    );
    module(name, READ_ASSET, &extra, &render)
}

fn text_answer(text: &str) -> String {
    format!(r#"{{"element":{{"Text":{{"text":"{text}","size":null,"bold":false,"color":null}}}}}}"#)
}

// A talk's files, kept in memory.

#[derive(Clone, Default)]
struct Files(Rc<RefCell<BTreeMap<String, Vec<u8>>>>);

impl Files {
    fn put(&self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.0.borrow_mut().insert(path.to_string(), bytes.into());
    }
}

impl TalkFiles for Files {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.borrow().get(path).cloned()
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.0.borrow().get(path).map(|bytes| bytes.len() as u64)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        let prefix = format!("{folder}/");
        let files = self.0.borrow();
        files.keys().filter(|name| name.strip_prefix(&prefix).is_some_and(|rest| !rest.contains('/'))).cloned().collect()
    }
}

/// The source of a component called `tag`.
fn source(tag: &str) -> String {
    format!("use toolkit::prelude::*;\n\n#[component]\nfn {tag}() -> Element {{\n    text(\"{tag}\")\n}}\n")
}

fn index(entries: &[(&str, &str, &str)]) -> Vec<u8> {
    let components = entries
        .iter()
        .map(|(tag, file, built_from)| BuildEntry { tag: tag.to_string(), file: file.to_string(), source_hash: source_hash(built_from.as_bytes()) })
        .collect();
    serde_json::to_vec(&BuildIndex { format: 1, components }).unwrap()
}

/// A talk of `slides`, with `components/hello.rs` for `Hello` built into `wasm`.
fn talk_with_hello(slides: &str, wasm: Vec<u8>) -> Files {
    let files = Files::default();
    files.put("slides.md", slides);
    files.put("components/hello.rs", source("Hello"));
    files.put("build/hello.wasm", wasm);
    files.put(INDEX_FILE, index(&[("Hello", "hello", &source("Hello"))]));
    files
}

fn open(files: &Files) -> Deck {
    let slides = String::from_utf8(files.read("slides.md").unwrap()).unwrap();
    Deck::open(Box::new(files.clone()), Fonts::bundled_only(), slides)
}

fn open_with(files: &Files, registry: Registry) -> Deck {
    let slides = String::from_utf8(files.read("slides.md").unwrap()).unwrap();
    Deck::with_registry(Box::new(files.clone()), Fonts::bundled_only(), slides, registry)
}

/// Every piece of text drawn on slide `index`, a paragraph at a time.
fn texts(deck: &mut Deck, index: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for spec in deck.specs(index) {
        for item in &spec.items {
            if let DisplayItem::Text { text, .. } = item {
                lines.extend(text.buffer.lines.iter().map(|line| line.text().trim_end().to_string()));
            }
        }
    }
    lines
}

fn shows(deck: &mut Deck, index: usize, wanted: &str) -> bool {
    texts(deck, index).iter().any(|line| line == wanted)
}

fn dashed_outlines(deck: &mut Deck, index: usize) -> usize {
    deck.specs(index).iter().flat_map(|spec| &spec.items).filter(|item| matches!(item, DisplayItem::Stroke { .. })).count()
}

fn info(deck: &Deck, tag: &str) -> ComponentInfo {
    deck.components().iter().find(|info| info.tag == tag).cloned().unwrap_or_else(|| panic!("no {tag} in {:?}", deck.components()))
}

fn has_problem(deck: &Deck, file: &str) -> bool {
    deck.component_problems().iter().any(|problem| problem.file == file)
}

#[test]
fn a_component_from_the_talk_draws_on_its_slide() {
    let files = talk_with_hello("# A slide\n\n<Hello />\n", answering("Hello", &text_answer("Hello from the talk")));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "Hello from the talk"), "{:?}", texts(&mut deck, 0));
    assert_eq!(deck.notices(0), Vec::<String>::new());
    assert_eq!(info(&deck, "Hello"), ComponentInfo { tag: "Hello".into(), file: "components/hello.rs".into(), state: ComponentState::UpToDate });
    assert_eq!(deck.component_problems(), &[]);
}

#[test]
fn its_tag_attributes_reach_it() {
    let files = talk_with_hello("<Hello name=\"Ada\" />\n\n---\n\n<Hello name=\"Bo\" />\n", looking_for("Hello", r#""name":"Ada""#));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "yes"), "{:?}", texts(&mut deck, 0));
    assert!(shows(&mut deck, 1, "no"), "{:?}", texts(&mut deck, 1));
}

#[test]
fn children_go_where_the_component_puts_them() {
    let answer = r#"{"element":{"Stack":{"horizontal":false,"gap":0.0,"children":[{"Text":{"text":"Before","size":null,"bold":false,"color":null}},"Children"]}}}"#;
    let files = talk_with_hello("<Hello>\n\nInside the tags\n\n</Hello>\n", answering("Hello", answer));
    let mut deck = open(&files);
    assert_eq!(texts(&mut deck, 0), ["Before", "Inside the tags"]);
}

#[test]
fn a_component_with_no_build_shows_that_it_needs_building() {
    let files = Files::default();
    files.put("slides.md", "<Hello />\n");
    files.put("components/hello.rs", source("Hello"));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "Hello needs building"), "{:?}", texts(&mut deck, 0));
    assert_eq!(dashed_outlines(&mut deck, 0), 1, "the placeholder");
    assert!(deck.notices(0).contains(&"Hello needs building".to_string()));
    assert_eq!(info(&deck, "Hello").state, ComponentState::NeedsBuilding);
}

#[test]
fn an_out_of_date_component_runs_its_last_build_and_says_so() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("The last build")));
    files.put(INDEX_FILE, index(&[("Hello", "hello", "an older source")]));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "The last build"), "{:?}", texts(&mut deck, 0));
    assert_eq!(deck.notices(0), ["Hello is out of date, so this is its last build"]);
    assert_eq!(info(&deck, "Hello").state, ComponentState::OutOfDate);
}

#[test]
fn a_component_that_crashes_leaves_the_rest_of_the_slide_alone() {
    let files = talk_with_hello("# Still here\n\n<Hello />\n", module("Hello", "", "", "unreachable"));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "Still here"), "{:?}", texts(&mut deck, 0));
    assert!(shows(&mut deck, 0, "Hello: stopped working"), "{:?}", texts(&mut deck, 0));
    assert!(deck.notices(0).contains(&"Hello: stopped working".to_string()));
}

#[test]
fn a_slide_of_hanging_components_stays_within_its_budget() {
    let slides = "<Hang />\n\n".repeat(40);
    let files = Files::default();
    files.put("slides.md", slides);
    files.put("components/hang.rs", source("Hang"));
    files.put("build/hang.wasm", module("Hang", "", "", "(loop br 0) (i64.const 0)"));
    files.put(INDEX_FILE, index(&[("Hang", "hang", &source("Hang"))]));
    let mut deck = open(&files);
    let started = Instant::now();
    let lines = texts(&mut deck, 0);
    assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
    assert_eq!(lines.iter().filter(|line| line.starts_with("Hang: ")).count(), 40, "{lines:?}");
    assert_eq!(dashed_outlines(&mut deck, 0), 40);
}

#[test]
fn a_build_claiming_another_name_does_not_run() {
    let files = talk_with_hello("<Hello />\n", answering("Other", &text_answer("Should not show")));
    let mut deck = open(&files);
    assert!(!shows(&mut deck, 0, "Should not show"));
    assert!(shows(&mut deck, 0, "Hello: can't be loaded"), "{:?}", texts(&mut deck, 0));
    assert_eq!(deck.component_schema("Hello"), None);
}

#[test]
fn a_component_cannot_take_a_built_ins_name() {
    let files = Files::default();
    files.put("slides.md", "<Box fill=\"#ff0000\">\n\nIn the box\n\n</Box>\n");
    files.put("components/box.rs", source("Box"));
    files.put("build/box.wasm", answering("Box", &text_answer("The talk's own box")));
    files.put(INDEX_FILE, index(&[("Box", "box", &source("Box"))]));
    let mut deck = open(&files);
    assert_eq!(texts(&mut deck, 0), ["In the box"]);
    assert!(deck.components().iter().all(|info| info.tag != "Box"));
    assert!(has_problem(&deck, "components/box.rs"), "{:?}", deck.component_problems());
}

#[test]
fn a_broken_index_counts_as_no_builds() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("Hello from the talk")));
    files.put(INDEX_FILE, "this isn't JSON");
    let mut deck = open(&files);
    assert_eq!(info(&deck, "Hello").state, ComponentState::NeedsBuilding);
    assert!(has_problem(&deck, INDEX_FILE), "{:?}", deck.component_problems());
    assert!(shows(&mut deck, 0, "Hello needs building"), "{:?}", texts(&mut deck, 0));
}

#[test]
fn a_file_with_an_unsafe_name_is_a_problem() {
    let files = Files::default();
    files.put("slides.md", "<Hello />\n");
    files.put("components/hello world.rs", source("Hello"));
    files.put("components/notes.md", "Not a component");
    let deck = open(&files);
    assert_eq!(deck.components(), &[]);
    assert!(has_problem(&deck, "components/hello world.rs"), "{:?}", deck.component_problems());
    assert_eq!(deck.component_problems().len(), 1, "{:?}", deck.component_problems());
}

#[test]
fn a_source_over_the_limit_is_a_problem() {
    let files = Files::default();
    files.put("slides.md", "<Hello />\n");
    files.put("components/hello.rs", format!("{}//{}\n", source("Hello"), "x".repeat(1024 * 1024)));
    let deck = open(&files);
    assert_eq!(deck.components(), &[]);
    assert!(has_problem(&deck, "components/hello.rs"), "{:?}", deck.component_problems());
}

#[test]
fn too_many_sources_stop_at_the_limit() {
    let files = Files::default();
    files.put("slides.md", "# Many\n");
    for n in 0..300 {
        files.put(&format!("components/c{n:03}.rs"), source(&format!("C{n:03}")));
    }
    let deck = open(&files);
    let tags: Vec<&str> = deck.components().iter().map(|info| info.tag.as_str()).collect();
    assert_eq!(tags.len(), 256);
    assert_eq!((tags[0], tags[255]), ("C000", "C255"));
    assert!(has_problem(&deck, "components"), "{:?}", deck.component_problems());
}

#[test]
fn two_sources_claiming_one_tag_are_a_problem() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("Hello from the talk")));
    files.put("components/zzz.rs", source("Hello"));
    let mut deck = open(&files);
    assert_eq!(deck.components().len(), 1);
    assert_eq!(info(&deck, "Hello").file, "components/hello.rs");
    assert!(has_problem(&deck, "components/zzz.rs"), "{:?}", deck.component_problems());
    assert!(shows(&mut deck, 0, "Hello from the talk"));
}

#[test]
fn a_new_build_is_picked_up() {
    let files = Files::default();
    files.put("slides.md", "# One\n\n---\n\n<Hello />\n");
    files.put("components/hello.rs", source("Hello"));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 1, "Hello needs building"));
    assert_eq!(deck.refresh_components(), Vec::<usize>::new(), "nothing changed");

    files.put("build/hello.wasm", answering("Hello", &text_answer("First build")));
    files.put(INDEX_FILE, index(&[("Hello", "hello", &source("Hello"))]));
    assert!(deck.refresh_components().contains(&1));
    assert!(shows(&mut deck, 1, "First build"), "{:?}", texts(&mut deck, 1));
    assert_eq!(info(&deck, "Hello").state, ComponentState::UpToDate);

    // A build replaced with the slides' text just as it was.
    files.put("build/hello.wasm", answering("Hello", &text_answer("Second build")));
    let slides = deck.source().to_string();
    assert!(deck.update(slides).contains(&1));
    assert!(shows(&mut deck, 1, "Second build"), "{:?}", texts(&mut deck, 1));
}

#[test]
fn a_build_with_no_source_still_runs() {
    let files = Files::default();
    files.put("slides.md", "<Hello />\n");
    files.put("build/hello.wasm", answering("Hello", &text_answer("Built elsewhere")));
    files.put(INDEX_FILE, index(&[("Hello", "hello", "a source that isn't here")]));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "Built elsewhere"), "{:?}", texts(&mut deck, 0));
    assert_eq!(info(&deck, "Hello").state, ComponentState::UpToDate);
}

#[test]
fn the_schema_of_a_component_is_there_for_a_form() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("Hello from the talk")));
    let mut deck = open(&files);
    assert_eq!(deck.component_schema("Box").map(|schema| schema.name), Some("Box".to_string()));
    let hello = deck.component_schema("Hello").expect("the talk's own component has a schema");
    assert_eq!((hello.name.as_str(), hello.children), ("Hello", true));
    assert_eq!(deck.component_schema("Nothing"), None);
}

#[test]
fn a_packed_talk_runs_its_components() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir_all(folder.join("components")).unwrap();
    std::fs::create_dir_all(folder.join("build")).unwrap();
    std::fs::write(folder.join("slides.md"), "<Hello />\n").unwrap();
    std::fs::write(folder.join("components/hello.rs"), source("Hello")).unwrap();
    std::fs::write(folder.join("build/hello.wasm"), answering("Hello", &text_answer("From the pack"))).unwrap();
    std::fs::write(folder.join(INDEX_FILE), index(&[("Hello", "hello", &source("Hello"))])).unwrap();

    let folder_deck = Deck::open(Box::new(FolderFiles::new(folder.clone())), Fonts::bundled_only(), "<Hello />\n".into());
    assert_eq!(folder_deck.components()[0].state, ComponentState::UpToDate);

    let out = dir.path().join("talk.slides");
    pack_folder(&folder, &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    let slides = packed.slides().unwrap();
    let mut deck = Deck::open(Box::new(packed), Fonts::bundled_only(), slides);
    assert_eq!(info(&deck, "Hello").state, ComponentState::UpToDate);
    assert!(shows(&mut deck, 0, "From the pack"), "{:?}", texts(&mut deck, 0));
}

#[test]
fn a_component_in_the_middle_of_a_line_is_noticed() {
    let files = talk_with_hello("Some text <Hello /> and more, <Text bold>bold</Text>\n", answering("Hello", &text_answer("Hi")));
    let mut deck = open(&files);
    assert_eq!(deck.notices(0), ["Hello is in the middle of a line. A component goes on a line of its own"]);
}

mod probes {
    use super::*;

    #[component]
    pub fn Measured(ctx: &Ctx) -> Element {
        text(format!("{}x{}", ctx.width(), ctx.height()))
    }

    /// Says which of a few files it could read.
    #[component]
    pub fn Reader(ctx: &Ctx) -> Element {
        let paths = ["assets/a.txt", "slides.md", "build/index.json", "assets/../slides.md", "assets/big.bin"];
        let found: Vec<String> = paths.iter().map(|path| format!("{path}={}", ctx.asset(path).is_some())).collect();
        text(found.join(" "))
    }

    /// Reads five files of the assets folder, and says which it got.
    #[component]
    pub fn Hoarder(ctx: &Ctx) -> Element {
        let got: Vec<&str> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| if ctx.asset(&format!("assets/{name}.bin")).is_some() { "yes" } else { "no" })
            .collect();
        text(got.join(" "))
    }
}

fn probe_registry() -> Registry {
    let mut registry = builtin_registry();
    registry.add(probes::Measured_component());
    registry.add(probes::Reader_component());
    registry.add(probes::Hoarder_component());
    registry
}

#[test]
fn a_positioned_component_is_told_its_box() {
    let files = Files::default();
    files.put("slides.md", "<Measured x=\"0\" y=\"0\" w=\"500\" h=\"300\" />\n\n---\n\n<Measured x=\"10\" />\n\n---\n\n<Measured />\n");
    let mut deck = open_with(&files, probe_registry());
    assert_eq!(texts(&mut deck, 0), ["500x300"]);
    assert_eq!(texts(&mut deck, 1), ["1200x0"]);
    assert_eq!(texts(&mut deck, 2), ["1720x0"], "in the flow, the region's width");

    let files = talk_with_hello("<Hello x=\"0\" y=\"0\" w=\"500\" h=\"300\" />\n", looking_for("Hello", r#","width":500.0,"height":300.0,"#));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "yes"), "a talk's component too: {:?}", texts(&mut deck, 0));
}

#[test]
fn a_built_in_reads_only_assets() {
    let files = Files::default();
    files.put("slides.md", "<Reader />\n");
    files.put("assets/a.txt", "a");
    files.put(INDEX_FILE, index(&[]));
    let mut deck = open_with(&files, probe_registry());
    assert_eq!(
        texts(&mut deck, 0).concat(),
        "assets/a.txt=true slides.md=false build/index.json=false assets/../slides.md=false assets/big.bin=false"
    );
}

/// Files that say every file is `len` bytes long, if `len_known`, and count their reads.
struct Sized {
    len: u64,
    len_known: bool,
    reads: Rc<Cell<u32>>,
    talk: Files,
}

impl TalkFiles for Sized {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        if !path.starts_with("assets/") {
            return self.talk.read(path);
        }
        self.reads.set(self.reads.get() + 1);
        Some(vec![0; self.len as usize])
    }

    fn len(&self, path: &str) -> Option<u64> {
        if !path.starts_with("assets/") {
            return self.talk.len(path);
        }
        self.len_known.then_some(self.len)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        self.talk.names_in(folder)
    }
}

const LIMIT: u64 = MEMORY_LIMIT_BYTES as u64;

#[test]
fn an_asset_too_large_is_not_read() {
    // On disk, with the length from the file's metadata. The file takes no room.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/a.txt"), "a").unwrap();
    std::fs::File::create(dir.path().join("assets/big.bin")).unwrap().set_len(LIMIT + 1).unwrap();
    let mut deck = Deck::with_registry(Box::new(FolderFiles::new(dir.path().into())), Fonts::bundled_only(), "<Reader />\n".into(), probe_registry());
    let found = texts(&mut deck, 0).concat();
    assert!(found.starts_with("assets/a.txt=true") && found.ends_with("assets/big.bin=false"), "{found}");

    // Never read at all, by a built-in or by a talk's own component.
    let talk = talk_with_hello("<Hoarder />\n\n<Hello />\n", reading("Hello", "assets/big.bin"));
    let reads = Rc::new(Cell::new(0));
    let files = Sized { len: LIMIT + 1, len_known: true, reads: reads.clone(), talk };
    let mut deck = Deck::with_registry(Box::new(files), Fonts::bundled_only(), "<Hoarder />\n\n<Hello />\n".into(), probe_registry());
    assert_eq!(texts(&mut deck, 0), ["no no no no no", "no"]);
    assert_eq!(reads.get(), 0);
}

#[test]
fn a_renders_reads_stop_at_the_budget() {
    let a_third = LIMIT / 3;
    for len_known in [true, false] {
        let talk = Files::default();
        let reads = Rc::new(Cell::new(0));
        let files = Sized { len: a_third, len_known, reads: reads.clone(), talk };
        let mut deck = Deck::with_registry(Box::new(files), Fonts::bundled_only(), "<Hoarder />\n\n<Hoarder />\n".into(), probe_registry());
        assert_eq!(texts(&mut deck, 0), ["yes yes yes no no", "yes yes yes no no"], "each render has a budget of its own");
        let read_past_the_budget = if len_known { 0 } else { 2 };
        assert_eq!(reads.get(), 2 * (3 + read_past_the_budget), "length known: {len_known}");
    }
}
