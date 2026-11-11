use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slides_components::{BuildEntry, BuildIndex, INDEX_FILE, MEMORY_LIMIT_BYTES, source_hash};
use slides_engine::*;
use toolkit::{Children, Ctx, Element, Registry, component, text};

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
    module_of_pages(name, imports, extra, render, 8, HEAP_AT)
}

/// As `module`, with `pages` of memory and `alloc` starting at `heap`.
fn module_of_pages(name: &str, imports: &str, extra: &str, render: &str, pages: u32, heap: u32) -> Vec<u8> {
    let schema = format!(r#"{{"name":"{name}","props":[],"children":true}}"#);
    let schema_data = data(SCHEMA_AT, schema.as_bytes());
    let schema_answer = packed(SCHEMA_AT, schema.len());
    let text = format!(
        r#"(module
  {imports}
  (memory (export "memory") {pages})
  (global $heap (mut i32) (i32.const {heap}))
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

    /// Places what's between its tags twice.
    #[component]
    pub fn Twice(_children: Children) -> Element {
        toolkit::col![Element::from(Children), Element::from(Children)]
    }

    /// Shows what `assets/a.txt` says.
    #[component]
    pub fn Echo(ctx: &Ctx) -> Element {
        text(ctx.asset_text("assets/a.txt").unwrap_or_else(|| "nothing".to_string()))
    }

    /// Text at first, and a problem from its first click on.
    #[component]
    pub fn Late(ctx: &Ctx) -> Element {
        if ctx.steps(1) >= 1 { toolkit::problem("Late trouble") } else { text("early") }
    }

    /// Reads a path that differs at every moment, and isn't there.
    #[component]
    pub fn Wanderer(ctx: &Ctx) -> Element {
        let path = format!("assets/missing-{}.txt", ctx.time());
        text(if ctx.asset(&path).is_some() { "found" } else { "missing" })
    }

    /// The slide's time.
    #[component]
    pub fn Ticking(ctx: &Ctx) -> Element {
        text(ctx.time().to_string())
    }

    /// A stack of 20,000 empty shapes.
    #[component]
    pub fn Crowd() -> Element {
        toolkit::stack(false, vec![toolkit::shape(toolkit::ShapeKind::Rect, 1.0, 1.0); 20_000])
    }

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
    registry.add(probes::Twice_component());
    registry.add(probes::Crowd_component());
    registry.add(probes::Echo_component());
    registry.add(probes::Late_component());
    registry.add(probes::Wanderer_component());
    registry.add(probes::Ticking_component());
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

/// Files that count each path's reads, and give no stamp.
struct Counted<T: TalkFiles> {
    inner: T,
    reads: Rc<RefCell<BTreeMap<String, u32>>>,
}

impl<T: TalkFiles> Counted<T> {
    fn new(inner: T) -> (Counted<T>, Rc<RefCell<BTreeMap<String, u32>>>) {
        let reads = Rc::new(RefCell::new(BTreeMap::new()));
        (Counted { inner, reads: reads.clone() }, reads)
    }
}

impl<T: TalkFiles> TalkFiles for Counted<T> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        *self.reads.borrow_mut().entry(path.to_string()).or_default() += 1;
        self.inner.read(path)
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.inner.len(path)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        self.inner.names_in(folder)
    }

    fn never_changes(&self) -> bool {
        self.inner.never_changes()
    }
}

#[test]
fn a_huge_index_reads_each_build_once_and_is_cut_short() {
    let files = Files::default();
    files.put("slides.md", "<T0 />\n");
    files.put("build/x.wasm", answering("T0", &text_answer("Zero")));
    let tags: Vec<String> = (0..2500).map(|n| format!("T{n}")).collect();
    let mut entries: Vec<(&str, &str, &str)> = tags.iter().map(|tag| (tag.as_str(), "x", "")).collect();
    entries.extend(std::iter::repeat_n(("Box", "x", ""), 2500));
    files.put(INDEX_FILE, index(&entries));
    let (counted, reads) = Counted::new(files);
    let mut deck = Deck::open(Box::new(counted), Fonts::bundled_only(), "<T0 />\n".into());
    assert_eq!(reads.borrow().get("build/x.wasm").copied(), Some(1), "one scan reads a build once");
    assert_eq!(deck.components().len(), 256);
    assert!(has_problem(&deck, INDEX_FILE), "{:?}", deck.component_problems());
    assert!(shows(&mut deck, 0, "Zero"), "{:?}", texts(&mut deck, 0));
}

#[test]
fn a_packed_talks_builds_are_not_read_again_by_an_update() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir_all(folder.join("components")).unwrap();
    std::fs::create_dir_all(folder.join("build")).unwrap();
    std::fs::write(folder.join("slides.md"), "<Hello />\n").unwrap();
    std::fs::write(folder.join("components/hello.rs"), source("Hello")).unwrap();
    std::fs::write(folder.join("build/hello.wasm"), answering("Hello", &text_answer("From the pack"))).unwrap();
    std::fs::write(folder.join(INDEX_FILE), index(&[("Hello", "hello", &source("Hello"))])).unwrap();
    let out = dir.path().join("talk.slides");
    pack_folder(&folder, &out, "0.1.0").unwrap();

    let (counted, reads) = Counted::new(open_packed(&out).unwrap());
    let mut deck = Deck::open(Box::new(counted), Fonts::bundled_only(), "<Hello />\n".into());
    assert!(shows(&mut deck, 0, "From the pack"));
    let before = reads.borrow().clone();
    assert_eq!(deck.update("<Hello />\n".into()), Vec::<usize>::new());
    assert_eq!(deck.update("<Hello />\n\n# More\n".into()), vec![0]);
    assert!(shows(&mut deck, 0, "From the pack"));
    assert_eq!(*reads.borrow(), before, "nothing of the pack is read again");
}

