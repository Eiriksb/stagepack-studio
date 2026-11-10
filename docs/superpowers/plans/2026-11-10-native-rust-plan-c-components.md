# Plan C: Custom components Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A coder writes one Rust file in a talk's `components` folder, the app builds it with its own bundled compiler, and the tag draws on the slide in a sandbox, including click steps and movement over time. A ready-made `<Chart>` works in any talk with no build.

**Architecture:** `toolkit` is compiled twice: natively for the built-ins and to WebAssembly for a talk's own components. A new crate, `slides-components`, checks and compiles source and runs the result in Wasmtime with no imports except one host function. The engine asks for a component at a `Moment` (a click step and a time), so a slide can differ from step to step. The app decides when building is allowed.

**Tech Stack:** Rust 1.98.0, Wasmtime 50.0.1, the `wasm32-unknown-unknown` target, proc-macro2, sha2, svgtypes, and the existing `toolkit`, `toolkit-macros`, `slides-format`, `slides-engine`, `slides-pdf`, `slides-pptx` and `stagepack-studio` crates.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`, sections "Custom components", "Charts" and "Export", and goals 14 and 17. The spike that proved the compile loop is in `docs/checks/native-rust-spikes.md`, and its code is on the branch `spike/native-rust` under `spikes/compile`.

## Global Constraints

- Work on the branch `plan-c`, cut from `main`.
- No GitHub Actions, for Windows builds or anything else. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`.
- Pin `wasmtime = { version = "=50.0.1", default-features = false, features = ["anyhow", "cranelift", "runtime", "std"] }`, `proc-macro2 = { version = "=1.0.107", features = ["span-locations"] }`, `sha2 = "=0.10.9"`, `svgtypes = "=0.16.2"`, `dirs = "=6.0.0"`, and `wat = "=1"` where tests use it. Add `thiserror` from the workspace.
- All our code is Rust. `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx` and `slides-components` don't depend on GPUI.
- `toolkit` builds for `wasm32-unknown-unknown`, and depends on nothing but `serde`, `serde_json` and `toolkit-macros`.
- A talk is untrusted input. Nothing in one may crash the app, write outside the chosen file, or read outside the talk.
- **Opening a talk never compiles and runs nothing outside the sandbox.** A component has no WASI and no imports but `toolkit.read_asset`.
- A component reads only under the talk's `assets` folder. `rustc` runs with a cleared environment.
- Sandbox limits: 64 MiB of memory, 250 ms per render, 1 s of renders per slide layout, 4 MiB of output, 16 MiB per module, 1 MiB per source file, 256 components per talk, 20,000 elements and 64 levels of nesting per render.
- **The interface is forever.** Version 1 is whatever `crates/toolkit/tests/interface_v1.rs` freezes. Later releases must run it, and a later task must not change what that test asserts.
- Files are written with `slides_engine::write_atomically`, so a crash can't damage an existing file.
- Slide units are 1920×1080, and one unit is 6350 EMU.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. It holds the owner's own uncommitted edit.
- Don't name real decks, clients or organisations in commits. Use fixtures.

Tests that need the compiler bundle are marked `#[ignore = "needs the compiler bundle"]`, so that `cargo test --workspace` works from a clean checkout. `scripts/test-components.sh` builds the bundle and runs them.

Not in this plan:
- The form on screen and the component palette (plan D3). Selecting something on the slide doesn't exist yet, so this plan delivers the schema and the fields a form will show.
- *Always build this folder* being on by default for talks made in the app: there's no New presentation yet (plan D5). `BuildPermissions::set_always` is the hook.
- Installers and where the bundle lands (plan E). Until then the bundle is found through `STUDIO_BUNDLE`, or beside the executable.
- Showing a component in the Slidev add-on (plan F), outside crates, and components that respond to the mouse or keyboard.

## Review Focus

- **A component that hangs, panics, eats memory or returns garbage:** a labelled box on that slide and nothing else wrong, including a slide with many of them. Owned by Task 5's `a_component_that_never_returns_is_stopped` and Task 6's `a_slide_of_hanging_components_stays_within_its_budget`.
- **A talk trying to reach outside itself:** a file read hidden behind a macro or a rename, a module importing anything else, and a path out of `assets`. Owned by Task 4's `a_file_read_hidden_in_a_macro_is_refused`, and Task 5's `a_module_with_any_other_import_is_refused` and `an_asset_path_outside_assets_reads_nothing`.
- **Source arriving by a save, a `git pull` or a packed file:** nothing compiles without a deliberate yes. Owned by Task 10's `opening_a_talk_starts_no_build`, `a_change_in_a_folder_not_allowed_starts_no_build` and `a_packed_talk_offers_no_build`.
- **A save that doesn't compile:** the last good build stays on the slide and in a packed copy, and the person is told. Owned by Task 10's `a_failed_build_keeps_the_last_good_one` and `a_packed_copy_says_which_builds_are_behind`.
- **A component built by an earlier release:** it still runs. Owned by Task 5's `a_component_built_for_version_one_still_runs`.

---

### Task 1: The toolkit's public shape and version 1 of the interface

**Files:**
- Create: `crates/toolkit/src/interface.rs`, `crates/toolkit/src/wasm.rs`, `crates/toolkit/tests/interface_v1.rs`, `crates/toolkit/tests/component.rs`
- Modify: `crates/toolkit/src/{lib,element,props,color}.rs`, `crates/toolkit/Cargo.toml`, `crates/toolkit-macros/src/lib.rs`, `crates/engine/src/{builtins,elements}.rs`

**Interfaces:**
- Produces, in `toolkit` (all in the prelude except the items under `__private`):
  - `Element` gains `Canvas { width: f32, height: f32, items: Vec<Placed> }` and `Problem { message: String }`. `Element::Path` gains `stroke: Option<Color>` and `stroke_width: f32`.
  - `pub struct Placed { pub x: f32, pub y: f32, pub width: f32, pub align: TextAlign, pub element: Element }`. A width of 0 means its own width.
  - `pub enum TextAlign { Left, Center, Right }`, defaulting to `Left`.
  - `pub fn canvas(width: f32, height: f32, items: Vec<Placed>) -> Element`, `pub fn at(x: f32, y: f32, element: Element) -> Placed`, `pub fn problem(message: impl Into<String>) -> Element`.
  - `Placed::centered_in(self, width: f32) -> Placed`, `Placed::right_aligned_in(self, width: f32) -> Placed`, `Element::stroke(self, color: Color, width: f32) -> Element`.
  - `pub const ABI_VERSION: u32 = 1`.
  - `pub struct CtxData { pub click_step: u32, pub time: f32, pub slide_width: f32, pub slide_height: f32, pub width: f32, pub height: f32, pub theme: Theme }`. `width` is the box the tag sits in, and `height` is 0 if the box grows with its content.
  - `pub struct RenderInput { pub props: BTreeMap<String, String>, pub ctx: CtxData }`
  - `pub struct RenderOutput { pub element: Element, pub steps: u32, pub stepped: bool, pub animated: bool }`
  - `pub struct ComponentSchema { pub name: String, pub props: Vec<PropField>, pub children: bool }`
  - `pub struct PropField { pub name: String, pub kind: PropKind, pub optional: bool }`
  - `pub enum PropKind { Text, Whole, Number, Flag, Color, File(Vec<String>), Choice(Vec<String>) }`
  - `pub fn asset_path_allowed(path: &str) -> bool`
  - `pub struct PictureFile(pub String)` (kind `File(["png", "jpg", "jpeg", "svg", "webp"])`) and `pub struct DataFile(pub String)` (kind `File(["csv"])`).
  - `trait Prop { fn kind() -> PropKind; const OPTIONAL: bool = false; fn from_attr(value: Option<&str>) -> Self; }`, replacing `TYPE_NAME`, implemented for `String`, `u32`, `i32`, `f32`, `bool`, `Color`, `PictureFile`, `DataFile` and `Option<T>`.
  - `#[derive(Prop)]` on an enum of unit variants: an attribute matches the kebab-cased variant name, the first variant is the fallback, and the kind is `Choice`.
  - `Ctx<'a>`, whose fields are now private: `Ctx::new(data: CtxData, assets: &'a dyn Fn(&str) -> Option<Vec<u8>>) -> Ctx<'a>`; `click_step(&self) -> u32` (marks stepped); `steps(&self, count: u32) -> u32` (marks stepped, records the largest count asked for, and returns the step held to `count`); `time(&self) -> f32` (marks animated); `slide_width`, `slide_height`, `width`, `height`, `theme`; `asset(&self, path: &str) -> Option<Vec<u8>>`; `asset_text(&self, path: &str) -> Option<String>`; `finish(self, element: Element) -> RenderOutput`.
  - `NativeComponent { name: &'static str, schema: fn() -> ComponentSchema, render: fn(&Props, &Ctx) -> Element }`.
