# Plan A: Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open a folder and see the talk drawn natively. Edit its files in any editor and the slide updates on save.

**Architecture:** Three new GPUI-free crates: `slides-format` turns the Markdown into a model with exact byte positions, `toolkit` defines the element tree and `#[component]`, and `slides-engine` lays a slide out and draws it to cached layers with tiny-skia. `crates/app` shows each layer as its own GPUI image. Data flows one way, from the file to the screen.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 (`gpui-fast`), markdown 1.0.0, tiny-skia 0.11.4, cosmic-text 0.19.0, Taffy 0.15.0, resvg 0.46.0, image 0.25, syntect 5, notify 8, proptest.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`. Spike results are in `docs/checks/native-rust-spikes.md`, and the spike code is at `spike/native-rust`.

## Global Constraints

- Work on the branch `native-rust`, cut from `plan-1`.
- On the desktop, run cargo through `scripts/cargo-safe.sh`.
- Pin `gpui-kit = { version = "=0.9.0", features = ["gpui-fast"] }`, `tiny-skia = "=0.11.4"`, `cosmic-text = "=0.19.0"`, `markdown = "=1.0.0"`, `taffy = "=0.15.0"`, `resvg = "=0.46.0"`.
- All our code is Rust. No web view, JavaScript engine or Node.
- Slide units are 1920×1080 for a 16:9 talk.
- Layout uses only bundled fonts and fonts carried in the talk. Characters they don't cover fall back to fonts on the computer, with a notice.
- The parser, engine and toolkit don't depend on GPUI.
- There is no second copy of the talk that can drift from the file.
- Each cached layer is its own image in GPUI's sprite atlas. Images hold straight-alpha BGRA. A layer that changes must release its old image with `drop_image`.
- Tag names are `<Box>`, `<Shape>`, `<Icon>`, `<Picture>`, `<Text>`, `<Notes>` and `<Region>`. Attributes `x`, `y`, `w`, `h`, `rotate`, `click` and `enter` place any component.
- No GitHub Actions, for Windows builds or anything else (owner's decision, 2026-11-10).
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. It holds the owner's own uncommitted edit.
- Don't name real decks, clients or organisations in commits. Use fixtures.

## Review Focus

- **A slide with a broken tag:** only that slide shows a labelled error, and the others still draw. Owned by Task 3's `bad_slide_doesnt_break_the_others` and Task 9's `slide_error_shows_a_labelled_placeholder`.
- **An editor that truncates a file before writing it:** an empty read must keep the last good talk on screen. Owned by Task 12's `empty_read_keeps_the_last_good_talk`.
- **A picture that is missing or corrupt:** a labelled placeholder appears, and nothing crashes. Owned by Task 9's `missing_picture_shows_a_placeholder` and `corrupt_picture_shows_a_placeholder`.
- **A talk that points outside its folder:** `../` and absolute paths must not read the computer's files. Owned by Task 8's `reading_outside_the_talk_is_refused`.
- **Files from other editors and systems:** CRLF line endings, a BOM, and `---` inside a code block must all work. Owned by Task 2's `crlf_offsets_are_exact`, `bom_is_skipped` and `rule_inside_code_fence_is_not_a_slide_break`.

---

### Task 1: Start the branch and clear out the web-view editor

**Files:**
- Delete: `crates/protocol/`, `crates/sidecar/`, `crates/app/src/{canvas,document,document_view,inspector}.rs`, `crates/app/tests/{document_state,ui_dialogs,ui_inspector}.rs`, `scripts/{dev-home,check-dev-home,check-saved}.sh`, `dev.env`, `fixtures/hello/`
- Modify: `Cargo.toml`, `crates/app/Cargo.toml`, `crates/app/src/{lib,main,workspace_view}.rs`, `crates/app/tests/ui_layout.rs`, `README.md`, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md`

**Interfaces:**
- Produces: `stagepack_studio::workspace_view::{WorkspaceView, TITLE_ID, CANVAS_ID}`, `WorkspaceView::for_test(window, cx)`, and `WorkspaceView::window_options(cx)`.

- [ ] **Step 1: Create the branch and bring the documents across**

Name each file. A folder-wide checkout would overwrite the owner's uncommitted roadmap edit.

```bash
git switch -c native-rust plan-1
git checkout spike/native-rust -- docs/superpowers/specs/2026-11-09-native-rust-slides-design.md docs/superpowers/plans/2026-11-09-native-rust-roadmap.md docs/superpowers/plans/2026-11-09-native-rust-spikes.md docs/superpowers/plans/2026-11-09-native-rust-spikes-decisions.md docs/checks/native-rust-spikes.md
```

Add `File: \`2026-11-09-native-rust-plan-a-engine.md\`.` under the roadmap's "A. Engine" heading.

```bash
git add docs/superpowers/plans/2026-11-09-native-rust-roadmap.md docs/superpowers/plans/2026-11-09-native-rust-plan-a-engine.md
git commit -m "Native Rust slides: design, roadmap, spike results and plan A"
```

- [ ] **Step 2: Rewrite `crates/app/tests/ui_layout.rs` to the new layout**

Keep `open_workspace`. Replace the tests with:

```rust
#[gpui_kit::test]
fn layout_has_a_canvas_under_the_title(cx: &mut TestAppContext) {
    let handle = open_workspace(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let title = window.find(TITLE_ID);
        let canvas = window.find(CANVAS_ID);
        assert!(title.visible() && canvas.visible());
        assert!(title.bounds().bottom() <= canvas.bounds().top());
    }).unwrap();
}
```

- [ ] **Step 3: Remove the old code**

Delete the listed files. Drop `gpui-webview`, `wry`, `gtk`, `webkit2gtk`, `studio-protocol` and `studio-sidecar` from `crates/app/Cargo.toml`, and the two crates from the workspace. In `main.rs`, use `gpui_kit::application()` with no forced X11 mode and remove `STUDIO_DEV_URL`. `WorkspaceView` keeps the title bar and an empty canvas area. Rewrite `README.md` to describe this product and `scripts/cargo-safe.sh`. The inspector returns in plan D, and its form approach stays in git history on `plan-1`.

- [ ] **Step 4: Verify the build and the removal**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: all pass.

Run: `grep -c -E 'name = "(wry|lb-wry|webkit2gtk|gpui-webview)"' Cargo.lock`
Expected: `0`.

- [ ] **Step 5: Commit**

```bash
git add -A crates scripts fixtures README.md Cargo.toml Cargo.lock dev.env
git commit -m "Remove the web-view editor, the sidecar and the bridge protocol"
```

### Task 2: Split a file into slides and read settings blocks

**Files:**
- Modify: `Cargo.toml` (add `crates/format`)
- Create: `crates/format/{Cargo.toml,src/lib.rs,src/split.rs,src/settings.rs,tests/split.rs}` (package `slides-format`)

**Interfaces:**
- Produces, in `slides_format`:
  - `pub struct SettingEntry { pub key: String, pub value: String, pub line: Range<usize>, pub value_range: Range<usize> }`
  - `pub struct Settings { pub range: Range<usize>, pub entries: Vec<SettingEntry> }` with `pub fn get(&self, key: &str) -> Option<&str>`
  - `pub struct RawSlide { pub range: Range<usize>, pub settings: Option<Settings>, pub body: Range<usize> }`
  - `pub fn split_slides(source: &str) -> Vec<RawSlide>`

- [ ] **Step 1: Write the failing tests in `crates/format/tests/split.rs`**