// What a talk can make the app lay out.

/// Where a large answer sits, past where the input is written.
const FAR_AT: u32 = 6_000_000;

/// A module called `name` whose every render answers `answer`, which can be megabytes long.
fn answering_large(name: &str, answer: &str) -> Vec<u8> {
    let extra = data(FAR_AT, answer.as_bytes());
    module_of_pages(name, "", &extra, &format!("(i64.const {})", packed(FAR_AT, answer.len())), 200, 4 * 65536)
}

/// A talk whose only component is `Fan`, a build with no source that answers `answer`.
fn talk_with_fan(slides: &str, answer: &str) -> Files {
    let files = Files::default();
    files.put("slides.md", slides);
    files.put("build/fan.wasm", answering_large("Fan", answer));
    files.put(INDEX_FILE, index(&[("Fan", "fan", "x")]));
    files
}

/// `<Fan>` nested `levels` deep around `inside`.
fn fans(levels: usize, inside: &str) -> String {
    format!("{}{inside}\n\n{}", "<Fan>\n\n".repeat(levels), "</Fan>\n\n".repeat(levels))
}

#[test]
fn a_component_placing_its_content_many_times_shows_a_box() {
    // From the review: a 2 KB talk with this took 101 s and 12 GB, and the nested one 8 s and 1 GB.
    for (children, levels, words) in [(19_990, 1, 400), (300, 2, 1)] {
        let answer = format!(r#"{{"element":{{"Stack":{{"horizontal":false,"gap":0.0,"children":[{}]}}}}}}"#, vec!["\"Children\""; children].join(","));
        let files = talk_with_fan(&fans(levels, &"word ".repeat(words)), &answer);
        let mut deck = open(&files);
        let started = Instant::now();
        let _ = deck.specs(0);
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
        let notices = deck.notices(0);
        assert!(notices.iter().any(|notice| notice == "Fan: gave an answer that can't be read"), "{notices:?}");
    }
}

#[test]
fn a_built_in_places_its_content_once() {
    let files = Files::default();
    files.put("slides.md", "<Twice>\n\nword\n\n</Twice>\n");
    let mut deck = open_with(&files, probe_registry());
    assert_eq!(texts(&mut deck, 0), ["word"]);
}

#[test]
fn a_slide_with_too_much_on_it_shows_a_box() {
    let files = Files::default();
    files.put("slides.md", "before\n\n<Crowd />\n\n<Crowd />\n\n<Crowd />\n\nafter\n");
    let mut deck = open_with(&files, probe_registry());
    let started = Instant::now();
    let shown = texts(&mut deck, 0);
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    assert!(shown.contains(&"before".to_string()), "{shown:?}");
    assert!(!shown.contains(&"after".to_string()), "nothing more is laid out: {shown:?}");
    assert!(deck.notices(0).iter().any(|notice| notice == "This slide has too much on it to show"), "{:?}", deck.notices(0));
}

#[test]
fn components_nested_deeply_lay_out_on_a_small_stack() {
    // 38 stacks around Children, in a tag nested 15 deep: about 570 levels before the limit.
    let laid_out = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut element = String::from("\"Children\"");
            for _ in 0..38 {
                element = format!(r#"{{"Stack":{{"horizontal":false,"gap":0.0,"children":[{element}]}}}}"#);
            }
            let files = talk_with_fan(&fans(15, "word"), &format!(r#"{{"element":{element}}}"#));
            let mut deck = open(&files);
            let _ = deck.layers(0, 0.5);
            deck.notices(0)
        })
        .unwrap()
        .join()
        .expect("it lays out without overflowing the stack");
    assert!(laid_out.iter().any(|notice| notice == "Some content is nested too deeply to show"), "{laid_out:?}");
}

/// `wasm` with a custom section of `extra` bytes, which loading skips over.
fn padded(mut wasm: Vec<u8>, extra: usize) -> Vec<u8> {
    let name = b"padding";
    let mut body = vec![name.len() as u8];
    body.extend_from_slice(name);
    body.resize(body.len() + extra, 0);
    wasm.push(0);
    let mut size = body.len();
    loop {
        let byte = (size & 0x7f) as u8;
        size >>= 7;
        if size == 0 {
            wasm.push(byte);
            break;
        }
        wasm.push(byte | 0x80);
    }
    wasm.extend(body);
    wasm
}

#[test]
fn a_talk_loads_at_most_16_mib_of_modules() {
    let files = Files::default();
    let tags = ["A", "B", "C", "D", "E"];
    files.put("slides.md", tags.map(|tag| format!("<{tag} />\n\n")).concat());
    for tag in tags {
        files.put(&format!("build/{}.wasm", tag.to_lowercase()), padded(answering(tag, &text_answer(tag)), 3_500_000));
    }
    let entries: Vec<(String, String)> = tags.iter().map(|tag| (tag.to_string(), tag.to_lowercase())).collect();
    let entries: Vec<(&str, &str, &str)> = entries.iter().map(|(tag, file)| (tag.as_str(), file.as_str(), "x")).collect();
    files.put(INDEX_FILE, index(&entries));
    let mut deck = open(&files);
    assert_eq!(texts(&mut deck, 0)[..4], ["A", "B", "C", "D"]);
    assert!(deck.notices(0).iter().any(|notice| notice == "E: too large to run"), "{:?}", deck.notices(0));
}

#[test]
fn a_long_problem_is_shortened_in_its_notice() {
    let answer = format!(r#"{{"element":{{"Problem":{{"message":"{}"}}}}}}"#, "é".repeat(1000));
    let files = talk_with_hello("<Hello />\n", answering_large("Hello", &answer));
    files.put("components/hello.rs", source("Hello"));
    let mut deck = open(&files);
    let notices = deck.notices(0);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].chars().count(), 200);
    assert!(notices[0].ends_with('…'));
}

#[test]
fn modules_that_fail_to_load_still_count_against_the_talks_budget() {
    let files = Files::default();
    let tags = ["A", "B", "C", "D", "E"];
    files.put("slides.md", tags.map(|tag| format!("<{tag} />\n\n")).concat());
    // Each is compiled before it's found to have none of what a component exports.
    for tag in tags {
        files.put(&format!("build/{}.wasm", tag.to_lowercase()), padded(wat::parse_str("(module)").unwrap(), 3_500_000));
    }
    let entries: Vec<(String, String)> = tags.iter().map(|tag| (tag.to_string(), tag.to_lowercase())).collect();
    let entries: Vec<(&str, &str, &str)> = entries.iter().map(|(tag, file)| (tag.as_str(), file.as_str(), "x")).collect();
    files.put(INDEX_FILE, index(&entries));
    let mut deck = open(&files);
    let notices = deck.notices(0);
    for tag in &tags[..4] {
        assert!(notices.iter().any(|notice| *notice == format!("{tag}: can't be loaded")), "{notices:?}");
    }
    assert!(notices.iter().any(|notice| notice == "E: too large to run"), "{notices:?}");
}

// A source that's refused, with a build of it.

#[test]
fn a_source_that_cannot_be_read_for_its_tag_runs_its_last_build_out_of_date() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("Hi")));
    // An unclosed quote on line 5.
    files.put("components/hello.rs", "use toolkit::prelude::*;\n\n#[component]\nfn Hello() -> Element {\n    text(\"Hello)\n}\n");
    let mut deck = open(&files);
    assert_eq!(info(&deck, "Hello").state, ComponentState::OutOfDate);
    assert_eq!(info(&deck, "Hello").file, "components/hello.rs");
    let problem = deck.component_problems().iter().find(|problem| problem.file == "components/hello.rs").cloned().unwrap();
    assert!(problem.line > 0, "{problem:?}");
    assert!(problem.message.contains("unclosed quote"), "{problem:?}");
    assert!(shows(&mut deck, 0, "Hi"), "it still runs its last build");
    assert!(deck.notices(0).iter().any(|notice| notice == "Hello is out of date, so this is its last build"));
}

