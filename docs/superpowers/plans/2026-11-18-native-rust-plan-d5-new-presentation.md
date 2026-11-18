# Plan D5: New Presentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Starting the app shows a start screen. From it a person picks a starter template and gets a new, untitled presentation they can edit at once, save with Save As, and get back after a crash, without ever seeing a folder dialog first.

**Architecture:** A template is a small talk bundled in the app: its Markdown and any pictures, embedded at build time. A new presentation is an ordinary `Document` whose place is *untitled*: its files live in memory, its edits go through the same history, and its working copy is kept under an id of its own until the first save. The first save writes a fresh folder or packed file from memory, after which it is an ordinary opened talk. The start screen is the window's view when no talk is open, and the layouts' names and areas move to one list in the format crate first, so a template can't name a layout one crate knows and another doesn't.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 with gpui-fast 0.1.13, gpui-component widgets, `include_str!`/`include_bytes!`, proptest, and the existing crates.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`: goal 10 and the save part of 11, the first success test (*They choose New presentation or double-click a talk… They never see Markdown, Rust, a terminal or a missing-dependency error*), "The talk on disk" (saving, working copies), "Custom components" (*Talks and components created in the app start with* Always build this folder *on*), and the anti-goals *Losing work*, *Code in the way*, *A talk reaching your computer* and *Needing anything else*. `docs/superpowers/plans/2026-11-17-native-rust-plan-d4-decisions.md` has D4's rulings and its list "For the start of plan D5".

## Global Constraints

- Work on the branch `plan-d5`, cut from `main`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx` and `slides-components` don't depend on GPUI.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **Nothing is lost.** A new presentation that has been edited is either saved, kept in its working copy, or given up by a person's own choice. A save that fails leaves it untitled, unsaved and whole. A crash before the first save brings it back at the next start.
- **A first save writes everything**: the text, every file the text uses (a template's pictures, pictures added since, a pasted component's source and build), and the pack's index. What's written opens again as the same talk: the same bytes of text and the same drawing.
- **A template is data, never code.** No template carries a component's source or a build, and nothing is compiled by making or saving a new presentation.
- **Opening the app touches nothing.** The start screen reads the app's own data folder and nothing else, makes no network request, and writes nothing until a person edits.
- A talk is untrusted input, and so is anything in the recovery folder. Nothing in one may crash the app or make it read or write outside the talk and the app's own data folder.
- Nobody is made to see Markdown, a tag name with angle brackets, a settings key, a path with `assets/`, or Rust. Every label and message is plain words.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless GPUI test.
- An idle window does no work. A keystroke keeps its 8 ms budget and a drag frame its 6 ms.
- The keyboard rule from D3 holds: while the list of slides or the notes have the keyboard, nothing on the slide is selected, being typed in, or waiting as a new text box.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits or in a template's words.

Not in this plan: changing a talk's colours or fonts after it's made (the design's first release has one theme per talk, set when it's made); templates from a file or from the internet; saving a talk as a template; presenting from the current slide; the Home tab's width; installers and the file type (plan E).

