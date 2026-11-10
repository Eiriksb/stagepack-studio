# Plan B: Present and export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Present a talk full screen with a presenter view, click steps, entrances and transitions, save and open packed `.sptx` files, and export to editable PowerPoint and to PDF.

**Architecture:** Layout now also records when each layer appears and what it is in semantic terms. Presenting is pure state plus two GPUI windows that move cached layers. The PDF writer walks the display list with krilla. The PowerPoint writer is a Rust port of the owner's `slidev-pptx-export` OOXML writer and reads the semantic elements.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0, krilla 0.8.2, zip 9.5, quick-xml 0.41 (tests), the existing `slides-format`, `toolkit` and `slides-engine` crates.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`, sections "The talk on disk", "Presenting", "Export", and goals 7, 8, 11 and 12. Plan A's results are in `docs/checks/plan-a.md`.

## Global Constraints

- Work on the branch `plan-b`, cut from `main`.
- No GitHub Actions, for Windows builds or anything else. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`.
- Pin `krilla = "=0.8.2"`, `zip = { version = "=9.5.0", default-features = false, features = ["deflate"] }`, and `quick-xml = "=0.41.0"` where it's used.
- All our code is Rust. Node may be used once, by hand, to generate reference files, and no script of it is committed.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf` and `slides-pptx` don't depend on GPUI.
- A talk is untrusted input. Nothing in one may crash the app, write outside the chosen file, or read outside the talk.
- Files are written with a temp file, sync and rename, so a crash can't damage an existing file.
- Slide units are 1920×1080. In PowerPoint and PDF one unit is 0.5 pt, which is 6350 EMU. A slide is 13.333×7.5 in, or 960×540 pt.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. It holds the owner's own uncommitted edit.
- Don't name real decks, clients or organisations in commits. Use fixtures.

The exporter to port from is at `~/.local/share/stagepack/runtimes/53/node_modules/slidev-pptx-export/src` (called `$EXPORTER` below). Node is at `~/.local/share/stagepack/node/bin/node`.

Not in this plan: custom components (`Ctx.click_step` stays 0 until plan C), the Animate panel (plan D), per-click PDF pages, and `.sptx` file association (plan E).

## Review Focus

- **Text full of XML's own characters:** `&`, `<`, quotes and control characters in a talk must still give a `.pptx` that opens. Owned by Task 6's `special_characters_stay_valid_xml`.
- **A save during a show that removes slides:** the show must stay on a slide that exists. Owned by Task 12's `a_save_that_removes_slides_keeps_the_show_on_a_real_slide`.
- **An export that fails part-way, or a target that's open elsewhere:** the old file must be left alone, and the person told. Owned by Task 10's `a_failed_export_leaves_the_old_file_alone`.
- **A machine with no second screen, or that reports no displays:** the show must still open. Owned by Task 12's `no_display_information_still_opens_a_window` and `one_screen_gives_one_full_screen_window`.
- **Slides that can't be drawn, and empty talks:** export must still work. Owned by Task 4's `a_slide_with_an_error_still_exports` and Task 8's `a_slide_with_an_error_still_exports` and `an_empty_talk_exports_one_slide`.

---

### Task 1: When layers appear

**Files:**
- Modify: `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md`, `crates/engine/src/{display,slide,elements,deck,lib}.rs`, and every test that builds a `LayerSpec`
- Create: `crates/engine/src/timeline.rs`, `crates/engine/tests/timeline.rs`