```rust
use slides_format::*;

const TWO: &str = "---\ntitle: Review\nformat: 1\n---\n\n# One\n\n---\nlayout: section\n---\n\n# Two\n";

#[test]
fn settings_blocks_belong_to_their_slides() {
    let slides = split_slides(TWO);
    assert_eq!(slides.len(), 2);
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("Review"));
    assert_eq!(slides[1].settings.as_ref().unwrap().get("layout"), Some("section"));
    assert_eq!(&TWO[slides[1].body.clone()], "\n# Two\n");
}

#[test]
fn plain_rule_starts_a_slide_with_no_settings() {
    let src = "# A\n\n---\n\n# B\n";
    let slides = split_slides(src);
    assert_eq!(slides.len(), 2);
    assert!(slides[1].settings.is_none());
    assert_eq!(&src[slides[1].body.clone()], "\n# B\n");
}

#[test]
fn rule_inside_code_fence_is_not_a_slide_break() {
    assert_eq!(split_slides("```\n---\n```\n").len(), 1);
    assert_eq!(split_slides("~~~\n---\n~~~\n").len(), 1);
}

#[test]
fn crlf_offsets_are_exact() {
    let src = "---\r\ntitle: A\r\n---\r\n# One\r\n---\r\n# Two\r\n";
    let slides = split_slides(src);
    assert_eq!(slides.len(), 2);
    assert_eq!(&src[slides[0].body.clone()], "# One\r\n");
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("A"));
    assert_eq!(&src[slides[1].body.clone()], "# Two\r\n");
}

#[test]
fn bom_is_skipped() {
    let slides = split_slides("\u{feff}---\ntitle: A\n---\n# One\n");
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("A"));
}

#[test]
fn value_range_points_at_the_value() {
    let src = "---\nlayout:   title  \n---\n";
    let settings = split_slides(src).remove(0).settings.unwrap();
    assert_eq!(&src[settings.entries[0].value_range.clone()], "title");
}

#[test]
fn every_byte_belongs_to_exactly_one_slide() {
    let slides = split_slides(TWO);
    assert_eq!(slides[0].range.start, 0);
    assert_eq!(slides[0].range.end, slides[1].range.start);
    assert_eq!(slides[1].range.end, TWO.len());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: FAIL, `split_slides` not found.

- [ ] **Step 3: Implement `split.rs` and `settings.rs`**

Walk the file line by line, tracking fences (a line starting with ` ``` ` or `~~~`). A line that is exactly `---` after trimming trailing spaces and `\r` is a break. It opens a settings block only when the very next line matches `^[A-Za-z_][A-Za-z0-9_.-]*:` and a closing `---` follows, with every line between blank or matching that pattern. A slide's `range` starts at its break line, and the first slide's starts at 0. A leading BOM belongs to the first slide's range but not to its settings.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/format
git commit -m "Format: split a talk into slides and read settings blocks"
```

### Task 3: Parse a slide into blocks with exact positions

**Files:**
- Create: `crates/format/src/{model,parse,attrs}.rs`, `crates/format/tests/parse.rs`
- Modify: `crates/format/src/lib.rs`, `crates/format/Cargo.toml`

**Interfaces:**
- Consumes: `split_slides`, `Settings`.
- Produces, in `slides_format`:
  - `#[derive(Clone, Debug, PartialEq, Default)] pub struct Style { pub bold: bool, pub italic: bool, pub underline: bool, pub code: bool, pub color: Option<String>, pub size: Option<u32>, pub link: Option<String> }`
  - `#[derive(Clone, Debug, PartialEq)] pub struct Span { pub text: String, pub style: Style }`
  - `pub struct Attr { pub name: String, pub value: String, pub name_range: Range<usize>, pub value_range: Range<usize> }`
  - `pub struct Tag { pub name: String, pub attrs: Vec<Attr>, pub children: Vec<Block>, pub range: Range<usize>, pub open_end: usize }` with `pub fn attr(&self, name: &str) -> Option<&str>`
  - `pub enum Block { Heading { level: u8, spans: Vec<Span>, range: Range<usize> }, Paragraph { spans: Vec<Span>, range: Range<usize> }, List { ordered: bool, items: Vec<Vec<Block>>, range: Range<usize> }, Table { rows: Vec<Vec<Vec<Span>>>, range: Range<usize> }, Code { language: Option<String>, code: String, range: Range<usize> }, Picture { src: String, alt: String, range: Range<usize> }, Tag(Tag), Region { name: String, range: Range<usize> } }` with `pub fn range(&self) -> &Range<usize>`
  - `pub struct Notes { pub text: String, pub range: Range<usize> }`
  - `pub struct SlideError { pub message: String }`
  - `pub struct Slide { pub range: Range<usize>, pub settings: Option<Settings>, pub blocks: Vec<Block>, pub notes: Option<Notes>, pub error: Option<SlideError> }`
  - `pub struct Talk { pub source: String, pub slides: Vec<Slide> }`
  - `pub fn parse_talk(source: &str) -> Talk`

- [ ] **Step 1: Write the failing tests in `crates/format/tests/parse.rs`**

```rust
use slides_format::*;

const SLIDE: &str = r#"# Results

Revenue is **up** <Text color="red" size="64">12%</Text> this year.

<Box x="120" y="240" w="800" rotate="5">

Inside the box

</Box>

<Region name="right" />

- one
- two

<Notes>
Mention the office.
</Notes>
"#;

fn slide() -> Talk { parse_talk(SLIDE) }

#[test]
fn blocks_come_out_in_order() {
    let talk = slide();
    let blocks = &talk.slides[0].blocks;
    assert!(matches!(blocks[0], Block::Heading { level: 1, .. }));
    assert!(matches!(blocks[1], Block::Paragraph { .. }));
    assert!(matches!(blocks[2], Block::Tag(_)));
    assert!(matches!(&blocks[3], Block::Region { name, .. } if name == "right"));
    assert!(matches!(blocks[4], Block::List { ordered: false, .. }));
    assert_eq!(blocks.len(), 5);
}

#[test]
fn spans_carry_their_styles() {
    let talk = slide();
    let Block::Paragraph { spans, .. } = &talk.slides[0].blocks[1] else { panic!() };
    assert_eq!(spans[1].text, "up");
    assert!(spans[1].style.bold);
    assert_eq!(spans[3].text, "12%");
    assert_eq!(spans[3].style.color.as_deref(), Some("red"));
    assert_eq!(spans[3].style.size, Some(64));
}

#[test]
fn every_block_range_points_at_its_source() {
    let talk = slide();
    assert_eq!(&SLIDE[talk.slides[0].blocks[0].range().clone()], "# Results");
    assert!(SLIDE[talk.slides[0].blocks[2].range().clone()].starts_with("<Box x=\"120\""));
    assert!(SLIDE[talk.slides[0].blocks[2].range().clone()].ends_with("</Box>"));
}

#[test]
fn attributes_know_their_exact_bytes() {
    let talk = slide();
    let Block::Tag(tag) = &talk.slides[0].blocks[2] else { panic!() };
    let rotate = tag.attrs.iter().find(|a| a.name == "rotate").unwrap();
    assert_eq!(&SLIDE[rotate.value_range.clone()], "5");
    assert_eq!(&SLIDE[rotate.name_range.clone()], "rotate");
    assert_eq!(tag.attr("x"), Some("120"));
    assert!(matches!(&tag.children[0], Block::Paragraph { .. }));
}

#[test]
fn notes_come_out_of_the_blocks() {
    assert_eq!(slide().slides[0].notes.as_ref().unwrap().text, "Mention the office.");
}

#[test]
fn positions_are_file_offsets_on_later_slides() {
    let src = "# One\n\n---\n\n# Two\n";
    let talk = parse_talk(src);
    assert_eq!(&src[talk.slides[1].blocks[0].range().clone()], "# Two");
}

#[test]
fn braces_are_plain_text() {
    let talk = parse_talk("Use {curly} braces.\n");
    let Block::Paragraph { spans, .. } = &talk.slides[0].blocks[0] else { panic!() };
    assert_eq!(spans[0].text, "Use {curly} braces.");
}

#[test]
fn a_picture_alone_on_a_line_is_a_picture_block() {
    let talk = parse_talk("![A cat](assets/cat.png)\n");
    assert!(matches!(&talk.slides[0].blocks[0], Block::Picture { src, alt, .. } if src == "assets/cat.png" && alt == "A cat"));
}

#[test]
fn tables_keep_rows_and_cells() {
    let talk = parse_talk("| a | b |\n|---|---|\n| 1 | **2** |\n");
    let Block::Table { rows, .. } = &talk.slides[0].blocks[0] else { panic!() };
    assert_eq!(rows.len(), 2);
    assert!(rows[1][1][0].style.bold);
}

#[test]
fn bad_slide_doesnt_break_the_others() {
    let talk = parse_talk("# One\n\n---\n\n<Box>\n\nnever closed\n\n---\n\n# Three\n");
    assert_eq!(talk.slides.len(), 3);
    assert!(talk.slides[1].error.as_ref().unwrap().message.contains("Box"));
    assert!(talk.slides[0].error.is_none());
    assert!(matches!(talk.slides[2].blocks[0], Block::Heading { .. }));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test parse`
Expected: FAIL, `parse_talk` not found.

- [ ] **Step 3: Implement `model.rs`, `parse.rs` and `attrs.rs`**