- `#[component]` still generates `<Name>_component() -> NativeComponent`. Under `#[cfg(target_arch = "wasm32")]` it also generates the exports `abi_version() -> u32`, `alloc(len: u32) -> u32`, `schema() -> u64` and `render(ptr: u32, len: u32) -> u64`, each a pointer and length packed as `(ptr << 32) | len`. The generated code reaches serde only through `toolkit::__private`. `toolkit::__private`'s wasm half is in `wasm.rs`, ported from the spike's `__private`, and reads assets through the import `#[link(wasm_import_module = "toolkit")] fn read_asset(ptr: u32, len: u32) -> u64`, where 0 means absent.

`asset_path_allowed` is true only for a path starting `assets/` with no empty, `.` or `..` part and no backslash.

- [ ] **Step 1: Write the failing tests**

`interface_v1.rs` holds literal JSON, which is the contract. Each test parses the literal, compares it with a value built in Rust, and serialises it back to the same text:

```rust
const STACK: &str = r#"{"Stack":{"horizontal":true,"gap":8.0,"children":[{"Text":{"text":"Hi","size":48.0,"bold":true,"color":{"r":1,"g":2,"b":3,"a":255}}},"Children","Empty"]}}"#;
const SHAPE: &str = r#"{"Shape":{"kind":"Rounded","width":10.0,"height":20.0,"fill":null,"outline":null,"outline_width":0.0,"radius":4.0,"padding":0.0,"child":null}}"#;
const PATH: &str = r#"{"Path":{"data":"M0 0","width":5.0,"height":6.0,"fill":null,"stroke":{"r":0,"g":0,"b":0,"a":255},"stroke_width":2.0}}"#;
const CANVAS: &str = r#"{"Canvas":{"width":100.0,"height":50.0,"items":[{"x":1.0,"y":2.0,"width":0.0,"align":"Center","element":"Empty"}]}}"#;
const PROBLEM: &str = r#"{"Problem":{"message":"No data"}}"#;
const OUTPUT: &str = r#"{"element":"Empty","steps":3,"stepped":true,"animated":false}"#;
const SCHEMA: &str = r#"{"name":"Score","props":[{"name":"team","kind":"Text","optional":false},{"name":"kind","kind":{"Choice":["bar","line"]},"optional":true},{"name":"logo","kind":{"File":["png"]},"optional":false}],"children":true}"#;
const INPUT: &str = r#"{"props":{"team":"A"},"ctx":{"click_step":1,"time":0.5,"slide_width":1920.0,"slide_height":1080.0,"width":800.0,"height":0.0,"theme":{"background":{"r":255,"g":255,"b":255,"a":255},"text":{"r":30,"g":41,"b":59,"a":255},"accent":{"r":37,"g":99,"b":235,"a":255},"font":"Inter","mono_font":"JetBrains Mono"}}}"#;
```

Also `an_output_with_fields_left_out_takes_their_defaults`: `{"element":"Empty"}` gives steps 0, stepped false, animated false.

`component.rs`, with a local `#[derive(Prop)] enum Kind { Bar, LineChart }` and `#[component] fn Score(team: String, points: u32, ready: bool, colour: Color, note: Option<String>, kind: Kind, logo: PictureFile, children: Children, ctx: &Ctx) -> Element`:
- `the_schema_says_what_each_argument_is`: names, in order, are `team, points, ready, colour, note, kind, logo`; kinds are `Text, Whole, Flag, Color, Text (optional), Choice(["bar", "line-chart"]), File([...])`; `children` is true.
- `a_choice_reads_kebab_case_and_falls_back_to_the_first`.
- `a_component_that_asks_for_nothing_varies_by_nothing`: `finish` gives steps 0, stepped false, animated false.
- `asking_for_steps_is_recorded`, with `ctx.steps(3)` then `ctx.steps(2)`: steps 3, stepped true.
- `steps_holds_the_step_to_the_count`: at click step 9, `steps(3)` returns 3.
- `reading_the_click_step_marks_it_stepped_and_adds_no_steps`.
- `asking_for_the_time_marks_it_animated`.
- `an_asset_path_is_held_to_the_assets_folder`: true for `assets/a.csv`, `assets/x/a.png`; false for `slides.md`, `assets/../slides.md`, `assets//a`, `assets/./a`, `/assets/a`, `assets\\a`, `build/a.wasm`, ``.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p toolkit`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Add `Serialize, Deserialize` to `Theme`. Use `#[serde(default)]` on `RenderOutput`'s three flags, and on `stroke`, `stroke_width` and `Placed::{width, align}`. Keep every attribute-name rule the macro already has. In `elements.rs`, build the `Ctx` through `Ctx::new` with the assets closure refusing every path for now, draw `Element::Problem` as the existing placeholder with a notice, and give `Element::Canvas` a `None` arm that Task 2 replaces.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and `scripts/cargo-safe.sh build -p toolkit --target wasm32-unknown-unknown --release`
Expected: PASS, and the wasm build succeeds.

- [ ] **Step 5: Commit**

```bash
git add crates/toolkit crates/toolkit-macros crates/engine/src/builtins.rs crates/engine/src/elements.rs Cargo.lock
git commit -m "Toolkit: version 1 of the component interface"
```

---

### Task 2: Canvases and vector paths

**Files:**
- Modify: `crates/engine/src/{elements,flow,native,pictures}.rs`, `crates/engine/Cargo.toml`
- Test: `crates/engine/tests/canvas.rs`

**Interfaces:**
- Consumes: `Element::Canvas`, `Placed`, `Element::Path`'s stroke (Task 1).
- Produces, in `slides_engine`:
  - `pub enum PathStep { Move(f32, f32), Line(f32, f32), Quad(f32, f32, f32, f32), Cubic(f32, f32, f32, f32, f32, f32), Close }`
  - `NativeContent::Path { steps: Vec<PathStep>, fill: Option<Color>, stroke: Option<Color>, stroke_width: f32 }`, with `NativeElement::rect` as its box and the steps relative to that box.