#[test]
fn a_refused_source_with_a_build_is_out_of_date() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("Hi")));
    files.put("components/hello.rs", vec![0xff, 0xfe, 0x00]);
    let mut deck = open(&files);
    assert_eq!(info(&deck, "Hello").state, ComponentState::OutOfDate);
    assert!(has_problem(&deck, "components/hello.rs"));
    assert!(shows(&mut deck, 0, "Hi"));
}

// An asset a component reads.

#[test]
fn a_changed_asset_draws_a_built_in_again() {
    let files = Files::default();
    files.put("slides.md", "<Echo />\n");
    files.put("assets/a.txt", "first");
    let mut deck = open_with(&files, probe_registry());
    assert_eq!(texts(&mut deck, 0), ["first"]);
    assert!(deck.update("<Echo />\n".to_string()).is_empty(), "nothing has changed yet");
    files.put("assets/a.txt", "second");
    assert_eq!(deck.update("<Echo />\n".to_string()), [0]);
    assert_eq!(texts(&mut deck, 0), ["second"]);
}

#[test]
fn a_changed_asset_is_noticed_though_another_slide_read_it_since() {
    let files = Files::default();
    let slides = "<Echo />\n\n---\n\n<Echo />\n";
    files.put("slides.md", slides);
    files.put("assets/a.txt", "first");
    let mut deck = open_with(&files, probe_registry());
    assert_eq!(texts(&mut deck, 0), ["first"]);
    files.put("assets/a.txt", "second");
    // The second slide is laid out for the first time now, and reads the file as it has become.
    assert_eq!(texts(&mut deck, 1), ["second"]);
    assert!(deck.update(slides.to_string()).contains(&0), "the first slide still shows what the file was");
    assert_eq!(texts(&mut deck, 0), ["second"]);
}