Parse each slide's body separately with `markdown::to_mdast`, using `Constructs { gfm_table: true, gfm_strikethrough: false, mdx_jsx_flow: true, mdx_jsx_text: true, html_flow: false, html_text: false, autolink: false, mdx_esm: false, mdx_expression_flow: false, mdx_expression_text: false, ..Constructs::default() }`. Add the body's start to every position. A parse failure becomes `Slide::error` with empty blocks.

`attrs.rs` scans the opening tag from a tag's start offset, because the markdown crate gives attributes no positions. It reads `name="value"`, `name='value'` and a bare `name`, whose value is `true`, and sets `open_end` to the offset just past `>` or `/>`.

Inline: `Strong`, `Emphasis`, `InlineCode` (`code`), `Link` (`link`) and `<Text color size bold italic underline>` set `Style`. A `Break` adds `\n` to the text. Neighbouring spans of equal style merge. Unknown inline tags keep their text content.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/format
git commit -m "Format: parse slides into blocks with exact byte positions"
```

### Task 4: Source edits that touch only what changed

**Files:**
- Create: `crates/format/src/edit.rs`, `crates/format/tests/edit.rs`
- Modify: `crates/format/src/lib.rs`, `crates/format/Cargo.toml` (dev-dependency `proptest = "1"`)

**Interfaces:**
- Consumes: `Tag`, `Attr`, `Slide`, `Settings`.
- Produces, in `slides_format`:
  - `#[derive(Clone, Debug, PartialEq)] pub struct SourceEdit { pub range: Range<usize>, pub text: String }`, with `pub fn apply(&self, source: &str) -> String` and `pub fn inverse(&self, source: &str) -> SourceEdit`
  - `pub fn set_attr(source: &str, tag: &Tag, name: &str, value: &str) -> SourceEdit`
  - `pub fn set_setting(source: &str, slide: &Slide, key: &str, value: &str) -> SourceEdit`

- [ ] **Step 1: Write the failing tests in `crates/format/tests/edit.rs`**

```rust
use proptest::prelude::*;
use slides_format::*;

fn first_tag(talk: &Talk) -> &Tag {
    match &talk.slides[0].blocks[0] { Block::Tag(tag) => tag, _ => panic!() }
}

#[test]
fn changing_an_attribute_touches_only_its_value() {
    let src = "<Box   x=\"1\"    y=\"2\" />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "y", "99").apply(src), "<Box   x=\"1\"    y=\"99\" />\n");
}

#[test]
fn a_new_attribute_goes_before_the_tag_ends() {
    let src = "<Box x=\"1\">\n\nHi\n\n</Box>\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "rotate", "5").apply(src), "<Box x=\"1\" rotate=\"5\">\n\nHi\n\n</Box>\n");
}

#[test]
fn a_new_attribute_on_a_self_closed_tag() {
    let src = "<Box x=\"1\" />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "y", "2").apply(src), "<Box x=\"1\" y=\"2\" />\n");
}

#[test]
fn changing_a_setting_touches_only_its_value() {
    let src = "---\nlayout:    title\n---\n\n# One\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "layout", "section").apply(src), "---\nlayout:    section\n---\n\n# One\n");
}

#[test]
fn a_new_setting_is_one_new_line_with_the_files_line_ending() {
    let src = "---\r\nlayout: title\r\n---\r\n# One\r\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "transition", "fade").apply(src), "---\r\nlayout: title\r\ntransition: fade\r\n---\r\n# One\r\n");
}

#[test]
fn a_slide_with_no_settings_gets_a_block() {
    let src = "# One\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "layout", "blank").apply(src), "---\nlayout: blank\n---\n# One\n");
}

proptest! {
    #[test]
    fn inverse_restores_the_source(prefix in ".{0,20}", old in ".{0,20}", suffix in ".{0,20}", new in ".{0,20}") {
        let source = format!("{prefix}{old}{suffix}");
        let edit = SourceEdit { range: prefix.len()..prefix.len() + old.len(), text: new };
        let changed = edit.apply(&source);
        prop_assert_eq!(edit.inverse(&source).apply(&changed), source);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test edit`
Expected: FAIL, `set_attr` not found.

- [ ] **Step 3: Implement `edit.rs`**

`inverse` covers the range the edit's new text will occupy and restores the old text. When a new setting is added, detect `\r\n` from the settings block's own lines. For a later slide that starts with a bare `---`, insert `key: value` and a closing `---` straight after that rule. A value that contains `"` is written with `&quot;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/format
git commit -m "Format: source edits that change only the bytes they mean to"
```

### Task 5: The toolkit and `#[component]`

**Files:**
- Modify: `Cargo.toml` (add `crates/toolkit`, `crates/toolkit-macros`)
- Create: `crates/toolkit-macros/{Cargo.toml,src/lib.rs}`, `crates/toolkit/{Cargo.toml,src/lib.rs,src/element.rs,src/color.rs,src/props.rs,tests/component.rs}`