Rulings made in the plan:
- **Five templates:** *Light*, *Dark*, *Warm*, *Ocean*, each one empty title slide in its colours, and *Tour*, a short talk that shows what a slide can hold. Colours are the talk's `color.background`, `color.text` and `color.accent`; the fonts are the bundled ones. Cost: a template can't differ by more than three colours and its first slides.
- **A new presentation is called *Untitled presentation*** until it's saved, and a second one open at the same time *Untitled presentation 2*. It has no `title:` of its own.
- **An untitled presentation that hasn't been edited closes without asking.** Once edited, closing asks, and *Save* there opens Save As; the window closes only when the save is done.
- **Save on an untitled presentation is Save As.** The Save tool is on for it from the start.
- **Save As suggests the Documents folder** (the home folder if there's none) and the packed form, as that is what a person who doesn't code sends to a colleague.
- **A talk saved from the app as a folder starts with *Always build this folder* on**, as the design has it. A packed talk never builds.
- **The app no longer quits when the first dialog is cancelled:** it shows the start screen. `stagepack-studio <path>` still opens that talk directly.
- **Ctrl+N opens a start screen in a new window.** Ctrl+O is *Open talk file…* and Ctrl+Shift+O *Open talk folder…*.
- **Picking a template or opening a talk from the start screen uses that window.**
- **Unsaved new presentations from last time are listed on the start screen**, newest first, each with *Open* and *Discard*. They're kept for 30 days, as other working copies are.

## Review Focus

- **The life of an untitled presentation.** Closed with no edits; closed with edits and *Save*, then the Save As dialog cancelled; Ctrl+S; two open at once; a crash before the first save and the next start; a save to a folder that isn't empty, to a file that exists, to a place that can't be written, and one that fails half-way. After each, nothing is lost and nothing is asked twice. Owned by Task 4's `an_untitled_talk_is_never_lost` and Task 5's `a_crash_before_the_first_save_brings_it_back`.
- **What the first save writes.** A template's picture, a picture added before the save, a picture added and then removed (not written), a component pasted in with its source and build, a 30 MB picture. The saved talk, opened again, has the same text bytes and draws the same slides, in both forms. Owned by Task 4's `the_first_save_writes_all_the_talk_uses`.
- **After the first save it's an ordinary talk.** Save is off until the next edit; an edit to `slides.md` from outside reloads it; its working copy is under the new place and the untitled one is gone; Save As again works; closing asks only if edited. Owned by Task 4's `once_saved_it_behaves_as_an_opened_talk`.
- **A template a person can read.** On each template the caret, the words *Click to add text*, a selection's outline and handles, a snap guide and a step badge stand out from the slide's background, and the PowerPoint and PDF exports carry its colours. Owned by Task 3's `every_mark_stands_out_on_every_template`.
- **A recovery folder in any state.** No folder at all; fifty unsaved presentations; one whose text doesn't parse, one with a missing file, one with a name pointing outside the folder, one written by a newer app; a folder that can't be read. The start screen shows what it can, says what it can't, and never crashes or reads outside. Owned by Task 5's `a_recovery_folder_in_any_state_is_listed_or_skipped`.

---

### Task 1: One list of layouts

**Files:**
- Create: `crates/format/src/layouts.rs`, `crates/format/tests/layouts.rs`
- Modify: `crates/format/src/{lib,rich,arrange_ops}.rs`, `crates/engine/src/{layouts,slide}.rs`, `crates/app/src/{layout_picker,selection,workspace_view}.rs`, `crates/app/src/document/slides.rs`

**Interfaces:**
- Produces, in `slides_format`:
  - `pub struct LayoutNames { pub name: &'static str, pub regions: &'static [&'static str], pub main: &'static str }`
  - `pub const LAYOUT_NAMES: [LayoutNames; 5]`, in the order title, content, two-columns, section, blank.
  - `pub const DEFAULT_LAYOUT: &str = "content";`
  - `pub fn layout_names(name: &str) -> Option<&'static LayoutNames>`
- No behaviour changes. `rich.rs`'s own list, the three other `DEFAULT_LAYOUT`s and the literal `"content"` in `workspace_view.rs` go; each uses these. The engine's `LayoutDef`s keep their rectangles, and a test holds them to this list.

- [ ] **Step 1: Write the failing tests**

- `crates/format/tests/layouts.rs`: `the_default_layout_is_listed`; `every_main_region_is_one_of_its_regions`; `names_are_distinct`.
- `crates/engine/tests/flow.rs`: `the_engines_layouts_are_the_formats`: `layouts()` has the same names in the same order, each with the same regions in the same order and the same main region as `LAYOUT_NAMES`.
- `crates/app/tests/ui_layouts.rs`: `every_layout_and_area_has_a_plain_name`: `layout_name` and `area_name` give words with no hyphen and no lower-case first letter for every name in `LAYOUT_NAMES`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test layouts`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and `scripts/cargo-safe.sh clippy --workspace --all-targets`
Expected: PASS, no golden image changed, and `grep -rn '"content"' crates/*/src` finds the name in `crates/format/src/layouts.rs` only.

- [ ] **Step 5: Commit**

```bash
git add crates/format crates/engine crates/app
git commit -m "Format: the layouts' names and areas are written in one place"
```

---

### Task 2: A talk held in memory, and written out whole

**Files:**
- Modify: `crates/engine/src/{files,packed,lib}.rs`
- Test: `crates/engine/tests/{files,packed}.rs`

**Interfaces:**
- Produces, in `slides_engine`:
  - `pub struct MemoryFiles`: a talk's files held in memory. `pub fn new(files: Vec<(String, Arc<[u8]>)>) -> MemoryFiles`; `impl TalkFiles` (`read`, `len`, `names_in`; `never_changes` is true). A path is used only if it is a plain relative path with no `..`, no leading `/`, no drive and no NUL; others are dropped by `new`.
  - `pub fn write_new_folder(to: &Path, slides: &str, files: &[NewFile]) -> Result<(), PackError>`: makes `to` (which must be missing or an empty folder) and writes `slides.md` and each file under it, each atomically, refusing any file path that isn't plain and relative as above. On any failure it removes what it wrote and leaves `to` as it found it.
  - `pub fn pack_new(out: &Path, slides: &str, app_version: &str, files: &[NewFile]) -> Result<(), PackError>`: writes a packed talk with this app's manifest, `slides.md` and the files, with the same temp file, sync and rename as `pack_folder_with_slides`. The result opens with `open_packed` and reads back every file.
- `NewFile` is the existing `(String, Vec<u8>)`.

- [ ] **Step 1: Write the failing tests**

- `files.rs`: `memory_files_read_what_they_were_given`; `names_in_lists_a_folder`; `a_path_that_leaves_the_talk_is_dropped` (`../x`, `/etc/x`, `a/../../x`, `C:\x`, a NUL).
- `packed.rs`: `a_new_pack_opens_and_reads_back_every_file`; `a_new_folder_has_slides_and_every_file`; `a_new_folder_in_a_folder_that_is_not_empty_is_refused_and_nothing_is_written`; `a_new_folder_that_fails_half_way_leaves_nothing` (a file path whose parent is a file); `a_file_path_that_leaves_the_folder_is_refused`; `a_new_pack_over_an_existing_file_replaces_it_whole` (and a failed write leaves the old file); `a_pack_and_a_folder_of_the_same_talk_read_alike` (property over generated text and up to eight files: both forms read back the same bytes for every path).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test files --test packed`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: a talk held in memory can be read and written out whole"
```

---

### Task 3: The templates

**Files:**
- Create: `crates/app/templates/{light,dark,warm,ocean,tour}/slides.md`, `crates/app/templates/tour/assets/photo.png` (copied from `fixtures/showcase/assets/photo.png`), `crates/app/src/templates.rs`, `crates/app/tests/templates.rs`
- Modify: `crates/app/src/lib.rs`, `crates/app/src/slide_editor/{marks,render}.rs` and `crates/engine/src/{flow,deck}.rs` only where a mark doesn't stand out

**Interfaces:**
- Consumes: Task 2's `MemoryFiles`.
- Produces, in `stagepack_studio::templates`:
  - `pub struct Template { pub id: &'static str, pub name: &'static str, pub says: &'static str, pub slides: &'static str, pub files: &'static [(&'static str, &'static [u8])] }`
  - `pub fn templates() -> &'static [Template]`: *Light*, *Dark*, *Warm*, *Ocean*, *Tour*, in that order, with ids `light`, `dark`, `warm`, `ocean`, `tour`. `says` is one line a person reads under the name (*White, with dark text*, and so on; *A short talk that shows what a slide can hold* for the tour).
  - `pub fn template(id: &str) -> Option<&'static Template>`
  - `pub fn contrast(a: Color, b: Color) -> f32`: the WCAG contrast ratio of two colours, 1 to 21.
  - `pub fn memory_files(template: &Template) -> MemoryFiles`
- The four colour templates are one slide: a settings block with `layout: title` and the three colours, and nothing else. *Light* is the app's default colours and writes none. *Dark*: background `#0f172a`, text `#f1f5f9`, accent `#38bdf8`. *Warm*: `#fdf6e3`, `#3b2f2f`, `#c2410c`. *Ocean*: `#0c4a6e`, `#f0f9ff`, `#fbbf24`.
- *Tour* is eight slides in the default colours, in plain words a person new to the app can follow: a title; a list shown one at a time; two columns; a picture and a shape that can be moved; a table; a chart; a section slide; a last slide saying how to save and present. It uses only what's built in: no component of the talk's own, no code block.
- In the editor, the caret, the words in an empty area, the selection's outline and handles, snap guides and step badges are drawn in a colour chosen against the slide's background where the theme's own colour doesn't stand out.

- [ ] **Step 1: Write the failing tests**

`crates/app/tests/templates.rs`:
- `every_template_reads_with_nothing_to_say`: each parses, lays out every slide with no notice, names only layouts in `LAYOUT_NAMES`, and has no `title:`.
- `no_template_carries_code`: no file under `components/` or `build/`, no tag that isn't built in, no code fence.
- `every_file_a_template_names_is_bundled`, and `no_bundled_file_is_unused`.
- `text_stands_out_on_every_template`: `contrast(text, background) >= 7.0` and `contrast(accent, background) >= 3.0`.
- `every_mark_stands_out_on_every_template` (Review Focus): in a headless window on each template's first slide, the caret, the *Click to add text* words, a selected text box's outline and handles, a snap guide during a drag and a step badge on the Animate tab each have a colour with `contrast` of at least 3.0 against the slide's background.
- `a_templates_colours_reach_powerpoint_and_pdf`: the Dark template exported both ways has its background and text colours in the output.
- `the_tour_draws_as_its_golden_pictures`: each of the tour's slides at 480 pixels wide matches a golden image in `crates/app/tests/golden/tour/` (written on the first run with `UPDATE_GOLDEN=1`, as the engine's golden tests do).
- `contrast_is_what_the_standard_says` (black on white 21, white on white 1, `#777` on white about 4.48).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test templates`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy. Look at the tour's eight golden pictures and say in the report what each shows.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/engine
git commit -m "App: five starter templates, each readable in its own colours"
```

---

### Task 4: An untitled presentation, and its first save

**Files:**
- Create: `crates/app/tests/untitled.rs`, `crates/app/tests/ui_untitled.rs`
- Modify: `crates/app/src/document/{open,mod,saving,files,reading}.rs`, `crates/app/src/{saving,save_flow,working_copies,recovery,ribbon,export}.rs`, `crates/app/src/workspace_view.rs`, `crates/app/src/workspace_view/saving.rs`

**Interfaces:**
- Consumes: Task 2's `MemoryFiles`, `write_new_folder`, `pack_new`; Task 3's `Template`.
- Produces:
  - `TalkPlace::Untitled(UntitledId)`, with `pub struct UntitledId(pub u128)` (random, written as 32 hex digits). `TalkPlace::directory()` for it is the Documents folder, or the home folder, or the current one. `TalkPlace::name()` is *Untitled presentation*.
  - `Document::new_untitled(template: &Template, id: UntitledId, fonts: Fonts) -> Result<Document, String>`: the template's text over `MemoryFiles` of its files. Not edited yet: `has_unsaved_changes()` is false.
  - `Document::untitled_with(id: UntitledId, text: String, files: Vec<(String, Arc<[u8]>)>, builds: Vec<BuildEntry>, fonts: Fonts) -> Result<Document, String>`: an untitled talk brought back from its working copy; `has_unsaved_changes()` is true.
  - `Document::is_untitled(&self) -> bool`; `Document::set_untitled_number(&mut self, n: usize)` (the name becomes *Untitled presentation 2* for 2 and up).
  - `SaveJob`, from an untitled place with a target, writes with `write_new_folder` or `pack_new`: the text, every file of the talk's memory files and added files that the text uses, pasted components' sources and builds, and the index. From an untitled place with no target it is `Err(SaveError::Failed)` and is never started by the window.
  - `Saved::moved_to` carries the new place and its files, as for any Save As, and `Document::saved` then holds an ordinary folder or packed talk.
- In the window:
  - `WorkspaceView::open_with` on an untitled document shows it as any talk. The title is *Untitled presentation · 1 / 1*.
  - *Save* (the tool, the menu item, Ctrl+S) on an untitled talk opens the Save As dialog, and the Save tool is on from the start. The close prompt's *Save* does the same and closes the window only when the save has finished; a cancelled dialog or a failed save leaves the window open and says why.
  - An untitled talk with no edits closes without asking.
  - The Save As file dialog starts in the Documents folder with the name *Untitled presentation* (`.sptx` for the packed form), and the dialog's packed button stays the first choice.
  - After a first save to a folder: the folder is watched, and `BuildPermissions::set_always` is called for it. After any first save the working copy under the untitled id is cleared and the window's title has the new name.
  - Export to PowerPoint and PDF work on an untitled talk, suggesting the Documents folder.

- [ ] **Step 1: Write the failing tests**

- `untitled.rs` (no window):
  - `a_new_talk_from_each_template_is_not_edited_yet`; `an_untitled_talks_name_and_number`.
  - `the_first_save_writes_all_the_talk_uses` (Review Focus), for each template and both forms: add a picture, add and remove another, paste a component with its source and build from a fixture talk; save; open what was written: the text is byte-equal, every slide's layers are pixel-equal to the untitled talk's, the removed picture isn't there, the component's source and build are, and a packed talk's index is.
  - `a_save_that_fails_leaves_it_untitled_and_whole`: to a folder that isn't empty, to a path under a file, to a read-only folder: `Err`, `is_untitled()` still, `has_unsaved_changes()` as before, nothing left at the target, and a later save elsewhere works.
  - `saving_a_thirty_megabyte_picture_works`.
  - `any_edits_then_a_first_save_reopen_alike` (property: a random run of typing, inserting and arranging on a template, then a save in either form, reopens with byte-equal text).
- `ui_untitled.rs`:
  - `an_untitled_talk_is_never_lost` (Review Focus): closed with no edits (no prompt, window gone); closed with edits and *Don't save* (gone); closed with edits and *Save*, the file dialog cancelled (window stays, still untitled, edits there); *Save* and a place chosen (written, window closes); Ctrl+S opens Save As; a failed save says so on the status line and the window stays.
  - `two_untitled_talks_have_their_own_names_and_copies`.
  - `once_saved_it_behaves_as_an_opened_talk` (Review Focus): Save is off until an edit; an edit to `slides.md` from outside reloads it; the working copy is under the new place and none is left under the untitled id; Save As again to the other form works; closing with no edits doesn't ask.
  - `a_folder_saved_from_the_app_is_always_built`, and `a_packed_talk_is_not`.
  - `save_as_starts_in_documents_with_the_untitled_name`.
  - `export_works_before_the_first_save`.
  - `the_save_tool_is_on_for_an_untitled_talk_and_off_once_saved`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test untitled --test ui_untitled`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Every `match` on `TalkPlace` gains its untitled arm on purpose: no `_ =>` that hides one.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a new presentation is untitled until its first save, which writes all of it"
```

---

### Task 5: Getting an untitled presentation back after a crash

**Files:**
- Modify: `crates/app/src/{recovery,working_copies}.rs`, `crates/app/src/workspace_view/saving.rs`
- Test: `crates/app/tests/recovery.rs`, `crates/app/tests/ui_untitled.rs`

**Interfaces:**
- Consumes: Task 4's `TalkPlace::Untitled`, `UntitledId`, `Document::untitled_with`.
- Produces, in `stagepack_studio::recovery`:
  - An untitled talk's working copy is kept as any talk's is, under a key made from its id, with `about.json` saying it is untitled (a field added so that older copies still read) and carrying the id and its name.
  - `pub struct UntitledCopy { pub id: UntitledId, pub name: String, pub when: SystemTime, pub slides: usize, pub first_words: String }`: `first_words` is the first slide's first line of text, 60 characters at most, or empty.
  - `pub fn untitled(&self) -> (Vec<UntitledCopy>, usize)`: every untitled copy that can be read and isn't held by a live window, newest first, 100 at most, and how many were skipped because they couldn't be read. It reads only inside the recovery folder, follows no symlink, and never panics.
  - `pub fn take_untitled(&self, id: UntitledId) -> Option<Found>`: the copy's text, files and builds, for `Document::untitled_with`; `None` if it's gone or held.
  - `pub fn discard_untitled(&self, id: UntitledId)`
  - `sweep` removes untitled copies as old as it removes others.
- A window showing an untitled talk holds its copy as a live window does for any talk, so a second window never lists or opens it.

- [ ] **Step 1: Write the failing tests**

- `recovery.rs`:
  - `an_untitled_copy_is_kept_listed_taken_and_discarded`.
  - `a_crash_before_the_first_save_brings_it_back` (Review Focus): make an untitled talk from a template, add a picture and type, let the working copy be kept, drop everything without saving (as a crash does); a fresh `Recovery` lists it with its slide count and first words; `take_untitled` and `untitled_with` give a talk with byte-equal text and the picture, unsaved; saving it writes all of it.
  - `a_recovery_folder_in_any_state_is_listed_or_skipped` (Review Focus): no folder; fifty copies (all listed, newest first); a copy whose text doesn't parse as a talk (listed: any text is a talk); one with `about.json` missing, truncated, or from a newer version with unknown fields; one whose file names point outside the folder; a `files` entry that is a symlink to `/etc`; a folder that can't be read. Readable ones are listed, the rest counted as skipped, nothing outside the folder is read, and nothing panics.
  - `a_copy_held_by_a_live_window_is_not_listed`.
  - `an_old_untitled_copy_is_swept`.
  - `older_about_files_still_read` (a D4-shaped `about.json` for a folder talk).
- `ui_untitled.rs`: `an_untitled_talk_keeps_its_copy_as_it_is_edited` (after an edit and the keep's pause, the copy is on disk; with no edits there is none); `the_first_save_clears_the_untitled_copy`; `closing_with_dont_save_discards_the_copy`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test recovery --test ui_untitled`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a new presentation that was never saved is kept and can be brought back"
```

---

### Task 6: The start screen

**Files:**
- Create: `crates/app/src/start_screen.rs`, `crates/app/tests/ui_start.rs`
- Modify: `crates/app/src/{main,lib,workspace_view}.rs`, `crates/app/src/workspace_view/{editing,saving}.rs`, `crates/app/tests/ui_slides.rs`

**Interfaces:**
- Consumes: Task 3's `templates()`, `memory_files`; Task 4's `Document::new_untitled`, `untitled_with`; Task 5's `Recovery::untitled`, `take_untitled`, `discard_untitled`.
- Produces:
  - Actions `NewPresentation` (Ctrl+N), and keys Ctrl+O for `OpenPacked` and Ctrl+Shift+O for `OpenFolder`.
  - `pub fn open_start_window(recovery: Option<Recovery>, cx: &mut App) -> Result<()>`: a window with no talk, showing the start screen.
  - `WorkspaceView::show(&mut self, document: Document, window, cx)`: puts a talk in a window that has none, as `open_with` would have built it: its panels, its tools, its recovery, and for a folder its watcher.
  - `stagepack_studio::start_screen::StartScreen`, shown by the window while it has no talk and no problem to show:
    - a heading *New presentation* and a row of five tiles (`template_tile_id(id)`), each a picture of the template's first slide, its name and its line; a click, or Enter with the tile focused, makes that presentation in this window. The first tile has the keyboard when the screen shows; Left and Right move along the row.
    - *Open a talk file…* (`START_OPEN_FILE_ID`) and *Open a talk folder…* (`START_OPEN_FOLDER_ID`), which open in this window.
    - when there are unsaved copies, a heading *Not saved last time* and a list (`unsaved_row_id(id)`), each with its name, its first words, how many slides, and when (*Today 14:05*, *Yesterday*, a date), with *Open* (`unsaved_open_id(id)`) and *Discard* (`unsaved_discard_id(id)`). Discard asks *Discard this presentation? It can't be brought back.* with *Discard* and *Cancel*. If some couldn't be read, one line says *2 couldn't be read.*
  - Tile pictures are drawn once, off the window's thread, at the tile's size, from a `Deck` on the template's `MemoryFiles` with bundled fonts; a tile shows its name on a plain panel until its picture is ready, and can be picked before.
- In the window:
  - `main.rs` with no argument opens the start window, and never quits because a dialog was cancelled. With a path it opens that talk as before.
  - The File menu gains *New presentation…* (first, with Ctrl+N beside it), which opens a start screen in a new window; the two Open items show their keys. From a window that shows a talk, *Open* still opens a new window; from a start screen it uses that window.
  - A talk that can't be opened from the start screen leaves the start screen showing, with the reason in words on a line under the buttons (`START_PROBLEM_ID`).
  - The title of a start-screen window is *Stagepack Studio*.
  - Each new untitled talk gets the lowest number not in use among the app's open windows.

- [ ] **Step 1: Write the failing tests**

`ui_start.rs`:
- `the_start_screen_shows_five_templates_and_two_ways_to_open`; `the_first_tile_has_the_keyboard_and_enter_makes_it`; `left_and_right_move_along_the_tiles`.
- `picking_a_template_shows_it_in_this_window_ready_to_type` (the title is *Untitled presentation · 1 / 1*, the slide list shows one slide, a click on *Click to add text* types, the Save tool is on).
- `each_template_can_be_picked` (five windows; each talk's source is the template's).
- `a_tile_shows_its_slides_picture_once_drawn`, and `a_tile_can_be_picked_before_its_picture`.
- `opening_a_folder_from_the_start_screen_uses_this_window` (with the dialog answered by the test platform), and `a_cancelled_dialog_leaves_the_start_screen`; `a_talk_that_cannot_be_opened_says_why_and_stays` (a folder with no `slides.md`; a file that isn't a talk).
- `unsaved_talks_are_listed_newest_first_with_their_words`; `open_brings_one_back_unsaved_and_takes_it_off_other_start_screens`; `discard_asks_and_then_removes_it`; `some_that_cannot_be_read_are_counted`.
- `ctrl_n_opens_a_start_screen_in_a_new_window`; `the_file_menu_has_new_presentation_and_shows_the_keys`; `a_second_untitled_talk_is_numbered_two`.
- `the_start_screen_does_nothing_while_idle` (no redraw over twenty seconds once the tiles are drawn), and `it_writes_nothing` (the app's data folder is byte-equal before and after showing it and picking nothing).
- `nothing_on_the_start_screen_shows_code` (no visible text holds `<`, `.md`, `assets/`, `slides.md`, a hyphenated layout name).
- In `ui_slides.rs`, `the_file_menu_names_both_ways_to_open_a_talk` gains the new item.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_start`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: starting the app shows templates to begin from and talks to open"
```

---

### Task 7: The real window, the record and the guides

**Files:**
- Create: `docs/checks/plan-d5.md`
- Modify: `README.md`, `docs/guide/components.md` where it says how a talk is made, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (the D5 line gains its file names), `crates/app/tests/typing_random.rs`

**Interfaces:**
- No code interfaces. `typing_random.rs`'s window run gains a start: half its runs begin on a start screen, pick a template, and at a random step save the untitled talk to a scratch folder or file and go on; the checks after each step stay, and at the end the saved talk, opened again, has the text the window has.

- [ ] **Step 1: Extend the random session and run it**

Run: `PROPTEST_CASES=1000 scripts/cargo-safe.sh test -p stagepack-studio --test typing_random`
Expected: PASS. A failure is a bug to fix in the code it points at, with a named test.

- [ ] **Step 2: Run the app in a real window**

On a scratch display, with `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME` and `HOME`'s Documents folder set to scratch folders, never the real ones, with screenshots to `target/check/d5-run/`: start the app with no argument; look at the start screen and each tile; make a talk from each template, type a title on each, and look at the caret and marks on the dark ones; on the tour, step through all eight slides and present it; add a picture, save as a packed talk, close, open it again from the start screen; make another, edit it, kill the app, start it again and bring the talk back from *Not saved last time*; discard another; do the success test's ten-slide talk from the *Light* template with text, a picture, shapes, a table and click animations, present it, save it, and export it to PowerPoint and PDF and open both. Look at every screenshot.

- [ ] **Step 3: Write `docs/checks/plan-d5.md`**

What ran and what was seen, the timings, what couldn't be checked, a Windows hand-check list and a Linux one (the real file dialogs, Documents on a computer with none, two screens, desktop PowerPoint), and known issues. Say plainly how far the first success test in the design now holds and what plan E still owes it.

- [ ] **Step 4: Update the README and the guides**

- [ ] **Step 5: Verify and commit**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and the release timing tests.
Expected: PASS.

```bash
git add docs/checks/plan-d5.md README.md docs/guide docs/superpowers/plans/2026-11-09-native-rust-roadmap.md crates/app/tests/typing_random.rs
git commit -m "Docs: what plan D5 was checked with"
```