**Interfaces:**
- Produces, in `slides_engine`:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Enter { #[default] Appear, Fade, FlyUp }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Transition { #[default] None, Fade, Push }`
  - `LayerSpec` gains `pub click: Option<u32>` and `pub enter: Enter`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct LayerTiming { pub click: Option<u32>, pub enter: Enter }`
  - `#[derive(Clone, Debug, PartialEq)] pub struct SlideTimeline { pub steps: u32, pub layers: Vec<LayerTiming>, pub transition: Transition }`
  - `Deck::timeline(&mut self, index: usize) -> SlideTimeline`, with `layers` in the same order as `Deck::layers`
  - `Deck::notes(&self, index: usize) -> Option<String>`, `Deck::specs(&mut self, index: usize) -> &[LayerSpec]`, `Deck::theme(&self) -> toolkit::Theme`, `Deck::fonts_mut(&mut self) -> &mut Fonts`

Written forms:

| Where | Form | Meaning |
| --- | --- | --- |
| Any positioned tag | `click="2"` | Appears on click 2. Whole numbers 1 to 999 |
| Any positioned tag | `enter="fade"` | `appear` (default), `fade` or `fly-up` |
| `region.<name>` line | `click=1 enter=fade` | The same, for the region |
| `region.<name>` line | `click=each` | Each top-level block, and each item of a top-level list, gets its own layer, on clicks 1, 2, 3… |
| Slide settings | `transition: fade` | `none` (default), `fade` or `push`, played on arriving at this slide |

A bad value counts as absent and raises a notice. A rotated region with `click=each` reveals as one layer on click 1, with a notice.

- [ ] **Step 1: Create the branch and commit the plan**

```bash
git switch -c plan-b main
```

Add ``File: `2026-11-10-native-rust-plan-b-present-export.md`.`` under the roadmap's "B. Present and export" heading.

```bash
git add docs/superpowers/plans/2026-11-09-native-rust-roadmap.md docs/superpowers/plans/2026-11-10-native-rust-plan-b-present-export.md
git commit -m "Plan B: present and export"
```

- [ ] **Step 2: Write the failing tests in `crates/engine/tests/timeline.rs`**

The helpers `lay_out(src) -> LaidOutSlide` and `deck(src) -> Deck` use `builtin_registry()`, `PictureCache::default()` and a `NoFiles` that reads nothing, as `tests/builtins.rs` and `tests/deck.rs` do.

```rust
#[test]
fn a_tag_appears_on_its_click() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" click=\"2\" enter=\"fade\" />\n");
    let layer = slide.layers.last().unwrap();
    assert_eq!((layer.click, layer.enter), (Some(2), Enter::Fade));
}

#[test]
fn layers_with_no_click_are_there_from_the_start() {
    let slide = lay_out("# T\n\nBody\n");
    assert!(slide.layers.iter().all(|l| l.click.is_none() && l.enter == Enter::Appear));
}

#[test]
fn a_region_line_sets_click_and_enter() {
    let slide = lay_out("---\nregion.body: click=1 enter=fly-up\n---\n\n# T\n\nBody\n");
    let body = slide.layers.last().unwrap();
    assert_eq!((body.click, body.enter), (Some(1), Enter::FlyUp));
}

#[test]
fn click_each_gives_every_list_item_its_own_layer() {
    let slide = lay_out("---\nregion.body: click=each\n---\n\n# T\n\n- one\n- two\n- three\n");
    let items = &slide.layers[2..];
    assert_eq!(items.iter().map(|l| l.click).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
    assert!(items[0].rect.y < items[1].rect.y && items[1].rect.y < items[2].rect.y);
    assert_eq!(items[0].rect.x, 100.0);
}

#[test]
fn click_each_counts_blocks_and_items_in_order() {
    let slide = lay_out("---\nregion.body: click=each\n---\n\n# T\n\nIntro\n\n- one\n- two\n");
    assert_eq!(slide.layers[2..].iter().map(|l| l.click).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
}

#[test]
fn a_rotated_region_reveals_as_one_with_a_notice() {
    let slide = lay_out("---\nregion.body: rotate=5 click=each\n---\n\n# T\n\n- one\n- two\n");
    assert_eq!(slide.layers.last().unwrap().click, Some(1));
    assert_eq!(slide.layers.len(), 3);
    assert!(!slide.notices.is_empty());
}

#[test]
fn a_bad_click_is_ignored_with_a_notice() {
    for bad in ["soon", "0", "1000", "-1", "1.5"] {
        let slide = lay_out(&format!("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"{bad}\" />\n"));
        assert_eq!(slide.layers.last().unwrap().click, None, "{bad}");
        assert!(!slide.notices.is_empty(), "{bad}");
    }
}

#[test]
fn an_unknown_entrance_appears_with_a_notice() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"1\" enter=\"spin\" />\n");
    assert_eq!(slide.layers.last().unwrap().enter, Enter::Appear);
    assert!(!slide.notices.is_empty());
}

#[test]
fn the_timeline_lists_steps_and_layers() {
    let mut d = deck("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"2\" />\n\n<Box x=\"9\" y=\"0\" w=\"9\" h=\"9\" click=\"1\" />\n");
    let timeline = d.timeline(0);
    assert_eq!(timeline.steps, 2);
    assert_eq!(timeline.layers.len(), d.layers(0, 1.0).len());
    assert_eq!(timeline.layers[0].click, None, "the background is always there");
}

#[test]
fn transition_is_read_from_the_slide() {
    assert_eq!(deck("---\ntransition: fade\n---\n\n# A\n").timeline(0).transition, Transition::Fade);
    assert_eq!(deck("---\ntransition: push\n---\n\n# A\n").timeline(0).transition, Transition::Push);
    assert_eq!(deck("# A\n").timeline(0).transition, Transition::None);
}

#[test]
fn an_unknown_transition_is_none_with_a_notice() {
    let mut d = deck("---\ntransition: spin\n---\n\n# A\n");
    assert_eq!(d.timeline(0).transition, Transition::None);
    assert!(!d.notices(0).is_empty());
}

#[test]
fn notes_come_from_the_slide() {
    let d = deck("# A\n\n<Notes>\nSay hello.\n</Notes>\n\n---\n\n# B\n");
    assert_eq!(d.notes(0).as_deref(), Some("Say hello."));
    assert_eq!(d.notes(1), None);
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test timeline`
Expected: FAIL, `Enter` not found.

- [ ] **Step 4: Implement it**

`timeline.rs` holds the two types and the parsing of the written forms. For `click=each`, `flow_region` lays the region out as before, then draws each split node on its own: a layer's `rect` is the block's or item's box on the slide, and its items are relative to that box. A split layer's `source` covers that block, or the item's first to last block. A positioned tag's reading goes through `TagPlace`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: all pass, and no golden image changes.

- [ ] **Step 6: Commit**

```bash
git add crates/engine
git commit -m "Engine: when layers appear, and how slides change"
```

### Task 2: What each layer is, for PowerPoint

**Files:**
- Create: `crates/engine/src/native.rs`, `crates/engine/tests/native.rs`
- Modify: `crates/engine/src/{display,flow,elements,slide,lib}.rs`

**Interfaces:**
- Consumes: `LayerSpec` from Task 1.
- Produces, in `slides_engine`:
  - `#[derive(Clone, Debug, PartialEq)] pub struct NativeRun { pub text: String, pub size: f32, pub bold: bool, pub italic: bool, pub underline: bool, pub color: Color, pub family: String, pub link: Option<String> }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum Bullet { None, Dot, Number(u32) }`
  - `#[derive(Clone, Debug, PartialEq)] pub struct NativeParagraph { pub runs: Vec<NativeRun>, pub align: Align, pub level: u8, pub bullet: Bullet }`
  - `#[derive(Clone)] pub enum NativeContent { Text { paragraphs: Vec<NativeParagraph>, fill: Option<Color>, inset: f32 }, Shape { kind: ShapeKind, fill: Option<Color>, outline: Option<Color>, outline_width: f32, radius: f32, paragraphs: Vec<NativeParagraph> }, Table { rows: Vec<Vec<Vec<NativeParagraph>>>, column_widths: Vec<f32>, row_heights: Vec<f32>, line: Color }, Picture { src: Option<String>, alt: String, pixmap: Arc<Pixmap> } }`
  - `#[derive(Clone)] pub struct NativeElement { pub rect: Rect, pub content: NativeContent }`, with `rect` relative to the layer's own rect
  - `LayerSpec` gains `pub native: Vec<NativeElement>`, empty for the background, and `pub is_title: bool`, true for the title region's layer

Rules: adjacent headings, paragraphs and lists in a region become one `Text` element. A table, code block or picture ends that run and is its own element, placed from the layout tree. A code block is `Text` with `fill` of `#f1f5f9` and `inset` of 24. A shape holding only plain text carries it in `paragraphs`. Inside a stack, each shape, text and picture is its own element. Icons and paths are `Picture` with `src: None`. An unknown component's or a broken slide's placeholder is `Text` with its label.

- [ ] **Step 1: Write the failing tests in `crates/engine/tests/native.rs`**

`lay_out` is as in Task 1, and `MapFiles` as in `tests/builtins.rs`.

```rust
fn text_of(p: &NativeParagraph) -> String { p.runs.iter().map(|r| r.text.as_str()).collect() }

#[test]
fn a_region_of_text_is_one_text_box() {
    let slide = lay_out("# T\n\nFirst\n\n## Sub\n\n- a\n- b\n");
    let body = slide.layers.last().unwrap();
    assert_eq!(body.native.len(), 1);
    let NativeContent::Text { paragraphs, fill: None, .. } = &body.native[0].content else { panic!() };
    assert_eq!(paragraphs.iter().map(text_of).collect::<Vec<_>>(), ["First", "Sub", "a", "b"]);
    assert_eq!(paragraphs[2].bullet, Bullet::Dot);
    assert!(paragraphs[1].runs[0].bold);
}

#[test]
fn only_the_title_region_is_marked_as_the_title() {
    let slide = lay_out("# T\n\nBody\n");
    assert_eq!(slide.layers.iter().map(|l| l.is_title).collect::<Vec<_>>(), [false, true, false]);
}

#[test]
fn numbered_and_nested_lists_keep_their_shape() {
    let slide = lay_out("# T\n\n1. one\n2. two\n   - inner\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(paragraphs[1].bullet, Bullet::Number(2));
    assert_eq!((paragraphs[2].bullet, paragraphs[2].level), (Bullet::Dot, 1));
}

#[test]
fn a_table_splits_the_text_around_it() {
    let slide = lay_out("# T\n\nBefore\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nAfter\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 3);
    let NativeContent::Table { rows, column_widths, row_heights, .. } = &native[1].content else { panic!() };
    assert_eq!((rows.len(), column_widths.len(), row_heights.len()), (2, 2, 2));
    assert!(native[0].rect.y < native[1].rect.y && native[1].rect.y < native[2].rect.y);
}

#[test]
fn code_is_a_text_box_with_a_panel() {
    let slide = lay_out("# T\n\n```rust\nfn main() {}\n```\n");
    let NativeContent::Text { fill, inset, paragraphs } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!((*fill, *inset), (Color::parse("#f1f5f9"), 24.0));
    assert_eq!(paragraphs[0].runs[0].family, "JetBrains Mono");
}

#[test]
fn a_box_with_text_is_a_shape_holding_it() {
    let slide = lay_out("<Box x=\"10\" y=\"10\" w=\"400\" h=\"200\" fill=\"red\">\n\nHello\n\n</Box>\n");
    let NativeContent::Shape { paragraphs, fill, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(text_of(&paragraphs[0]), "Hello");
    assert_eq!(*fill, Color::parse("red"));
}

#[test]
fn shapes_keep_their_kind() {
    for (name, kind) in [("rect", ShapeKind::Rect), ("rounded", ShapeKind::Rounded), ("ellipse", ShapeKind::Ellipse),
                         ("line", ShapeKind::Line), ("arrow", ShapeKind::Arrow), ("callout", ShapeKind::Callout)] {
        let slide = lay_out(&format!("<Shape kind=\"{name}\" x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n"));
        assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Shape { kind: k, .. } if *k == kind), "{name}");
    }
}

#[test]
fn pictures_remember_their_file_and_icons_have_none() {
    let mut png = Vec::new();
    image::RgbaImage::new(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let slide = lay_out_with("![A cat](a.png)\n\n<Icon name=\"arrow-right\" x=\"0\" y=\"0\" w=\"64\" h=\"64\" />\n", &[("a.png", png)]);
    let picture = slide.layers.iter().flat_map(|l| &l.native).find(|n| matches!(&n.content, NativeContent::Picture { src: Some(_), .. })).unwrap();
    assert!(matches!(&picture.content, NativeContent::Picture { src: Some(s), alt, .. } if s == "a.png" && alt == "A cat"));
    assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Picture { src: None, .. }));
}

#[test]
fn links_are_kept() {
    let slide = lay_out("# T\n\n[home](https://example.com)\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(paragraphs[0].runs[0].link.as_deref(), Some("https://example.com"));
}

#[test]
fn an_unknown_tag_is_labelled_text() {
    let slide = lay_out("<Mystery x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n");
    assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Text { .. }));
}
```

`NativeRun` carries the span's link, so `Flow` needs to pass it alongside each `TextRun`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test native`
Expected: FAIL, `NativeContent` not found.

- [ ] **Step 3: Implement `native.rs`**

After `compute_layout`, walk the same Taffy tree that `draw` walks. Register semantic nodes (`Text`, list, table, code) in a map keyed by `NodeId` as `Flow` builds them, so one layout gives both outputs. Every slide-unit number comes from the layout tree.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: all pass, and no golden image changes.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: record what each layer is, for export"
```

### Task 3: Packed `.sptx` files

**Files:**
- Create: `crates/engine/src/packed.rs`, `crates/engine/tests/packed.rs`
- Modify: `crates/engine/src/{files,lib}.rs`, `crates/engine/Cargo.toml`

**Interfaces:**
- Produces, in `slides_engine`:
  - `pub const MANIFEST_NAME: &str = "talk.json";`, `pub const STAGEPACK_MANIFEST: &str = "stagepack.json";`, `pub const FORMAT_VERSION: u32 = 1;`
  - `#[derive(Clone, Copy)] pub struct PackLimits { pub entry_bytes: u64, pub total_bytes: u64, pub entries: usize }`, with `Default` of 256 MiB, 2 GiB and 10,000
  - `#[derive(Debug)] pub enum PackError { NotATalk, SlidevTalk, TooNew { format: u32 }, UnsafeName(String), TooLarge(String), Corrupt(String), Io(std::io::Error) }`, with `Display` giving a sentence a person can read
  - `pub struct PackedFiles`, `Send + Sync`, implementing `TalkFiles`, reading entries on demand from the zip behind a `Mutex`; `PackedFiles::slides(&self) -> Result<String, PackError>`, `PackedFiles::names(&self) -> Vec<String>`
  - `pub fn open_packed(path: &Path) -> Result<PackedFiles, PackError>`, `pub fn open_packed_limited(path: &Path, limits: PackLimits) -> Result<PackedFiles, PackError>`
  - `pub struct PackReport { pub skipped: Vec<String> }`
  - `pub fn pack_folder(folder: &Path, out: &Path, app_version: &str) -> Result<PackReport, PackError>`
  - `pub fn unpack_to_folder(packed: &Path, folder: &Path) -> Result<(), PackError>`
  - `pub fn write_atomically(out: &Path, write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> std::io::Result<()>`
  - `impl TalkFiles for Arc<dyn TalkFiles + Send + Sync>`

The manifest is `{"format":1,"app":"<version>","entry":"slides.md"}`. A pack holds `slides.md`, `talk.json`, and everything under `components/`, `assets/` and `build/`, sorted by name, with fixed timestamps. Symlinks are left out and listed in `skipped`. Pictures are stored and text is deflated.

- [ ] **Step 1: Write the failing tests in `crates/engine/tests/packed.rs`**

```rust
fn talk_folder() -> tempfile::TempDir; // slides.md, assets/a.png, components/c.rs, build/c.wasm, notes.txt, .git/x, target/y

#[test]
fn pack_then_open_gives_the_same_files() {
    let dir = talk_folder();
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    assert_eq!(packed.slides().unwrap(), std::fs::read_to_string(dir.path().join("slides.md")).unwrap());
    assert_eq!(packed.read("assets/a.png"), Some(std::fs::read(dir.path().join("assets/a.png")).unwrap()));
}

#[test]
fn packing_leaves_out_everything_else() {
    let names = pack_and_list(&talk_folder());
    assert_eq!(names, ["assets/a.png", "build/c.wasm", "components/c.rs", "slides.md", "talk.json"]);
}

#[test]
fn packing_twice_gives_identical_bytes() { /* two packs of one folder compare equal */ }

#[test]
fn a_stagepack_file_is_recognised() {
    let path = zip_with(&[("stagepack.json", b"{}"), ("slides.md", b"# A")]);
    assert!(matches!(open_packed(&path), Err(PackError::SlidevTalk)));
}

#[test]
fn a_zip_without_a_manifest_is_refused() {
    assert!(matches!(open_packed(&zip_with(&[("slides.md", b"# A")])), Err(PackError::NotATalk)));
}

#[test]
fn something_that_is_not_a_zip_is_refused() { /* a text file gives PackError::Corrupt or NotATalk, with no panic */ }

#[test]
fn a_newer_format_is_refused() {
    let path = zip_with(&[("talk.json", br#"{"format":2,"app":"9","entry":"slides.md"}"#), ("slides.md", b"# A")]);
    assert!(matches!(open_packed(&path), Err(PackError::TooNew { format: 2 })));
}

#[test]
fn unsafe_names_are_refused() {
    for name in ["../x", "/abs", "a\\b", "C:/x", "a/../b"] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        assert!(matches!(open_packed(&path), Err(PackError::UnsafeName(_))), "{name}");
    }
}

#[test]
fn oversized_entries_are_refused() {
    let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), ("assets/big", &vec![0u8; 4096])]);
    let limits = PackLimits { entry_bytes: 1024, ..Default::default() };
    assert!(matches!(open_packed_limited(&path, limits), Err(PackError::TooLarge(_))));
}

#[test]
fn too_many_entries_are_refused() { /* limits.entries of 1 */ }

#[test]
fn reading_outside_the_pack_gives_nothing() {
    let dir = talk_folder();
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    assert_eq!(packed.read("../slides.md"), None);
    assert_eq!(packed.read("missing"), None);
}

#[test]
fn a_symlink_is_left_out_and_reported() { /* unix only */ }

#[cfg(unix)]
#[test]
fn a_failed_pack_leaves_the_old_file_and_no_temp() {
    // Make assets/a.png unreadable, pack over an existing out: Err, out unchanged, and only out in its folder.
}

#[test]
fn unpack_then_pack_round_trips() { /* the same names and bytes */ }

#[test]
fn unpacking_refuses_a_folder_with_things_in_it() { /* Err, and the folder unchanged */ }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test packed`
Expected: FAIL, `pack_folder` not found.

- [ ] **Step 3: Implement `packed.rs`**

Read each entry with `Read::take(limit + 1)`, because a zip's stated size can lie. A temp file goes beside the target as `.<name>.<pid>.tmp`. `write_atomically` syncs the file, renames, retries the rename 5 times at 250 ms on Windows, and syncs the folder on Unix. It removes the temp file on any failure.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-engine --test packed`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/engine
git commit -m "Engine: packed .sptx files, safely read and atomically written"
```

### Task 4: Export to PDF

**Files:**
- Modify: `Cargo.toml` (add `crates/pdf`), `crates/engine/src/text.rs`
- Create: `crates/pdf/{Cargo.toml,src/lib.rs,tests/pdf.rs}` (package `slides-pdf`)

**Interfaces:**
- Consumes: `Deck::{specs, fonts_mut, slide_count}` from Task 1.
- Produces: `slides_pdf::write_pdf(deck: &mut Deck) -> Result<Vec<u8>, String>`, and in `slides_engine`, `Fonts::face_bytes(&mut self, id: fontdb::ID) -> Option<(Arc<Vec<u8>>, u32)>`.

One page per slide, 960×540 pt, with every layer drawn in its final state, whatever its click.

- [ ] **Step 1: Write the failing tests in `crates/pdf/tests/pdf.rs`**

A helper `tool(name)` returns `None` when a command isn't installed, and the test then prints that it was skipped. `showcase_pdf()` opens `fixtures/showcase` with `Fonts::bundled_only()` and writes the PDF to a temp folder.

```rust
#[test]
fn one_page_per_slide_at_sixteen_by_nine() {
    let info = run("pdfinfo", &[showcase_pdf()]);
    assert!(info.contains("Pages:           7"), "{info}");
    assert!(info.contains("960 x 540 pts"), "{info}");
}

#[test]
fn text_is_selectable() {
    let text = run("pdftotext", &[showcase_pdf(), "-".into()]);
    assert!(text.contains("Stagepack Showcase") && text.contains("Two columns"), "{text}");
}

#[test]
fn fonts_are_embedded_and_subset() {
    let fonts = run("pdffonts", &[showcase_pdf()]);
    let inter = fonts.lines().find(|l| l.contains("Inter")).expect(&fonts);
    assert!(inter.contains("yes yes"), "embedded and subset: {inter}");
}

#[test]
fn pages_match_the_engines_own_drawing() {
    // pdftoppm at 1920x1080 against compose() of the same slide: the mean absolute difference per channel is under 6.
}

#[test]
fn layers_waiting_for_a_click_are_on_the_page() {
    // A talk with <Box ... click="1">Later</Box>: pdftotext finds "Later".
}

#[test]
fn a_rotated_box_is_rotated() {
    // pages_match_the_engines_own_drawing's comparison, for a Box with rotate="15".
}

#[test]
fn emoji_and_accents_survive() {
    // A slide with blåbær and 🎉 writes, pdftotext finds blåbær, and the rendered page has coloured pixels.
}

#[test]
fn a_slide_with_an_error_still_exports() {
    // "# A\n\n---\n\n<Box>\n\nnever closed\n\n---\n\n# C\n" gives 3 pages.
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pdf`
Expected: FAIL, `write_pdf` not found.

- [ ] **Step 3: Implement `crates/pdf/src/lib.rs`**

`krilla::Document::new()`, `start_page_with(PageSettings::from_wh(960.0, 540.0))`, and a page transform of scale 0.5. For each `LayerSpec`, push translate and rotate about the centre of `rect`, then draw items. `Fill` and `Stroke` convert `tiny_skia::Path` to `krilla::geom::Path`. `Text` walks `layout_runs()`, grouping neighbouring glyphs with the same `font_id`, size and colour into one `surface.draw_glyphs`, and takes font data from `Fonts::face_bytes`. Underlines are stroked lines. `Picture` is `Image::from_png` of `pixmap.encode_png()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-pdf`
Expected: all pass.

- [ ] **Step 5: Look at a page**

Run `pdftoppm -png -r 100` on the showcase PDF's slide 6 into `target/`, and open the image.
Expected: the table, shapes, arrow, picture and icon look as they do in the app.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/engine crates/pdf
git commit -m "PDF export with selectable text and subset fonts"
```

### Task 5: PowerPoint foundations

**Files:**
- Modify: `Cargo.toml` (add `crates/pptx`)
- Create: `crates/pptx/{Cargo.toml,src/lib.rs,src/xml.rs,src/package.rs,src/theme.rs,src/master.rs,src/presentation.rs,tests/parts.rs,tests/reference/README.md,tests/reference/*.xml}` (package `slides-pptx`)

**Interfaces:**
- Produces, in `slides_pptx`, ports of the same-named JS with snake_case names:
  - `xml::{esc, emu_from_units(units: f32) -> i64, color_xml, solid_fill}`
  - `package::{Package, REL, CT}`, with `Package::{add_xml, add_media, rel, to_bytes}` and a fixed zip timestamp
  - `theme::theme_xml(name: &str, theme: &Theme) -> String`
  - `master::{layout_defs, layout_xml, master_xml}`, with only the `titleOnly` and `blank` layouts
  - `presentation::{presentation_xml, pres_props_xml, view_props_xml, app_xml, core_xml}`

The Rust must reproduce the JS output byte for byte, for the same inputs. That's what protects the forms the exporter measured in real PowerPoint.

- [ ] **Step 1: Generate the reference files**

Write a throwaway script in the scratchpad that imports `$EXPORTER/ooxml/*.mjs` and writes each function's output to `crates/pptx/tests/reference/`, one file per case. The geometry is `slideGeometry({ width: 13.333, height: 7.5, marginX: 0.6 }, null)` from `$EXPORTER/geometry.mjs`. `tests/reference/README.md` lists each file and the exact call that produced it. Don't commit the script.

- [ ] **Step 2: Write the failing tests in `crates/pptx/tests/parts.rs`**

One test per reference file, each `assert_eq!(rust_output, include_str!("reference/<name>.xml"))`. Add:

```rust
#[test]
fn units_become_emu() {
    assert_eq!(xml::emu_from_units(1920.0), 12192000);
    assert_eq!(xml::emu_from_units(1080.0), 6858000);
}

#[test]
fn escaping_covers_xml_and_drops_control_characters() {
    assert_eq!(xml::esc("a & <b> \"c\""), "a &amp; &lt;b&gt; &quot;c&quot;");
    assert_eq!(xml::esc("a\u{0B}b\u{FFFE}c\td"), "abc\td");
}

#[test]
fn a_package_is_a_zip_with_content_types_first() {
    let mut package = Package::default();
    package.add_xml("/ppt/presentation.xml", CT::PRESENTATION, "<a/>");
    let names = zip_names(&package.to_bytes().unwrap());
    assert_eq!(names[0], "[Content_Types].xml");
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test parts`
Expected: FAIL, the modules don't exist.

- [ ] **Step 4: Port the modules**

Port `xml.mjs`, `package.mjs`, `theme.mjs`, `master.mjs` and `presentation.mjs` from `$EXPORTER/ooxml/`. Keep the source's comments that record what was measured in PowerPoint. Leave out charts, gradients, shadows and video.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test parts`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/pptx
git commit -m "PowerPoint: the package, theme, master and presentation parts"
```

### Task 6: PowerPoint text, shapes, tables and pictures

**Files:**
- Create: `crates/pptx/src/{text,shapes,table}.rs`, `crates/pptx/tests/elements.rs`, more `tests/reference/*.xml`
- Modify: `crates/pptx/src/lib.rs`

**Interfaces:**
- Consumes: `NativeParagraph`, `NativeRun`, `NativeContent` from Task 2.
- Produces:
  - `text::{rpr_xml, ppr_xml, body_pr_xml, tx_body_xml(paragraphs: &[NativeParagraph], ctx: &TextCtx, body: &BodyOptions, tag: &str) -> String}`
  - `TextCtx { lang: String, link: Box<dyn FnMut(&str) -> String> }`
  - `shapes::{xfrm_xml(rect: Rect, rotate: f32) -> String, placeholder_xml, shape_xml, picture_xml, graphic_frame_xml, background_xml, slide_xml}`
  - `table::{table_xml, table_styles_xml}`

Font size: `sz` is `round(size * 50)`, held to 100..400000. Rotation: `rot` is `round(degrees * 60000)`, wrapped to 0..21600000. Shape geometry:

| `ShapeKind` | Output |
| --- | --- |
| `Rect` | `prst="rect"` |
| `Rounded` | `prst="roundRect"` with `adj` of `round(radius / min(w, h) * 100000)`, at most 50000 |
| `Ellipse` | `prst="ellipse"` |
| `Line` | `prst="line"` |
| `Arrow` | `prst="line"`, and `<a:tailEnd type="triangle"/>` last inside `<a:ln>` |
| `Callout` | `prst="wedgeRoundRectCallout"` |

- [ ] **Step 1: Generate more reference files, as in Task 5**

Cases, each from the matching JS function: a plain run; a bold, italic, underlined, coloured run with a typeface; a linked run; a bulleted, a numbered and a level-1 paragraph; centred text; a text box; a filled rectangle with text; a rounded rectangle; an ellipse; a picture; a 2×2 table; a slide wrapping one shape.

- [ ] **Step 2: Write the failing tests in `crates/pptx/tests/elements.rs`**

One test per reference file, plus:

```rust
#[test]
fn sizes_and_angles_convert() {
    assert!(run_xml(48.0).contains("sz=\"2400\""));
    assert!(xfrm_xml(RECT, 90.0).contains("rot=\"5400000\""));
    assert!(xfrm_xml(RECT, -90.0).contains("rot=\"16200000\""));
}

#[test]
fn an_arrow_is_a_line_with_a_head() {
    let xml = shape_of(ShapeKind::Arrow);
    assert!(xml.contains("prst=\"line\"") && xml.contains("<a:tailEnd type=\"triangle\"/></a:ln>"));
}

#[test]
fn a_rounded_box_gets_its_radius() {
    let xml = rounded(Rect { x: 0.0, y: 0.0, w: 400.0, h: 200.0 }, 24.0);
    assert!(xml.contains("val 12000"), "{xml}");
}

#[test]
fn special_characters_stay_valid_xml() {
    let xml = run_xml_of("a & <b> \"c\" \u{0B} 🎉");
    let mut reader = quick_xml::Reader::from_str(&format!("<r xmlns:a=\"x\">{xml}</r>"));
    loop { match reader.read_event().unwrap() { quick_xml::events::Event::Eof => break, _ => {} } }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test elements`
Expected: FAIL, the modules don't exist.

- [ ] **Step 4: Port `text.mjs`, `shapes.mjs` and `table.mjs`**

Inputs are the native types, and the reference cases give the matching JS objects. A text with a `\n` becomes `<a:br>`, as in `text.mjs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test elements`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/pptx
git commit -m "PowerPoint: text, shapes, tables and pictures"
```

### Task 7: PowerPoint animations, transitions and notes

**Files:**
- Create: `crates/pptx/src/{timing,notes}.rs`, `crates/pptx/tests/timing.rs`, more `tests/reference/*.xml`
- Modify: `crates/pptx/src/lib.rs`

**Interfaces:**
- Consumes: `Enter`, `Transition` from Task 1.
- Produces:
  - `timing::Effect { pub spid: u32, pub is_text: bool, pub has_fill: bool, pub click: u32, pub enter: Enter }`
  - `timing::animation_timing_xml(effects: &[Effect]) -> String`, empty when there's nothing to time
  - `timing::transition_xml(transition: Transition) -> String`: `Fade` is `<p:transition><p:fade/></p:transition>`, `Push` is `<p:transition><p:push dir="l"/></p:transition>`
  - `notes::{notes_master_xml, notes_slide_xml(text: &str, ctx: &TextCtx) -> String}`

Effects, inside the same step and group structure as `timing.mjs`, with its `grpId` and `bldP` handling:

| `Enter` | `presetID` | `presetSubtype` | Children |
| --- | --- | --- | --- |
| `Fade` | 10 | 0 | as `timing.mjs` for `dir: "in"` |
| `Appear` | 1 | 0 | only the `visibility` `p:set` |
| `FlyUp` | 2 | 4 | the `visibility` `p:set`, then two `p:anim`, each 500 ms |

The first `p:anim` of `FlyUp` holds `ppt_x` at `#ppt_x`. The second moves `ppt_y` from `1+#ppt_h/2` to `#ppt_y`. Each takes this form, with `<ATTR>`, `<FROM>` and `<TO>` filled in and ids from the same counter as the rest:

```xml
<p:anim calcmode="lin" valueType="num"><p:cBhvr additive="base"><p:cTn id="N" dur="500" fill="hold"/><p:tgtEl><p:spTgt spid="S"/></p:tgtEl><p:attrNameLst><p:attrName><ATTR></p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm="0"><p:val><p:strVal val="<FROM>"/></p:val></p:tav><p:tav tm="100000"><p:val><p:strVal val="<TO>"/></p:val></p:tav></p:tavLst></p:anim>
```

- [ ] **Step 1: Generate reference files for fades, transitions and notes, as in Task 5**

`animationTimingXml({ effects: [...] })` for one fade, and for a text and a picture fading on two clicks; `transitionXml`; `notesMasterXml`; `notesSlideXml`.

- [ ] **Step 2: Write the failing tests in `crates/pptx/tests/timing.rs`**

One test per reference file, plus:

```rust
#[test]
fn nothing_to_time_is_empty() { assert_eq!(animation_timing_xml(&[]), ""); }

#[test]
fn appear_only_sets_visibility() {
    let xml = animation_timing_xml(&[effect(1, Enter::Appear)]);
    assert!(xml.contains("presetID=\"1\"") && xml.contains("style.visibility"));
    assert!(!xml.contains("animEffect") && !xml.contains("p:anim "));
}

#[test]
fn fly_up_comes_from_below_the_slide() {
    let xml = animation_timing_xml(&[effect(1, Enter::FlyUp)]);
    assert!(xml.contains("presetID=\"2\"") && xml.contains("presetSubtype=\"4\""));
    assert!(xml.contains("1+#ppt_h/2") && xml.contains("ppt_y"));
}

#[test]
fn clicks_are_in_order_and_share_a_step() {
    let xml = animation_timing_xml(&[effect_on(5, 2), effect_on(6, 1), effect_on(7, 1)]);
    assert_eq!(xml.matches("nodeType=\"clickEffect\"").count(), 2);
    assert_eq!(xml.matches("nodeType=\"withEffect\"").count(), 1);
    assert!(xml.find("spid=\"6\"").unwrap() < xml.find("spid=\"5\"").unwrap());
}

#[test]
fn every_time_node_has_a_unique_id() { /* collect every cTn id for a mixed timing, and none repeats */ }
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test timing`
Expected: FAIL, the modules don't exist.

- [ ] **Step 4: Port `timing.mjs` and `notes.mjs`, and add `Appear` and `FlyUp`**

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test timing`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/pptx
git commit -m "PowerPoint: click animations, transitions and notes"
```

### Task 8: Write a whole `.pptx`

**Files:**
- Create: `crates/pptx/src/write.rs`, `crates/pptx/tests/write.rs`
- Modify: `crates/pptx/src/lib.rs`

**Interfaces:**
- Consumes: `Deck::{specs, timeline, notes, theme, title, slide_count}`, `TalkFiles`, Tasks 5 to 7.
- Produces: `slides_pptx::PptxOutput { pub bytes: Vec<u8>, pub notices: Vec<String> }` and `slides_pptx::write_pptx(deck: &mut Deck, files: &dyn TalkFiles) -> Result<PptxOutput, String>`.

Rules:
- A slide with a `title` layer uses the `titleOnly` layout, and that layer's text becomes the title placeholder with an explicit box. Other slides use `blank`.
- An element's box on the slide is its layer's `rect` position plus its own `rect`. In a rotated layer, its centre turns around the layer's centre and it takes the layer's `rotate`.
- Every element in a layer with a `click` gets an effect on that click, with that layer's `enter`.
- A picture with a `src` of `png`, `jpg`, `jpeg` or `gif` that `files.read` returns uses those bytes. Otherwise it's `pixmap.encode_png()`. Identical bytes are stored once.
- The slide's background is `theme.background`.

- [ ] **Step 1: Write the failing tests in `crates/pptx/tests/write.rs`**

`showcase()` exports `fixtures/showcase` with `Fonts::bundled_only()`. `part(bytes, name)` reads one zip entry as a string.

```rust
#[test]
fn every_slide_has_a_part() { /* ppt/slides/slide1.xml to slide7.xml exist, and no slide8 */ }

#[test]
fn content_types_cover_every_part() { /* every zip entry has an Override or a Default extension */ }

#[test]
fn relationships_point_at_parts_that_exist() { /* resolve every internal Target in every .rels */ }

#[test]
fn every_xml_part_is_well_formed() { /* quick-xml reads each .xml and .rels to Eof */ }

#[test]
fn shapes_are_native() {
    let xml = part(&showcase().bytes, "ppt/slides/slide6.xml");
    for expected in ["prst=\"ellipse\"", "<a:tbl>", "<p:pic>", "tailEnd type=\"triangle\""] {
        assert!(xml.contains(expected), "{expected}");
    }
}

#[test]
fn a_title_is_a_title_placeholder() {
    let xml = part(&showcase().bytes, "ppt/slides/slide2.xml");
    assert!(xml.contains("type=\"title\""));
}

#[test]
fn click_steps_become_animations() {
    let out = export("<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" fill=\"red\" click=\"1\" enter=\"fade\">\n\nHi\n\n</Box>\n");
    assert!(part(&out.bytes, "ppt/slides/slide1.xml").contains("presetID=\"10\""));
}

#[test]
fn a_transition_is_written() {
    let out = export("---\ntransition: fade\n---\n\n# A\n");
    assert!(part(&out.bytes, "ppt/slides/slide1.xml").contains("<p:fade/>"));
}

#[test]
fn notes_become_notes_slides() {
    let out = export("# A\n\n<Notes>\nSay hello.\n</Notes>\n");
    assert!(part(&out.bytes, "ppt/notesSlides/notesSlide1.xml").contains("Say hello."));
}

#[test]
fn identical_pictures_are_stored_once() { /* two slides using a.png give one ppt/media entry */ }

#[test]
fn links_become_hyperlinks() { /* slide1.xml has hlinkClick, and its .rels has External */ }

#[test]
fn a_rotated_layer_turns_its_elements_around_its_centre() { /* a Box with rotate=90 has rot=5400000 and its centre unchanged */ }

#[test]
fn exporting_twice_gives_identical_bytes() { /* compare two exports */ }

#[test]
fn a_slide_with_an_error_still_exports() { /* 3 slides for a talk whose middle slide is broken */ }

#[test]
fn an_empty_talk_exports_one_slide() { /* "" gives slide1.xml */ }

#[test]
fn it_opens_in_libreoffice() {
    // With soffice installed: -env:UserInstallation in a temp folder, --headless --convert-to pdf.
    // pdfinfo says 7 pages, and pdftotext finds "Stagepack Showcase".
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test write`
Expected: FAIL, `write_pptx` not found.

- [ ] **Step 3: Implement `write.rs`**

Port the part wiring of `writePptx` in `$EXPORTER/ooxml/index.mjs`, including `MediaStore`, the master id space that starts at 2147483648, and the order of `pkg.rel` calls.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-pptx`
Expected: all pass.

- [ ] **Step 5: Look at it**

Convert the showcase's `.pptx` to PDF with LibreOffice, render slides 5 and 6 to PNG in `target/`, and open them.
Expected: positions, text, the table, shapes and picture match the app, apart from small text-wrap differences.

- [ ] **Step 6: Commit**

```bash
git add crates/pptx
git commit -m "PowerPoint: write a whole editable .pptx"
```

### Task 9: Embed the fonts

**Files:**
- Create: `crates/pptx/src/eot.rs`, `crates/pptx/tests/fonts.rs`
- Modify: `crates/pptx/src/{write,presentation,lib}.rs`, `crates/engine/src/text.rs`

**Interfaces:**
- Consumes: `presentation_xml`'s `embedded_fonts`, `Package` from Task 5.
- Produces:
  - `eot::{FontInfo, font_info(data: &[u8]) -> Result<FontInfo, String>, embeddable(fs_type: u16) -> bool, eot_from_ttf(data: &[u8]) -> Result<Vec<u8>, String>}`
  - in `slides_engine`, `pub struct EmbeddableFace { pub family: String, pub bold: bool, pub italic: bool, pub data: Arc<Vec<u8>> }` and `Fonts::embeddable_faces(&self, family: &str) -> Vec<EmbeddableFace>`, for bundled and talk fonts only

Only families the slides use are embedded. A family that can't be embedded raises the notice `<family> can't be embedded, so PowerPoint may show another font`.

- [ ] **Step 1: Write the failing tests in `crates/pptx/tests/fonts.rs`**

```rust
#[test]
fn eot_matches_the_original_writer() {
    let eot = eot_from_ttf(include_bytes!("../../engine/assets/fonts/Inter-Regular.ttf")).unwrap();
    assert_eq!(hex(&Sha256::digest(&eot)), "<sha256 of eotFromTtf's output, generated as in Task 5>");
}

#[test]
fn font_info_reads_the_family() {
    let info = font_info(include_bytes!("../../engine/assets/fonts/Inter-Bold.ttf")).unwrap();
    assert_eq!(info.family, "Inter");
    assert!(info.weight >= 600);
}

#[test]
fn embeddable_follows_fs_type() {
    assert!(embeddable(0));
    assert!(!embeddable(0x0002));
}

#[test]
fn used_families_are_embedded_and_listed() {
    let out = export("# A\n\n`code`\n");
    let names = zip_names(&out.bytes);
    assert!(names.iter().any(|n| n.starts_with("ppt/fonts/") && n.ends_with(".fntdata")));
    let xml = part(&out.bytes, "ppt/presentation.xml");
    assert!(xml.contains("<p:embeddedFont><p:font typeface=\"Inter\"") && xml.contains("typeface=\"JetBrains Mono\""));
}

#[test]
fn unused_families_are_not_embedded() {
    let xml = part(&export("# A\n").bytes, "ppt/presentation.xml");
    assert!(!xml.contains("JetBrains Mono"));
}

#[test]
fn a_family_that_cannot_be_embedded_says_so() {
    let out = export("---\nfont: Not A Real Font\n---\n\n# A\n");
    assert!(out.notices.iter().any(|n| n.contains("Not A Real Font")));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-pptx --test fonts`
Expected: FAIL, `eot_from_ttf` not found.

- [ ] **Step 3: Port `eot.mjs`, add `embeddable_faces`, and embed as `embedFonts` does**

Each style's EOT goes to `/ppt/fonts/fontN.fntdata`, related from the presentation with `REL.font`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p slides-pptx`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/pptx crates/engine
git commit -m "PowerPoint: embed the fonts a talk uses"
```

### Task 10: Open, save and export from the window

**Files:**
- Create: `crates/app/src/export.rs`, `crates/app/tests/export.rs`
- Modify: `crates/app/src/{document,workspace_view,main,lib}.rs`, `crates/app/tests/{document_state,ui_slides}.rs`, `crates/app/Cargo.toml`

**Interfaces:**
- Consumes: Tasks 3, 4, 8.
- Produces, in `stagepack_studio`:
  - `document::OpenProblem { SlidevTalk, Other(String) }`, and `Document::open_packed(path: &Path, fonts: Fonts) -> Result<Document, OpenProblem>`
  - `Document::{layers_of(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>>, timeline_of(&mut self, index: usize) -> SlideTimeline, notes_of(&self, index: usize) -> Option<String>, is_packed(&self) -> bool, save_packed(&self, out: &Path) -> Result<Vec<String>, String>, save_folder(&self, folder: &Path) -> Result<(), String>, export_input(&self) -> ExportInput}`
  - `export::ExportInput { pub source: String, pub files: Arc<dyn TalkFiles + Send + Sync>, pub talk_fonts: Vec<Vec<u8>> }`
  - `export::ExportKind { PowerPoint, Pdf }`
  - `export::run_export(input: ExportInput, kind: ExportKind, out: &Path) -> Result<Vec<String>, String>`, blocking, returning notices
  - `export::stagepack_executable(env: impl Fn(&str) -> Option<String>, exists: impl Fn(&Path) -> bool) -> Option<PathBuf>`: `$XDG_DATA_HOME/stagepack/stagepack`, else `$HOME/.local/share/stagepack/stagepack`, and on Windows `%LOCALAPPDATA%\Stagepack\stagepack.exe`
  - `WorkspaceView` holds `Option<Entity<Document>>`
  - Actions `OpenFolder`, `OpenPacked`, `SavePacked`, `SaveFolder`, `ExportPowerPoint`, `ExportPdf`
  - `workspace_view::{FILE_MENU_ID, EXPORT_MENU_ID, STATUS_ID}`

The title bar gets *File* and *Export* dropdowns. *Save as packed talk* shows for a folder, and *Save as folder* for a packed talk. A packed talk isn't watched. `main.rs` opens a file or a folder argument. Exports run on a background task, and the status line says what happened. A Stagepack file shows `This is a Slidev talk made for Stagepack, so this app can't open it yet.`, with an *Open in Stagepack* button if `stagepack_executable` finds one, which runs `stagepack open <file>`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/app/tests/export.rs
#[test]
fn run_export_writes_a_pptx() { /* the file exists, starts with PK, and has ppt/slides/slide1.xml */ }

#[test]
fn run_export_writes_a_pdf() { /* the file starts with %PDF */ }

#[test]
fn run_export_replaces_an_existing_file() { /* the old content is gone */ }

#[cfg(unix)]
#[test]
fn a_failed_export_leaves_the_old_file_alone() {
    // The target's folder is read-only: Err with a readable message, the old file unchanged.
}

#[test]
fn stagepack_is_found_where_it_installs() { /* $XDG_DATA_HOME, then $HOME/.local/share */ }

#[test]
fn no_stagepack_means_no_offer() { /* exists is false: None */ }

// crates/app/tests/document_state.rs
#[test]
fn a_packed_talk_opens() { /* pack a folder, open_packed: same title and slide_count */ }

#[test]
fn a_stagepack_file_says_so() { /* a zip with stagepack.json gives OpenProblem::SlidevTalk */ }

#[test]
fn saving_a_packed_copy_then_opening_it_shows_the_same_talk() { /* same source */ }

#[test]
fn a_packed_talk_can_be_saved_as_a_folder() { /* slides.md appears in the folder */ }

// crates/app/tests/ui_slides.rs
#[gpui_kit::test]
fn the_title_bar_has_file_and_export(cx: &mut TestAppContext) { /* FILE_MENU_ID and EXPORT_MENU_ID are visible */ }

#[gpui_kit::test]
fn the_slide_still_changes_with_the_document_shared(cx: &mut TestAppContext) { /* right arrow still gives "T · 2 / 2" */ }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: FAIL, `export` not found.

- [ ] **Step 3: Implement it**

`git show plan-1:crates/app/src/document_view.rs` has a working dialog pattern, with `window.open_dialog`. Use `cx.prompt_for_new_path` for targets, suggesting `<title>.pptx`, `<title>.pdf` or `<title>.sptx`. `run_export` builds its own `Deck` from `ExportInput` with `Fonts::with_system_fallback()`, so nothing is shared with the window, and writes through `write_atomically`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/app
git commit -m "App: open and save packed talks, and export from the window"
```

### Task 11: What the show looks like at each moment

**Files:**
- Create: `crates/app/src/present.rs`, `crates/app/tests/present.rs`
- Modify: `crates/app/src/{slide_canvas,lib}.rs`, `crates/app/tests/ui_slides.rs`

**Interfaces:**
- Consumes: `LayerTiming`, `Transition`, `Enter`.
- Produces, in `stagepack_studio::present`, with no GPUI:
  - `pub const ENTER_SECONDS: f32 = 0.4;`, `pub const TRANSITION_SECONDS: f32 = 0.5;`, `pub const FLY_UNITS: f32 = 80.0;`
  - `pub fn ease(t: f32) -> f32`, which is `1.0 - (1.0 - t).powi(3)` with `t` clamped to 0..1
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Place { pub slide: usize, pub step: u32 }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum Moved { Revealed { step: u32 }, Arrived { from: usize, transition: Transition }, BackedUp, Stayed }`
  - `pub struct Session`, with `Session::new(steps: Vec<u32>, transitions: Vec<Transition>) -> Session`, `next(&mut self) -> Moved`, `previous(&mut self) -> Moved`, `place(&self) -> Place`, `upcoming(&self) -> Option<Place>`, and `refit(&mut self, steps: Vec<u32>, transitions: Vec<Transition>)`. After `refit`, a place past the last slide becomes the last slide at its last step, and a step past a slide's steps becomes its last step.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct LayerLook { pub visible: bool, pub opacity: f32, pub offset_y: f32 }`
  - `pub fn layer_look(timing: LayerTiming, step: u32, revealing: Option<(u32, f32)>) -> LayerLook`, where `revealing` is the step being revealed and seconds since it began, and `offset_y` is in slide units
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct SlideLook { pub opacity: f32, pub offset_x: f32 }`, `offset_x` as a fraction of the slide's width
  - `pub fn slide_looks(transition: Transition, seconds: f32) -> (SlideLook, SlideLook)`, outgoing then incoming
- Produces, on `SlideCanvas`: `set_looks(&mut self, looks: Vec<LayerLook>, cx: &mut Context<Self>)` and `set_slide_look(&mut self, look: SlideLook, cx: &mut Context<Self>)`. Without them everything is fully visible. A hidden layer keeps its image in the atlas.

- [ ] **Step 1: Write the failing tests in `crates/app/tests/present.rs`**

```rust
fn session() -> Session { Session::new(vec![2, 0, 1], vec![Transition::None, Transition::Fade, Transition::Push]) }

#[test]
fn next_reveals_each_step_then_moves_on() {
    let mut s = session();
    assert_eq!(s.next(), Moved::Revealed { step: 1 });
    assert_eq!(s.next(), Moved::Revealed { step: 2 });
    assert_eq!(s.next(), Moved::Arrived { from: 0, transition: Transition::Fade });
    assert_eq!(s.place(), Place { slide: 1, step: 0 });
}

#[test]
fn next_stops_at_the_end() {
    let mut s = Session::new(vec![0], vec![Transition::None]);
    assert_eq!(s.next(), Moved::Stayed);
}

#[test]
fn previous_steps_back_with_no_animation() {
    let mut s = session();
    s.next();
    assert_eq!(s.previous(), Moved::BackedUp);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });
    assert_eq!(s.previous(), Moved::Stayed);
}

#[test]
fn going_back_a_slide_lands_on_its_last_step() {
    let mut s = session();
    s.next(); s.next(); s.next();
    assert_eq!(s.previous(), Moved::BackedUp);
    assert_eq!(s.place(), Place { slide: 0, step: 2 });
}

#[test]
fn upcoming_is_the_next_step_or_slide() {
    let mut s = session();
    assert_eq!(s.upcoming(), Some(Place { slide: 0, step: 1 }));
    s.next(); s.next();
    assert_eq!(s.upcoming(), Some(Place { slide: 1, step: 0 }));
    s.next(); s.next(); s.next();
    assert_eq!(s.upcoming(), None);
}

#[test]
fn refit_keeps_the_show_on_a_real_place() {
    let mut s = session();
    s.next(); s.next(); s.next(); s.next();
    s.refit(vec![1], vec![Transition::None]);
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

const LATER: fn(Enter) -> LayerTiming = |enter| LayerTiming { click: Some(2), enter };

#[test]
fn a_layer_waits_for_its_click() {
    assert!(!layer_look(LATER(Enter::Fade), 1, None).visible);
    assert_eq!(layer_look(LATER(Enter::Fade), 2, None), LayerLook { visible: true, opacity: 1.0, offset_y: 0.0 });
}

#[test]
fn a_fade_rises_with_time() {
    let look = layer_look(LATER(Enter::Fade), 2, Some((2, 0.2)));
    assert!((look.opacity - ease(0.5)).abs() < 1e-4);
}

#[test]
fn fly_up_starts_below_and_lands() {
    assert_eq!(layer_look(LATER(Enter::FlyUp), 2, Some((2, 0.0))).offset_y, FLY_UNITS);
    assert_eq!(layer_look(LATER(Enter::FlyUp), 2, Some((2, 0.4))).offset_y, 0.0);
}

#[test]
fn appear_is_instant() {
    assert_eq!(layer_look(LATER(Enter::Appear), 2, Some((2, 0.0))).opacity, 1.0);
}

#[test]
fn only_the_layers_of_the_step_being_revealed_animate() {
    let earlier = LayerTiming { click: Some(1), enter: Enter::Fade };
    assert_eq!(layer_look(earlier, 2, Some((2, 0.0))).opacity, 1.0);
}

#[test]
fn a_fade_transition_brings_the_new_slide_over_the_old() {
    let (out, inc) = slide_looks(Transition::Fade, 0.25);
    assert_eq!(out, SlideLook { opacity: 1.0, offset_x: 0.0 });
    assert!((inc.opacity - ease(0.5)).abs() < 1e-4);
}

#[test]
fn a_push_slides_both_to_the_left() {
    let (out, inc) = slide_looks(Transition::Push, 0.5);
    assert_eq!((out.offset_x, inc.offset_x), (-1.0, 0.0));
    let (out, inc) = slide_looks(Transition::Push, 0.0);
    assert_eq!((out.offset_x, inc.offset_x), (0.0, 1.0));
}

#[test]
fn no_transition_is_immediate() {
    assert_eq!(slide_looks(Transition::None, 0.0).1, SlideLook { opacity: 1.0, offset_x: 0.0 });
}

// crates/app/tests/ui_slides.rs
#[gpui_kit::test]
fn hidden_layers_are_not_painted_but_keep_their_images(cx: &mut TestAppContext) {
    // set_looks with one layer hidden: images() still lists it, and has_image_atlas_entry is false until it's shown.
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test present`
Expected: FAIL, `present` not found.

- [ ] **Step 3: Implement `present.rs` and the canvas changes**

`paint_image` has no opacity of its own, but it honours the element opacity of a parent `div`. So `SlideCanvas::render` becomes a `div` holding one `.absolute().size_full().opacity(o)` child for each run of consecutive layers with the same opacity, each with its own `canvas`. `offset_y` is scaled like layer positions. `offset_x` shifts the whole slide, clipped to the window.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "Present: where the show is, and how each layer looks at each moment"
```

### Task 12: The audience and presenter windows

**Files:**
- Create: `crates/app/src/present_view.rs`, `crates/app/tests/ui_present.rs`
- Modify: `crates/app/src/{present,workspace_view,lib}.rs`, `crates/app/tests/present.rs`

**Interfaces:**
- Consumes: `Entity<Document>` and `Document::{layers_of, timeline_of, notes_of}` from Task 10; Task 11.
- Produces:
  - `present::ScreenInfo { pub id: u32, pub primary: bool }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum WindowPlan { Both { audience: u32, presenter: u32 }, AudienceOnly { screen: Option<u32> }, Rehearsal }`
  - `present::choose_windows(screens: &[ScreenInfo], rehearsing: bool) -> WindowPlan`
  - `present::clock_text(elapsed_seconds: u64) -> String`
  - `present_view::{Presentation, AudienceView, PresenterView}`, and `present_view::start_show(document: Entity<Document>, rehearsing: bool, cx: &mut App) -> Entity<Presentation>`
  - `present_view::{CURRENT_ID, NEXT_ID, NOTES_ID, TIMER_ID, COUNTER_ID}`
  - Actions `Present` (F5) and `Rehearse` (Shift+F5), and `workspace_view::PRESENT_ID`

During a show, Right, PageDown, Space, Enter and a click on the audience window mean next. Left, PageUp and Backspace mean previous. Escape ends it. The audience window is full screen, black behind the slide, and presenter view holds the current slide at its current step, the upcoming place (or `End of the talk`), the notes, `mm:ss` from the start of the show, and `3 / 7`. *Rehearse* opens both as ordinary windows on the current screen. A save during a show calls `Session::refit` and redraws.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/app/tests/present.rs
#[test]
fn one_screen_gives_one_full_screen_window() {
    assert_eq!(choose_windows(&[ScreenInfo { id: 1, primary: true }], false), WindowPlan::AudienceOnly { screen: Some(1) });
}

#[test]
fn two_screens_put_the_audience_on_the_other_one() {
    let screens = [ScreenInfo { id: 1, primary: true }, ScreenInfo { id: 2, primary: false }];
    assert_eq!(choose_windows(&screens, false), WindowPlan::Both { audience: 2, presenter: 1 });
}

#[test]
fn no_display_information_still_opens_a_window() {
    assert_eq!(choose_windows(&[], false), WindowPlan::AudienceOnly { screen: None });
}

#[test]
fn rehearsing_opens_ordinary_windows() {
    assert_eq!(choose_windows(&[ScreenInfo { id: 1, primary: true }], true), WindowPlan::Rehearsal);
}

#[test]
fn the_clock_reads_minutes_and_seconds() {
    assert_eq!(clock_text(0), "00:00");
    assert_eq!(clock_text(65), "01:05");
    assert_eq!(clock_text(3725), "62:05");
}

// crates/app/tests/ui_present.rs: start a show with rehearsing true on the test platform.
#[gpui_kit::test]
fn right_reveals_then_moves_on(cx: &mut TestAppContext) { /* COUNTER_ID reads 1 / 2 through both clicks, then 2 / 2 */ }

#[gpui_kit::test]
fn every_next_key_works(cx: &mut TestAppContext) { /* right, pagedown, space and enter each advance */ }

#[gpui_kit::test]
fn the_presenter_view_shows_notes_and_next(cx: &mut TestAppContext) { /* NOTES_ID has the note, NEXT_ID is visible */ }

#[gpui_kit::test]
fn the_last_slide_says_the_talk_is_over(cx: &mut TestAppContext) { /* NEXT_ID reads End of the talk */ }

#[gpui_kit::test]
fn the_timer_counts_from_the_start(cx: &mut TestAppContext) { /* advance the clock 65 s: TIMER_ID reads 01:05 */ }

#[gpui_kit::test]
fn an_animation_stops_asking_for_frames_when_it_ends(cx: &mut TestAppContext) { /* after 0.4 s the layer's look is settled and no frame is requested */ }

#[gpui_kit::test]
fn a_transition_releases_the_old_slide(cx: &mut TestAppContext) { /* after 0.5 s the outgoing canvas is gone */ }

#[gpui_kit::test]
fn a_save_during_the_show_updates_it(cx: &mut TestAppContext) { /* reload a changed note: NOTES_ID shows it */ }

#[gpui_kit::test]
fn a_save_that_removes_slides_keeps_the_show_on_a_real_slide(cx: &mut TestAppContext) { /* on 3 / 3, reload to one slide: COUNTER_ID reads 1 / 1 */ }

#[gpui_kit::test]
fn escape_closes_the_show_and_leaves_the_editor(cx: &mut TestAppContext) { /* the show's windows are gone, the workspace remains */ }

#[gpui_kit::test]
fn f5_starts_a_show(cx: &mut TestAppContext) { /* a new window opens */ }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: FAIL, `choose_windows` not found.

- [ ] **Step 3: Implement it**

`Presentation` is an entity both windows observe. It uses `cx.background_executor().now()` for time, so tests can advance the clock, and `window.request_animation_frame()` only while an animation runs. `Moved::Arrived` keeps the outgoing `SlideCanvas` until the transition ends. A full-screen window uses `WindowBounds::Fullscreen(display.bounds())` and `display_id`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "Present: the audience and presenter windows"
```

### Task 13: Check it for real, and record what ran

**Files:**
- Create: `docs/checks/plan-b.md`, `fixtures/show/slides.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: everything above.

`fixtures/show` is a three-slide talk using a `fade` and a `fly-up` entrance, `click=each`, a `fade` and a `push` transition, and notes.

- [ ] **Step 1: Run every test**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: all pass, with no warnings.

- [ ] **Step 2: Drive the real app on a nested display**

With `Xephyr` and XTest key events, as in `docs/checks/plan-a.md`: open `fixtures/show`, press Shift+F5, screenshot both windows, press Right through every click and slide, and screenshot mid-way. Then check that the export files open: `pdftotext` on the PDF, and LibreOffice on the `.pptx`.
Expected: layers appear click by click, and the presenter view shows the notes, next slide, timer and counter.

- [ ] **Step 3: Write `docs/checks/plan-b.md`, and update `README.md`**

Record the test counts you measured and what the real-window run showed, and say what wasn't exercised. Add the owner's optional Windows check, by hand on their Windows PC, in the form `docs/checks/plan-a.md` uses, covering: presenting on two screens, and opening an exported `.pptx` in desktop PowerPoint, with its animations, fonts and shapes. Say plainly that animated export has not been checked in PowerPoint itself.

- [ ] **Step 4: Commit**

```bash
git add docs/checks/plan-b.md fixtures/show README.md
git commit -m "Plan B check: what ran, and how to check Windows and PowerPoint"
```