- Removes `path_pixmap`: a path no longer goes through a picture.

A canvas is a node of a fixed size, its items absolutely positioned at (`x`, `y`) and noted `Semantic::Stack` so that each stands alone in an export. An item's `width` of 0 leaves it its own width. An item's `align` is the text alignment inside that width. Path data goes through `svgtypes::SimplifyingPathParser`. `Content` gains a `Path` variant holding the parsed path and its look, and `Flow::draw` emits `DisplayItem::Fill` and `DisplayItem::Stroke`, so a path is sharp at any scale and a vector in the PDF. A fill is the element's `fill` or, if it has no `stroke` either, the theme's text colour. Stroke widths are held to `MAX_LINE_WIDTH`.

- [ ] **Step 1: Write the failing tests**

Build each case through a `LayoutEnv` with a test registry, as `crates/engine/tests/builtins.rs` does:
- `things_on_a_canvas_sit_where_they_are_placed`: a `Shape(Rect, 40, 20)` at (100, 200) on a 800×400 canvas gives a `Fill` whose bounds are left 100, top 200, right 140, bottom 220, relative to the layer.
- `text_centred_in_a_width_sits_in_the_middle_of_it`: a short text `centered_in(400)` at x 100 has its centre at 300, within 2 units.
- `a_canvas_holds_its_size_in_the_flow`: the block after an 800×400 canvas starts 424 units down (400 plus the 24 gap).
- `each_thing_on_a_canvas_exports_on_its_own`: two texts on a canvas give two `NativeContent::Text` elements.
- `a_path_is_drawn_as_a_vector`: `path("M0 0 L10 0 L10 10 Z", 10, 10)` gives a `DisplayItem::Fill` and no `DisplayItem::Picture`.
- `a_path_with_a_stroke_and_no_fill_has_only_a_stroke`.
- `a_path_exports_its_steps`: gives `NativeContent::Path` with `[Move(0,0), Line(10,0), Line(10,10), Close]`.
- `a_curve_in_a_path_exports_as_a_curve`: an `A` arc gives at least one `Cubic`.
- `a_path_that_cannot_be_read_is_a_placeholder_with_a_notice`: keeps today's behaviour.
- `an_absurd_canvas_is_held_to_the_limits`: width 1e9 and a NaN-free huge `x` don't panic, and the node is held to `MAX_SIZE`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test canvas`
Expected: FAIL.

- [ ] **Step 3: Implement it, and make the writers compile**

`slides-pdf` needs nothing new, since it already draws `Fill` and `Stroke`. In `slides-pptx`, give `NativeContent::Path` arms that write nothing, which Task 3 replaces.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS. If a golden image changes because a path is now a vector, say so in the report.

- [ ] **Step 5: Commit**

```bash
git add crates/engine crates/pptx/src Cargo.lock
git commit -m "Layout: canvases, and paths drawn as vectors"
```

---

### Task 3: Freeform shapes in PowerPoint

**Files:**
- Modify: `crates/pptx/src/{shapes,write}.rs`
- Test: `crates/pptx/tests/elements.rs`

**Interfaces:**
- Consumes: `NativeContent::Path`, `PathStep` (Task 2).
- Produces: `pub fn freeform_xml(id: u32, rect: Rect, rotate: f32, steps: &[PathStep], fill: Option<Color>, stroke: Option<(Color, f32)>) -> String` in `shapes.rs`.