**Interfaces:**
- Produces, in `toolkit`:
  - `#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)] pub struct Color { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }` with `pub fn parse(text: &str) -> Option<Color>` for `#rgb`, `#rrggbb` and the names `black`, `white`, `red`, `green`, `blue`, `yellow`, `orange`, `purple`, `gray`
  - `pub struct Theme { pub background: Color, pub text: Color, pub accent: Color, pub font: String, pub mono_font: String }` with `Default` (`#ffffff`, `#1e293b`, `#2563eb`, `Inter`, `JetBrains Mono`)
  - `#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)] pub enum ShapeKind { Rect, Rounded, Ellipse, Line, Arrow, Callout }`
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub enum Element { Text { text: String, size: Option<f32>, bold: bool, color: Option<Color> }, Stack { horizontal: bool, gap: f32, children: Vec<Element> }, Shape { kind: ShapeKind, width: f32, height: f32, fill: Option<Color>, outline: Option<Color>, outline_width: f32, child: Option<Box<Element>> }, Picture { src: String, width: f32, height: f32 }, Icon { name: String, size: f32, color: Option<Color> }, Path { data: String, width: f32, height: f32, fill: Option<Color> }, Children, Empty }`
  - Builders `text`, `shape`, `picture`, `icon`, `path`, and the macros `row!` and `column!`; `Element::{size, bold, fill, outline, gap}`
  - `pub struct Ctx { pub click_step: u32, pub time: f32, pub slide_width: f32, pub slide_height: f32, pub theme: Theme }`
  - `pub struct Children;` with `impl From<Children> for Element`
  - `pub struct Props(pub BTreeMap<String, String>)`, and `pub trait Prop: Sized { const TYPE_NAME: &'static str; fn from_attr(value: Option<&str>) -> Self; }` for `String`, `u32`, `f32`, `bool`, `Color`, `Option<T>`
  - `pub struct PropInfo { pub name: &'static str, pub type_name: &'static str }`
  - `#[derive(Clone, Copy)] pub struct NativeComponent { pub name: &'static str, pub schema: &'static [PropInfo], pub render: fn(&Props, &Ctx) -> Element }`
  - `#[derive(Default)] pub struct Registry` with `pub fn add(&mut self, component: NativeComponent)` and `pub fn get(&self, name: &str) -> Option<NativeComponent>`
  - `prelude`, re-exporting all of the above and `component`
  - `#[component] fn Name(...) -> Element` also generates `pub fn Name_component() -> NativeComponent`

- [ ] **Step 1: Write the failing tests in `crates/toolkit/tests/component.rs`**

```rust
use toolkit::prelude::*;
use std::collections::BTreeMap;

#[component]
fn Score(team: String, points: u32, colour: Color, ctx: &Ctx, children: Children) -> Element {
    column![text(format!("{team} {points} {}", ctx.click_step)).fill(colour), children.into()]
}

fn ctx() -> Ctx {
    Ctx { click_step: 2, time: 0.0, slide_width: 1920.0, slide_height: 1080.0, theme: Theme::default() }
}

fn props(pairs: &[(&str, &str)]) -> Props {
    Props(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>())
}

#[test]
fn schema_lists_only_real_arguments() {
    let names: Vec<_> = Score_component().schema.iter().map(|p| (p.name, p.type_name)).collect();
    assert_eq!(names, [("team", "String"), ("points", "u32"), ("colour", "Color")]);
}

#[test]
fn attributes_become_arguments() {
    let element = (Score_component().render)(&props(&[("team", "Oslo"), ("points", "3"), ("colour", "red")]), &ctx());
    let Element::Stack { children, .. } = element else { panic!() };
    assert_eq!(children[0], text("Oslo 3 2").fill(Color::parse("red").unwrap()));
    assert_eq!(children[1], Element::Children);
}

#[test]
fn a_missing_or_bad_number_is_zero_not_a_crash() {
    let element = (Score_component().render)(&props(&[("points", "many")]), &ctx());
    let Element::Stack { children, .. } = element else { panic!() };
    assert!(matches!(&children[0], Element::Text { text, .. } if text == " 0 2"));
}

#[test]
fn registry_finds_components_by_tag_name() {
    let mut registry = Registry::default();
    registry.add(Score_component());
    assert!(registry.get("Score").is_some());
    assert!(registry.get("Nope").is_none());
}

#[component]
fn Outlined(outline_width: u32) -> Element {
    text(outline_width.to_string())
}

#[test]
fn underscored_arguments_read_hyphenated_attributes() {
    assert_eq!((Outlined_component().render)(&props(&[("outline-width", "4")]), &ctx()), text("4"));
    assert_eq!(Outlined_component().schema[0].name, "outline-width");
}

#[test]
fn colours_parse() {
    assert_eq!(Color::parse("red"), Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    assert_eq!(Color::parse("#ff0000"), Color::parse("red"));
    assert_eq!(Color::parse("#0f0"), Some(Color { r: 0, g: 255, b: 0, a: 255 }));
    assert_eq!(Color::parse("nonsense"), None);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p toolkit`
Expected: FAIL, `prelude` not found.

- [ ] **Step 3: Implement the two crates**

Start from `spikes/compile/macros/src/lib.rs` on `spike/native-rust`. The macro passes an argument typed `&Ctx` the context and an argument typed `Children` the `Children` value. Neither appears in the schema. An argument's attribute name is its Rust name with `_` written as `-`. Other arguments are read with `<T as Prop>::from_attr(props.0.get(name).map(String::as_str))`. Add `#[allow(non_snake_case)]`. WebAssembly exports are plan C.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p toolkit`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/toolkit crates/toolkit-macros
git commit -m "Toolkit: elements, props and the component macro"
```

### Task 6: Bundled fonts and shaped text

**Files:**
- Modify: `Cargo.toml` (add `crates/engine`)
- Create: `crates/engine/{Cargo.toml,src/lib.rs,src/text.rs,tests/text.rs,assets/fonts/*}` (package `slides-engine`)

**Interfaces:**
- Consumes: `toolkit::Color`.
- Produces, in `slides_engine`:
  - `pub struct Notice(pub String)`
  - `pub struct Fonts { pub system: cosmic_text::FontSystem, pub cache: cosmic_text::SwashCache, .. }`, with `pub fn bundled_only() -> Fonts`, `pub fn with_system_fallback() -> Fonts` and `pub fn load_talk_font(&mut self, data: Vec<u8>)`
  - `#[derive(Clone)] pub struct TextRun { pub text: String, pub size: f32, pub bold: bool, pub italic: bool, pub underline: bool, pub color: toolkit::Color, pub family: String }`
  - `pub enum Align { Left, Center, Right }`
  - `pub struct ShapedText { pub buffer: std::sync::Arc<cosmic_text::Buffer>, pub width: f32, pub height: f32, pub underlines: Vec<(f32, f32, f32, toolkit::Color)>, pub notices: Vec<Notice> }`
  - `pub fn shape_text(fonts: &mut Fonts, runs: &[TextRun], width: Option<f32>, align: Align) -> ShapedText`
  - `pub fn draw_text(target: &mut tiny_skia::PixmapMut, text: &ShapedText, fonts: &mut Fonts, transform: tiny_skia::Transform)`

- [ ] **Step 1: Bring the fonts in**

```bash
gh release download v4.1 -R rsms/inter -p 'Inter-4.1.zip' -D "$TMP"
gh release download v2.304 -R JetBrains/JetBrainsMono -p 'JetBrainsMono-2.304.zip' -D "$TMP"
```

Extract `extras/ttf/Inter-{Regular,Bold,Italic,BoldItalic}.ttf` and `fonts/ttf/JetBrainsMono-{Regular,Bold,Italic}.ttf` to `crates/engine/assets/fonts/`, with each project's licence text. Copy `/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf` there too. Expected: 8 font files, about 14 MB in all, and `assets/fonts/LICENSES.md` naming each font's OFL licence.

- [ ] **Step 2: Write the failing tests in `crates/engine/tests/text.rs`**

```rust
use slides_engine::*;
use toolkit::Color;

fn run(text: &str) -> TextRun {
    TextRun { text: text.into(), size: 48.0, bold: false, italic: false, underline: false, color: Color { r: 0, g: 0, b: 0, a: 255 }, family: "Inter".into() }
}

#[test]
fn latin_and_emoji_need_no_notice() {
    let mut fonts = Fonts::bundled_only();
    assert!(shape_text(&mut fonts, &[run("blåbær 🎉 Ж")], None, Align::Left).notices.is_empty());
}

#[test]
fn characters_nothing_covers_raise_a_notice() {
    let mut fonts = Fonts::bundled_only();
    assert!(shape_text(&mut fonts, &[run("日本語")], None, Align::Left).notices[0].0.contains("日"));
}

#[test]
fn wrapping_respects_the_width() {
    let mut fonts = Fonts::bundled_only();
    let long = run("word ".repeat(40).trim());
    let wrapped = shape_text(&mut fonts, &[long.clone()], Some(400.0), Align::Left);
    let single = shape_text(&mut fonts, &[long], None, Align::Left);
    assert!(wrapped.height > single.height * 2.0);
    assert!(wrapped.width <= 400.0);
}

#[test]
fn drawing_twice_gives_the_same_pixels() {
    let mut fonts = Fonts::bundled_only();
    let text = shape_text(&mut fonts, &[run("Same 🎉")], None, Align::Left);
    let mut a = tiny_skia::Pixmap::new(400, 100).unwrap();
    let mut b = tiny_skia::Pixmap::new(400, 100).unwrap();
    draw_text(&mut a.as_mut(), &text, &mut fonts, tiny_skia::Transform::identity());
    draw_text(&mut b.as_mut(), &text, &mut fonts, tiny_skia::Transform::identity());
    assert_eq!(a.data(), b.data());
    assert!(a.pixels().iter().any(|p| p.alpha() > 0));
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test text`
Expected: FAIL, `Fonts` not found.

- [ ] **Step 4: Implement `text.rs`**

Port `draw_buffer` from `spikes/drawing/src/lib.rs` on `spike/native-rust`. Shape in slide units, so text wraps the same at any zoom, and let the transform scale the outlines. Embed the fonts with `include_bytes!`. A glyph with `glyph_id == 0` raises `Notice("No font has the character …")`. `with_system_fallback` also calls `load_system_fonts`, and a glyph from a font outside the bundled set raises `Notice("Uses a font on this computer for …")`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-engine --test text`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/engine
git commit -m "Engine: bundled fonts and shaped text"
```

### Task 7: Layers and the renderer

**Files:**
- Create: `crates/engine/src/{display,render}.rs`, `crates/engine/tests/{common/mod.rs,render.rs}`, `crates/engine/tests/golden/`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `ShapedText`, `Fonts`, `draw_text`, `toolkit::Color`.
- Produces, in `slides_engine`:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }`
  - `pub enum DisplayItem { Fill { path: tiny_skia::Path, color: toolkit::Color }, Stroke { path: tiny_skia::Path, color: toolkit::Color, width: f32 }, Text { text: ShapedText, x: f32, y: f32 }, Picture { pixmap: std::sync::Arc<tiny_skia::Pixmap>, rect: Rect } }`
  - `pub struct LayerSpec { pub source: Option<Range<usize>>, pub rect: Rect, pub rotate: f32, pub items: Vec<DisplayItem> }`, where item coordinates are relative to `rect`'s top-left corner
  - `pub struct Layer { pub pixmap: tiny_skia::Pixmap, pub x: i32, pub y: i32 }`. It has no source range, because a cached layer can outlive the positions it was drawn from. Use the fresh `LayerSpec` for those.
  - `pub fn render_layer(spec: &LayerSpec, scale: f32, fonts: &mut Fonts) -> Layer`
  - `pub fn compose(target: &mut tiny_skia::Pixmap, layers: &[Layer])`
  - `pub fn to_bgra_frame(pixmap: &tiny_skia::Pixmap) -> image::Frame`, straight alpha
- Produces for tests, in `tests/common/mod.rs`: `pub fn assert_golden(name: &str, pixmap: &tiny_skia::Pixmap)`. It compares bytes with `tests/golden/<name>.png`, rewrites the file when `UPDATE_GOLDEN=1`, and on a mismatch writes `target/golden-actual/<name>.png` and panics with that path.

- [ ] **Step 1: Write the failing tests in `crates/engine/tests/render.rs`**

```rust
mod common;
use slides_engine::*;
use tiny_skia::PathBuilder;
use toolkit::Color;

const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };

fn square_layer(rotate: f32) -> LayerSpec {
    let path = PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap());
    LayerSpec { source: None, rect: Rect { x: 200.0, y: 100.0, w: 100.0, h: 100.0 }, rotate, items: vec![DisplayItem::Fill { path, color: RED }] }
}

#[test]
fn an_unrotated_layer_sits_at_its_rect() {
    let layer = render_layer(&square_layer(0.0), 1.0, &mut Fonts::bundled_only());
    assert_eq!((layer.x, layer.y), (200, 100));
}

#[test]
fn scale_multiplies_position_and_size() {
    let layer = render_layer(&square_layer(0.0), 2.0, &mut Fonts::bundled_only());
    assert_eq!((layer.x, layer.y), (400, 200));
    assert!(layer.pixmap.width() >= 200);
}

#[test]
fn rotation_turns_around_the_centre() {
    let layer = render_layer(&square_layer(45.0), 1.0, &mut Fonts::bundled_only());
    assert!(layer.pixmap.width() >= 141 && layer.pixmap.width() <= 144);
    assert!(layer.x < 200 && layer.x > 170);
}

#[test]
fn frames_hold_straight_alpha_in_bgra_order() {
    let mut pixmap = tiny_skia::Pixmap::new(1, 1).unwrap();
    pixmap.pixels_mut()[0] = tiny_skia::ColorU8::from_rgba(255, 0, 0, 128).premultiply();
    assert_eq!(to_bgra_frame(&pixmap).buffer().as_raw(), &vec![0, 0, 255, 128]);
}

#[test]
fn golden_primitives() {
    let mut fonts = Fonts::bundled_only();
    let layers = vec![render_layer(&square_layer(0.0), 1.0, &mut fonts), render_layer(&square_layer(30.0), 1.0, &mut fonts)];
    let mut target = tiny_skia::Pixmap::new(480, 270).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    compose(&mut target, &layers);
    common::assert_golden("primitives", &target);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test render`
Expected: FAIL, `LayerSpec` not found.

- [ ] **Step 3: Implement `display.rs` and `render.rs`**

Port `element`, `compose` and `to_bgra_frame` from `spikes/drawing/src/lib.rs`. Rotation is baked into the layer, so moving one is a plain blit. Draw items in order.

- [ ] **Step 4: Create the golden, look at it, and verify**

Run: `UPDATE_GOLDEN=1 scripts/cargo-safe.sh test -p slides-engine --test render`, then open `crates/engine/tests/golden/primitives.png`.
Expected: a red square and a red square turned 30°.

Run: `scripts/cargo-safe.sh test -p slides-engine --test render`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: layers, the renderer and golden images"
```

### Task 8: Layouts, regions and flowing Markdown

**Files:**
- Create: `crates/engine/src/{files,layouts,flow,highlight,slide}.rs`, `crates/engine/tests/{flow,files}.rs`
- Modify: `crates/engine/src/lib.rs`, `crates/engine/Cargo.toml`

**Interfaces:**
- Consumes: `slides_format::{Talk, Slide, Block, Span}`, `shape_text`, `LayerSpec`, `toolkit::{Registry, Theme}`.
- Produces, in `slides_engine`:
  - `pub trait TalkFiles { fn read(&self, path: &str) -> Option<Vec<u8>>; }`
  - `pub struct FolderFiles { .. }` with `pub fn new(root: PathBuf) -> FolderFiles`
  - `pub struct RegionDef { pub name: &'static str, pub rect: Rect, pub size: f32, pub bold: bool, pub align: Align }`
  - `pub struct LayoutDef { pub name: &'static str, pub regions: &'static [RegionDef], pub main: &'static str }`
  - `pub fn layout_by_name(name: &str) -> Option<&'static LayoutDef>`
  - `pub fn talk_theme(talk: &Talk) -> Theme`, reading `color.background`, `color.text`, `color.accent`, `font` and `font.mono` from the first slide's settings
  - `pub fn highlight(code: &str, language: Option<&str>) -> Vec<Vec<(toolkit::Color, String)>>`
  - `pub struct LayoutEnv<'a> { pub fonts: &'a mut Fonts, pub registry: &'a Registry, pub files: &'a dyn TalkFiles }`
  - `pub struct LaidOutSlide { pub layers: Vec<LayerSpec>, pub notices: Vec<Notice> }`
  - `pub fn layout_slide(talk: &Talk, index: usize, env: &mut LayoutEnv) -> LaidOutSlide`

Layouts, in slide units. A `title` region has size 64 bold unless stated, and a body region has size 40:

| Layout | Regions | Main |
| --- | --- | --- |
| `title` | `title` (160, 340, 1600, 220) size 96 bold centred; `body` (160, 580, 1600, 180) size 44 centred | `body` |
| `content` | `title` (100, 60, 1720, 140); `body` (100, 220, 1720, 780) | `body` |
| `two-columns` | `title` (100, 60, 1720, 140); `left` (100, 220, 840, 780); `right` (980, 220, 840, 780) | `left` |
| `section` | `title` (160, 420, 1600, 240) size 96 bold centred; `body` (160, 680, 1600, 200) size 44 centred | `body` |
| `blank` | `body` (0, 0, 1920, 1080) | `body` |

A slide with no `layout` uses `content`.

- [ ] **Step 1: Write the failing tests in `crates/engine/tests/flow.rs` and `tests/files.rs`**

```rust
// tests/flow.rs
mod common;
use slides_engine::*;
use slides_format::parse_talk;
use toolkit::Registry;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

fn lay_out(source: &str) -> (LaidOutSlide, Fonts) {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let registry = Registry::default();
    let slide = layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry: &registry, files: &NoFiles });
    (slide, fonts)
}

fn source_of<'a>(source: &'a str, layer: &LayerSpec) -> &'a str { &source[layer.source.clone().unwrap()] }

#[test]
fn first_heading_goes_to_the_title_region() {
    let src = "# Title here\n\nBody text\n";
    let (slide, _) = lay_out(src);
    assert_eq!(slide.layers[0].source, None);          // the background
    assert_eq!(slide.layers[1].rect, Rect { x: 100.0, y: 60.0, w: 1720.0, h: 140.0 });
    assert!(source_of(src, &slide.layers[1]).contains("Title here"));
    assert!(source_of(src, &slide.layers[2]).contains("Body text"));
}

#[test]
fn region_marker_sends_what_follows_to_that_region() {
    let src = "---\nlayout: two-columns\n---\n\n# T\n\nLeft\n\n<Region name=\"right\" />\n\nRight\n";
    let (slide, _) = lay_out(src);
    let right = slide.layers.iter().find(|l| l.rect.x == 980.0).unwrap();
    assert!(source_of(src, right).contains("Right"));
}

#[test]
fn a_moved_region_uses_its_settings_line() {
    let src = "---\nregion.title: x=10 y=20 w=500 h=100 rotate=3\n---\n\n# T\n";
    let (slide, _) = lay_out(src);
    assert_eq!(slide.layers[1].rect, Rect { x: 10.0, y: 20.0, w: 500.0, h: 100.0 });
    assert_eq!(slide.layers[1].rotate, 3.0);
}

#[test]
fn an_unknown_layout_uses_content_and_says_so() {
    let (slide, _) = lay_out("---\nlayout: fancy\n---\n\n# T\n");
    assert_eq!(slide.layers[1].rect.y, 60.0);
    assert!(slide.notices[0].0.contains("fancy"));
}

#[test]
fn background_uses_the_talks_colour() {
    let src = "---\ncolor.background: #123456\n---\n\n# T\n";
    let talk = parse_talk(src);
    assert_eq!(talk_theme(&talk).background, toolkit::Color::parse("#123456").unwrap());
}

#[test]
fn rust_keywords_get_their_own_colour() {
    let lines = highlight("fn main() {}\n", Some("rust"));
    let colours: std::collections::HashSet<_> = lines[0].iter().map(|(c, _)| (c.r, c.g, c.b)).collect();
    assert!(colours.len() >= 2);
}

#[test]
fn an_unknown_language_still_comes_back_as_text() {
    let lines = highlight("hello\n", Some("not-a-language"));
    assert_eq!(lines[0].iter().map(|(_, t)| t.as_str()).collect::<String>().trim_end(), "hello");
}

#[test]
fn golden_markdown_slide() {
    let src = "# A heading\n\nA paragraph with **bold**, *italic* and <Text color=\"red\">red</Text> words.\n\n- First\n- Second\n\n| Name | Score |\n|------|-------|\n| Oslo | 3 |\n\n```rust\nfn main() {}\n```\n";
    let (slide, mut fonts) = lay_out(src);
    let layers: Vec<_> = slide.layers.iter().map(|l| render_layer(l, 1.0, &mut fonts)).collect();
    let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
    compose(&mut target, &layers);
    common::assert_golden("markdown", &target);
}

// tests/files.rs
use slides_engine::*;

#[test]
fn reads_a_file_inside_the_talk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/a.txt"), b"hi").unwrap();
    assert_eq!(FolderFiles::new(dir.path().into()).read("assets/a.txt"), Some(b"hi".to_vec()));
}

#[test]
fn reading_outside_the_talk_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("secret.txt"), b"no").unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir(&talk).unwrap();
    let files = FolderFiles::new(talk.clone());
    assert_eq!(files.read("../secret.txt"), None);
    assert_eq!(files.read(dir.path().join("secret.txt").to_str().unwrap()), None);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), talk.join("link.txt")).unwrap();
        assert_eq!(files.read("link.txt"), None);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test flow --test files`
Expected: FAIL, `layout_slide` not found.

- [ ] **Step 3: Implement the five files**

- `slide.rs` assigns blocks to regions as the design says: the first heading goes to `title` if the layout has one, the rest to `main`, and a `Block::Region` redirects what follows. Layers are the background, then each non-empty region in layout order, then positioned tags in source order. A layer's `source` spans its first to its last block.
- `region.<name>` values are space-separated `key=value` pairs, with `x`, `y`, `w`, `h`, `rotate` and `click`. Missing keys keep the layout's value.
- `flow.rs` lays a region out as a Taffy column with `compute_layout_with_measure`, using one leaf per block and `shape_text` to measure. Headings inside a body are size 64, 48 and 40 for levels 1 to 3 and otherwise the region's size. The gap between blocks is 24. Lists indent 48 and draw `•` or `1.` in the indent. Tables are a Taffy grid with a 1-unit outline per cell and a bold first row. Code blocks use the mono font at size 32 on a `#f1f5f9` rounded panel with padding 24.
- `highlight.rs` uses syntect's default syntaxes and the `InspiredGitHub` theme, held in a `OnceLock`.
- `FolderFiles::read` canonicalises the path and refuses anything not under the canonicalised root.
- Add `syntect = "5"`, `taffy = "=0.15.0"`, and dev-dependency `tempfile = "3"`.

- [ ] **Step 4: Create the golden, look at it, and verify**

Run: `UPDATE_GOLDEN=1 scripts/cargo-safe.sh test -p slides-engine --test flow`, then open `crates/engine/tests/golden/markdown.png`.
Expected: heading in the title position, then the styled paragraph, list, table and highlighted code stacked below it with no overlap.

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/engine
git commit -m "Engine: layouts, regions and flowing Markdown"
```

### Task 9: Built-in components, pictures and placeholders

**Files:**
- Create: `crates/engine/src/{builtins,elements,pictures,placeholder}.rs`, `crates/engine/assets/icons/{carbon.json,LICENSE}`, `crates/engine/tests/builtins.rs`
- Modify: `crates/engine/src/{slide,flow,lib}.rs`, `crates/engine/Cargo.toml`

**Interfaces:**
- Consumes: `layout_slide`, `LayoutEnv`, `toolkit::{Element, Registry, Ctx, Props}`, `TalkFiles`.
- Produces, in `slides_engine`:
  - `pub fn builtin_registry() -> Registry`, holding `Box`, `Shape`, `Icon` and `Picture`, all written with `#[component]`
  - `pub struct PictureCache { .. }` with `Default`
  - `LayoutEnv` gains `pub pictures: &'a mut PictureCache`
  - `pub const PLACEMENT_ATTRS: [&str; 7] = ["x", "y", "w", "h", "rotate", "click", "enter"]`

Component props:
- `Box`: `fill`, `outline`, `outline-width`, `radius`, and `children`.
- `Shape`: `kind` (`rect`, `rounded`, `ellipse`, `line`, `arrow`, `callout`), `fill`, `outline`, `outline-width`, and `children` centred inside.
- `Icon`: `name`, `size` (default 64), `color`.
- `Picture`: `src`.

- [ ] **Step 1: Write the failing tests in `crates/engine/tests/builtins.rs`**

Add `pictures: &mut PictureCache::default()` to the `LayoutEnv` in `tests/flow.rs`, and use `builtin_registry()` there. Helper `lay_out` as in `tests/flow.rs`, with a `MapFiles(HashMap<String, Vec<u8>>)` implementing `TalkFiles`.

```rust
#[test]
fn a_box_is_its_own_layer_at_its_position() {
    let src = "<Box x=\"100\" y=\"200\" w=\"300\" h=\"150\" rotate=\"10\">\n\nHello\n\n</Box>\n";
    let (slide, _) = lay_out(src, &[]);
    let layer = slide.layers.last().unwrap();
    assert_eq!(layer.rect, Rect { x: 100.0, y: 200.0, w: 300.0, h: 150.0 });
    assert_eq!(layer.rotate, 10.0);
    assert_eq!(layer.source.clone().unwrap(), 0..src.trim_end().len());
}

#[test]
fn an_unknown_tag_is_a_labelled_placeholder_and_a_notice() {
    let (slide, _) = lay_out("<Mystery x=\"10\" y=\"10\" w=\"400\" h=\"200\" />\n", &[]);
    assert!(slide.notices.iter().any(|n| n.0.contains("Mystery")));
    assert!(slide.layers.last().unwrap().items.iter().any(|i| matches!(i, DisplayItem::Text { .. })));
}

#[test]
fn missing_picture_shows_a_placeholder() {
    let (slide, _) = lay_out("<Picture src=\"assets/none.png\" x=\"0\" y=\"0\" w=\"100\" h=\"100\" />\n", &[]);
    assert!(slide.notices.iter().any(|n| n.0.contains("assets/none.png")));
    assert!(!slide.layers.last().unwrap().items.iter().any(|i| matches!(i, DisplayItem::Picture { .. })));
}

#[test]
fn corrupt_picture_shows_a_placeholder() {
    let (slide, _) = lay_out("![x](assets/bad.png)\n", &[("assets/bad.png", b"not a png".to_vec())]);
    assert!(slide.notices.iter().any(|n| n.0.contains("assets/bad.png")));
}

#[test]
fn a_real_picture_is_drawn() {
    let mut png = Vec::new();
    image::RgbaImage::new(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let (slide, _) = lay_out("<Picture src=\"a.png\" x=\"0\" y=\"0\" w=\"100\" h=\"100\" />\n", &[("a.png", png)]);
    assert!(slide.layers.last().unwrap().items.iter().any(|i| matches!(i, DisplayItem::Picture { .. })));
}

#[test]
fn an_unknown_icon_is_a_notice_not_a_crash() {
    let (slide, _) = lay_out("<Icon name=\"not-an-icon\" x=\"0\" y=\"0\" w=\"64\" h=\"64\" />\n", &[]);
    assert!(slide.notices.iter().any(|n| n.0.contains("not-an-icon")));
}

#[test]
fn slide_error_shows_a_labelled_placeholder() {
    let (slide, _) = lay_out("<Box>\n\nnever closed\n", &[]);
    assert!(slide.layers.iter().any(|l| l.items.iter().any(|i| matches!(i, DisplayItem::Text { .. }))));
    assert!(slide.notices.iter().any(|n| n.0.contains("Box")));
}

#[test]
fn golden_shapes_and_icons() {
    let mut src = String::from("<Box x=\"60\" y=\"60\" w=\"500\" h=\"200\" fill=\"#dbeafe\" radius=\"24\">\n\nIn a box\n\n</Box>\n\n");
    for (i, kind) in ["rect", "rounded", "ellipse", "line", "arrow", "callout"].iter().enumerate() {
        src += &format!("<Shape kind=\"{kind}\" x=\"{}\" y=\"400\" w=\"260\" h=\"180\" fill=\"orange\" outline=\"black\" outline-width=\"4\" />\n\n", 60 + i * 300);
    }
    src += "<Icon name=\"arrow-right\" x=\"60\" y=\"700\" w=\"128\" h=\"128\" size=\"128\" />\n";
    let (slide, mut fonts) = lay_out(&src, &[]);
    let layers: Vec<_> = slide.layers.iter().map(|l| render_layer(l, 1.0, &mut fonts)).collect();
    let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
    compose(&mut target, &layers);
    common::assert_golden("shapes", &target);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test builtins`
Expected: FAIL, `builtin_registry` not found.

- [ ] **Step 3: Implement the four files**

- `elements.rs` turns a `toolkit::Element` tree into `DisplayItem`s inside a box. A `Shape` or `Picture` with a width or height of 0 fills the box on that axis, which is how `w` and `h` reach a built-in. It lays `Stack`s out with Taffy, and replaces `Element::Children` with the tag's child blocks through `flow.rs`, so positions never leave the host.
- `slide.rs`: a tag with any of `x`, `y`, `w` or `h` is a positioned layer. A tag without them flows as a block in its region. `PLACEMENT_ATTRS` are not passed as props.
- `pictures.rs` decodes with `image`, or `usvg` and `resvg` for `.svg`. It limits a picture's longest edge to 4096 pixels, and caches by path.
- Icons: copy `@iconify-json/carbon/icons.json` and its licence from `~/.local/share/stagepack/runtimes/53/node_modules/@iconify-json/carbon/` to `assets/icons/`. Wrap an icon's `body` in an `<svg viewBox="0 0 32 32">`, replace `currentColor` with the colour, and render it with `resvg`.
- `placeholder.rs` draws a dashed grey outline and a centred label, for unknown tags, bad pictures and a slide error. A slide error's layer fills the slide and shows `SlideError::message`.
- Add `resvg = "=0.46.0"` and `serde_json`.

- [ ] **Step 4: Create the goldens, look at them, and verify**

Run: `UPDATE_GOLDEN=1 scripts/cargo-safe.sh test -p slides-engine`, then open `crates/engine/tests/golden/shapes.png`.
Expected: each shape is recognisable, the box's text sits inside it, and the icon is an arrow.

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/engine
git commit -m "Engine: built-in components, pictures, icons and placeholders"
```

### Task 10: The deck: only redraw what changed

**Files:**
- Create: `crates/engine/src/deck.rs`, `crates/engine/tests/deck.rs`, `fixtures/showcase/{slides.md,assets/photo.png}`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `parse_talk`, `layout_slide`, `render_layer`, `builtin_registry`, `PictureCache`, `FolderFiles`.
- Produces, in `slides_engine`:
  - `pub struct Deck { .. }`
  - `pub fn open(files: Box<dyn TalkFiles>, fonts: Fonts, source: String) -> Deck`
  - `pub fn update(&mut self, source: String) -> Vec<usize>`, returning the indices of slides that changed
  - `pub fn slide_count(&self) -> usize`
  - `pub fn source(&self) -> &str`
  - `pub fn layers(&mut self, index: usize, scale: f32) -> Vec<std::sync::Arc<Layer>>`
  - `pub fn notices(&mut self, index: usize) -> Vec<String>`
  - `pub fn title(&self) -> Option<String>`

- [ ] **Step 1: Write the failing tests in `crates/engine/tests/deck.rs`**

```rust
mod common;
use slides_engine::*;
use std::sync::Arc;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

const TALK: &str = "# One\n\n---\n\n# Two\n\n---\n\n# Three\n";

fn deck(source: &str) -> Deck { Deck::open(Box::new(NoFiles), Fonts::bundled_only(), source.to_string()) }

#[test]
fn editing_one_slide_reports_only_that_slide() {
    let mut d = deck(TALK);
    assert_eq!(d.update(TALK.replace("Two", "Changed")), vec![1]);
}

#[test]
fn unchanged_slides_keep_their_layers() {
    let mut d = deck(TALK);
    let before = d.layers(0, 0.5);
    d.update(TALK.replace("Two", "Changed"));
    assert!(Arc::ptr_eq(&before[1], &d.layers(0, 0.5)[1]));
}

#[test]
fn only_changed_blocks_get_new_layers() {
    let src = "# Title\n\nBody\n";
    let mut d = deck(src);
    let before = d.layers(0, 0.5);
    d.update("# Title\n\nBody changed\n".to_string());
    let after = d.layers(0, 0.5);
    assert!(Arc::ptr_eq(&before[1], &after[1]), "the title didn't change");
    assert!(!Arc::ptr_eq(&before[2], &after[2]), "the body did");
}

#[test]
fn a_new_scale_redraws() {
    let mut d = deck(TALK);
    let small = d.layers(0, 0.5);
    let large = d.layers(0, 1.0);
    assert!(large[0].pixmap.width() > small[0].pixmap.width());
}

#[test]
fn adding_a_slide_reports_the_slides_whose_text_changed() {
    let mut d = deck(TALK);
    assert_eq!(d.update(format!("{TALK}\n---\n\n# Four\n")), vec![2, 3]);
    assert_eq!(d.slide_count(), 4);
}

#[test]
fn changing_the_talks_settings_redraws_everything() {
    let mut d = deck("---\ncolor.text: red\n---\n\n# One\n\n---\n\n# Two\n");
    assert_eq!(d.update("---\ncolor.text: blue\n---\n\n# One\n\n---\n\n# Two\n".to_string()), vec![0, 1]);
}

#[test]
fn title_comes_from_the_talks_settings() {
    assert_eq!(deck("---\ntitle: Review\n---\n\n# One\n").title().as_deref(), Some("Review"));
}

#[test]
fn golden_showcase() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase");
    let source = std::fs::read_to_string(root.join("slides.md")).unwrap();
    let mut d = Deck::open(Box::new(FolderFiles::new(root)), Fonts::bundled_only(), source);
    for index in 0..d.slide_count() {
        assert!(d.notices(index).is_empty(), "slide {index}: {:?}", d.notices(index));
        let layers: Vec<_> = d.layers(index, 1.0);
        let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
        for layer in &layers {
            target.draw_pixmap(layer.x, layer.y, layer.pixmap.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
        common::assert_golden(&format!("showcase-{index}"), &target);
    }
}
```

`fixtures/showcase/slides.md` has one slide per layout, then one with shapes, an icon, a picture and a table, and one with code. Together they use every built-in. `assets/photo.png` is a small generated test image, with no real people or places.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test deck`
Expected: FAIL, `Deck` not found.

- [ ] **Step 3: Implement `deck.rs`**

A slide is unchanged when its source text is the same and the first slide's settings are the same. Lay every changed slide out again, then reuse an `Arc<Layer>` when its source text, `rect`, `rotate` and scale match, so a changed slide keeps the pixels of the blocks that didn't change. The background is keyed by the theme. When slides shift position because one was added or removed, report the indices whose text differs at that index.

- [ ] **Step 4: Create the goldens, look at them, and verify**

Run: `UPDATE_GOLDEN=1 scripts/cargo-safe.sh test -p slides-engine --test deck`, then open each `showcase-*.png`.
Expected: every slide looks right, with nothing overlapping.

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/engine fixtures/showcase
git commit -m "Engine: a deck that redraws only what changed, and the showcase fixture"
```

### Task 11: Show a talk in the window

**Files:**
- Create: `crates/app/src/{document,slide_canvas}.rs`, `crates/app/tests/document_state.rs`, `crates/app/tests/ui_slides.rs`
- Modify: `crates/app/src/{lib,main,workspace_view}.rs`, `crates/app/Cargo.toml`

**Interfaces:**
- Consumes: `Deck`, `FolderFiles`, `Fonts`, `Layer`, `to_bgra_frame`.
- Produces, in `stagepack_studio`:
  - `document::Document`, with `pub fn open(folder: &Path, fonts: Fonts) -> Result<Document, String>`, `pub fn current(&self) -> usize`, `pub fn slide_count(&self) -> usize`, `pub fn next(&mut self)`, `pub fn previous(&mut self)`, `pub fn title(&self) -> String`, `pub fn layers(&mut self, scale: f32) -> Vec<Arc<Layer>>`, `pub fn source(&self) -> &str`
  - `slide_canvas::SlideCanvas`, a GPUI view, with `pub fn set_layers(&mut self, layers: Vec<Arc<Layer>>, cx: &mut Context<Self>)`
  - `WorkspaceView::open(folder: PathBuf, window, cx)`, and the test constructor `WorkspaceView::open_with(document: Document, window, cx)`

- [ ] **Step 1: Write the failing tests**

```rust
// crates/app/tests/document_state.rs
use slides_engine::Fonts;
use stagepack_studio::document::Document;

fn talk(source: &str) -> (tempfile::TempDir, Document) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    let doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    (dir, doc)
}

#[test]
fn next_and_previous_stop_at_the_ends() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    doc.previous();
    assert_eq!(doc.current(), 0);
    doc.next(); doc.next();
    assert_eq!(doc.current(), 1);
}

#[test]
fn a_folder_with_no_slides_file_says_so() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Document::open(dir.path(), Fonts::bundled_only()).err().unwrap().contains("slides.md"));
}

#[test]
fn the_title_says_where_you_are() {
    let (_d, mut doc) = talk("---\ntitle: Review\n---\n\n# One\n\n---\n\n# Two\n");
    doc.next();
    assert_eq!(doc.title(), "Review · 2 / 2");
}

// crates/app/tests/ui_slides.rs: open with `WorkspaceView::open_with`, then
#[gpui_kit::test]
fn right_arrow_moves_to_the_next_slide(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n");
    cx.simulate_keystrokes(handle, "right");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(TITLE_ID).label(), Some("T · 2 / 2"));
    }).unwrap();
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: FAIL, `document` not found.

- [ ] **Step 3: Implement `document.rs`, `slide_canvas.rs` and the wiring**

- `Document::open` reads `slides.md` from the folder and loads any `assets/fonts/*.{ttf,otf}`. `title` uses the talk's `title` setting, or else the folder's name.
- `SlideCanvas` draws in a `canvas`, letterboxed to 16:9. Each `Arc<Layer>` keeps one `Arc<RenderImage>`, built with `to_bgra_frame`, and reused while the `Arc<Layer>` is the same. Images that fall out are released with `drop_image` on the next paint.
- The render scale is the canvas's device width over 1920. Re-render layers when it changes by more than 1%.
- `WorkspaceView` binds Left, Right, PageUp and PageDown. The app starts with `Fonts::with_system_fallback()`.
- `main.rs` takes a folder argument, or asks with `prompt_for_paths` (`directories: true`, `files: false`).
- Notices appear as a small line under the slide.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: all pass.

- [ ] **Step 5: Run it and look**

Run the app on `fixtures/showcase`, with a screenshot by window name through `xwd`, as in the spikes.
Expected: slide 1 drawn correctly and letterboxed, and the title reading `… · 1 / N`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/app
git commit -m "App: open a folder and show its slides"
```

### Task 12: Update on save, and View Markdown

**Files:**
- Create: `crates/app/src/{watcher,markdown_panel}.rs`, `crates/app/tests/watcher.rs`
- Modify: `crates/app/src/{document,workspace_view,lib}.rs`, `crates/app/tests/{document_state,ui_slides}.rs`, `crates/app/Cargo.toml`

**Interfaces:**
- Consumes: `Document`, `Deck::update`.
- Produces, in `stagepack_studio`:
  - `document::Reload` as `pub enum Reload { Changed(Vec<usize>), Unchanged, KeptLastGood }`
  - `Document::reload(&mut self, contents: Option<String>) -> Reload`
  - `watcher::watch(folder: &Path, on_change: impl Fn() + Send + 'static) -> notify::Result<notify::RecommendedWatcher>`
  - `markdown_panel::MarkdownPanel`, a GPUI view with `pub fn set_source(&mut self, source: &str, cx: &mut Context<Self>)`
  - `workspace_view::MARKDOWN_ID`, and the action `ToggleMarkdown`, bound to Ctrl+M

- [ ] **Step 1: Write the failing tests**

```rust
// add to crates/app/tests/document_state.rs
use stagepack_studio::document::Reload;

#[test]
fn saving_a_change_reports_the_slide() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    assert!(matches!(doc.reload(Some("# One\n\n---\n\n# Changed\n".into())), Reload::Changed(s) if s == vec![1]));
}

#[test]
fn saving_the_same_bytes_changes_nothing() {
    let (_d, mut doc) = talk("# One\n");
    assert!(matches!(doc.reload(Some("# One\n".into())), Reload::Unchanged));
}

#[test]
fn empty_read_keeps_the_last_good_talk() {
    let (_d, mut doc) = talk("# One\n");
    assert!(matches!(doc.reload(Some(String::new())), Reload::KeptLastGood));
    assert!(matches!(doc.reload(None), Reload::KeptLastGood));
    assert_eq!(doc.source(), "# One\n");
}

#[test]
fn reload_keeps_your_place() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    doc.next();
    doc.reload(Some("# One\n\n---\n\n# Two!\n".into()));
    assert_eq!(doc.current(), 1);
}

#[test]
fn removing_slides_moves_you_to_the_last_one() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n\n---\n\n# Three\n");
    doc.next(); doc.next();
    doc.reload(Some("# One\n".into()));
    assert_eq!(doc.current(), 0);
}

// crates/app/tests/watcher.rs
#[test]
fn a_change_to_the_slides_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slides.md");
    std::fs::write(&file, "a").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = stagepack_studio::watcher::watch(dir.path(), move || { let _ = tx.send(()); }).unwrap();
    std::fs::write(&file, "b").unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(5)).expect("a change event");
}

#[test]
fn other_files_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = stagepack_studio::watcher::watch(dir.path(), move || { let _ = tx.send(()); }).unwrap();
    std::fs::write(dir.path().join("notes.txt"), "b").unwrap();
    assert!(rx.recv_timeout(std::time::Duration::from_millis(800)).is_err());
}

// add to crates/app/tests/ui_slides.rs
#[gpui_kit::test]
fn ctrl_m_shows_the_markdown(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(MARKDOWN_ID).visible());
    }).unwrap();
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: FAIL, `reload` not found.

- [ ] **Step 3: Implement the watcher, reload and the panel**

- `watch` uses `notify::recommended_watcher` on the folder, non-recursively, and calls `on_change` for events naming `slides.md`.
- `WorkspaceView` debounces change events by 100 ms, reads the file on a background task, passes the result to `Document::reload`, and sets the canvas layers only when the result is `Changed` and contains the current slide.
- `MarkdownPanel` is a read-only `uniform_list` of the source's lines in the mono font, in a panel to the right of the canvas.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: all pass.

- [ ] **Step 5: Try it for real**

Run the app on a copy of `fixtures/showcase`. Change a heading in the copy's `slides.md` from the shell while the app runs, then screenshot the window.
Expected: the slide shows the new heading, with no restart.

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/app
git commit -m "App: update on save, and View Markdown"
```

### Task 13: Record what ran, and how to check Windows

This project doesn't use GitHub Actions (owner's decision, 2026-11-10). An earlier version of this task added a workflow, and its one run was cancelled before it reported anything.

**Files:**
- Delete: `.github/workflows/engine.yml`
- Create: `docs/checks/plan-a.md`

**Interfaces:**
- Consumes: the tests from Tasks 2 to 12.

- [ ] **Step 1: Remove the workflow**

```bash
git revert --no-edit bf0a075
```

Expected: `.github` no longer exists in the working tree.

- [ ] **Step 2: Run every test locally**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: all pass.

- [ ] **Step 3: Write `docs/checks/plan-a.md`, and commit**

Record the test counts you measured, the Linux real-window check, what wasn't exercised, and that nothing has run on Windows. Add the owner's optional Windows check, by hand on their Windows PC, with Rust 1.98.0 and the MSVC build tools installed:

1. `git clone -b native-rust https://github.com/Eiriksb/stagepack-studio.git`
2. `cargo test -p slides-format -p toolkit -p slides-engine`. If only golden tests fail, the images differ slightly from Linux, so send back which ones.
3. `cargo run --release -p stagepack-studio -- fixtures\showcase`, then confirm the slides match the images in `crates/engine/tests/golden/`, that Left and Right change slides, and that Ctrl+M opens the Markdown panel.

```bash
git add docs/checks/plan-a.md
git commit -m "Plan A check: what ran, and how to check Windows"
```
