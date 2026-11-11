# Plan D1: Slides and safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The window becomes an editor: tool tabs across the top, a strip of slide thumbnails on the left to add, duplicate, delete, reorder and hide slides, notes under the slide, undo and redo, Save and Save As, and nothing lost in a crash.

**Architecture:** Every change is a `SourceEdit` on the talk's Markdown, so one history undoes all of them. Slide operations are pure functions in `slides-format` that give edits. `Document` holds the history and the unsaved text; the file on disk changes only at Save. A working copy in the app's data folder follows each change, for recovery. The window is rebuilt from small views: a ribbon, a slide strip, the canvas and a notes panel.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 (its `tab`, `list`, `menu`, `dialog`, `input` and `resizable` components), and the existing `slides-format`, `slides-engine` and `stagepack-studio` crates. `proptest = "=1"` for tests, as the typing spike used.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`: goals 1, 8 (notes), 9 (undo) and 11; "One source of truth: the Markdown"; "The talk on disk" (saving, and changes made outside the app); "The editor"; and every anti-goal, above all *Rewriting the user's text*, *Losing work* and *Code in the way*.

## Global Constraints

- Work on the branch `plan-d1`, cut from `main`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx` and `slides-components` don't depend on GPUI.
- **A visual edit changes only the bytes of what was edited.** No function here re-serialises a slide or a talk. Line endings (`\n` or `\r\n`), a byte-order mark, trailing spaces and the bytes of every untouched slide survive every operation. Each operation has a test that says so, on a `\r\n` talk too.
- **Undo is exact:** applying an edit and then its inverse gives back the same bytes.
- A talk is untrusted input. Nothing in one may crash the app, write outside the chosen file, or read outside the talk.
- Files are written with `slides_engine::write_atomically`. A crash or kill during Save never leaves a damaged talk.
- Nobody is made to see Markdown, Rust or a terminal. Every label and message is plain words.
- Every interactive element has an id constant and `test_support()`, as the existing ones do, and every behaviour has a headless GPUI test.
- Work that can take long (saving, packing, thumbnails of many slides) never blocks the UI thread for more than a frame or two on an ordinary talk.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: typing on the slide (D2), inserting things (D3), moving things on the slide and the Animate tab's tools (D4), templates (D5), installers (E). The Insert and Animate tabs exist and are empty but for one line saying their tools are on the way.

## Review Focus

- **A slide operation on a hand-written file:** `\r\n` endings, a BOM, no newline at the end, a `---` inside a code fence, an empty slide, a slide that's only a settings block. Other slides' bytes don't change. Owned by Task 1's property test `slide_operations_leave_other_slides_bytes_alone`.
- **The first slide moving or going:** the talk's own settings (title, colours, fonts) stay with the talk. Owned by Task 1's `the_talks_settings_stay_when_the_first_slide_moves`.
- **A kill at any moment:** during an edit, a save, or a recovery write. The talk on disk is whole, and the unsaved work comes back. Owned by Task 5's `a_kill_mid_save_leaves_the_talk_whole` and `unsaved_work_is_offered_back_after_a_crash`.
- **The file changing underneath unsaved work:** nobody's changes are thrown away without being asked. Owned by Task 4's `an_outside_change_while_unsaved_asks`.
- **Undo after anything:** after a save, after a reload, across slide operations and notes. Owned by Task 3's `undo_goes_back_through_every_kind_of_change`.

---

### Task 1: Slide operations as source edits

**Files:**
- Create: `crates/format/src/slide_ops.rs`, `crates/format/tests/slide_ops.rs`
- Modify: `crates/format/src/{lib,edit}.rs`, `crates/format/Cargo.toml`

