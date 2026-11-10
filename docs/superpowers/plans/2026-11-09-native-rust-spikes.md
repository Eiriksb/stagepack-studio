# Native Rust spikes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Answer the design's three go-or-no-go questions with throwaway builds and measured numbers, before any product code is planned.

**Architecture:** Three small programs under `spikes/`, on a branch that is never merged. What survives is `docs/checks/native-rust-spikes.md`, which records each measurement against its limit. Pure logic gets real tests. Windows and input methods get short checklists for the owner.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 (`gpui-fast`), tiny-skia 0.11.4, cosmic-text 0.19.0, markdown 1.0.0, Wasmtime, syn 2, serde, proptest.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`, section "Order of work".

## Global Constraints

- Everything happens on the branch `spike/native-rust`, cut from `plan-1`. It is never merged.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Every measurement uses `--release`.
- Pin `gpui-kit = { version = "=0.9.0", features = ["gpui-fast"] }`, `tiny-skia = "=0.11.4"`, `cosmic-text = "=0.19.0"`, `markdown = "=1.0.0"`.
- Slide units are 1920×1080 for a 16:9 talk.
- Fonts are loaded from a fixed list with system font discovery off: `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` beside it, and `/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf`. The app's real fonts come later.
- A component compiles with `rustc` directly for `wasm32-unknown-unknown`, with no Cargo and no WASI. Its exports are `abi_version`, `alloc`, `schema` and `render`.
- Reload goal: under two seconds for a 100-line component. The owner accepted "within a few seconds", which this plan reads as a 5-second limit.
- The whole install must fit in 600 MB on disk.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. It holds the owner's own uncommitted edit.
- Don't name real decks, clients or organisations in commits. Use fixtures.

## Review Focus

- **Long sessions:** thousands of drag or fade frames must not grow memory, because every frame adds an image to GPUI's atlas. Owned by Task 2's 3000-frame run.
- **Characters outside the basic plane in input-method ranges:** after `æ🎉` the caret must land after the emoji, because GPUI speaks UTF-16 and Rust strings are bytes. Owned by Task 6's `utf16_offsets_round_trip`.
- **Typing Markdown's own symbols:** `*`, `<`, `#` and `1.` typed as text must stay text. Owned by Task 5's `symbols_stay_text`.
- **A talk in a folder with spaces and non-ASCII letters:** compiling must still work. Owned by Task 3's `handles_spaces_and_non_ascii_paths`.
- **A component that stops compiling:** the error must give the file and line, and the last good build must stay in place. Owned by Task 3's `broken_source_reports_line_and_keeps_last_build`.

---

### Task 1: Draw a test slide with tiny-skia and cosmic-text

**Files:**
- Modify: `Cargo.toml` (add `spikes/drawing` to `members`)
- Create: `spikes/drawing/Cargo.toml`, `spikes/drawing/src/lib.rs`, `spikes/drawing/src/bin/spike-raster.rs`, `spikes/drawing/tests/raster.rs`
- Create: `docs/checks/native-rust-spikes.md`

**Interfaces:**
- Produces, in crate `spike_drawing`:
  - `pub struct Fonts { pub system: cosmic_text::FontSystem, pub cache: cosmic_text::SwashCache }` with `pub fn spike_set() -> Fonts`
  - `pub struct Layer { pub name: &'static str, pub pixmap: tiny_skia::Pixmap, pub x: i32, pub y: i32 }`
  - `pub fn build_layers(fonts: &mut Fonts, width: u32, height: u32) -> Vec<Layer>`
  - `pub fn compose(target: &mut tiny_skia::Pixmap, layers: &[Layer])`
  - `pub fn compose_fade(target: &mut tiny_skia::Pixmap, from: &tiny_skia::Pixmap, to: &tiny_skia::Pixmap, t: f32)`
  - `pub fn render_slide(fonts: &mut Fonts, width: u32, height: u32) -> tiny_skia::Pixmap`
  - `pub fn draw_buffer(target: &mut tiny_skia::PixmapMut, buffer: &cosmic_text::Buffer, fonts: &mut Fonts, transform: tiny_skia::Transform)`
  - `pub fn to_bgra_frame(pixmap: &tiny_skia::Pixmap) -> image::Frame`