#[test]
fn an_asset_that_appears_draws_a_talks_component_again() {
    let files = talk_with_hello("<Hello />\n", reading("Hello", "assets/a.txt"));
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "no"));
    assert!(deck.refresh_components().is_empty());
    files.put("assets/a.txt", "here");
    assert_eq!(deck.refresh_components(), [0]);
    assert!(shows(&mut deck, 0, "yes"), "{:?}", texts(&mut deck, 0));
}

#[test]
fn the_whole_slides_notices_are_those_of_what_it_draws() {
    let files = Files::default();
    files.put("slides.md", "<Late />\n");
    let mut deck = open_with(&files, probe_registry());
    assert!(deck.notices(0).is_empty(), "at the start: {:?}", deck.notices(0));
    assert_eq!(deck.notices_whole(0), ["Late trouble"]);
    assert!(texts(&mut deck, 0).contains(&"Late trouble".to_string()), "the whole slide draws it");
}

#[test]
fn what_components_read_is_remembered_for_at_most_256_paths() {
    let files = Files::default();
    let slides = "<Wanderer />\n\n---\n\n<Chart kind=\"bar\" data=\"assets/sales.csv\" />\n";
    files.put("slides.md", slides);
    files.put("assets/sales.csv", "L,S\nA,1\n");
    let mut deck = open_with(&files, probe_registry());
    for frame in 0..1000 {
        let _ = deck.layers_at(0, Moment { step: 0, time: frame as f32 }, 0.1);
    }
    assert!(deck.assets_remembered() <= 256, "{}", deck.assets_remembered());
    // The chart's file is read once the record is full, so it isn't in it. Past what it can
    // remember, any change might matter, so the chart still sees its new data.
    assert!(!texts(&mut deck, 1).contains(&"B".to_string()));
    files.put("assets/sales.csv", "L,S\nA,1\nB,2\n");
    assert!(deck.update(slides.to_string()).contains(&1));
    assert!(texts(&mut deck, 1).contains(&"B".to_string()), "{:?}", texts(&mut deck, 1));
}

