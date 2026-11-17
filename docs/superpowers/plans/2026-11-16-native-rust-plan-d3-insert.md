# Plan D3: Insert Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put things on a slide from the Insert tab: pictures (from a file, dropped from the desktop, pasted), shapes and arrows with fill, outline and text inside, text boxes, icons from a picker, tables with typing in cells, and components from a palette, each set up in a form. A thing on the slide can be selected, deleted, copied and pasted, also into another talk with the files it uses.

**Architecture:** Everything inserted is a tag or a Markdown table written into `slides.md` by a byte-range `Change`, through the same `Document` history as every other edit. A file added to a talk (a picture, a CSV) lives in the window's working copy until Save and is shown to the engine through an overlay over the talk's files, so undo, crash recovery and both saved forms work the same way. Selecting comes from the engine's layout, which already knows each layer's source range. One inspector panel builds a form from a tag's component schema, so pictures, shapes, icons, charts and a talk's own components are all set up the same way.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 with gpui-fast 0.1.13 (`ExternalPaths`, `ClipboardEntry::Image`, `prompt_for_paths`), gpui-component widgets (`Input`, `NumberInput`, `Select`, `Checkbox`, `ColorPicker`, `Popover`, `v_virtual_list`), the `image` crate already in the engine, proptest, and the existing crates.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`: goals 3, 4, 6 and the element part of 9; "Layouts and moving things" (things inserted from the ribbon are positioned tags); "Custom components" (forms for non-coders); "Charts"; "The editor" (the clipboard carries the Markdown, the assets it uses, and any custom component's source and compiled file); and the anti-goals *Rewriting the user's text*, *Editing the wrong thing*, *Code in the way*, *Losing work* and *A talk reaching your computer*. `docs/superpowers/plans/2026-11-12-native-rust-plan-d2-decisions.md` has D2's rulings and what it left for this plan.

## Global Constraints

- Work on the branch `plan-d3`, cut from `main`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx` and `slides-components` don't depend on GPUI.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **A visual edit changes only the bytes of what was edited.** Inserting adds its own lines and at most the blank lines that keep it apart. Deleting removes the thing's own lines. Changing a setting changes that attribute's bytes. No other byte of the talk changes, on any slide. Property-tested for every operation.
- **What's written reads back as what was meant.** Every tag and table the app writes parses to exactly the element intended, whatever text a person typed into a field: quotes, `<`, `>`, `&`, `{`, new lines, `---`, backticks.
- **If the app can't be sure what a click maps to, it refuses and says so.**
- **Undo is exact.** One insert, one delete, one field committed, one run of typing in a cell: one undo step each.
- A talk is untrusted input, and so is any file, clipboard content or dropped path. Nothing in one may crash the app or make it read or write outside the talk and the app's own data folder. A panic during an edit becomes a refused edit.
- **Nothing is compiled by inserting or pasting.** The build permission rules stay as they are.
- Nobody is made to see Markdown, a tag name with angle brackets, a path with `assets/`, or Rust. Every label and message is plain words.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless GPUI test.
- An idle window does no work.
- A keystroke in a table cell on a 500-slide talk costs under 8 ms at the median before drawing, in the release timing test.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: moving, resizing and rotating with the mouse, align, distribute, order, snapping and the Animate tab (D4); templates (D5); cropping, grouping, video and audio; merging table cells; a table's column widths and cell colours; links.

Rulings made in the plan:
- **Where a new thing goes.** With nothing to drag yet, a new thing is put in the middle of the slide at a fixed size, and 40 units down and right of that for each thing already exactly there, up to ten times. D4 moves it. Its place can be typed in the inspector's *Place* fields until then.
- **A picture is written as `<Picture src="…" x y w h />`**, never as `![]()`: a positioned tag moves on its own. A hand-written `![]()` can be selected and deleted, and shows its file in the inspector, read-only.
- **A table is plain Markdown in the slide's main region**, at the end of what's there. It isn't positioned: Markdown tables can't sit in a tag's attributes, and a table in a `<Box>` is something a person can write by hand.
- **Files added to a talk are named for what they were**, made safe (`[a-z0-9-]`, 40 characters at most, then the extension), with `-2`, `-3` and so on if the name is taken by different bytes. The same bytes added twice use the file already there. A pasted picture is `pasted-picture.png`.
- **A file is saved only if the saved text uses it.** Files added and then undone stay in the working copy for the session, so Redo works, and aren't written by Save.
- **One added file is at most 32 MiB.** A picture must be one the engine can read (`png`, `jpg`, `jpeg`, `webp`, `svg`); a pasted or dropped picture of another kind the system gives (`bmp`, `tiff`, `gif`) is written as PNG.
- **A thing is selected by one click and typed in by a second.** A click on a box or shape that holds text selects it; a click on its text while it's selected, or a double-click, puts the caret there. A click on a region's text still puts the caret there at once, as in D2: regions are selected in D4.
- **`<Shape>`'s `kind` becomes a choice** of `rect`, `rounded`, `ellipse`, `line`, `arrow`, `callout`. Any other value still draws a rectangle, as before, and the form shows it as *Rectangle* without writing anything.

## Review Focus

- **Text typed into a form field or a table cell that means something to Markdown or to a tag:** `"`, `'` and both together, `<`, `/>`, `{`, `|`, a new line, `---`, a backtick, `&quot;`, a NUL. It stays that text, and the slide and every other slide stay whole. Owned by Task 2's `any_value_reads_back_as_typed` and Task 9's `any_cell_text_reads_back_as_typed`.
- **Files that aren't what they say:** a `.png` holding text, a 20,000 by 20,000 picture, an SVG with a script or an outside link, a zero-byte file, a file that vanishes between the dialog and the read, a symlink, a folder dropped as if it were a file, a name of 300 characters or `../../x.png` or `CON.png`. Nothing is added, a sentence says why, and nothing is read or written outside the talk. Owned by Task 4's `a_file_that_cannot_be_used_adds_nothing` and `no_name_leaves_the_talk`.
- **Hand-written elements under the selection:** a tag over several lines, with single-quoted or unquoted or repeated attributes, with a trailing comment-like text, inside a list item or a `<Box>`, two on one line, or with the same range start as its region. Selecting, changing one setting and deleting touch only that tag's bytes. Owned by Task 2's `a_hand_written_tag_keeps_its_other_bytes` and Task 5's `deleting_a_nested_element_leaves_its_parent`.
- **Hand-written tables:** no leading or trailing pipes, an alignment row with colons, cells with escaped pipes, inline code holding a pipe, uneven rows, a table straight after a paragraph or inside a list, CRLF. Typing in one cell changes that cell's bytes only, and adding a row or column keeps every other cell's bytes. Owned by Task 9's `typing_in_a_cell_changes_only_that_cell` and `a_new_row_keeps_every_other_byte`.
- **Saving, closing, crashing or pasting with a file not yet saved:** a picture added and never saved comes back after a crash; Save As to the other form carries it; a packed talk gains it; a save that fails half-way leaves the old talk whole and the picture still pending. Owned by Task 4's `an_added_file_survives_each_way_of_saving` and `a_crash_keeps_an_added_file`.