- [ ] **Step 1: Create the branch and commit the approved documents**

```bash
git switch -c spike/native-rust plan-1
git add docs/superpowers/specs/2026-11-09-native-rust-slides-design.md docs/superpowers/plans/2026-11-09-native-rust-roadmap.md docs/superpowers/plans/2026-11-09-native-rust-spikes.md
git commit -m "Native Rust slides: design, roadmap and spike plan"
```

- [ ] **Step 2: Write the failing tests in `spikes/drawing/tests/raster.rs`**

```rust
fn layer<'a>(layers: &'a [Layer], name: &str) -> &'a Layer {
    layers.iter().find(|l| l.name == name).unwrap()
}

#[test]
fn same_pixels_twice() {
    let mut fonts = Fonts::spike_set();
    let a = render_slide(&mut fonts, 1920, 1080);
    let b = render_slide(&mut fonts, 1920, 1080);
    assert_eq!(a.data(), b.data());
}

#[test]
fn has_every_layer() {
    let names: Vec<_> = build_layers(&mut Fonts::spike_set(), 1920, 1080).iter().map(|l| l.name).collect();
    assert_eq!(names, ["background", "card", "ellipse", "arrow", "picture", "rotated-text", "paragraph", "emoji"]);
}

#[test]
fn rotated_text_is_taller_than_its_line() {
    let layers = build_layers(&mut Fonts::spike_set(), 1920, 1080);
    assert!(layer(&layers, "rotated-text").pixmap.height() > 200);
}

#[test]
fn emoji_is_coloured() {
    let layers = build_layers(&mut Fonts::spike_set(), 1920, 1080);
    let colour = layer(&layers, "emoji").pixmap.pixels().iter()
        .any(|p| p.alpha() > 0 && (p.red() != p.green() || p.green() != p.blue()));
    assert!(colour);
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test --release -p spike-drawing`
Expected: FAIL, `build_layers` not found.

- [ ] **Step 4: Implement the interfaces in `spikes/drawing/src/lib.rs`**

Content in slide units, scaled by `width / 1920`:
- `background`: a linear gradient over the whole slide.
- `card`: a 600×300 rounded rectangle with a gradient fill.
- `ellipse`, and `arrow` as a path with a 6-unit outline.
- `picture`: a 640×360 generated checkerboard, rotated 10°.
- `rotated-text`: a line rotated 15° with three runs: regular 48, bold red 64 reading `12%`, regular 48.
- `paragraph`: 60 words at size 32.
- `emoji`: `🎉` at size 96.