#[test]
fn a_path_over_256_bytes_is_not_read() {
    let long = format!("assets/{}.txt", "a".repeat(260));
    let files = talk_with_hello("<Hello />\n", reading("Hello", &long));
    files.put(&long, "here");
    let mut deck = open(&files);
    assert!(shows(&mut deck, 0, "no"));
}

#[test]
fn a_show_holds_the_layouts_of_at_most_two_slides_at_a_moment() {
    let files = Files::default();
    let slides = ["<Ticking />\n"; 5].join("\n---\n\n");
    files.put("slides.md", slides);
    let mut deck = open_with(&files, probe_registry());
    let at = |time: f32| Moment { step: 0, time };
    for slide in 0..5 {
        let _ = deck.layers_at(slide, at(1.0), 0.1);
        // The next one, as a presenter's thumbnail would.
        let _ = deck.layers_at((slide + 1) % 5, at(0.5), 0.1);
        assert!(deck.moments_held() <= 2, "after slide {slide}: {}", deck.moments_held());
    }
    // Going back and forth between two keeps both.
    let _ = deck.layers_at(2, at(2.0), 0.1);
    let _ = deck.layers_at(3, at(2.0), 0.1);
    let _ = deck.layers_at(2, at(2.0), 0.1);
    assert_eq!(deck.moments_held(), 2);
}

#[test]
fn the_components_available_are_the_built_ins_and_the_talks_own() {
    let files = talk_with_hello("<Hello />\n", answering("Hello", &text_answer("Hi")));
    files.put("components/apple.rs", source("Apple"));
    let deck = open(&files);
    assert_eq!(deck.available_components(), ["Apple", "Box", "Chart", "Hello", "Icon", "Picture", "Shape"]);
}