---

### Task 1: Groundwork left by D2

**Files:**
- Modify: `crates/format/src/{split,parse,rich_write}.rs`
- Test: `crates/format/tests/{rich_write,rich_read,parse}.rs`, `crates/format/tests/common/mod.rs`, `crates/app/tests/typing_bytes.rs`

**Interfaces:**
- No signature changes.
- `split.rs`: a line opens a code fence only as CommonMark says: three or more backticks with no backtick later on the line, or three or more tildes. So `` ```a```b `` is a paragraph, and a `---` after it starts a slide.
- `parse.rs` and `rich_write.rs`: a list item whose whole text is `&nbsp;` and that has items nested under it reads as an item holding one no-break space, as it did before the empty-line spelling changed, so its list can be typed in. An item that is `&nbsp;` with nothing under it still reads as empty.
- `rich_write.rs`, the whole-flow write (`whole_change`): a paragraph or item that the edit didn't change is written with its own old bytes, indentation and marker included, wherever its old bytes still read back as the same item at the same level in the new flow. Only where they wouldn't (its level changed, or its parent went away) is it written afresh.

- [ ] **Step 1: Write the failing tests**

- `parse.rs`: `a_line_of_backticks_with_more_backticks_is_not_a_fence`: `` "```a```b\n- \n---\na" `` is two slides. `rich_read.rs`: `reading_odd_text_never_panics` passes at `PROPTEST_CASES=4000`.
- `rich_read.rs`: `a_no_break_space_item_with_items_under_it_can_be_typed_in`: in `"# T\n\n- &nbsp;\n  - b\n- c\n"`, typing `x` at the end of `b` gives `"# T\n\n- &nbsp;\n  - bx\n- c\n"`.
- `rich_write.rs`, property `a_structural_edit_keeps_untouched_items_bytes`, over `gen_talk()` with the hand-indented shapes D2 added: apply one of set kind, toggle list, indent, outdent, join by Backspace, split by Enter, paste of two paragraphs, to one paragraph; every other paragraph of the flow whose kind and level are the same afterwards keeps its exact old line bytes. Named case: `"- a\n    - b\n        - c\n- d\n"`, emptying `c` leaves `- a\n    - b\n` and `- d\n` byte-exact.
- `typing_bytes.rs`: add the structural operations to the table, with `assert_eq!(bad, 0)`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, then `PROPTEST_CASES=4000 scripts/cargo-safe.sh test -p slides-format --test rich_write --test rich_read` three times.
Expected: PASS each time. If the third bullet can't hold for an operation, say exactly which and why in the report; don't weaken the property without saying.

- [ ] **Step 5: Commit**

```bash
git add crates/format crates/app/tests/typing_bytes.rs
git commit -m "Format: a list written again keeps its untouched items as they were"
```

---

### Task 2: Element operations in the format crate

**Files:**
- Create: `crates/format/src/element_ops.rs`, `crates/format/tests/element_ops.rs`
- Modify: `crates/format/src/{lib,edit}.rs`