Draw text with glyph outlines from `SwashCache::get_outline_commands`, filled by tiny-skia with the layer's transform. Colour glyphs come from `SwashCache::get_image`. Each layer already holds its rotation, so moving one is a plain blit. `Fonts::spike_set` builds a `fontdb::Database` from the fixed list only. `to_bgra_frame` swaps red and blue, because GPUI's `RenderImage` holds BGRA.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test --release -p spike-drawing`
Expected: 4 passed.

- [ ] **Step 6: Implement `spike-raster` and measure**

`spike-raster <width> <height>` renders 20 times, writes `target/spikes/slide-<width>.png`, and prints one JSON line: `{"width","height","raster_ms_p50","raster_ms_p95"}`.

Run: `scripts/cargo-safe.sh run --release -p spike-drawing --bin spike-raster -- 1920 1080`, then again with `3840 2160`.
Expected: `raster_ms_p50` at most 150 at 1920×1080, and at most 500 at 3840×2160.

- [ ] **Step 7: Look at the picture**

Open `target/spikes/slide-1920.png`. Expected: the rotated line is sharp and legible, `12%` is bold, red and larger, and the emoji is in colour.

- [ ] **Step 8: Start the results file and commit**

Create `docs/checks/native-rust-spikes.md` with a table of spike, measure, limit and result. Add the two raster rows and the picture check.

```bash
git add Cargo.toml Cargo.lock spikes/drawing docs/checks/native-rust-spikes.md
git commit -m "Spike: draw a test slide with tiny-skia and cosmic-text"
```

### Task 2: Show the slide in a GPUI window and measure it

**Files:**
- Create: `spikes/drawing/src/bin/spike-window.rs`
- Modify: `spikes/drawing/Cargo.toml`, `docs/checks/native-rust-spikes.md`

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces: the command `spike-window --width W --height H --mode drag|fade --frames N`, which prints one JSON line: `{"mode","width","height","frames","work_ms_p50","work_ms_p95","frame_ms_p50","frame_ms_p95","rss_mb_at_60","rss_mb_at_end"}`.

- [ ] **Step 1: Implement `spike-window`**

- A 1280×720 window using `gpui_kit::application()`, with no forced X11 mode.
- A view holding the layers and the last image, drawn through `canvas`. Each frame composes into one bitmap of the requested size, builds an `Arc<RenderImage>` from `to_bgra_frame`, calls `window.paint_image` scaled to the window, calls `window.drop_image` on the previous one, then `window.request_animation_frame()`.
- `drag` moves the `card` layer around a circle of radius 200 units.
- `fade` cross-fades between two pre-composed slides over 30 frames with `compose_fade`, repeating.
- `work_ms` covers composing and building the image. `frame_ms` is the time between consecutive paints. RSS comes from `/proc/self/statm`. After `--frames` it prints the JSON line and quits.

This machine's only display is 1920×1200, so "4K" here means a 3840×2160 bitmap scaled into the window. Run on the real display, not a nested server, because nested servers have no real GPU.

- [ ] **Step 2: Measure smoothness**

Build with `scripts/cargo-safe.sh build --release -p spike-drawing`, then run `target/release/spike-window` with each of:
- `--width 1920 --height 1080 --mode drag --frames 600`
- `--width 1920 --height 1080 --mode fade --frames 600`
- `--width 3840 --height 2160 --mode drag --frames 600`
- `--width 3840 --height 2160 --mode fade --frames 600`

Expected: `frame_ms_p95` at most 17.5 at 1920×1080, which is 60 frames per second, and at most 34 at 3840×2160, which is 30.

- [ ] **Step 3: Measure memory over a long session**

Run: `target/release/spike-window --width 1920 --height 1080 --mode drag --frames 3000`
Expected: `rss_mb_at_end` minus `rss_mb_at_60` is at most 64.

- [ ] **Step 4: Record the verdict and commit**

Add one row per run. Write spike 1's verdict as yes or no. A no means the design's fallback, a wgpu compositor, has to be discussed with the owner.

```bash
git add spikes/drawing docs/checks/native-rust-spikes.md Cargo.lock
git commit -m "Spike: measure the slide bitmap through GPUI"
```

### Task 3: Compile a Rust component with a trimmed compiler on Linux

**Files:**
- Modify: `Cargo.toml` (add `spikes/compile/macros`, `spikes/compile/toolkit`, `spikes/compile/host`)
- Create: `spikes/compile/macros/{Cargo.toml,src/lib.rs}` (crate `toolkit_macros`)
- Create: `spikes/compile/toolkit/{Cargo.toml,src/lib.rs}` (crate `toolkit`)
- Create: `spikes/compile/host/{Cargo.toml,src/lib.rs,src/main.rs}` (crate `compile-host`), `spikes/compile/host/tests/compile.rs`
- Create: `spikes/compile/fixtures/score.rs`, `spikes/compile/bundle.sh`

**Interfaces:**
- Produces, in crate `compile_host`:
  - `pub struct Bundle { pub root: std::path::PathBuf }`
  - `pub struct Diagnostic { pub file: String, pub line: u32, pub message: String }`
  - `pub fn compile(bundle: &Bundle, source: &Path, out: &Path) -> Result<Duration, Vec<Diagnostic>>`
  - `pub fn render(wasm: &Path, props_json: &str) -> anyhow::Result<String>`
  - `pub fn schema(wasm: &Path) -> anyhow::Result<String>`
- Produces the command `compile-host measure --bundle DIR --source FILE --saves N [--opt 0|1|2]`, which prints one JSON line: `{"saves","opt","compile_ms_p50","compile_ms_p95","render_ok","bundle_mb","wasm_kb"}`. Its work folder is `<temp>/my talk æøå 100%`, created by the program.

- [ ] **Step 1: Write the failing tests in `spikes/compile/host/tests/compile.rs`**

```rust
fn bundle() -> Bundle {
    Bundle { root: std::env::var_os("SPIKE_BUNDLE").expect("run bundle.sh first").into() }
}