**Interfaces:**
- Produces, in `slides_format`:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Change { pub edits: Vec<SourceEdit> }`: edits whose ranges are all in the text before any is applied, sorted, not overlapping. `Change::apply(&self, source: &str) -> String`, `Change::inverse(&self, source: &str) -> Change` (ranges in the text after applying), `Change::single(edit: SourceEdit) -> Change`, `Change::is_empty`.
  - `pub const TALK_SETTINGS: [&str; 6] = ["title", "color.background", "color.text", "color.accent", "font", "font.mono"]`
  - `pub fn insert_slide(talk: &Talk, after: Option<usize>, text: &str) -> Change`: `text` is the new slide's own content, with no break line. `None` puts it first.
  - `pub fn delete_slide(talk: &Talk, index: usize) -> Option<Change>`: `None` for the only slide.
  - `pub fn duplicate_slide(talk: &Talk, index: usize) -> Change`
  - `pub fn move_slide(talk: &Talk, from: usize, to: usize) -> Change`: `to` is the index it has afterwards.
  - `pub fn set_hidden(talk: &Talk, index: usize, hidden: bool) -> Change`: the slide setting `hidden: true`, removed again when false. `pub fn is_hidden(slide: &Slide) -> bool`.
  - `pub fn remove_setting(source: &str, slide: &Slide, key: &str) -> Option<SourceEdit>`: removes the entry's whole line, and the whole block if it was the only entry.
  - `pub fn set_notes(talk: &Talk, index: usize, notes: &str) -> Change`: replaces what's inside `<Notes>`, adds the element at the slide's end if there's none, and removes the element when `notes` is empty. Text that would end the element early (`</Notes`) is refused by writing it with a zero-width break the parser reads back as the same text; if that can't be done exactly, return the notes with `</Notes` made harmless and say how in the doc comment.

**Rules:**
- A slide's bytes move as they are. A break line (`---` and its line ending) is added or removed only where two slides now meet or part. The talk's line ending is the first one found in the file, or `\n`.
- The first slide has no break line before it unless it has a settings block. When another slide becomes first, or the first slide is deleted or moved away, the entries named in `TALK_SETTINGS` are taken out of the old first slide's block and put in the new first slide's block, in their old order, byte for byte in their values. A first slide whose block only held talk settings loses the block.
- Duplicating the first slide gives the copy none of the talk's settings.

- [ ] **Step 1: Write the failing tests**

Unit tests, each on `\n` and on `\r\n` text, comparing whole strings:
- `a_slide_is_added_after_another`, `a_slide_is_added_first_and_the_talks_settings_stay_first`, `a_slide_is_added_to_a_talk_with_no_newline_at_its_end`.
- `a_slide_is_deleted`, `the_only_slide_cannot_be_deleted`, `deleting_the_first_slide_keeps_the_talks_settings`.
- `a_slide_is_duplicated_with_its_settings_and_notes`, `the_first_slides_copy_has_none_of_the_talks_settings`.
- `a_slide_moves_down`, `a_slide_moves_up`, `a_slide_moves_to_where_it_is` (an empty change), `the_talks_settings_stay_when_the_first_slide_moves`.
- `a_slide_is_hidden_and_shown_again` (the second gives back the first text exactly), `hiding_adds_a_block_to_a_slide_with_none`.
- `notes_are_added_changed_and_removed`, `notes_with_markdown_in_them_come_back_as_written`, `notes_cannot_end_their_own_element`.
- `a_break_inside_a_code_fence_is_not_a_slide`.
- `a_change_and_its_inverse_give_back_the_same_bytes`.

Property tests with proptest, over talks generated from 1 to 8 slides made of headings, paragraphs, lists, fenced code holding `---`, settings blocks, notes, either line ending, with and without a BOM and a final newline:
- `slide_operations_leave_other_slides_bytes_alone`: for a random operation, every slide not named by it has the same bytes after as before, apart from the talk settings rule.
- `every_operation_undoes_exactly`.
- `the_result_always_parses_to_the_slides_expected`: counts and order, by a marker in each slide.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test slide_ops`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/format Cargo.lock
git commit -m "Format: adding, removing, moving and hiding slides as source edits"
```

---

### Task 2: Hidden slides and thumbnails in the engine

**Files:**
- Modify: `crates/engine/src/{deck,lib}.rs`, `crates/pdf/src/lib.rs`, `crates/pptx/src/write.rs`, `crates/app/src/{document,present}.rs`
- Test: `crates/engine/tests/deck.rs`, `crates/pdf/tests/*.rs`, `crates/pptx/tests/write.rs`, `crates/app/tests/ui_present.rs`

**Interfaces:**
- Consumes: `is_hidden` (Task 1).
- Produces, in `slides_engine`:
  - `Deck::is_hidden(&self, index: usize) -> bool`, `Deck::shown_slides(&self) -> Vec<usize>` (all of them if every slide is hidden, so a show is never empty).
  - `Deck::thumbnail(&mut self, index: usize, width: u32) -> Arc<tiny_skia::Pixmap>`: the whole slide (its last step) composed onto one pixmap `width` wide and 9/16 as tall, kept per slide until that slide changes. Kept thumbnails count against the drawn-layer memory budget.
- A show and both exports skip hidden slides. The PowerPoint export instead writes a hidden slide with `show="0"` on its `p:sld`, as PowerPoint itself does, so nothing is lost. The PDF leaves them out.
- `Document`'s `TalkShape` gives the shown slides only, and a show's "3 / 7" counts shown slides.

- [ ] **Step 1: Write the failing tests**

- `a_hidden_slide_is_left_out_of_a_show`, `a_talk_with_every_slide_hidden_still_shows_them`.
- `a_hidden_slide_is_left_out_of_a_pdf`, `a_hidden_slide_is_hidden_in_powerpoint_too`.
- `a_thumbnail_is_the_whole_slide_at_the_width_asked_for`: 320 gives 320×180, and a pixel inside a filled box has the box's colour.
- `a_thumbnail_is_kept_until_its_slide_changes`: `Arc::ptr_eq` across two calls, and a new one after `update` changes that slide only.
- `thumbnails_stay_within_the_memory_budget`.
- `a_thumbnail_of_a_slide_that_builds_shows_its_last_step`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test deck`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine crates/pdf crates/pptx crates/app
git commit -m "Engine: hidden slides, and a thumbnail of each slide"
```

---

### Task 3: The edit history

**Files:**
- Create: `crates/app/src/history.rs`, `crates/app/tests/history.rs`
- Modify: `crates/app/src/{document,lib}.rs`
- Test: `crates/app/tests/document_state.rs`

**Interfaces:**
- Consumes: `Change` and the slide operations (Task 1).
- Produces, in `stagepack_studio`:
  - `history::History`: `new() -> History`; `record(&mut self, label: &'static str, undo: Change, current_after: usize)`; `undo(&mut self) -> Option<Step>`; `redo(&mut self) -> Option<Step>`; `can_undo`, `can_redo`; `undo_label(&self) -> Option<&'static str>`, `redo_label`; `mark_saved(&mut self)`; `is_saved(&self) -> bool`. `Step { change: Change, current: usize }`. At most 1,000 steps and 64 MiB of edit text are kept; the oldest go first, and once one has gone, `is_saved` can't become true again by undoing past it.
  - `Document::edit(&mut self, label: &'static str, change: Change) -> Vec<usize>`: applies it to the text, updates the deck, records the inverse, and returns the slides to show again. An empty change records nothing.
  - `Document::undo(&mut self) -> Option<Vec<usize>>`, `Document::redo`, `Document::can_undo`, `can_redo`, `undo_label`, `redo_label`, `Document::has_unsaved_changes(&self) -> bool`.
  - `Document::talk(&self) -> &slides_format::Talk` (through a new `Deck::talk`).
  - Slide commands on `Document`, each one `edit` with a label: `add_slide(&mut self, layout: &str)` (after the current slide, and makes it current; its text is a settings block naming the layout unless it's `content`, and an empty heading line `# `), `duplicate_current`, `delete_current`, `move_slide(from, to)`, `set_current_hidden(bool)`, `set_notes(index, &str)`, `select(index)`.
  - After an edit, undo or redo, `current` is the slide the change is about, so the person sees what changed.
- `Document::reload` (text from disk) clears nothing when the text is the same. When the text differs and there are no unsaved changes, it takes the text and keeps the history only if... no: it clears the history, since its ranges no longer fit. Task 4 decides what happens when there are unsaved changes.

Labels are what Undo shows: "Add slide", "Duplicate slide", "Delete slide", "Move slide", "Hide slide", "Show slide", "Edit notes".

- [ ] **Step 1: Write the failing tests**

`history.rs`:
- `undo_then_redo_gives_the_same_text`, `a_new_edit_drops_what_could_be_redone`.
- `saved_is_true_again_after_undoing_back_to_it`, `and_false_after_redoing_past_it`.
- `the_oldest_steps_go_when_there_are_too_many`, `saved_cannot_come_back_once_its_step_has_gone`.
- `labels_name_the_next_undo_and_redo`.

`document_state.rs`:
- `undo_goes_back_through_every_kind_of_change`: add, duplicate, move, hide, notes and delete in a row, then undo each, comparing the text at every step with what it was, and then redo them all.
- `the_current_slide_follows_what_changed`: after deleting slide 3 and undoing, the current slide is 3.
- `adding_a_slide_makes_it_the_current_one`, `deleting_the_last_slide_moves_to_the_one_before`.
- `the_only_slide_cannot_be_deleted`.
- `a_reload_from_disk_empties_the_history`.
- `typing_notes_in_a_row_is_one_step`: `set_notes` calls on the same slide with nothing else in between replace the last step rather than adding one, so one Undo takes back the typing.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test history`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/engine/src/deck.rs
git commit -m "App: a history of edits, with undo and redo"
```

---

### Task 4: Save, Save As, and changes from outside

**Files:**
- Create: `crates/app/src/saving.rs`, `crates/app/tests/saving.rs`
- Modify: `crates/app/src/{document,lib,workspace_view,watcher}.rs`, `crates/engine/src/packed.rs`
- Test: `crates/engine/tests/packed.rs`, `crates/app/tests/ui_slides.rs`

**Interfaces:**
- Consumes: `History::mark_saved`, `Document::has_unsaved_changes` (Task 3), `write_atomically`, `pack_folder`.
- Produces:
  - `slides_engine::repack_with_slides(packed: &Path, out: &Path, slides: &str, app_version: &str) -> Result<(), PackError>`: a copy of a packed talk with a new `slides.md`, every other entry byte for byte, written atomically. `out` may be `packed` itself.
  - `slides_engine::copy_talk_folder(from: &Path, to: &Path, slides: &str) -> Result<Vec<String>, PackError>`: `to` must be missing or empty. Copies what `pack_folder` would pack, with the same refusals, and returns what was left out.
  - `saving::SaveJob`, built on the UI thread from a `Document` and run anywhere: `Document::save_job(&self) -> SaveJob`, `Document::save_as_job(&self, target: SaveTarget) -> SaveJob`, `SaveJob::run(self) -> Result<Saved, String>`, `Document::saved(&mut self, saved: Saved)`, which marks the history saved and, for Save As, makes the new place the talk's place. `SaveTarget { Folder(PathBuf), Packed(PathBuf) }`.
  - `Document::stamp_on_disk`: what the app last wrote or read, so that the watcher's event for the app's own save isn't taken for someone else's change.
  - `Reload` gains `Conflict`: the file changed on disk while there are unsaved changes. Nothing is taken.
  - `Document::take_theirs(&mut self, text: String)` and `Document::keep_mine(&mut self)`, which answers the question until the file changes again.
- In the window: File → Save (`ctrl-s`), Save As… (`ctrl-shift-s`), and the title shows a dot before the name while there are unsaved changes. A bar (`OUTSIDE_CHANGE_ID`) says "This talk was changed outside the app." with **Keep my changes** and **Load the changed file**. Closing the window with unsaved changes asks: **Save**, **Don't save**, **Cancel** (`on_window_should_close` and the kit's dialog). Save As asks with the system dialog, offering "Talk folder" and "Packed talk (.sptx)".

Saving a folder talk writes only `slides.md`. Saving runs on the background executor. A failed save says why and leaves the talk unsaved. "Save as packed talk" and "Save as folder" from plan B become Save As's two kinds, and their menu items go.

- [ ] **Step 1: Write the failing tests**

`packed.rs`: `a_repacked_talk_has_the_new_slides_and_everything_else_as_it_was`, `repacking_over_itself_is_atomic`, `a_folder_is_copied_with_the_new_slides`.

`saving.rs`:
- `saving_writes_the_text_and_nothing_else`, `saving_keeps_the_files_line_endings` (they're the text's own).
- `a_failed_save_leaves_the_talk_unsaved_and_the_file_as_it_was` (a read-only folder, unix).
- `saving_a_packed_talk_rewrites_the_pack`.
- `save_as_a_folder_then_edits_go_there`, `save_as_packed_from_a_folder`.
- `the_apps_own_save_is_not_an_outside_change`.
- `an_outside_change_with_nothing_unsaved_is_taken`.
- `an_outside_change_while_unsaved_asks`: `Reload::Conflict`, and the text is still mine.
- `loading_theirs_drops_mine_and_empties_the_history`, `keeping_mine_then_saving_writes_mine`.

`ui_slides.rs`: `the_title_shows_unsaved_changes`, `ctrl_s_saves`, `the_outside_change_bar_offers_both`, `closing_with_unsaved_changes_asks`, `dont_save_closes_without_writing`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test saving`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/engine
git commit -m "App: Save and Save As, and asking when the file changes underneath"
```

---

### Task 5: Recovery after a crash

**Files:**
- Create: `crates/app/src/recovery.rs`, `crates/app/tests/recovery.rs`
- Modify: `crates/app/src/{lib,workspace_view,document}.rs`

**Interfaces:**
- Consumes: `Document::has_unsaved_changes`, `Document::source`, `Document::place`, the app's data folder as `permissions.rs` finds it.
- Produces, in `stagepack_studio::recovery`:
  - `pub struct Recovery { root: PathBuf }`, `Recovery::new(root)`, `Recovery::default_root() -> Option<PathBuf>` (`recovery` in the app's data folder).
  - `keep(&self, place: &TalkPlace, base_hash: &str, text: &str) -> io::Result<()>`: writes `<root>/<key>/slides.md` and `<root>/<key>/about.json` (the talk's path, the SHA-256 of the file the edits started from, and the time), each atomically. `<key>` is the first 16 hex characters of the SHA-256 of the talk's canonical path.
  - `clear(&self, place: &TalkPlace)`, `find(&self, place: &TalkPlace) -> Option<Found>` with `Found { text: String, base_matches: bool, when: SystemTime }`.
  - `sweep(&self, older_than: Duration)`: removes working copies older than 30 days.
- The window keeps the working copy up to date one second after the last edit, on the background executor, and clears it on a save and on closing with "Don't save". On opening a talk with a working copy, a bar (`RECOVERY_ID`) says "Changes from last time weren't saved." with **Bring them back** and **Discard**. If the file on disk has changed since, it adds "The talk has also changed since then." Bringing them back makes them one undoable edit ("Recover changes"), so nothing on disk changes until Save.
- A working copy larger than 64 MiB isn't read.

- [ ] **Step 1: Write the failing tests**

- `a_working_copy_is_kept_and_found`, `it_is_cleared_by_a_save`, `and_by_choosing_not_to_save`.
- `unsaved_work_is_offered_back_after_a_crash`: edit, keep, drop the document without saving, open again, and the bar offers it; **Bring them back** gives the edited text, unsaved.
- `discard_removes_it`.
- `a_talk_changed_since_says_so`.
- `two_talks_with_the_same_folder_name_do_not_mix`.
- `a_kill_mid_save_leaves_the_talk_whole`: with a temp file left beside `slides.md` as an interrupted `write_atomically` would leave it, the talk opens as it was and the working copy is offered.
- `a_broken_about_file_is_ignored`, `an_enormous_working_copy_is_ignored`.
- `old_working_copies_are_swept`.
- `the_working_copy_follows_edits_after_a_moment` (UI test with the test clock).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test recovery`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: unsaved work is kept, and offered back after a crash"
```

---

### Task 6: The editor's window: ribbon, slide strip and notes

**Files:**
- Create: `crates/app/src/{ribbon,slide_strip,notes_panel}.rs`, `crates/app/tests/ui_editor.rs`
- Modify: `crates/app/src/{lib,workspace_view}.rs`

**Interfaces:**
- Consumes: `Deck::thumbnail` (Task 2), the `Document` commands (Task 3), saving (Task 4).
- Produces:
  - `ribbon::Ribbon`, a view with tabs **Home**, **Insert**, **Animate**, **Slide Show**, **View**, each a row of labelled groups of buttons with icons and text. Ids: `RIBBON_ID`, `TAB_HOME_ID`, `TAB_INSERT_ID`, `TAB_ANIMATE_ID`, `TAB_SLIDE_SHOW_ID`, `TAB_VIEW_ID`, and one per button.
    - Home: *Slides* group: **New slide** (a split button: the click adds a `content` slide, the arrow lists the five layouts by their plain names), **Duplicate**, **Delete**, **Hide** (reads **Show** on a hidden slide). *History* group: **Undo**, **Redo**, each disabled when there's nothing, with the step's label in its tooltip ("Undo Delete slide").
    - Insert and Animate: one muted line, "Tools for this tab are on the way."
    - Slide Show: **From the start** (F5), **Rehearse** (Shift+F5).
    - View: toggles **Notes**, **Slide list**, **Markdown**.
  - `slide_strip::SlideStrip`, a view 220 wide on the left, resizable between 140 and 400: one row per slide with its number, its thumbnail, and a muted look with a crossed-eye icon when hidden. The current slide has an accent outline. It's a virtual list, and thumbnails are made for the rows on screen, a few per frame, on the UI thread's idle time. Ids: `STRIP_ID`, and `strip_row_id(index)`.
  - `notes_panel::NotesPanel`, under the slide, 160 tall, resizable, a multi-line text field with the placeholder "Notes for this slide". Id `NOTES_ID`. Its text is the current slide's notes; typing calls `Document::set_notes`; changing slide swaps the text.
- The File and Export menus stay in the title bar. The Present button moves to the Slide Show tab.
- Keys, in the window's context: `ctrl-z` Undo, `ctrl-shift-z` and `ctrl-y` Redo, `ctrl-m` New slide, `ctrl-d` Duplicate, `ctrl-shift-m` Markdown (moved from `ctrl-m`). With the strip focused: `up`/`down` select, `delete` and `backspace` delete, `ctrl-up`/`ctrl-down` move the slide. `left`/`right`/`pageup`/`pagedown` still change slide when the canvas has the focus, and never while the notes field does.

- [ ] **Step 1: Write the failing tests**

- `the_window_has_its_tabs_and_the_home_tab_is_open`, `each_tab_shows_its_own_tools`.
- `the_strip_has_a_row_for_each_slide_and_marks_the_current_one`.
- `clicking_a_row_shows_that_slide`.
- `new_slide_adds_one_after_the_current_and_selects_it`, `the_layout_list_adds_a_slide_of_that_layout`.
- `duplicate_delete_and_hide_work_from_the_ribbon`, `delete_is_disabled_for_the_only_slide`.
- `undo_and_redo_are_disabled_until_there_is_something`, `undo_names_what_it_will_undo`.
- `the_keys_do_what_the_buttons_do`.
- `typing_in_notes_changes_the_slides_notes_and_one_undo_takes_it_back`.
- `arrow_keys_in_the_notes_field_do_not_change_slide`.
- `changing_slide_swaps_the_notes`.
- `the_view_tab_hides_and_shows_each_panel`.
- `a_thumbnail_follows_an_edit_to_its_slide`.
- `a_talk_of_a_thousand_slides_opens_without_making_a_thousand_thumbnails`.
- `a_packed_talk_can_be_edited_the_same_way`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_editor`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

`workspace_view.rs` is already 1,000 lines. Move what you touch into the new views, and leave it the job of putting them together.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: an editor's window, with tool tabs, a slide list and notes"
```

---

### Task 7: Reordering by dragging, and the slide's menu

**Files:**
- Modify: `crates/app/src/slide_strip.rs`
- Test: `crates/app/tests/ui_editor.rs`

**Interfaces:**
- Consumes: `SlideStrip`, `Document::move_slide` (Tasks 3 and 6).
- Produces: dragging a row shows the thumbnail under the pointer and a line between rows where it will land, and dropping moves the slide there as one undoable step. The strip scrolls when the pointer nears its top or bottom during a drag. A right-click on a row opens a menu: **New slide**, **Duplicate**, **Delete**, **Hide** or **Show**, **Move up**, **Move down**. `Escape` during a drag cancels it.

- [ ] **Step 1: Write the failing tests**

- `dragging_a_row_down_moves_the_slide_there`, `dragging_a_row_up`, `dropping_a_row_on_itself_changes_nothing` (and records no undo step).
- `the_line_shows_where_it_will_land`.
- `escape_cancels_a_drag`.
- `one_undo_puts_a_dragged_slide_back`.
- `the_rows_menu_does_each_thing`.
- `move_up_is_disabled_for_the_first_slide`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_editor drag`
Expected: FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: drag a slide to reorder it, and a menu on each slide"
```

---

### Task 8: Run it for real and write down what happened

**Files:**
- Create: `docs/checks/plan-d1.md`
- Modify: `README.md`, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md`

No new behaviour. Fix what the run finds, each with a test.

- [ ] **Step 1: Build**

Run: `scripts/cargo-safe.sh build --release -p stagepack-studio`

- [ ] **Step 2: Drive it in a real window**

On a nested display (Xephyr on :80), with the app's data folder pointed at a scratch folder, on scratch copies of `fixtures/showcase` and `fixtures/components`:
1. Open a talk. Look at the whole window: tabs, strip, slide, notes. Say honestly how it looks, and fix what's ugly or cramped: spacing, alignment, text cut off, icons missing.
2. Add a slide of each layout, duplicate one, hide one, delete one, drag one to a new place, and type notes. Undo all of it and redo all of it.
3. Save, and compare `slides.md` with the original using `git diff --no-index`: only what was edited differs.
4. Edit again, and kill the app with `kill -9`. Open the talk: the bar offers the changes back.
5. Edit, then change `slides.md` from outside: the bar asks.
6. Save As a packed talk, close, open the packed talk, edit and save it.
7. Open a generated talk of 500 slides, scroll the strip fast, and record the memory and how it feels.
8. Start a show: hidden slides are skipped.

Look at every screenshot you rely on.

- [ ] **Step 3: Write `docs/checks/plan-d1.md`**

Like `docs/checks/plan-c.md`: test counts per crate, what the real run showed with its numbers, what wasn't exercised (Windows, the system's Save dialog on the nested display, a real second screen), and the by-hand Windows check.

- [ ] **Step 4: Update the README and the roadmap's D1 line**

- [ ] **Step 5: Commit**

```bash
git add docs/checks/plan-d1.md README.md docs/superpowers/plans/2026-11-09-native-rust-roadmap.md
git commit -m "Plan D1 check: what ran, and what to check by hand"
```