Child order in `a:custGeom` is `avLst, gdLst, ahLst, cxnLst, rect, pathLst`. A path's `w` and `h` are the box in EMU, and points are in EMU relative to the box.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_freeform_writes_its_path_in_emu() {
    let steps = [PathStep::Move(0.0, 0.0), PathStep::Line(100.0, 0.0), PathStep::Close];
    let xml = shapes::freeform_xml(7, Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, 0.0, &steps, Some(Color::rgb(255, 0, 0)), None);
    assert!(xml.contains(r#"<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="r" b="b"/><a:pathLst><a:path w="635000" h="635000"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="635000" y="0"/></a:lnTo><a:close/></a:path></a:pathLst></a:custGeom>"#));
}
```

Also:
- `a_curve_writes_its_control_points`: `Cubic` writes `a:cubicBezTo` with three `a:pt`, and `Quad` writes `a:quadBezTo` with two.
- `a_line_with_no_fill_says_so`: no fill writes `<a:noFill/>` and the `a:ln` with its width.
- `a_freeform_is_valid_xml_inside_a_slide`, parsed with quick-xml.
- `a_slide_with_a_path_exports_a_freeform`, through `write_pptx`: the slide part holds `a:custGeom` and no `p:pic`.
- `points_are_never_negative_in_a_box_of_no_size`: a zero-size rect doesn't divide by zero and writes `w="0"`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test elements freeform`
Expected: FAIL.

- [ ] **Step 3: Implement it**

Wire `NativeContent::Path` in `element_xml`, `left_out_links` and `has_fill`. Reuse `xfrm_xml`, `solid_fill_of`, `emu_from_units` and `line_xml`. The name is `Freeform {id}`.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-pptx`
Expected: PASS, and the reference files are unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/pptx
git commit -m "PowerPoint: paths arrive as freeform shapes"
```

---

### Task 4: The compiler bundle, the source check and compiling

**Files:**
- Create: `crates/components/{Cargo.toml,src/lib.rs,src/source_check.rs,src/compile.rs,tests/source_check.rs,tests/compile.rs}`, `scripts/make-bundle.sh`, `scripts/make-bundle.ps1`, `scripts/test-components.sh`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces, in `slides_components`:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Diagnostic { pub file: String, pub line: u32, pub message: String }`
  - `pub struct SourceInfo { pub tag: String }`
  - `pub fn check_source(file: &str, source: &str) -> Result<SourceInfo, Vec<Diagnostic>>`
  - `pub fn component_tag(source: &str) -> Option<String>`: the name of the first `#[component]` function, without judging the file.
  - `pub struct Bundle { pub root: PathBuf }`, `Bundle::find() -> Option<Bundle>`: `STUDIO_BUNDLE` if set, or else a `bundle` folder beside the executable, in either case only if `bin/rustc` (`bin/rustc.exe` on Windows) is in it.
  - `pub const COMPILE_TIME_LIMIT: Duration` of 60 seconds.
  - `pub fn compile(bundle: &Bundle, file: &str, source: &str) -> Result<Vec<u8>, Vec<Diagnostic>>`, where `file` is the talk-relative name used in diagnostics, such as `components/score.rs`.
  - `pub fn compile_with_limit(bundle: &Bundle, file: &str, source: &str, limit: Duration) -> Result<Vec<u8>, Vec<Diagnostic>>`

**The source check** lexes with `proc_macro2::TokenStream::from_str`, walking into every group. A file that won't lex is refused, as the lexer disagreeing with `rustc` must never let something through. It refuses:
- any identifier, raw or not, equal to `include`, `include_str`, `include_bytes`, `env`, `option_env` or `debugger_visualizer`, anywhere, including inside macro input;
- `path` anywhere inside an attribute;
- `feature` as the first thing in a `#![...]`;
- `mod` not followed by an identifier and then a brace group;
- a file with no `#[component]` function, or with more than one.

Each refusal has the line it was found on and a plain sentence, for example ``"`env` can't be used in a component, as it could read settings from this computer"``.

**Compiling** ports `compile_with` from the spike (`git show spike/native-rust:spikes/compile/host/src/lib.rs`) with these changes. It runs `check_source` first. It copies the source to a private scratch folder as `component.rs`. It compiles with `--crate-name component`, `--edition 2024`, `--crate-type cdylib`, `--target wasm32-unknown-unknown`, `-C opt-level=1`, `-C strip=debuginfo` and `--remap-path-prefix <scratch>=components`, so no path from this computer reaches the build. It clears the environment, except the temp variables pointing at the scratch folder and `SYSTEMROOT` on Windows. Standard error is read on its own thread, so a full pipe can't stall it, and the process is killed at the limit. Diagnostics naming `component.rs` are given `file`.

**`scripts/make-bundle.sh <dir>`** ports the spike's `bundle.sh`: `bin/rustc`, `lib/librustc_driver-*`, `lib/libLLVM*`, `rust-lld`, and the wasm target's `.rlib` and `.rmeta` files. It builds `toolkit` with `--release --target wasm32-unknown-unknown --target-dir target/bundle-build`, a target folder of its own so that only one variant of each library is copied. It copies `libtoolkit.rlib`, `deps/*.rlib` and `deps/*.rmeta` to `toolkit/`, the host `libtoolkit_macros-*.so` and `libserde_derive-*.so` to `toolkit/deps/`, and `crates/toolkit` and `crates/toolkit-macros` to `toolkit-src/` with a generated `toolkit-src/Cargo.toml` standing in for the workspace. `make-bundle.ps1` is the same for Windows, ported from the spike's `bundle.ps1`.

**`scripts/test-components.sh`** runs `make-bundle.sh target/bundle` and then `STUDIO_BUNDLE=$PWD/target/bundle scripts/cargo-safe.sh test --workspace -- --ignored`.

- [ ] **Step 1: Write the failing tests**

`source_check.rs`, each case wrapped in a valid `#[component] fn A() -> Element { ... }` where needed:
- `a_read_of_the_computer_is_refused`, a table of `include!("x")`, `include_str!("x")`, `include_bytes!("x")`, `env!("HOME")`, `option_env!("HOME")`, `#[path = "x"] mod a;`, `mod a;`, `#![feature(x)]`, each giving `Err` with at least one diagnostic.
- `a_file_read_hidden_in_a_macro_is_refused`, a table of `std::include_str!("x")`, `core::env!("HOME")`, `use std::include_bytes as b;`, `r#include_str!("x")`, `macro_rules! m { ($x:ident) => { $x!("HOME") } } m!(env);`, `macro_rules! m { ($a:tt $b:ident) => { $a $b; } } m!(mod foo);`, `#[cfg_attr(all(), path = "x")] mod a {}`, `#[debugger_visualizer(gdb_script_file = "x")] mod a {}`.
- `words_in_strings_and_comments_are_fine`: `"env!"`, `// include_str!`, `/* mod a; */`.
- `a_module_with_a_body_is_fine`.
- `a_refusal_names_its_line`: `env!` on the third line gives `line: 3`, and `file` as passed.
- `a_file_with_no_component_is_refused` and `a_file_with_two_components_is_refused`.
- `the_tag_is_the_function_name`: `SourceInfo { tag: "Score".into() }`.
- `a_file_that_will_not_lex_is_refused`: an unterminated string.
- `a_refused_source_never_reaches_the_compiler`: `compile` with a bundle at a folder that doesn't exist returns the refusal, not a failure to start the compiler.

`compile.rs`, all but the last `#[ignore = "needs the compiler bundle"]`, with the bundle from `Bundle::find()`:
- `a_component_compiles_to_webassembly`: the bytes start `\0asm`.
- `an_error_names_the_file_and_line`: a type error on line 4 gives `file == "components/score.rs"` and `line == 4`.
- `nothing_of_this_computer_is_in_the_build`: the wasm doesn't contain the temp directory's path or the user's home path.
- `a_compile_that_never_ends_is_stopped` (not ignored, `#[cfg(unix)]`): a fake bundle whose `bin/rustc` is a script running `sleep 60`, with a limit of 200 ms, returns a diagnostic mentioning the time within 5 s.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-components`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/test-components.sh` (after `chmod +x scripts/*.sh`)
Expected: PASS, including the ignored tests.

- [ ] **Step 5: Commit**

```bash
git add crates/components scripts/make-bundle.sh scripts/make-bundle.ps1 scripts/test-components.sh Cargo.toml Cargo.lock
git commit -m "Components: the compiler bundle, the source check and compiling"
```

---

### Task 5: The sandbox

**Files:**
- Create: `crates/components/src/sandbox.rs`, `crates/components/tests/sandbox.rs`, `crates/components/tests/fixtures/v1/{score.rs,score.wasm,README.md}`
- Modify: `crates/components/{Cargo.toml,src/lib.rs}`

**Interfaces:**
- Consumes: `RenderInput`, `RenderOutput`, `ComponentSchema`, `ABI_VERSION`, `asset_path_allowed` (Task 1).
- Produces, in `slides_components`:
  - `pub const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024`, `RENDER_TIME_LIMIT: Duration` (250 ms), `MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024`, `MAX_MODULE_BYTES: usize = 16 * 1024 * 1024`, `MAX_ELEMENTS: usize = 20_000`, `MAX_ELEMENT_DEPTH: usize = 64`.
  - `#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)] pub enum ComponentError { TooLarge, Malformed(String), NewerVersion(u32), TookTooLong, OutOfMemory, Crashed, BadOutput(String) }`
  - `ComponentError::short(&self) -> &'static str`, the words for the labelled box: `"too large to run"`, `"can't be loaded"`, `"built by a newer version of this app"`, `"took too long"`, `"used too much memory"`, `"stopped working"`, `"gave an answer that can't be read"`.
  - `pub type Assets = Rc<dyn Fn(&str) -> Option<Vec<u8>>>`
  - `pub struct WasmComponent`, with `load(bytes: &[u8]) -> Result<WasmComponent, ComponentError>`, `schema(&self) -> &ComponentSchema`, and `render(&self, input: &RenderInput, assets: Assets) -> Result<RenderOutput, ComponentError>`.

One process-wide `wasmtime::Engine` with `epoch_interruption(true)` and `max_wasm_stack(256 * 1024)`, in a `OnceLock`, with one background thread calling `increment_epoch` every 10 ms. Each `render` is a new `Store` and instance, so nothing survives between renders. The store has a `ResourceLimiter` that holds memory to the limit, allows one instance, one memory and one table, and records that it refused growth, which is how `OutOfMemory` is told apart from `Crashed`. The epoch deadline is the limit in ticks, and a trap at the deadline is `TookTooLong`. The only import a module may have is the function `toolkit.read_asset`. Check `module.imports()` at load, rather than leaving it to instantiation. `load` also requires the four exports and a `memory` export, reads `abi_version` (above `ABI_VERSION` is `NewerVersion`) and reads the schema.

`read_asset` refuses unless `asset_path_allowed`, allows at most 64 reads per render, reads up to `MEMORY_LIMIT_BYTES`, and places the bytes through the module's own `alloc`. Every pointer and length from the module is bounds-checked by reading through `Memory`, and a length over `MAX_OUTPUT_BYTES` is refused before anything is allocated.

After parsing, `render` counts the elements and depth (`BadOutput` if over the limits), and replaces any `Element::Picture` whose `src` fails `asset_path_allowed` with `Element::Problem`.

- [ ] **Step 1: Write the failing tests**

Modules are written in WAT with the `wat` crate, as a helper that fills in `abi_version`, `alloc`, `schema` and `memory` around a given `render` body:
- `a_component_that_never_returns_is_stopped`: `(loop br 0)` gives `TookTooLong` within 2 s.
- `a_component_that_eats_memory_is_stopped`: a `memory.grow` loop followed by `unreachable` gives `OutOfMemory`.
- `a_component_that_panics_is_reported`: `unreachable` gives `Crashed`.
- `a_pointer_out_of_bounds_is_reported`: `BadOutput`.
- `an_enormous_answer_is_refused`: a length of 2^31 gives `BadOutput`.
- `an_answer_that_is_not_json_is_refused`: `BadOutput`.
- `an_answer_nested_too_deeply_is_refused` and `an_answer_with_too_many_elements_is_refused`.
- `a_module_with_any_other_import_is_refused`: importing `wasi_snapshot_preview1.fd_write` gives `Malformed`.
- `a_module_missing_an_export_is_refused`: `Malformed`.
- `a_newer_version_is_refused_with_its_own_message`: `abi_version` 2 gives `NewerVersion(2)`.
- `a_module_too_large_is_refused_before_it_is_compiled`: `TooLarge`.
- `each_render_starts_fresh`: a module incrementing a global and returning it gives the same answer twice.
- `an_asset_path_outside_assets_reads_nothing`: a module calling `read_asset` with `assets/../slides.md` never reaches the assets closure, whose call count stays 0.
- `an_asset_reaches_the_component`: the closure's bytes end up in the module's answer.
- `a_picture_outside_assets_becomes_a_problem`.
- `a_component_built_for_version_one_still_runs`: loads the committed `score.wasm`, and compares its schema and its render output with exact expected values. This test needs no bundle.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-components --test sandbox`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Write `tests/fixtures/v1/score.rs` (the design's `Score`, using a `row!`, `text` and a `shape`), and build `score.wasm` once with `compile` and the bundle. `README.md` says, in two sentences, that this file is what proves the promise and must never be regenerated.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-components`
Expected: PASS. Record how long `WasmComponent::load` takes on `score.wasm`, and the size of that file, in the report.

- [ ] **Step 5: Commit**

```bash
git add crates/components Cargo.lock
git commit -m "Components: a sandbox that holds them to their limits"
```

---

### Task 6: A talk's components on its slides

**Files:**
- Create: `crates/components/src/index.rs`, `crates/engine/src/talk_components.rs`, `crates/engine/tests/talk_components.rs`
- Modify: `crates/engine/src/{deck,elements,flow,files,packed,slide,lib}.rs`, `crates/engine/Cargo.toml`, `crates/format/src/{model,parse}.rs`, `crates/engine/tests/builtins.rs`

**Interfaces:**
- Consumes: `WasmComponent`, `ComponentError`, `Assets` (Task 5), `component_tag`, `Diagnostic` (Task 4).
- Produces, in `slides_components`:
  - `pub fn source_hash(bytes: &[u8]) -> String` (SHA-256, lowercase hex).
  - `#[derive(Serialize, Deserialize)] pub struct BuildIndex { pub format: u32, pub components: Vec<BuildEntry> }`, `pub struct BuildEntry { pub tag: String, pub file: String, pub source_hash: String }`.
  - `pub const INDEX_FILE: &str = "build/index.json"`.
  - `pub fn safe_stem(stem: &str) -> bool`: letters, digits, `_` and `-` only, 1 to 64 long.
- Produces, in `slides_engine`:
  - `TalkFiles::names_in(&self, folder: &str) -> Vec<String>`, defaulting to none: the files directly in `folder`, as talk paths, in name order. Implemented for `FolderFiles` (regular files only), `PackedFiles` and the `Arc` forwarding impl.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ComponentState { UpToDate, OutOfDate, NeedsBuilding }`
  - `#[derive(Clone, Debug, PartialEq)] pub struct ComponentInfo { pub tag: String, pub file: String, pub state: ComponentState }`
  - `pub struct TalkComponents`, with `scan(files: Rc<dyn TalkFiles>) -> TalkComponents`, `none() -> TalkComponents` for tests, `infos(&self) -> &[ComponentInfo]`, `problems(&self) -> &[Diagnostic]`, `schema(&mut self, tag: &str) -> Option<ComponentSchema>`.
  - `pub(crate) enum Rendered { Output(RenderOutput), NeedsBuilding, Failed(ComponentError), OutOfTime }`, and `pub(crate) fn render(&mut self, tag: &str, input: &RenderInput) -> Option<Rendered>`, with `None` for a tag the talk has no component for.
  - `pub(crate) fn new_layout(&mut self)`, which restarts the budget of `SLIDE_RENDER_BUDGET` (1 s).
  - `LayoutEnv` gains `components: &'a mut TalkComponents`.
  - `Deck::components(&self) -> &[ComponentInfo]`, `Deck::component_problems(&self) -> &[Diagnostic]`, `Deck::component_schema(&mut self, tag: &str) -> Option<ComponentSchema>` (built-ins too), `Deck::refresh_components(&mut self) -> Vec<usize>` (the slides to show again).
  - `Deck::with_registry(files: Box<dyn TalkFiles>, fonts: Fonts, source: String, registry: Registry) -> Deck`, which `Deck::open` calls with the built-ins, and which tests use.
- `Deck` holds `files: Rc<dyn TalkFiles>`.

**Scanning** reads `build/index.json` (a file that can't be parsed counts as no builds, with a problem) and at most 256 sources from `names_in("components")`, each at most 1 MiB, ignoring all but `.rs`. A stem failing `safe_stem` is a problem. A source's tag comes from `component_tag`. Its state is `UpToDate` if the index has that tag with this source's hash and `build/<file>.wasm` exists, `NeedsBuilding` if there's no such build, and `OutOfDate` otherwise. A build with no source still runs, and is `UpToDate`, as a talk can carry a build without its source. Two sources claiming one tag is a problem, and the first in name order wins.

**Running:** `render` loads `build/<file>.wasm` on first use and keeps it. A module whose schema names a different tag is `Failed(Malformed)`. Past the budget, it gives `OutOfTime` without running anything.

`Flow::render_tag(tag, room: (f32, f32))` looks in order: the `Text` tag, the registry, then `TalkComponents`, and the room becomes `CtxData::width` and `height`. In the flow the room is the region's width and 0, and for a positioned tag it's the `w` it gives or 1200, and the `h` it gives or 0. A failure becomes `Element::Problem` with `"{tag}: {short}"`, so the one labelled box serves every case. `NeedsBuilding` says `"{tag} needs building"`. An `OutOfDate` component runs and adds the notice `"{tag} is out of date, so this is its last build"`. A talk's component named like a built-in is a problem and never runs. Native components get an assets closure holding them to `asset_path_allowed` and reading through `files`.

`Deck::update` calls the scan again, and anything but the same builds and sources counts like a replaced picture, so slides are laid out again.

**Blocks only:** `Slide` gains `inline_tags: Vec<String>`, filled in `parse.rs` for an inline tag other than `Text`, and `layout_slide` adds the notice `"{name} is in the middle of a line. A component goes on a line of its own"`.

- [ ] **Step 1: Write the failing tests**

With a map of files holding `slides.md`, `build/index.json` and modules from a WAT helper that answers with fixed JSON:
- `a_component_from_the_talk_draws_on_its_slide`: `<Hello />` draws its text.
- `its_tag_attributes_reach_it`.
- `children_go_where_the_component_puts_them`: `Element::Children` lays out the Markdown between the tags.
- `a_component_with_no_build_shows_that_it_needs_building`: a placeholder and the words `Hello needs building`.
- `an_out_of_date_component_runs_its_last_build_and_says_so`.
- `a_component_that_crashes_leaves_the_rest_of_the_slide_alone`: the heading still draws, and there's a box saying `Hello: stopped working`.
- `a_slide_of_hanging_components_stays_within_its_budget`: forty `<Hang />` tags take under 3 s, and every one shows a box.
- `a_build_claiming_another_name_does_not_run`.
- `a_component_cannot_take_a_built_ins_name`: a talk's `Box` never runs, and the built-in still draws.
- `a_broken_index_counts_as_no_builds`.
- `a_file_with_an_unsafe_name_is_a_problem`.
- `too_many_sources_stop_at_the_limit`.
- `a_new_build_is_picked_up`: after the files change, `refresh_components` lists the slide, and the new text draws.
- `a_build_with_no_source_still_runs`.
- `the_schema_of_a_component_is_there_for_a_form`: for both a built-in and a talk's own.
- `a_packed_talk_runs_its_components`: through `pack_folder` and `open_packed`.
- `a_component_in_the_middle_of_a_line_is_noticed`.
- `a_positioned_component_is_told_its_box`: `w="500" h="300"` gives `ctx.width() == 500.0` and `ctx.height() == 300.0`, through `Deck::with_registry`.
- `a_built_in_reads_only_assets`: through `Deck::with_registry`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test talk_components`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

`crates/engine/tests/builtins.rs` gets `components: &mut TalkComponents::none()` in its `LayoutEnv`s.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/components crates/engine crates/format Cargo.lock
git commit -m "Engine: a talk's own components on its slides"
```

---

### Task 7: A slide at a moment

**Files:**
- Modify: `crates/engine/src/{deck,elements,slide,timeline,display}.rs`, `crates/pptx/src/write.rs`
- Test: `crates/engine/tests/moments.rs`, `crates/pptx/tests/write.rs`

**Interfaces:**
- Consumes: `Ctx::finish`, `RenderOutput` (Task 1), `Deck::with_registry` (Task 6).
- Produces, in `slides_engine`:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Moment { pub step: u32, pub time: f32 }`, with `Moment::START`.
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct ComponentAsks { pub steps: u32, pub stepped: bool, pub animated: bool, pub tags: Vec<String> }`, where `tags` names, once each, the components that vary.
  - `LayoutEnv` gains `moment: Moment` and `asks: ComponentAsks`. `LaidOutSlide` gains `asks`.
  - `LayerSpec` gains `varies: bool`: something in this layer changes with the moment.
  - `SlideTimeline` gains `animated: bool`, and its `steps` is now the larger of its layers' clicks and what components asked for, held to `MAX_CLICK`.
  - `Deck::layers_at(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>>`
  - `Deck::varying_tags(&mut self, index: usize) -> Vec<String>`
  - `Deck::layers` and `Deck::specs` and `Deck::specs_fonts_and_files` now give the slide's *whole* state: its last step at time 0. That's what the editor shows and what exports draw.

A moment that a slide doesn't vary by is the same as `Moment::START` in that respect: a slide with no `stepped` is step 0, and one with no `animated` is time 0. `SlideState.laid` stays the layout at `START`, and `SlideState.at_moment: Option<(Moment, Laid)>` holds the one other. A slide with nothing varying is laid out once, whatever it's asked for. `LayerKey` gains `moment: Option<(u32, u32)>`, set only for a layer with `varies`, because `drawing_fingerprint` doesn't see colours. `Drawn` remembers which moment `ordered` is for. `keep_pictures_within_budget` lets go of `at_moment` as well. PowerPoint adds one notice per entry of `varying_tags`: `"{tag} changes as the talk plays, so PowerPoint shows it in its last state"`.

- [ ] **Step 1: Write the failing tests**

Native test components through `Deck::with_registry`: `#[component] fn Count(ctx: &Ctx) -> Element { text(ctx.steps(3).to_string()) }`, `Clock` showing `ctx.time()`, `Tint` whose shape's colour changes with the step while its geometry doesn't, and `Rows`, which counts its renders in a static.
- `a_component_that_asks_for_steps_adds_them_to_the_slide`: `timeline(0).steps == 3`.
- `the_most_clicks_on_the_slide_win`: with a `click="5"` box, `steps == 5`.
- `each_step_shows_what_the_component_gives_at_that_step`: layers at steps 0 and 2 differ, and text `2` is drawn at step 2.
- `a_colour_change_between_steps_is_not_mistaken_for_the_same_layer`: `Tint`'s layers at steps 0 and 1 aren't the same pixels.
- `what_does_not_vary_keeps_its_image_between_steps`: `Arc::ptr_eq` for a heading's layer at steps 0 and 1.
- `the_same_moment_is_not_drawn_twice`: `Arc::ptr_eq` for two calls at one moment.
- `a_slide_with_nothing_varying_is_laid_out_once`: `Rows` ran once across moments 0, 1 and 2.
- `the_whole_slide_is_its_last_step`: `Deck::specs` draws text `3`.
- `the_time_reaches_the_component`: `Clock` at time 1.5.
- `a_slide_that_moves_says_so`: `timeline(0).animated`.
- `a_slide_that_moves_is_still_at_time_zero_for_an_export`.
- `pictures_held_at_a_moment_are_let_go_within_the_budget`.

In `crates/pptx/tests/write.rs`: `a_component_that_varies_is_noted_when_exported`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test moments`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine crates/pptx
git commit -m "Engine: a slide at a moment, for components that change"
```

---

### Task 8: The ready-made chart

**Files:**
- Create: `crates/engine/src/chart.rs`, `crates/engine/tests/chart.rs`, `crates/engine/tests/golden/chart.png`
- Modify: `crates/engine/src/{builtins,lib}.rs`

**Interfaces:**
- Consumes: `canvas`, `at`, `problem`, `Ctx::steps`, `Ctx::asset_text`, `#[derive(Prop)]`, `DataFile` (Task 1).
- Produces: `#[derive(Prop)] enum ChartKind { Bar, Line, Pie }`, and the built-in `#[component] fn Chart(kind: ChartKind, labels: String, values: String, data: Option<DataFile>, title: Option<String>, build: bool, ctx: &Ctx) -> Element`. Pure helpers, `pub(crate)` and tested through a `#[doc(hidden)] pub mod chart_parts` re-export: `nice_ceiling(value: f64) -> f64`; `parse_inline(labels: &str, values: &str) -> Result<ChartData, String>`; `parse_csv(text: &str) -> Result<ChartData, String>`; `series_color(theme: &Theme, index: usize) -> Color`. `pub struct ChartData { pub labels: Vec<String>, pub series: Vec<(String, Vec<f64>)> }`.

Limits are 1000 points and 16 series. The size is `ctx.width()`, or 1200 if that's 0, by `ctx.height()`, or else that width × 9/16 held to 675. The plot leaves 64 at the top for a title if there is one, 48 at the bottom for a legend if there are several series, 96 at the left for tick labels and 56 at the bottom for category labels. Text is 24 units, and the title 40 and bold.

`nice_ceiling` is the smallest of 1, 2, 5 or 10 times a power of ten at or above the value, and 1 for 0. The axis runs from 0, or the negative of `nice_ceiling` of the magnitude if any value is negative, to `nice_ceiling` of the largest. There are five labelled ticks.

Bars are `ShapeKind::Rect`, each category's group filling 80% of its slot. A line is `Element::Path` with `stroke` and no fill, 6 wide, with a 16×16 `Ellipse` at each point. Pie slices are filled paths using `A` arcs, and one slice of everything is an `Ellipse`. Colours come from `series_color`: the accent for the first, then the accent's hue turned 47° for each next one. A pie's slices are coloured as series.

With `build`, the chart calls `ctx.steps(n)`, where n is the number of series if there are several, or else the number of bars, points or slices, and draws only as many as `steps` returned.

- [ ] **Step 1: Write the failing tests**

- `nice_ceilings`: 24→50, 10→10, 0→1, 0.37→0.5, 101→200.
- `labels_and_values_are_read`: `"Q1, Q2"` and `"10, 24"`.
- `a_mismatch_says_what_it_found`: `Err("The chart has 3 labels and 2 values")`.
- `a_value_that_is_not_a_number_says_so`: `Err("\"x\" isn't a number")`.
- `csv_gives_series_by_column`: a header `Month,Sales,Costs` and two rows.
- `csv_handles_quotes`: `"A, B",1`.
- `an_empty_chart_says_so`: `Err("The chart has no data")`.
- `the_limits_are_held`: 1001 points or 17 series is an `Err`.
- `bars_are_as_tall_as_their_values`: 10, 24, 17 on a chart of known height give shapes at 0.2, 0.48 and 0.34 of the plot's height.
- `a_negative_value_goes_below_the_axis`.
- `a_line_chart_draws_a_stroked_path_and_a_point_each`.
- `a_pie_chart_draws_a_slice_each`, and `one_value_is_a_whole_circle`, and `a_pie_cannot_show_a_negative_value` (`Element::Problem`).
- `a_csv_file_is_read_from_assets`, through `Deck` with `<Chart kind="bar" data="assets/sales.csv" />`.
- `a_csv_outside_assets_is_refused` and `a_missing_csv_shows_a_labelled_box` (`Can't read assets/x.csv`).
- `building_adds_a_click_for_each_bar`: `timeline(0).steps == 3`, with one bar at step 1 and all three at the end.
- `several_series_build_one_series_per_click`.
- `not_building_adds_no_clicks`.
- `a_chart_exports_as_real_shapes`: its bars are `NativeContent::Shape`, its labels `NativeContent::Text` and its line `NativeContent::Path`.
- `a_chart_looks_the_same`: a golden image.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test chart`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it, and generate the golden image the way `crates/engine/tests` does for the others**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS. Look at the golden image yourself before committing it.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Charts: bar, line and pie, ready-made"
```

---

### Task 9: Clicks and movement in a show

**Files:**
- Modify: `crates/app/src/{present_view,document}.rs`
- Test: `crates/app/tests/ui_present.rs`

**Interfaces:**
- Consumes: `Deck::layers_at`, `Moment`, `SlideTimeline::animated` (Task 7), `<Chart build>` (Task 8).
- Produces: `Document::layers_at(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>>`. `show_on` takes the moment's time. `Presentation::time_at(&self, now: Instant) -> f32`: seconds since the current slide appeared, with the transition's first frame as its start, held to steps of 1/30 s so that both windows ask for the same moment and share one layout.

While the slide showing is `animated`, each view keeps asking for frames. When `time_at` has changed since it last drew, it calls `show_on` again. A slide that doesn't move asks for no frames, as today. The next-step thumbnail shows its own step at time 0.

- [ ] **Step 1: Write the failing tests**

- `a_chart_that_builds_takes_a_press_for_each_bar`: three bars take three Rights to reach 3 of 3 and the fourth moves on.
- `each_press_draws_the_new_step`: the canvas's images differ between steps.
- `going_back_shows_the_step_before`.
- `the_next_step_shows_what_the_next_press_brings`.
- `a_slide_that_moves_keeps_asking_for_frames`: a talk folder with a WAT module answering `"animated":true`, written to `build/` with its index.
- `a_still_slide_asks_for_no_frames`.
- `the_time_starts_again_on_each_slide`.
- `a_save_during_a_moving_slide_keeps_the_show_on_a_real_slide`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_present`
Expected: FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "Present: components change with clicks and over time"
```

---

### Task 10: Building, and the rules for when

**Files:**
- Create: `crates/app/src/{builds,permissions}.rs`, `crates/app/tests/builds.rs`
- Modify: `crates/app/src/{lib,watcher,workspace_view,document}.rs`, `crates/app/Cargo.toml`, `crates/app/tests/ui_slides.rs`

**Interfaces:**
- Consumes: `compile`, `Bundle`, `Diagnostic`, `BuildIndex`, `INDEX_FILE`, `source_hash` (Tasks 4 and 6), `write_atomically`.
- Produces, in `stagepack_studio`:
  - `permissions::BuildPermissions`: `load(file: PathBuf) -> BuildPermissions`, `default_file() -> Option<PathBuf>` (the app's data folder, from `dirs`), `always(&self, folder: &Path) -> bool`, `set_always(&mut self, folder: &Path) -> io::Result<()>`. Folders are kept by canonical path, in a JSON file, written atomically. A file that can't be read counts as no permissions.
  - `builds::Compile = Arc<dyn Fn(&str, &str) -> Result<Vec<u8>, Vec<Diagnostic>> + Send + Sync>`
  - `builds::BuildOutcome { pub built: Vec<String>, pub problems: Vec<Diagnostic> }`
  - `builds::build_components(folder: &Path, compile: &Compile) -> BuildOutcome`: for each out-of-date or unbuilt source, compile it, and only on success write `build/<file>.wasm` and then `build/index.json`, keeping other entries.
  - `builds::behind(folder: &Path) -> Vec<String>`: the tags of the components whose source no longer matches their build.
  - `watcher::Change { Slides, Component }`, and `watch(folder, on_change: impl Fn(Change) + Send + 'static)`, recursive, where only `slides.md` at the top and `*.rs` directly in `components` count.
  - `Document::components`, `Document::component_problems`, `Document::reload_components(&mut self) -> Reload`.
  - `WorkspaceView` ids `pub const BUILD_ONCE_ID: &str = "build-once"`, `BUILD_ALWAYS_ID: &str = "build-always"`, `COMPONENT_PROBLEMS_ID: &str = "component-problems"`, and `pub fn set_compile_for_test(&mut self, compile: Compile)`.

The rules, from the design:
- Opening a talk never builds. A bar appears for a folder talk with anything out of date or unbuilt: *Build components* (once) and *Always build this folder*.
- A component's change on disk builds only in an always folder, debounced as `reload_soon` is. One build runs at a time, and a change during it starts one more after it.
- A packed talk never builds and shows no bar.
- Problems from the source check, the compiler and the scan show in a panel below the slide, as `components/score.rs:12: message`. A successful build clears its own.
- A failed build changes no file in `build/`.
- With no bundle, the bar's buttons are replaced by the sentence `This copy of the app has no compiler, so components can't be built.`.
- `TalkPlace::save_packed` adds, for each out-of-date component: `"{tag} couldn't be built just now, so the packed talk carries its last good build"`.

- [ ] **Step 1: Write the failing tests**

`builds.rs`, with a counting fake `Compile` returning a WAT-built module:
- `building_writes_the_build_and_its_index`.
- `an_up_to_date_component_is_not_built_again`.
- `a_failed_build_keeps_the_last_good_one`: the wasm and index are byte for byte as before, and the problem is reported.
- `one_failure_does_not_stop_the_others`.
- `a_file_with_an_unsafe_name_is_not_built`.
- `permission_is_kept_outside_the_talk`: after `set_always`, the talk's folder has no new file.
- `a_folder_is_known_however_it_is_written`: `talk/../talk`.
- `a_broken_permissions_file_counts_as_none`.
- `a_packed_copy_says_which_builds_are_behind`.

`ui_slides.rs`:
- `opening_a_talk_starts_no_build`: the fake was called 0 times.
- `the_bar_offers_both_choices`.
- `build_once_builds_and_shows_the_component`: and a later change builds nothing.
- `build_always_is_remembered_and_builds_on_change`.
- `a_change_in_a_folder_not_allowed_starts_no_build`.
- `a_packed_talk_offers_no_build`.
- `a_problem_shows_its_file_and_line`.
- `no_compiler_says_so`.

`watcher` tests: `a_save_to_a_component_counts`, `a_write_to_the_build_folder_does_not`, `a_rust_file_elsewhere_does_not`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test builds`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Builds run on the background executor. Ids follow the existing `NOTICES_ID` and `PROBLEM_ID`.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app Cargo.lock
git commit -m "App: build components, and only when allowed to"
```

---

### Task 11: For coders: the Cargo file, the guide and examples

**Files:**
- Create: `crates/app/src/cargo_file.rs`, `docs/guide/components.md`, `fixtures/components/slides.md`, `fixtures/components/components/{score,callout,reveal,clock,bars}.rs`, `fixtures/components/assets/sales.csv`, `crates/components/tests/examples.rs`
- Modify: `crates/app/src/{lib,workspace_view}.rs`, `scripts/make-bundle.sh`, `scripts/make-bundle.ps1`

**Interfaces:**
- Consumes: `Bundle` (Task 4), `component_tag`.
- Produces: `cargo_file::write_cargo_file(folder: &Path, bundle: &Bundle) -> Result<PathBuf, String>`. It writes `Cargo.toml` at the talk's top level with an empty `[workspace]`, `toolkit = { path = "<bundle>/toolkit-src/toolkit" }`, and one `[[example]]` per component with `crate-type = ["cdylib"]`. It refuses to overwrite one that exists. It's reached only through a File menu item, *Set up for a code editor*, shown for a folder talk when there's a bundle.

The guide is short and has no Rust knowledge beyond functions: where a file goes, the tag, arguments and their form fields, `row!`, `col!`, `canvas`, `children`, `ctx.steps`, `ctx.time`, `ctx.asset_text`, what can't be used and why, and what the labelled boxes mean. Every code block in it is taken from a file in `fixtures/components`, and `make-bundle` copies it to `<bundle>/guide/components.md`.

`score.rs` is the design's example. `callout.rs` uses `children`. `reveal.rs` uses `ctx.steps`. `clock.rs` uses `ctx.time` and a `canvas`. `bars.rs` is the guide's worked example: a small bar chart on a `canvas`, in under 40 lines, built the way `<Chart>` is. `fixtures/components/slides.md` uses all of them and a `<Chart>` from `assets/sales.csv`.

- [ ] **Step 1: Write the failing tests**

`cargo_file` tests in `crates/app/tests/builds.rs`:
- `the_cargo_file_lists_each_component`.
- `an_existing_cargo_file_is_left_alone`.
- `nothing_writes_a_cargo_file_unasked`: after opening and building, there's no `Cargo.toml`.

`examples.rs`, `#[ignore = "needs the compiler bundle"]`:
- `every_example_builds_and_runs`: compiles each file in `fixtures/components/components`, loads it in `WasmComponent` and renders it.
- `the_guide_quotes_only_the_examples`: every fenced `rust` block in the guide appears word for word in one of those files. This one needs no bundle, so it isn't ignored.
- `the_cargo_file_works_with_cargo` (ignored, and `#[cfg(unix)]`): `cargo check --examples` in a copy of the fixture passes with the written `Cargo.toml`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test builds cargo_file`
Expected: FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/test-components.sh`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/components docs/guide fixtures/components scripts
git commit -m "Components: a guide, examples, and a Cargo file on request"
```

---

### Task 12: Run it for real and write down what happened

**Files:**
- Create: `docs/checks/plan-c.md`
- Modify: `README.md`, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md`

No new behaviour, and no Rust changes unless the run finds something. Fix what it finds, each with a test.

- [ ] **Step 1: Build for real**

Run: `scripts/make-bundle.sh target/bundle` and `scripts/cargo-safe.sh build --release -p stagepack-studio`

- [ ] **Step 2: Drive it in a real window**

On a nested display (Xephyr on :80), with `STUDIO_BUNDLE=$PWD/target/bundle`, on a scratch copy of `fixtures/components`:
1. Open it with no builds. The components show *needs building*, the bar offers both choices, and no compiler ran.
2. Press *Build components*. They appear. Record the seconds it took.
3. Press *Always build this folder*. Change a string in `score.rs` and save. The slide changes. Record the seconds from save to changed.
4. Add a type error and save. The panel names the file and line, and the last good build stays on the slide.
5. Fix it, and start a show. Press through `reveal` and the building chart. Catch `clock` moving in two screenshots.
6. Export to PDF and PPTX, and open both. Check that the chart's bars are shapes, using the XML. Convert the PPTX with LibreOffice and look at it.
7. Pack it, open the packed file with `STUDIO_BUNDLE` unset, and present it. It shows with no prompt and no bar.
8. Put a component that loops forever on a slide, build it, and see the labelled box.

Look at the screenshots yourself.

- [ ] **Step 3: Write `docs/checks/plan-c.md`**

Like `docs/checks/plan-b.md`: the measured test counts per crate, including how many are ignored and that they pass under `scripts/test-components.sh`; the bundle's size; what the real run showed; and "Not exercised". Say plainly that no Windows machine has run this, that the Windows bundle script is untested, and that real PowerPoint hasn't opened a freeform. Give the by-hand Windows check: clone `main`, build the bundle with `make-bundle.ps1`, and under an account with no Rust, build and show a component, then open a talk with a chart in desktop PowerPoint and say whether its lines and pie slices can be selected and recoloured.

- [ ] **Step 4: Update the README and the roadmap's plan C section**

One paragraph in the README pointing to the guide. In the roadmap, name the file for plan C, as A and B are named.

- [ ] **Step 5: Commit**

```bash
git add docs/checks/plan-c.md README.md docs/superpowers/plans/2026-11-09-native-rust-roadmap.md
git commit -m "Plan C check: what ran, and how to check Windows and PowerPoint"
```