/// Copies fixtures/score.rs to <temp>/<folder>/score.rs and compiles it to out.wasm beside it.
fn build_fixture(folder: &str) -> (tempfile::TempDir, PathBuf, PathBuf);  // (guard, source, out)

const PROPS: &str = r#"{"team":"Oslo","points":3,"colour":"green"}"#;

#[test]
fn compiles_and_renders() {
    let (_guard, _source, out) = build_fixture("plain");
    assert!(render(&out, PROPS).unwrap().contains("Oslo"));
}

#[test]
fn schema_lists_arguments() {
    let (_guard, _source, out) = build_fixture("plain");
    assert_eq!(schema(&out).unwrap(),
        r#"[{"name":"team","type":"String"},{"name":"points","type":"u32"},{"name":"colour","type":"Color"}]"#);
}

#[test]
fn handles_spaces_and_non_ascii_paths() {
    let (_guard, _source, out) = build_fixture("my talk æøå 100%");
    assert!(render(&out, PROPS).unwrap().contains("Oslo"));
}

#[test]
fn broken_source_reports_line_and_keeps_last_build() {
    let (_guard, source, out) = build_fixture("plain");
    let good = std::fs::read(&out).unwrap();
    std::fs::write(&source, "use toolkit::prelude::*;\n#[component]\nfn Broken() -> Element {\n    let x: u32 = \"no\";\n    text(\"a\")\n}\n").unwrap();
    let diags = compile(&bundle(), &source, &out).unwrap_err();
    assert_eq!(diags[0].line, 4);
    assert!(diags[0].file.ends_with(".rs"));
    assert_eq!(std::fs::read(&out).unwrap(), good);
}
```

- [ ] **Step 2: Implement `toolkit_macros` and `toolkit`**

- `toolkit::prelude` exports `Element`, `Color`, `text`, `row!`, `badge` and the `component` attribute. `Element` is a serde-serialisable tree.
- `#[component]` turns `fn Name(args…) -> Element` into a private function plus `#[unsafe(no_mangle)] pub extern "C"` exports:
  - `abi_version() -> u32`, which returns 1
  - `alloc(len: u32) -> u32`
  - `schema() -> u64`
  - `render(ptr: u32, len: u32) -> u64`
- Returned `u64`s are `(ptr << 32) | len`. Props arrive as JSON, and the tree and schema leave as JSON.
- The generated code reaches serde only through a `toolkit::__private` re-export, because a component is compiled with `--extern toolkit` alone.

- [ ] **Step 3: Write `spikes/compile/fixtures/score.rs` and `bundle.sh`**

The fixture is `fn Score(team: String, points: u32, colour: Color) -> Element` in about 100 lines, using helper functions, a loop, `format!` and a `Vec`.

`bundle.sh <dir>` copies from `rustc --print sysroot` only:
- `bin/rustc`, `lib/librustc_driver-*.so`, `lib/libLLVM*`
- `lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld`
- `lib/rustlib/wasm32-unknown-unknown/lib/*.rlib`

It then runs `cargo build --release -p toolkit --target wasm32-unknown-unknown` and copies `libtoolkit.rlib` to `<dir>/toolkit/`, its `deps/*.rlib` to `<dir>/toolkit/deps/`, and the host `libtoolkit_macros-*.so` to `<dir>/toolkit/deps/`.

If `rustc` then asks for a host crate, add the smallest set from `lib/rustlib/x86_64-unknown-linux-gnu/lib` and record the size it adds.

- [ ] **Step 4: Run the tests to verify they fail**

Run: `spikes/compile/bundle.sh target/spikes/bundle && SPIKE_BUNDLE=$PWD/target/spikes/bundle scripts/cargo-safe.sh test --release -p compile-host`
Expected: FAIL, `compile` not found.

- [ ] **Step 5: Implement `compile`, `render` and `schema`**

`compile` runs this with a cleared environment, writes to a temp file, and renames over `out` only on success:

```
<bundle>/bin/rustc --sysroot <bundle> --edition 2024 --crate-name <stem> --crate-type cdylib
  --target wasm32-unknown-unknown -C opt-level=1 -C strip=debuginfo
  -L dependency=<bundle>/toolkit/deps --extern toolkit=<bundle>/toolkit/libtoolkit.rlib
  --error-format=json -o <temp> <source>
```

`render` and `schema` load the module in Wasmtime with no imports at all.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `SPIKE_BUNDLE=$PWD/target/spikes/bundle scripts/cargo-safe.sh test --release -p compile-host`
Expected: 4 passed.

- [ ] **Step 7: Measure a save, at each optimisation level**

Build with `scripts/cargo-safe.sh build --release -p compile-host`.

Run: `target/release/compile-host measure --bundle target/spikes/bundle --source spikes/compile/fixtures/score.rs --saves 20 --opt 1`, then with `--opt 0` and `--opt 2`. Each save changes a string literal in a temp copy.
Expected: `render_ok` is true. `compile_ms_p95` at most 2000 meets the goal, and over 5000 at every level is a no. `bundle_mb` at most 480, which leaves room in the 600 MB budget.

- [ ] **Step 8: Check it runs with no Rust available**

```bash
bwrap --unshare-all --clearenv --setenv PATH /usr/bin:/bin --ro-bind /usr /usr --symlink usr/lib /lib --symlink usr/lib64 /lib64 --symlink usr/bin /bin --proc /proc --dev /dev --tmpfs /tmp --tmpfs /home --ro-bind target/spikes/bundle /opt/bundle --ro-bind target/release/compile-host /opt/compile-host --ro-bind spikes/compile/fixtures /opt/fixtures /opt/compile-host measure --bundle /opt/bundle --source /opt/fixtures/score.rs --saves 5
```

Expected: one JSON line with `render_ok` true. First confirm `/usr/bin/rustc` doesn't exist, or the check proves nothing.

- [ ] **Step 9: Record and commit**

Add the rows for each optimisation level, the bundle size and the clean run.

```bash
git add Cargo.toml Cargo.lock spikes/compile docs/checks/native-rust-spikes.md
git commit -m "Spike: compile a Rust component with a trimmed compiler"
```

### Task 4: Check the compile loop on a real Windows machine

**Files:**
- Create: `.github/workflows/spike-compile-windows.yml`, `spikes/compile/bundle.ps1`, `spikes/compile/run-check.cmd`
- Modify: `docs/checks/native-rust-spikes.md`

**Interfaces:**
- Consumes: Task 3's crates and the `measure` command.
- Produces: a downloadable zip holding `compile-host.exe`, `bundle\`, `fixtures\` and `run-check.cmd`.

This machine has no Windows cross tools, so CI builds the zip.

- [ ] **Step 1: Write `bundle.ps1`, the Windows twin of `bundle.sh`**

It copies `bin\rustc.exe` and `bin\*.dll`, `lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe` and the wasm `*.rlib` files, then builds the toolkit and copies `toolkit_macros-*.dll` instead of the `.so`.

- [ ] **Step 2: Write `run-check.cmd`**

It warns if `where rustc` finds anything, runs `"%~dp0compile-host.exe" measure --bundle "%~dp0bundle" --source "%~dp0fixtures\score.rs" --saves 20`, then pauses.

- [ ] **Step 3: Write the workflow**

Trigger: push to `spike/native-rust`. Job on `windows-latest`: install Rust 1.98.0 with the `wasm32-unknown-unknown` target, run `bundle.ps1`, run `cargo build --release -p compile-host`, and upload the zip as `spike-compile-windows`.

- [ ] **Step 4: Push and confirm the build**

```bash
git add .github spikes/compile
git commit -m "Spike: build the compile loop for a Windows check"
git push -u origin spike/native-rust
gh run watch --exit-status
```

Expected: the run succeeds and the artifact exists.

- [ ] **Step 5: Add the owner's checklist to `docs/checks/native-rust-spikes.md`, and hand it over**

1. On the Windows PC, use a user account with no Rust. `where rustc` should find nothing.
2. Download `spike-compile-windows` from the Actions run, and unzip it to a folder with a space in its name.
3. Double-click `run-check.cmd`, and send back the JSON line it prints.
4. If Windows says a DLL is missing, send its name.

Expected: `render_ok` true, and `compile_ms_p95` at most 5000.

- [ ] **Step 6: Record the result and commit**

Write spike 2's verdict as yes or no, using both machines.

```bash
git add docs/checks/native-rust-spikes.md
git commit -m "Spike: record the compile loop on Windows"
```

### Task 5: Write styled text back to Markdown without touching anything else

**Files:**
- Modify: `Cargo.toml` (add `spikes/typing`)
- Create: `spikes/typing/Cargo.toml`, `spikes/typing/src/lib.rs`, `spikes/typing/src/spans.rs`, `spikes/typing/tests/round_trip.rs`

**Interfaces:**
- Produces, in crate `spike_typing::spans`:
  - `#[derive(Clone, Debug, PartialEq, Default)] pub struct Style { pub bold: bool, pub italic: bool, pub color: Option<String>, pub size: Option<u32> }`
  - `#[derive(Clone, Debug, PartialEq)] pub struct Span { pub text: String, pub style: Style }`
  - `pub fn parse_inline(markdown: &str) -> Result<Vec<Span>, String>`
  - `pub fn write_inline(spans: &[Span]) -> String`
  - `pub fn find_paragraphs(file: &str) -> Vec<std::ops::Range<usize>>`
  - `pub fn replace_paragraph(file: &str, paragraph: std::ops::Range<usize>, spans: &[Span]) -> String`

- [ ] **Step 1: Write the failing tests in `spikes/typing/tests/round_trip.rs`**

```rust
const STYLED: &str = r#"Revenue is **up** <Text color="red" size="64">12%</Text> this year."#;

#[test]
fn styled_line_round_trips_byte_for_byte() {
    assert_eq!(write_inline(&parse_inline(STYLED).unwrap()), STYLED);
}

#[test]
fn styled_line_has_the_right_spans() {
    let spans = parse_inline(STYLED).unwrap();
    assert_eq!(spans[1], Span { text: "up".into(), style: Style { bold: true, ..Default::default() } });
    assert_eq!(spans[3].style, Style { color: Some("red".into()), size: Some(64), ..Default::default() });
}

#[test]
fn bold_inside_a_coloured_tag() {
    let md = r#"<Text color="red">**big** word</Text>"#;
    assert_eq!(write_inline(&parse_inline(md).unwrap()), md);
}

#[test]
fn accents_and_emoji_survive() {
    let md = "blåbær é 🎉";
    assert_eq!(parse_inline(md).unwrap()[0].text, md);
}

#[test]
fn symbols_stay_text() {
    for typed in ["2 * 3 < 4", "_x_ and \\ and [a](b)", "1. not a list", "- not a list", "# not a heading", "<Box>"] {
        let spans = vec![Span { text: typed.into(), style: Style::default() }];
        assert_eq!(parse_inline(&write_inline(&spans)).unwrap(), spans);
    }
}

#[test]
fn bold_with_edge_spaces_is_still_bold() {
    let spans = vec![
        Span { text: "a".into(), style: Style::default() },
        Span { text: " b ".into(), style: Style { bold: true, ..Default::default() } },
        Span { text: "c".into(), style: Style::default() },
    ];
    let back = parse_inline(&write_inline(&spans)).unwrap();
    assert_eq!(back.iter().map(|s| s.text.as_str()).collect::<String>(), "a b c");
    assert!(back.iter().any(|s| s.text == "b" && s.style.bold));
}

#[test]
fn rest_of_file_untouched() {
    let file = "---\r\nlayout:   title\r\n---\r\n\r\n#   Odd   heading\r\n\r\nFirst   paragraph.\r\n\r\nSecond one.\r\n\r\n* a\r\n*   b\r\n";
    let second = find_paragraphs(file)[1].clone();
    let spans = vec![Span { text: "Changed".into(), style: Style::default() }];
    let out = replace_paragraph(file, second.clone(), &spans);
    assert_eq!(&out[..second.start], &file[..second.start]);
    assert_eq!(&out[second.start + "Changed".len()..], &file[second.end..]);
}

proptest! {
    #[test]
    fn writing_then_parsing_keeps_text_and_style(spans in non_empty_spans()) {
        prop_assert_eq!(parse_inline(&write_inline(&spans)).unwrap(), normalise(&spans));
    }
}
```

`non_empty_spans()` draws 1 to 6 spans from an alphabet of letters, `æ`, `🎉`, `*`, `_`, `<`, `\`, `#`, `[` and inner spaces, with any `Style`, and no leading or trailing space on the whole paragraph. `normalise` merges neighbouring spans of equal style and moves edge spaces out of bold and italic spans.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test --release -p spike-typing --test round_trip`
Expected: FAIL, `parse_inline` not found.

- [ ] **Step 3: Implement `spans.rs`**

Parse with the `markdown` crate: `frontmatter`, `mdx_jsx_flow` and `mdx_jsx_text` on, and `html_flow`, `html_text`, `mdx_esm` and the `mdx_expression_*` constructs off. `parse_inline` merges neighbouring spans of equal style.

`write_inline` backslash-escapes ASCII punctuation Markdown could read, and escapes a line-start `1.`, `-`, `+` or `#`. It moves edge spaces outside `**` and `*`, and wraps consecutive spans of the same colour and size in one `<Text>`, with any bold inside.

CommonMark can't always close `**` or `*`, for example after punctuation directly before a letter, or between adjacent bold and italic. So after writing a marked span, check that it parses back. If it doesn't, write `<Text bold="true">` or `<Text italic="true">` instead, and record how often the property test needed this.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test --release -p spike-typing --test round_trip`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock spikes/typing
git commit -m "Spike: write styled text back to Markdown exactly"
```

### Task 6: Type and style words on the slide

**Files:**
- Create: `spikes/typing/src/editing.rs`, `spikes/typing/src/bin/spike-typing.rs`, `spikes/typing/tests/editing.rs`, `spikes/typing/fixture.md`, `spikes/typing/fixture.original.md`
- Modify: `spikes/typing/Cargo.toml` (add `spike-drawing` and gpui-kit), `spikes/typing/src/lib.rs`, `docs/checks/native-rust-spikes.md`

**Interfaces:**
- Consumes: `spike_drawing::{Fonts, draw_buffer}`, and Task 5's `parse_inline`, `write_inline`, `find_paragraphs`, `replace_paragraph`.
- Produces, in `spike_typing::editing`:
  - `pub fn utf16_to_byte(text: &str, utf16: usize) -> usize`
  - `pub fn byte_to_utf16(text: &str, byte: usize) -> usize`
  - `pub struct TextBox`, with `pub fn from_spans(spans: &[Span], fonts: &mut Fonts) -> TextBox`, `pub fn insert(&mut self, text: &str, fonts: &mut Fonts)`, `pub fn select(&mut self, bytes: std::ops::Range<usize>)`, `pub fn apply(&mut self, change: StyleChange, fonts: &mut Fonts)`, and `pub fn spans(&self) -> Vec<Span>`
  - `pub enum StyleChange { ToggleBold, Color(String), Size(u32) }`

- [ ] **Step 1: Write the failing tests in `spikes/typing/tests/editing.rs`**

```rust
#[test]
fn utf16_offsets_round_trip() {
    let text = "æ🎉x";
    for (utf16, byte) in [(0, 0), (1, 2), (3, 6), (4, 7)] {
        assert_eq!(utf16_to_byte(text, utf16), byte);
        assert_eq!(byte_to_utf16(text, byte), utf16);
    }
}

#[test]
fn colouring_one_word_splits_the_span() {
    let mut fonts = Fonts::spike_set();
    let mut tb = TextBox::from_spans(&[Span { text: "one two three".into(), style: Style::default() }], &mut fonts);
    tb.select(4..7);
    tb.apply(StyleChange::Color("red".into()), &mut fonts);
    assert_eq!(write_inline(&tb.spans()), r#"one <Text color="red">two</Text> three"#);
}

#[test]
fn typing_inside_a_styled_word_keeps_its_style() {
    let mut fonts = Fonts::spike_set();
    let mut tb = TextBox::from_spans(&parse_inline("a **bc** d").unwrap(), &mut fonts);
    tb.select(3..3);
    tb.insert("X", &mut fonts);
    assert_eq!(write_inline(&tb.spans()), "a **bXc** d");
}

#[test]
fn typing_emoji_keeps_it_in_the_markdown() {
    let mut fonts = Fonts::spike_set();
    let mut tb = TextBox::from_spans(&parse_inline("ab").unwrap(), &mut fonts);
    tb.select(1..1);
    tb.insert("🎉", &mut fonts);
    assert_eq!(write_inline(&tb.spans()), "a🎉b");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test --release -p spike-typing --test editing`
Expected: FAIL, `TextBox` not found.

- [ ] **Step 3: Implement `editing.rs`**

Use cosmic-text's `Editor` for the caret, selection and insertion, as the design says. Per-word styles are `AttrsList::add_span` on the line, and `spans` reads them back with `get_span`. If `Editor` gets in the way, record how in the results file.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test --release -p spike-typing --test editing`
Expected: all pass.

- [ ] **Step 5: Implement `spike-typing <file.md>`**

It opens the file's first paragraph in a text box on a white slide.
- `impl EntityInputHandler` for the view, using the UTF-16 helpers. Call `window.handle_input` with `ElementInputHandler::new` in paint. Marked (composing) text shows underlined.
- Keys: arrows, Shift+arrows, Backspace, Ctrl+B, Ctrl+1 for red, Ctrl+2 for size 64, and Ctrl+S.
- Ctrl+S writes the file with `replace_paragraph`.
- Draw with `draw_buffer`, and draw the caret and selection on top with GPUI.

- [ ] **Step 6: Add the owner's checklist to `docs/checks/native-rust-spikes.md`, and hand it over**

Run: `scripts/cargo-safe.sh run --release -p spike-typing --bin spike-typing -- spikes/typing/fixture.md` on the real desktop, then:

1. Type `blåbær`.
2. Press the dead acute key and then `e`. It should give `é`.
3. Paste `🎉`. It should appear in colour, and the caret should land after it.
4. Select a word with Shift+arrows, then press Ctrl+B, Ctrl+1 and Ctrl+2. Only that word should change.
5. Press Ctrl+S, then run `git diff --no-index spikes/typing/fixture.original.md spikes/typing/fixture.md`. Only the first paragraph should differ.

Create `spikes/typing/fixture.md` and `fixture.original.md` as identical copies, with a settings block, a heading, two paragraphs and a list, in deliberately odd spacing.

- [ ] **Step 7: Record the verdict and commit**

Write spike 3's verdict as yes or no.

```bash
git add spikes/typing docs/checks/native-rust-spikes.md Cargo.lock
git commit -m "Spike: type and style words on the slide"
```

## When every task is done

`docs/checks/native-rust-spikes.md` holds a yes or no for each spike. If all three are yes, write plan A next. If any is no, stop and bring the numbers to the owner before planning further.