**Interfaces:**
- Produces, in `slides_format`:
  - `pub struct NewTag<'a> { pub name: &'a str, pub attrs: &'a [(&'a str, &'a str)], pub body: Option<&'a str> }`. `body` is Markdown for a tag with children; `None` writes a self-closing tag.
  - `pub fn tag_text(tag: &NewTag) -> Option<String>`: the tag as it will be written, on one line when self-closing, or opening tag, blank line, body, blank line, closing tag. Values are quoted with `"`, or with `'` when the value holds `"` and no `'`; a value holding both, a new line, or a control character is refused (`None`). A name that isn't `[A-Z][A-Za-z0-9]*` or an attribute name that isn't `[a-z][a-z0-9-]*` is refused.
  - `pub fn insert_block(talk: &Talk, slide: usize, text: &str) -> Option<Change>`: adds `text` as a block of its own at the end of the slide's content: after the last block, before `<Notes>` if there are notes, with exactly one blank line before it and after it where something follows, in the slide's own line ending. On a slide of only settings, or with nothing at all, it goes where typed text would (as `write_into_region` does). `None` when the result wouldn't read back as the old blocks followed by the new one, unchanged.
  - `pub fn insert_block_in_region(talk: &Talk, slide: usize, region: &str, text: &str) -> Option<Change>`: the same at the end of a named region's content, writing the `<Region name="…" />` line first when the region has none.
  - `pub fn delete_block(talk: &Talk, slide: usize, range: &Range<usize>) -> Option<Change>`: removes the block or tag with exactly that range, at any depth, with its line and one of the blank lines around it. `None` if no block has that range, or if what's left wouldn't read back as the old blocks without it (for example where the removed tag was all that kept two lists apart).
  - `pub fn find_tag<'a>(talk: &'a Talk, slide: usize, range: &Range<usize>) -> Option<&'a Tag>`, at any depth.
  - `pub fn set_tag_attr(talk: &Talk, slide: usize, tag: &Range<usize>, name: &str, value: Option<&str>) -> Option<Change>`: sets the attribute (through `set_attr`, with `tag_text`'s quoting rules), or with `None` removes it with one space before it. Removing an attribute that isn't there is `Some(empty)`. Where an attribute is written twice, the one the parser uses is the one changed.
  - `pub fn block_text(talk: &Talk, slide: usize, range: &Range<usize>) -> Option<String>`: the block's own bytes, for copying, with `\n` line endings.
- All of them are wrapped against panics as `write_flow` is.

- [ ] **Step 1: Write the failing tests**

`element_ops.rs`, property tests over `gen_talk()` (extend `block()` with tags: self-closing and with children, multi-line, single-quoted, unquoted and repeated attributes, two tags on following lines, a tag inside a list item and inside a `<Box>`, a `![]()` picture, a table):
- `inserting_adds_only_its_own_lines`: every byte of the old source is in the new one in order, what's added is `text` and line endings only, every other slide's bytes are the same, and the slide's blocks are the old ones plus one.
- `deleting_then_undoing_is_exact`, and `deleting_removes_only_its_own_lines`.
- `any_value_reads_back_as_typed`: for any string from a generator weighted to `"`, `'`, `<`, `/>`, `{`, `}`, `|`, `&quot;`, `---`, backticks, `\`, spaces at the ends, emoji: `set_tag_attr` either returns `None` or the reparsed tag's `attr(name)` equals the string, and every other attribute and every byte outside the tag is unchanged.
- `a_hand_written_tag_keeps_its_other_bytes`: setting or removing one attribute on each hand-written shape changes only that attribute's bytes.
- Named: `insert_into_an_empty_slide`, `insert_before_notes`, `insert_after_a_list_stays_out_of_the_list` (the new tag isn't read as part of the last item), `insert_into_the_right_column_writes_its_marker`, `deleting_the_tag_between_two_lists_is_refused`, `a_value_with_both_quotes_is_refused`, `crlf_talks_keep_crlf`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test element_ops`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-format`, and `PROPTEST_CASES=4000 scripts/cargo-safe.sh test -p slides-format --test element_ops`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/format
git commit -m "Format: a block can be added to a slide, taken away, and have a setting changed"
```

---

### Task 3: The engine says what's under the pointer, and lists its icons

**Files:**
- Modify: `crates/engine/src/{text_geometry,deck,pictures,builtins,lib}.rs`, `crates/toolkit/src/element.rs` only if `ShapeKind` needs `Prop`
- Test: `crates/engine/tests/{text_geometry,builtins,deck}.rs`

**Interfaces:**
- Produces, in `slides_engine`:
  - `pub struct ElementBox { pub source: Range<usize>, pub rect: Rect, pub rotate: f32, pub holds_text: bool }`: a selectable thing: its tag's or block's whole source range, its drawn rectangle in slide units before turning, its turn about the rectangle's centre, and whether there's an editable text flow inside it.
  - `Hit` gains `Element(ElementBox)`. `TextLayout::hit` gives it for a point on a positioned tag, a picture (tag or `![]()`), an icon, a shape, a component or a table, in place of `Uneditable`. For a point on editable text inside a positioned tag it still gives `Hit::Text`. `Uneditable` stays for code, quotes and text mixed with tags.
  - `pub fn element_at(&self, x: f32, y: f32) -> Option<ElementBox>` on `TextLayout`: the topmost selectable thing under the point, whether or not the point is on its text. The last drawn wins. A turned element is hit in its turned shape.
  - `pub fn element(&self, source: &Range<usize>) -> Option<ElementBox>`: the same for a known range, for drawing the selection after an edit.
  - `pub fn icon_names() -> &'static [&'static str]`, sorted, and `pub fn icon_preview(name: &str, size: u32, color: [u8; 4]) -> Option<tiny_skia::Pixmap>`.
  - `Shape`'s `kind` argument is a choice: its schema field is `PropKind::Choice(["rect","rounded","ellipse","line","arrow","callout"])`. An unknown value in a talk still draws a rectangle and gives no notice it didn't give before.
  - `Deck::default_size(&mut self, tag: &str, attrs: &[(&str, &str)]) -> (f32, f32)`: the size a new tag of that kind should be given: a picture's own pixels scaled to fit 960 by 540 without growing, an icon its `size` squared, anything else 640 by 360.
- Golden images don't change.

- [ ] **Step 1: Write the failing tests**

- `text_geometry.rs`: `a_click_on_each_kind_of_thing_gives_its_source` over `fixtures/showcase` (box, ellipse, arrow, icon, picture, table) and `fixtures/components` (chart, a talk's own component): `element_at` at the centre of each returns the range whose text starts with that tag or the table's first line; `a_point_in_a_turned_box_follows_the_turn`; `the_last_drawn_wins_where_two_overlap`; `text_in_a_box_is_still_text` (`hit` gives `Text`, `element_at` gives the box with `holds_text`); `a_point_on_nothing_gives_nothing`; property `every_element_is_found_at_its_centre_and_nowhere_outside_its_rect` over the generator's talks; `geometry_never_panics_on_any_point` including NaN and infinities.
- `builtins.rs`: `a_shapes_kind_is_a_choice`, `an_unknown_kind_still_draws_a_rectangle` (same layers as before the change, compared with a saved spec), `every_listed_icon_draws` (all names give a pixmap with some ink), `icon_names_are_sorted_and_2776`.
- `deck.rs`: `a_new_pictures_size_fits_and_keeps_its_shape` (4000×1000 → 960×240; 100×50 → 100×50; unreadable → 640×360).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test text_geometry`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS, goldens unchanged. App code that matched on `Hit` treats `Element` as it treated `Uneditable` for now, with the sentence it gave before.

- [ ] **Step 5: Commit**

```bash
git add crates/engine crates/toolkit crates/app
git commit -m "Engine: a point on the slide gives the thing there and where it's written"
```

---

### Task 4: Files added to a talk

**Files:**
- Create: `crates/app/src/added_files.rs`, `crates/app/tests/added_files.rs`
- Modify: `crates/app/src/{document,saving,recovery,working_copies,lib}.rs`, `crates/engine/src/{files,packed}.rs`
- Test: `crates/app/tests/{saving,recovery}.rs`, `crates/engine/tests/{files,packed}.rs`

**Interfaces:**
- Produces, in `slides_engine`: `pub struct WithAdded { .. }` implementing `TalkFiles`: `WithAdded::new(under: Arc<dyn TalkFiles + Send + Sync>, added: Arc<AddedSet>)`, reading an added file first. `pub struct AddedSet` is a shared, append-only map of path to bytes with `get`, `names_in`, and a stamp that changes when a file is added.
- Produces, in `packed.rs`: `repack_with_slides` gains `extra: &[(String, Vec<u8>)]`, written as new entries; an entry already in the pack with the same name and other bytes is an error, not an overwrite.
- Produces, in the app, `added_files`:
  - `pub const MAX_ADDED_BYTES: usize = 32 * 1024 * 1024`
  - `pub enum FileKind { Picture, Data, ComponentSource, ComponentBuild }`, with its folder (`assets`, `assets`, `components`, `build`) and allowed extensions (`png jpg jpeg webp svg`; `csv`; `rs`; `wasm`).
  - `pub enum NotAdded { TooLarge, NotAPicture, NotThatKind, Unreadable, NameTaken }`, whose `Display` is the sentence shown: "That file is larger than 32 MB, so it wasn't added.", "That file isn't a picture this app can show.", "That kind of file can't be used here.", "That file couldn't be read.", "This talk already has a different file with that name."
  - `pub fn safe_name(suggested: &str, ext: &str) -> String`: lower-case, anything outside `[a-z0-9]` to `-`, runs of `-` closed up and trimmed, at most 40 characters, `file` if nothing is left, never a Windows device name (`con`, `prn`, `aux`, `nul`, `com1`–`com9`, `lpt1`–`lpt9` get `-file` added).
  - `pub fn picture_bytes(bytes: Vec<u8>, hint_ext: Option<&str>) -> Result<(Vec<u8>, &'static str), NotAdded>`: checks the bytes are a picture the engine reads and returns them with their real extension, from the content, not the name; `bmp`, `tiff` and `gif` content is written again as PNG; an SVG is kept as it is (the engine's reader already ignores scripts and outside links).
- Produces, on `Document`:
  - `pub fn add_file(&mut self, kind: FileKind, suggested: &str, bytes: Vec<u8>) -> Result<String, NotAdded>`: returns the path to write in the talk, such as `assets/photo.png`. The same bytes as a file the talk has, saved or added, return that file's path. A taken name with other bytes gets `-2` up to `-99`, then `NameTaken`. `ComponentSource` and `ComponentBuild` never rename: a clash is `NameTaken`.
  - `pub fn read_outside_file(path: &Path) -> Result<Vec<u8>, NotAdded>` (an associated function): refuses a folder, a symlink to anything but a regular file, and anything over the limit, checked on the open handle's length before reading and again while reading.
  - `pub fn added_files(&self) -> Vec<(String, Arc<[u8]>)>`: those the current text uses, for a save.
- Saving: a folder save writes each used added file (temp file, sync, rename, never over a different file) before `slides.md`; a packed save passes them as `extra`; Save As to either form carries them. After a save that wrote them, they're part of the talk and no longer pending. A failed save leaves them pending.
- Working copies: a slot keeps `files/<path>` beside `slides.md`, written when the file is added; recovery brings them back with the text; letting a slot go removes them.

- [ ] **Step 1: Write the failing tests**

- `added_files.rs`: `a_file_that_cannot_be_used_adds_nothing` (table: text in a `.png`, zero bytes, 20,000×20,000 PNG header, 33 MiB, a folder, a dangling symlink, a symlink to a folder, a path that doesn't exist; each gives its `NotAdded`, no file, no change to the talk); `no_name_leaves_the_talk` (property: for any suggested name, including `../../x`, `C:\x`, `con`, 300 characters, empty, only dots, non-ASCII, the returned path is `assets/[a-z0-9-]{1,40}\.[a-z]+` and `FolderFiles` accepts it); `the_same_bytes_twice_are_one_file`; `a_taken_name_gets_a_number`; `a_bmp_becomes_a_png`; `an_svg_with_a_script_is_kept_and_draws_without_it`; `the_engine_sees_an_added_file_at_once` (a slide using it draws it, no notice).
- `an_added_file_survives_each_way_of_saving` (table: folder Save, packed Save, Save As folder→packed, packed→folder, folder→folder: afterwards the talk opened fresh from disk draws the picture, and the file's bytes are exact); `a_file_added_and_undone_is_not_saved`; `redo_after_undo_still_has_the_file`; `a_failed_save_keeps_the_file_pending_and_the_old_talk_whole`; `a_packed_save_never_replaces_a_different_entry`.
- `recovery.rs`: `a_crash_keeps_an_added_file` (add, type, drop the document without saving, recover: the picture draws); `letting_go_removes_kept_files`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test added_files`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/engine
git commit -m "App: a file added to a talk is kept with its unsaved changes and saved with it"
```

---

### Task 5: Selecting a thing, and taking it away

**Files:**
- Create: `crates/app/src/selection.rs`, `crates/app/tests/ui_select.rs`
- Modify: `crates/app/src/{document,slide_editor,slide_canvas,workspace_view,lib}.rs`, `crates/app/src/workspace_view/editing.rs`

**Interfaces:**
- Consumes: `TextLayout::{hit, element_at, element}`, `ElementBox`, `delete_block`, `find_tag`.
- Produces, on `Document`:
  - `pub fn selected(&self) -> Option<&Selected>`, `pub struct Selected { pub slide: usize, pub source: Range<usize> }`.
  - `pub fn select(&mut self, slide: usize, source: Range<usize>) -> bool` (false if nothing has that range), `pub fn deselect(&mut self)`.
  - `pub fn delete_selected(&mut self) -> Result<Vec<usize>, EditRefused>`: one undo step labelled "Delete".
  - `pub fn selected_tag(&self) -> Option<&Tag>` and `pub fn selected_box(&mut self) -> Option<ElementBox>`.
  - The selection follows its element through every edit: after any change it's found again by where its range moved to; if its bytes were removed (a delete, an undo of its insert, an outside change) the selection is dropped. Undo of a delete selects the thing again; undo of an insert drops it. Selecting ends text editing, and beginning text editing elsewhere drops the selection.
- `EditRefused` gains `CouldNotBeRemoved`: "That couldn't be taken away without changing what's around it."
- In the slide editor: a click where `hit` gives `Element` selects it. A click on text inside a positioned tag selects the tag when it isn't selected, and puts the caret in the text when it is; a double-click puts the caret there at once. A click on nothing or on region text drops the selection. Escape while typing in a selected tag's text ends the typing and keeps the tag selected; Escape again drops the selection. Delete and Backspace with a thing selected and no caret take it away. Tab and Shift+Tab with a thing selected move the selection to the next and previous thing on the slide in source order.
- Drawn by GPUI over the slide: a 2-pixel accent outline on the selected thing's rectangle, turned with it. No handles yet.
- Ids: `SELECTION_OUTLINE_ID`. The status line says what's selected in plain words: "Picture selected", "Shape selected", "Icon selected", "Table selected", "Chart selected", "Text box selected", or the component's tag name with spaces before capitals and " selected".

- [ ] **Step 1: Write the failing tests**

`ui_select.rs`, on `fixtures/showcase` and small sources: `a_click_selects_and_escape_lets_go`; `delete_takes_away_only_the_selected_thing` (source equals the old source without the tag's lines; other slides byte-exact); `undo_brings_it_back_selected`; `a_second_click_on_a_boxes_text_types_in_it`; `a_double_click_types_at_once`; `escape_from_typing_keeps_the_box_selected`; `typing_elsewhere_drops_the_selection`; `tab_goes_to_the_next_thing`; `deleting_a_nested_element_leaves_its_parent` (an icon inside a `<Box>`); `a_thing_that_cannot_be_removed_says_so` (the tag between two lists); `an_outside_change_that_removes_the_thing_drops_the_selection`; `the_outline_is_where_the_thing_is_drawn` (within a pixel, also for a turned box, at two window sizes); `delete_while_typing_deletes_a_letter_not_the_box`; `a_selected_thing_survives_typing_on_another_slide_and_coming_back`; `nothing_redraws_while_a_thing_sits_selected`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_select`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS. D2's `clicking_a_table_says_it_cannot_be_edited_yet` and its like change to expect the selection, and say so in the report.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a thing on the slide can be selected and taken away"
```

---

### Task 6: The inspector: a form for the selected thing

**Files:**
- Create: `crates/app/src/inspector.rs`, `crates/app/src/inspector/{fields,labels}.rs`, `crates/app/tests/ui_inspector.rs`
- Modify: `crates/app/src/{document,workspace_view,lib}.rs`

**Interfaces:**
- Consumes: `Deck::component_schema`, `ComponentSchema`, `PropField`, `PropKind`, `set_tag_attr`, `Document::add_file`, `Selected`.
- Produces, on `Document`:
  - `pub fn schema_of_selected(&mut self) -> Option<ComponentSchema>`.
  - `pub fn set_selected_attr(&mut self, name: &str, value: Option<&str>) -> Result<Vec<usize>, EditRefused>`: one undo step labelled "Change setting". Setting a value equal to the one there writes nothing and makes no step.
- Produces `inspector::Inspector`, a panel at the window's right, 300 pixels wide, shown only while a thing is selected:
  - A heading naming the thing as the status line does.
  - One field per schema argument, in schema order, labelled from the argument's name: `_` and `-` to spaces, first letter upper-case (`outline_width` → "Outline width"). Built-ins have their labels and order in `labels.rs`: for a shape *Shape*, *Fill*, *Outline*, *Outline width*; for a box *Fill*, *Outline*, *Outline width*, *Corner radius*, *Padding*; for an icon *Icon*, *Size*, *Colour*; for a picture *Picture*; for a chart *Kind*, *Title*, *Labels*, *Values*, *Data file*, *Build up by clicks*.
  - By kind: `Text` an `Input`; `Whole` and `Number` a `NumberInput` (`Whole` refuses a fraction); `Flag` a `Checkbox`; `Color` a `ColorPicker` with a *None* button when the argument is optional; `Choice` a `Select` of its options, shown with first letter upper-case and `-` as spaces; `File` a button *Choose file…* with the file's name beside it, opening the system dialog filtered to its extensions and adding the file with `add_file`.
  - A *Place* group for a positioned tag: *Left*, *Top*, *Width*, *Height*, *Turn*, as `NumberInput`s writing `x`, `y`, `w`, `h`, `rotate`, held to the engine's limits (0–7680, 1–3840, −360–360).
  - A field is committed on Enter, on leaving the field, or at once for a checkbox, a choice, a colour from the palette and a file. A colour dragged on the sliders is committed when the picker closes. Escape in a field puts back the value from the talk.
  - An optional field emptied removes the attribute. A required text field emptied writes `name=""`.
  - A value the writer refuses (both kinds of quote, a new line) leaves the field showing what was typed, with the sentence "That can't hold both kinds of quote mark or a new line." under it, and writes nothing.
  - For a talk's own component with no working build, the panel says "Build this talk's components to set this up." and shows no fields. For a tag the talk doesn't know, "This talk has no component of that name." For a `![]()` picture, its file's name, read-only. For a table, the row and column tools of Task 10 will sit here; until then, its size as "3 columns, 4 rows".
  - Every field shows the talk's current value after any edit, undo or outside change, unless that field has the keyboard.
- Ids: `INSPECTOR_ID`, `inspector::field_id(name) -> String` (`"inspector-field-<name>"`), `INSPECTOR_CHOOSE_FILE_ID`.

- [ ] **Step 1: Write the failing tests**

`ui_inspector.rs`: `the_panel_shows_only_with_a_selection`; `a_shape_shows_its_fields_with_its_values`; `changing_a_fill_writes_only_that_attribute`; `emptying_an_optional_field_removes_it`; `a_number_out_of_range_is_held_to_the_range`; `a_fraction_in_a_whole_number_is_refused`; `a_choice_writes_its_kebab_name`; `a_checkbox_writes_at_once_and_undoes_in_one_step`; `escape_puts_the_old_value_back`; `a_value_with_both_quotes_says_so_and_writes_nothing`; `anything_typed_in_a_text_field_reads_back_or_is_refused` (the Review Focus strings through the real field); `undo_updates_the_field`; `a_field_with_the_keyboard_is_not_overwritten_by_an_outside_change`; `a_component_with_no_build_asks_for_a_build`; `an_unknown_tag_says_so`; `place_fields_move_the_thing` (the outline follows); `a_chart_shows_plain_labels`; `the_same_value_makes_no_undo_step`; `choosing_a_data_file_adds_it_and_writes_its_path` (through a test seam for the dialog: `Inspector::chosen_file(path)`).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_inspector`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: the selected thing is set up in a form made from its arguments"
```

---

### Task 7: The Insert tab: pictures, shapes, text boxes

**Files:**
- Create: `crates/app/src/insert.rs`, `crates/app/src/insert_tools.rs`, `crates/app/tests/ui_insert.rs`
- Modify: `crates/app/src/{document,ribbon,slide_editor,workspace_view,lib}.rs`, `crates/app/assets` icon list if new tool icons are needed

**Interfaces:**
- Consumes: `insert_block`, `tag_text`, `NewTag`, `Deck::default_size`, `Document::{add_file, read_outside_file, select}`, `picture_bytes`.
- Produces `insert` (pure, no GPUI): `pub fn place_for(size: (f32, f32), taken: &[(f32, f32)]) -> (f32, f32)`: the top-left that centres `size` on 1920 by 1080, moved 40 down and right for each time that exact place is in `taken`, at most ten times, and held inside the slide.
- Produces, on `Document`:
  - `pub fn insert_tag(&mut self, label: &'static str, name: &str, attrs: &[(&str, &str)], body: Option<&str>) -> Result<Vec<usize>, EditRefused>`: writes the tag on the current slide with `x`, `y`, `w`, `h` from `default_size` and `place_for`, as whole numbers, selects it, one undo step.
  - `pub fn insert_picture(&mut self, suggested: &str, bytes: Vec<u8>) -> Result<(), InsertFailed>`, `pub enum InsertFailed { File(NotAdded), Edit(EditRefused) }`.
- The Insert tab, in place of "on the way": a *Pictures* group (*Picture*, opening the system dialog for one or more pictures); a *Shapes* group (*Shapes*, a dropdown of *Rectangle*, *Rounded rectangle*, *Ellipse*, *Line*, *Arrow*, *Callout*, each with its icon; and *Text box*); and room for Tasks 8 to 11.
  - A shape is written `<Shape kind="…" fill="…" x y w h />`, 480 by 320, with the theme's accent as `fill` for the closed kinds; *Line* and *Arrow* are 480 by 120 with `outline` the theme's text colour and `outline-width="6"`, no fill. *Rectangle* writes `kind="rect"`.
  - *Text box* writes a `<Box>` 640 by 200 with an empty body and puts the caret in it. Left empty, it's taken away again when the typing ends, in the same undo step as its insert, so nothing is left to undo.
  - Typing with a shape selected and no caret starts text inside it: the tag gets a body and the caret, and the first character is typed. This works for `Shape` and `Box` only.
- A picture file dropped on the slide from outside the app (`ExternalPaths`) is inserted, each file in turn, one undo step each; other kinds of file say "Only pictures can be dropped on a slide." Dropped on the slide list, nothing happens.
- Ctrl+V with a picture on the clipboard (`ClipboardEntry::Image`) and no slide data, text data of this app, or caret, inserts it as `pasted-picture.png`. With a caret in text, a clipboard picture ends the typing and is inserted the same way. A clipboard of file paths (`ClipboardEntry::ExternalPaths`) is treated as a drop.
- Ids: `INSERT_PICTURE_ID`, `INSERT_SHAPES_ID`, `insert_tools::shape_id(kind)`, `INSERT_TEXT_BOX_ID`.

- [ ] **Step 1: Write the failing tests**

`ui_insert.rs` (file dialogs through a test seam `WorkspaceView::chosen_pictures(paths)`; clipboard pictures written to the test clipboard):
- `place_for` table: empty → (720, 380) for 480×320; once taken → (760, 420); ten times → held; a size larger than the slide → (0, 0).
- `a_picture_from_a_file_lands_in_the_middle_selected` (source gains one `<Picture src="assets/…" x= y= w= h= />` line; aspect kept; the engine draws it with no notice); `two_pictures_chosen_at_once_are_two_undo_steps`; `a_file_that_is_not_a_picture_says_so_and_adds_nothing`; `a_dropped_picture_is_inserted`; `a_dropped_text_file_says_only_pictures`; `a_pasted_picture_is_inserted`; `a_pasted_picture_while_typing_ends_the_typing_first`; `pasting_text_still_pastes_text_when_both_are_on_the_clipboard`.
- `each_shape_is_written_and_drawn` (all six; the layer's native shape kind matches); `a_text_box_takes_the_caret_and_typing_goes_in_it`; `an_empty_text_box_leaves_nothing_behind_and_nothing_to_undo`; `typing_on_a_selected_shape_puts_words_in_it` (`<Shape …>` gains a body holding the letters; export to PPTX has the text in the shape); `undo_of_an_insert_takes_it_away_and_deselects`; `inserting_changes_no_other_byte` (diff of sources); `inserting_on_a_slide_with_notes_goes_before_them`; `the_insert_tab_fits_at_800_pixels_or_scrolls`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_insert`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS, including the `icons` test that every icon the app names is carried.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: pictures, shapes and text boxes go onto a slide from the Insert tab"
```

---

### Task 8: The icon picker

**Files:**
- Create: `crates/app/src/icon_picker.rs`, `crates/app/tests/ui_icons.rs`
- Modify: `crates/app/src/{insert_tools,inspector/fields,lib}.rs`

**Interfaces:**
- Consumes: `slides_engine::{icon_names, icon_preview}`, `Document::insert_tag`, `set_selected_attr`.
- Produces `icon_picker::IconPicker`: a popover from the Insert tab's *Icon* tool: a search field with the keyboard, and under it a virtual grid, eight across, of icons drawn 32 pixels in the text colour, each with its name as tooltip and accessible name.
  - `pub fn matching(query: &str) -> Vec<&'static str>` (pure): names holding every word of the query, case and `-` ignored; names that start with the query first, then the rest, each group in name order. An empty query gives every name.
  - Previews are drawn only for the rows on screen, and kept; scrolling the whole list keeps under 64 MB of previews.
  - Enter picks the first match; a click picks that icon; Escape closes. Picking writes `<Icon name="…" size="128" x y w h />` with `w` and `h` 128, closes, and selects it.
- In the inspector, an icon's *Icon* field is a button showing the icon and its name, opening the same picker to change `name`.
- Ids: `INSERT_ICON_ID`, `ICON_SEARCH_ID`, `icon_picker::icon_id(name)`.

- [ ] **Step 1: Write the failing tests**

`ui_icons.rs`: `matching` table (`""` → 2776; `"arrow right"` holds `arrow-right` first; `"ARROW-R"`; `"zzzz"` → none; a query of 10,000 characters returns at once); `the_picker_opens_with_the_keyboard_in_search`; `typing_narrows_the_grid`; `enter_inserts_the_first_match`; `a_click_inserts_that_icon_selected` (source line exact); `escape_closes_and_inserts_nothing`; `only_rows_on_screen_are_drawn` (count of previews made after opening is under 200); `no_matches_says_so` ("No icons match."); `the_inspector_changes_an_icons_name_in_place` (only the `name` value's bytes change).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_icons`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: an icon is chosen from a searchable picker"
```

---

### Task 9: Tables in the format crate and the engine

**Files:**
- Create: `crates/format/src/table_ops.rs`, `crates/format/tests/table_ops.rs`
- Modify: `crates/format/src/{model,parse,rich,rich_write,lib}.rs`, `crates/engine/src/{flow,native,text_geometry}.rs`
- Test: `crates/format/tests/{rich_read,rich_write}.rs`, `crates/engine/tests/text_geometry.rs`

**Interfaces:**
- `Block::Table` gains `cells: Vec<Vec<CellSource>>` and `rule: Range<usize>` (the alignment row's line), beside `rows`. `pub struct CellSource { pub inline: Range<usize>, pub only_text: bool }`: the cell's content bytes without the padding spaces, and whether its content is text the model holds exactly. Rows shorter than the header have no `CellSource` for the missing cells.
- `FlowPlace` gains `Cell { table: Range<usize>, row: usize, column: usize }`. `text_flows` gives one flow per cell that exists and is `only_text`, each with exactly one `Body` paragraph, after the flows it gave before. Row 0 is the header.
- `write_flow` for a cell flow writes that cell's inline bytes only. A `|` in the text is written `\|`; a new line is refused (`None`); an empty cell is written as one space between its pipes. A cell flow given more than one paragraph, or a paragraph that isn't `Body`, is refused.
- Produces `table_ops`, each returning `Option<Change>` and changing only the lines it must:
  - `pub fn new_table(columns: usize, rows: usize) -> Option<String>`: a table with `columns` (1–20) and `rows` body rows (1–50) under a header, cells a single space, `| --- |` rule, every line starting and ending with `|`.
  - `pub fn add_row(talk: &Talk, slide: usize, table: &Range<usize>, after: usize) -> Option<Change>`: a new empty row after row `after`, in the style of the row above it (leading and trailing pipes or not), with as many cells as the header.
  - `pub fn remove_row(.., row: usize)`: not the header (`None` for 0), and not the last body row.
  - `pub fn add_column(.., after: Option<usize>)`: an empty cell in every row and `---` in the rule, after that column, or first for `None`. Rows shorter than the header are first filled with empty cells up to the column needed, and only as far as needed.
  - `pub fn remove_column(.., column: usize)`: not the last column.
  - `pub fn table_size(talk, slide, table) -> Option<(usize, usize)>`: columns, and rows with the header.
  - Limits: no change may give more than 20 columns, 51 rows, or pass `MAX_SLIDE_BYTES`.
- Engine: `TextLayout` answers carets, selections and hits inside a cell as for any flow: `hit` on a cell's text or its empty area gives `Hit::Text` with that cell's flow index and the nearest position; a point on a table's border or on a cell that has no flow gives `Hit::Element` for the table. `pub fn cell_rect(&self, table: &Range<usize>, row: usize, column: usize) -> Option<Rect>`.

- [ ] **Step 1: Write the failing tests**

- `table_ops.rs`, property tests over generated tables (with and without edge pipes, colons in the rule, escaped pipes, inline code holding a pipe, bold and `<Text>` in cells, uneven rows, after a paragraph, CRLF, in a talk from `gen_talk()`): `typing_in_a_cell_changes_only_that_cell` (bytes outside the cell's `inline` range are the same, and every other cell reads the same); `any_cell_text_reads_back_as_typed` (the Review Focus strings and random text: refused, or the cell's spans equal the text and the table has the same shape); `a_new_row_keeps_every_other_byte`; `a_new_column_keeps_every_other_cells_bytes`; `adding_then_removing_is_the_start_again` for rows and for columns on a regular table; `no_change_breaks_the_table_or_the_slide`; `the_limits_hold`.
- Named: `new_table_reads_back_as_that_many_cells`; `the_header_cannot_be_removed`; `a_short_row_is_filled_only_as_far_as_needed`; `a_cell_holding_an_icon_tag_has_no_flow`; `an_empty_cell_can_be_typed_in_and_emptied_again` (bytes return to the start).
- `text_geometry.rs`: `a_click_in_each_cell_gives_its_flow` on `fixtures/showcase`'s table; `a_caret_in_an_empty_cell_sits_in_the_cell`; `a_click_on_a_tables_border_gives_the_table`; `cell_rects_tile_the_table`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test table_ops`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, and `PROPTEST_CASES=4000 scripts/cargo-safe.sh test -p slides-format --test table_ops`
Expected: PASS, goldens unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/format crates/engine
git commit -m "Format and engine: a table's cells can be typed in, and rows and columns added"
```

---

### Task 10: Tables in the app

**Files:**
- Create: `crates/app/src/table_tools.rs`, `crates/app/tests/ui_tables.rs`
- Modify: `crates/app/src/{document,slide_editor,text_editing,insert_tools,inspector,lib}.rs`, `crates/app/tests/edit_speed.rs`

**Interfaces:**
- Consumes: Task 9's `table_ops`, `FlowPlace::Cell`, `cell_rect`; `insert_block`.
- Produces, on `Document`: `pub fn insert_table(&mut self, columns: usize, rows: usize) -> Result<Vec<usize>, EditRefused>` (at the end of the main region; the caret goes in the first header cell); `pub fn table_at_caret(&self) -> Option<(Range<usize>, usize, usize)>`; `pub fn add_row(&mut self, below: bool)`, `add_column(&mut self, right: bool)`, `remove_row(&mut self)`, `remove_column(&mut self)`, each on the caret's cell, one undo step, returning `Result<Vec<usize>, EditRefused>`, the caret staying in a cell that makes sense (the new row's same column; after a removal, the row or column that took its place).
- The Insert tab's *Table* tool opens a grid of 8 by 6 squares; moving over it shows "3 × 4 table" and a click inserts that many columns and body rows.
- Typing in a cell is D2's typing, with these differences: Enter moves to the same column of the next row, and in the last row adds one; Tab and Shift+Tab move to the next and previous cell, Tab in the last cell adding a row; Up and Down at a cell's first and last line move to the cell above and below; bullets, numbering, indent, headings and alignment tools are off in a cell, with the tooltip "Not in a table."; a paste of several paragraphs goes in as one line, the paragraphs joined by a space; a pasted new line is a space.
- While the caret is in a cell, or a table is selected, a *Table* group shows at the end of the Home tab and in the inspector: *Row above*, *Row below*, *Column left*, *Column right*, *Delete row*, *Delete column*, each off with a tooltip when it can't be done ("A table keeps its heading row.", "A table keeps at least one row.", "A table keeps at least one column.", "A table can have 20 columns at most.", "A table can have 50 rows at most.").
- A cell the model can't hold (an icon, a picture, a component inside it) says "This cell holds more than text, so it can't be typed in yet." on a click.
- Ids: `INSERT_TABLE_ID`, `table_tools::grid_id(columns, rows)`, `TABLE_ROW_ABOVE_ID`, `TABLE_ROW_BELOW_ID`, `TABLE_COLUMN_LEFT_ID`, `TABLE_COLUMN_RIGHT_ID`, `TABLE_DELETE_ROW_ID`, `TABLE_DELETE_COLUMN_ID`.

- [ ] **Step 1: Write the failing tests**

`ui_tables.rs`: `the_grid_inserts_that_many_cells_and_takes_the_caret`; `typing_in_a_cell_writes_only_that_cell` (source diff); `tab_and_shift_tab_walk_the_cells`; `tab_in_the_last_cell_adds_a_row`; `enter_goes_down_a_row`; `a_pipe_typed_in_a_cell_stays_in_the_cell`; `a_pasted_list_goes_in_as_one_line`; `bold_and_colour_work_in_a_cell`; `bullets_are_off_in_a_cell`; `each_row_and_column_tool_does_its_one_thing` (source exact, caret where said); `the_tools_are_off_when_they_cannot_be_done`; `a_run_of_typing_in_a_cell_is_one_undo_step`, and `undo_of_a_new_row_puts_the_caret_back`; `a_hand_written_table_is_typed_in_without_changing_its_spacing`; `a_cell_holding_an_icon_says_so`; `a_table_selected_by_its_border_can_be_deleted`; `a_dead_key_in_a_cell_composes` (D2's input-method path through a cell).
- `edit_speed.rs`: `a_keystroke_in_a_table_cell_is_quick`: a 6 by 8 table on a slide of the 500-slide talk, 200 keystrokes, median under 8 ms.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_tables`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, then `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`
Expected: PASS. Put the medians in the report.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a table is inserted, typed in cell by cell, and grown and shrunk"
```

---

### Task 11: The component palette

**Files:**
- Create: `crates/app/src/palette.rs`, `crates/app/tests/ui_palette.rs`
- Modify: `crates/app/src/{document,insert_tools,inspector,lib}.rs`

**Interfaces:**
- Consumes: `Deck::{available_components, component_schema, components}`, `ComponentState`, `Document::insert_tag`.
- Produces, on `Document`: `pub fn palette(&mut self) -> Vec<PaletteEntry>`, `pub struct PaletteEntry { pub tag: String, pub label: String, pub ours: bool, pub state: Option<ComponentState>, pub takes_content: bool }`: *Chart* first, then the talk's own components in name order. `Box`, `Shape`, `Icon` and `Picture` aren't listed: they have tools of their own. `label` is the tag with a space before each inner capital.
- `pub fn default_attrs(schema: &ComponentSchema) -> Vec<(String, String)>` (pure, in `palette.rs`): what a new tag is written with: every required argument at a starting value (`Text` empty, `Whole` and `Number` `0`, `Flag` not written, `Color` the theme's accent, `Choice` its first option, `File` not written), optional ones not written. For *Chart*: `kind="bar" labels="A, B, C" values="3, 5, 2"`, so it shows something at once.
- The Insert tab's *Component* tool opens a list of the palette: each entry its label; a talk's own component that's out of date is marked "out of date", and one with no build is greyed with the tooltip "Build this talk's components first." A click inserts the tag, 640 by 360, selected, with the inspector showing its form. A component that takes content is written with an empty body, and typing with it selected starts text inside it, as for a shape.
- With no components of its own, the list holds *Chart* and a line: "This talk has no components of its own."
- Inserting never builds anything and never changes build permission.
- Ids: `INSERT_COMPONENT_ID`, `palette::entry_id(tag)`.

- [ ] **Step 1: Write the failing tests**

`ui_palette.rs`, on a scratch copy of `fixtures/components` and on `fixtures/show`: `the_palette_lists_chart_then_the_talks_own`; `a_talk_with_none_says_so`; `inserting_a_chart_draws_bars_at_once` (no labelled error box in the slide's layers); `inserting_a_talks_component_writes_its_required_arguments` (Score: `team=""`, `points="0"`, a colour; it draws without the failed-component box); `default_attrs` table over each `PropKind`, required and optional; `a_component_that_needs_building_is_greyed_and_inserts_nothing`; `an_out_of_date_component_is_marked_and_still_inserts`; `a_component_that_takes_content_can_be_typed_in` (Callout); `inserting_builds_nothing` (the build counter and the permission store are unchanged); `the_form_changes_the_new_chart` (kind to pie through the inspector; the slide's layers change); `a_packed_talk_lists_and_inserts_its_components`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_palette`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a chart or one of the talk's components is put on a slide from a list"
```

---

### Task 12: Copying things, and what a paste brings with it

**Files:**
- Modify: `crates/app/src/{clipboard,clipboard_tools,document,slide_editor,lib}.rs`, `crates/app/src/workspace_view/editing.rs`
- Test: `crates/app/tests/{clipboard,ui_select}.rs`

**Interfaces:**
- Consumes: `block_text`, `insert_block`, `Document::{add_file, selected, has_file}`, `FileKind`.
- The clipboard's data gains a third kind, and both slide and element kinds gain files: `{"kind":"element","version":1,"markdown":…,"files":[{"path":"assets/photo.png","bytes":"<base64>"}]}`; `"slide"` gains the same optional `"files"`. `pub const MAX_CLIPBOARD_FILES_BYTES: usize = 64 * 1024 * 1024` in all, counted before decoding; over it, the copy carries no files and the paste says "The pictures were too large to copy with it."
- Copying a selected thing (Ctrl+C, the Home tab's Copy) puts its Markdown as plain text and this data on the clipboard, with every file it uses that the talk has: a `src` or `data` argument's file, and for a talk's own component its `components/<name>.rs` and `build/<name>.wasm`. Cut does that and deletes it, one undo step. Copying a slide carries the files of everything on it the same way.
- Pasting an element (Ctrl+V with element data, and no caret in text): it's checked as a pasted slide is (it must read as exactly one block, a tag or a table, within `MAX_SLIDE_BYTES`, with no settings and no slide break), written with `insert_block` on the current slide, moved 40 units down and right when its `x` and `y` are exactly those of a thing already there, and selected. Its files are added first with `add_file`, and each path in the pasted Markdown is rewritten to the path `add_file` gave where the name had to change.
- A pasted file is trusted no more than a dropped one: its path must be `assets/`, `components/` or `build/` followed by one safe name with an allowed extension for that folder, a picture must read as a picture, and every limit of Task 4 holds. A file that fails is left out, the thing is still pasted, and D2's "Some pictures on the pasted slide aren't in this talk." is said.
- A pasted component's source and build are added only if the talk has no component of that name. If it has one with the same source, it's used as it is. If it has one with different source, nothing is added and the paste says "This talk already has a different component called Score. The pasted one uses this talk's." A paste never builds, never changes build permission, and never replaces a component's source or build. A pasted build with no matching source hash is shown as out of date, by the rules that already hold.
- Pasting a slide now brings its files the same way, replacing D2's ruling that they show as placeholders.
- Duplicate: Ctrl+D with a thing selected copies and pastes it in one undo step, without touching the clipboard.

- [ ] **Step 1: Write the failing tests**

- `clipboard.rs` (pure): `element_data_round_trips`; `hostile_element_data_is_refused_or_made_safe`, a table of well-formed JSON: a path of `../x.png`, `assets/../../x`, `/etc/x`, `assets/a/b.png`, `build/x.exe`, `components/x.rs` holding `include_str!`, 2,000 files, files adding to 65 MiB by their stated length, bytes that aren't base64, markdown holding two blocks, a slide break, a settings block, a `<Notes>`; property `no_pasted_path_leaves_the_three_folders`; `a_paste_never_sets_a_talk_wide_setting` still holds with files.
- `ui_select.rs`: `copy_and_paste_a_shape_gives_a_second_one_offset_and_selected`; `cut_then_undo_brings_it_back`; `duplicate_leaves_the_clipboard_alone`; `a_picture_pasted_into_another_talk_brings_its_file` (two windows in one test app; the second talk draws it, and after Save its folder holds the file); `a_picture_whose_name_is_taken_is_renamed_in_the_pasted_tag`; `a_slide_pasted_into_another_talk_brings_its_pictures`; `a_component_pasted_into_a_talk_without_it_brings_source_and_build_and_builds_nothing`; `a_component_that_differs_is_not_replaced_and_says_so`; `an_element_pasted_while_typing_is_refused_in_words` (with a caret in text, element data is not typed in as Markdown: nothing is pasted, and the status says "That can't be pasted into text."); `a_table_can_be_copied_and_pasted`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test clipboard --test ui_select`
Expected: the new tests FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a thing copied from a slide brings its files into the talk it's pasted in"
```

---

### Task 13: The real window, and the record

**Files:**
- Create: `docs/checks/plan-d3.md`
- Modify: `README.md`, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (the D3 line: its file and where the record is), `docs/guide/components.md` (a short section: what a person who doesn't code sees for each argument type, with the label rule)
- Fix, each in its own commit with a test: whatever the run finds.

**What to do:**

- [ ] **Step 1:** Build in release and run the app on a nested X display as D2's run did (`docs/checks/plan-d2.md` says how, including the screenshot helper that reads the picture's stated pixel size, the scratch `XDG_*` folders, and scratch copies of the fixtures outside the repository). Look at every screenshot.
- [ ] **Step 2:** On a scratch copy of `fixtures/showcase`, do the first success test's middle: add slides and put on them a picture from a file (through the system dialog if the display can show it, else through a drop sent with the X drag protocol or the test seam, saying which), a pasted picture from an outside clipboard owner, each of the six shapes with words typed in two of them, a text box, three icons found by searching, a 3 by 4 table filled by Tab, with a row and a column added and removed, and a chart changed to a pie in the inspector. Select, change fill and outline, delete, undo everything, redo everything, save, and `git diff --no-index` the original against the saved `slides.md`: only what was added differs. Open the saved talk fresh: every picture shows. Do the same on a packed talk.
- [ ] **Step 3:** On a scratch copy of `fixtures/components`: insert *Score* and *Callout* from the palette, fill in their forms, copy one to a second window on `fixtures/show` and save it there. Kill the app with a picture added and unsaved, start it, bring the changes back: the picture shows.
- [ ] **Step 4:** Present with F5 and export to PDF and PowerPoint: the selection outline and the inspector never show, every inserted thing does, shapes with words are shapes with words in the PowerPoint file, and the table is a table.
- [ ] **Step 5:** How it looks: the Insert tab at 1,280 and 800 pixels, the inspector at both, the icon picker, the table grid, the selection outline on a turned box. Fix what's wrong and cheap; list the rest.
- [ ] **Step 6:** Write `docs/checks/plan-d3.md` in the form of `docs/checks/plan-d2.md`: what ran with test totals per crate, what the window did, what wasn't exercised, a Windows check by hand (numbered, starting from a clone of `main`, including the system Open dialog for pictures, a drop from Explorer, a paste from Snipping Tool and from a browser, a paste between two windows, desktop PowerPoint on the exported file, and D2's open items) and a Linux check by hand for X11 and Wayland (the same, with a drop from the file manager and a paste from a screenshot tool), and what's known and not fixed.
- [ ] **Step 7:** Run `scripts/cargo-safe.sh test --workspace` and `scripts/cargo-safe.sh clippy --workspace --all-targets`, and the release timing tests.
- [ ] **Step 8: Commit** the record and docs.

```bash
git add docs/checks/plan-d3.md README.md docs/superpowers/plans/2026-11-09-native-rust-roadmap.md docs/guide/components.md
git commit -m "Docs: what plan D3's run did, and what to check by hand"
```
