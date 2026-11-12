# Plan D2: Text Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Click on a slide and type. Empty regions say "Click to add text". Selected words can be made bold, italic, underlined, larger or coloured, paragraphs can become bullets or numbers, and text and slides can be copied and pasted, also between talks.

**Architecture:** The text of a region or box is edited as a small rich-text model: paragraphs of styled spans. Editing operations on it are pure functions. After each change the model is written back as Markdown, changing only the paragraphs that changed, through the same `Document` history as every other edit. The engine lays the slide out as before and answers geometry questions (where is the caret, what's under the pointer), so there's one text layout, not two. Three pieces of groundwork from D1's final review come first.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 (its `color_picker`, `select`, `button` and `EntityInputHandler`), cosmic-text 0.19.0 through the engine, proptest, and the existing crates. The typing spike on the branch `spike/native-rust` (`spikes/typing`) proved writing styled spans back to Markdown, and its `spans.rs` is the starting point for the writer.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`: goals 2 and 9; "The Markdown"; "Layouts and moving things" (empty regions show "Click to add text" in the editor and nothing when presenting or exporting); "The editor" (typing on the slide, writing text back); and the anti-goals *Rewriting the user's text*, *Editing the wrong thing*, *Code in the way* and *Losing work*. `docs/superpowers/plans/2026-11-11-native-rust-plan-d1-decisions.md` has D1's rulings and what it left for this plan.

## Global Constraints

- Work on the branch `plan-d2`, cut from `main`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx` and `slides-components` don't depend on GPUI.
- **A visual edit changes only the bytes of what was edited.** Typing in one paragraph rewrites that paragraph's inline content and nothing else: not its heading marker or list marker unless that changed, not the paragraphs beside it, not another slide. Line endings, a BOM and trailing spaces elsewhere survive.
- **What's written reads back the same.** For any text model the editor can produce, writing it as Markdown and parsing it gives back the same paragraphs and spans. Property-tested.
- **If the app can't be sure which text a click maps to, it refuses and says so.** Text the model can't represent exactly (a paragraph holding an inline component or picture, a table, a code block, a block quote) isn't editable in this plan, and clicking it says so in plain words, once.
- **Undo is exact**, and a run of typing is one undo step.
- A talk is untrusted input. Nothing in one may crash the app. A format or engine panic during an edit becomes a refused edit.
- Typing must feel immediate: one keystroke on a 500-slide talk costs under 8 ms before drawing, measured in a test in release.
- Nobody is made to see Markdown. Every label and message is plain words. Markdown symbols typed as text stay text.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless GPUI test.
- An idle window does no work, apart from the caret's blink while text is being edited, which stops after ten seconds without input.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: inserting pictures, shapes, tables, links and components (D3); moving, resizing and rotating things, and the Animate tab (D4); templates (D5); right-to-left text and Chinese, Japanese or Korean input (after the first release); find and replace; spell check.

Rulings made in the plan:
- **Alignment belongs to the region or box**, not to each paragraph: Markdown has no paragraph alignment, and a region's place is already a line in the slide's settings. It's written as `align=left|center|right` on the `region.<name>` line, and as an `align` attribute on a positioned tag.
- **A text box that's turned is edited as it lies**, with the caret turned with it. The design's "shown upright while typing" waits for D4, which owns rotation.
- **Pasting a slide into another talk brings its text.** Pictures and components it uses that the other talk doesn't have show as placeholders with a notice. Carrying assets across is D3's, which builds the way to add a file to a talk.

## Review Focus

- **Hand-written Markdown under the caret:** a paragraph wrapped over several lines, one with `**` that can't close, escaped symbols, a link, inline code, a hard break, `<Text>` tags nested or with attributes the editor doesn't know. Typing one letter keeps everything else in the paragraph meaning what it meant. Owned by Task 4's `typing_a_letter_keeps_the_rest_of_a_hand_written_paragraph`.
- **Markdown symbols typed as text:** `*`, `_`, `#`, `-`, `1.`, `<`, `` ` ``, `[`, `\`, `---` at the start of a line, a line of only spaces. They stay text and never become structure or break a slide. Owned by Task 4's `anything_typed_reads_back_as_typed`.
- **Keys with the caret at an edge:** Backspace at the start of the first paragraph, Delete at the end of the last, Enter in an empty list item, a selection across paragraphs replaced by typing. Owned by Task 5's `edges_never_lose_or_invent_text`.
- **Input methods and odd characters:** dead keys, a composition cancelled half-way, emoji and other characters outside the basic plane, combining marks, a paste of a megabyte. Owned by Task 8's `a_cancelled_composition_leaves_no_text` and Task 5's `carets_never_land_inside_a_character`.
- **Saving, closing or reloading with text half-typed:** nothing typed is lost, and a composition in progress is ended first. Owned by Task 7's `text_being_typed_is_in_the_talk_before_a_save`.

---

### Task 1: Groundwork: fast edits

**Files:**
- Create: `crates/format/src/reparse.rs`, `crates/format/tests/reparse.rs`, `crates/app/tests/edit_speed.rs`
- Modify: `crates/format/src/{lib,parse}.rs`, `crates/engine/src/deck.rs`, `crates/app/src/document.rs`
- Test: `crates/engine/tests/deck.rs`, `crates/app/tests/document_state.rs`

**Interfaces:**
- Produces, in `slides_format`: `pub fn reparse(old: &Talk, new_source: String) -> Talk`. It splits the new text, reuses every slide whose bytes are the same as a slide of `old` (matched in order, from both ends), with its ranges shifted, and parses only the rest. Its result always equals `parse_talk(&new_source)`.
- Produces, in `slides_engine`: `Deck::update_text(&mut self, talk: Talk) -> Vec<usize>`, an update for an edit made in the app: it doesn't stat pictures or rescan components. Slides are matched to the old ones by their text, from both ends, so a slide added or removed in the middle keeps every other slide's version, layout and drawn layers.
- `Document::make` parses its candidate with `reparse`, checks it, and updates the deck once, with `update_text`. A refused edit doesn't touch the deck. Undo, redo, `recover`, `take_theirs`, opening and reloads parse under the same panic guard as edits, and a panic there is reported in plain words and leaves the talk as it was (or, on opening, shows the problem instead of the talk).

- [ ] **Step 1: Write the failing tests**

- `reparse.rs`, property tests over the generator in `tests/slide_ops.rs` (move it to a shared `tests/common/mod.rs`): `reparse_equals_a_full_parse` for a random edit (a slide operation, a notes edit, or a random splice of bytes anywhere, including ones that add or remove a `---` or a code fence); and `unchanged_slides_are_not_parsed_again`, counted through a test-only counter of slides parsed.
- `deck.rs`: `a_slide_added_in_the_middle_keeps_the_others_drawn` (`Arc::ptr_eq` on a later slide's layers, and its version unchanged); `an_edit_made_in_the_app_reads_no_files` (a counting `TalkFiles`).
- `document_state.rs`: `a_refused_edit_leaves_the_deck_alone` (slide versions unchanged); `a_panic_while_undoing_leaves_the_talk_as_it_was`.
- `edit_speed.rs`, `#[ignore = "timing; run in release"]`: `a_keystroke_on_a_long_talk_is_quick`: on a generated 500-slide talk, 200 single-character notes edits through `Document`, median under 8 ms.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test reparse`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, then `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`
Expected: PASS. Put the median in the report.

- [ ] **Step 5: Commit**

```bash
git add crates/format crates/engine crates/app
git commit -m "Edits made in the app read only what changed"
```

---

### Task 2: Groundwork: an edit touches one slide, and the window's save state

**Files:**
- Create: `crates/app/src/{save_flow,working_copies}.rs`, `crates/app/tests/save_flow.rs`
- Modify: `crates/app/src/{document,lib,workspace_view}.rs`, `crates/app/src/workspace_view/{saving,editing}.rs`
- Test: `crates/app/tests/document_state.rs`

**Interfaces:**
- Produces: `Document::edit_slide(&mut self, label: &'static str, index: usize, change: Change, run: EditRun) -> Result<Vec<usize>, EditRefused>`. It refuses, changing nothing, unless every other slide's bytes are the same before and after and the slide count is the same. `EditRun { New, Continue(RunId) }` says whether this joins the open run in the history; it returns the run through `Document::open_run(&self) -> Option<RunId>`. `set_notes`, `set_current_hidden` and every text edit in this plan go through it.
- Produces `save_flow::SaveFlow`, a plain state machine with no GPUI: `SaveFlow::Idle | Saving { again: bool, then_close: bool, reload_after: bool }`, with `ask_save(&mut self) -> Vec<Effect>`, `ask_close(&mut self, unsaved: bool) -> Vec<Effect>`, `read_arrived(&mut self) -> Vec<Effect>` and `finished(&mut self, outcome: SaveOutcome, still_unsaved: bool) -> Vec<Effect>`. `Effect { StartSave, Reload, AskAboutOutsideChange, Close, AskBeforeClosing, Say(String) }`.
- Produces `working_copies::WorkingCopies`, owning the recovery handle, what's kept, what was adopted, the count of clears and the job chain, with `talk_changed`, `saved(place)`, `let_go(place)`, `brought_back(found)` and `settled() -> Task<()>`. The view keeps no working-copy fields.
- Behaviour doesn't change. Every existing saving and recovery test passes unchanged, apart from how tests reach the moved state.

- [ ] **Step 1: Write the failing tests**

- `document_state.rs`: `an_edit_that_changes_another_slide_is_refused`; `a_typed_code_fence_cannot_swallow_the_next_slide` (a change that opens a fence on slide 1 so that slide 2's break disappears and a `---` inside slide 3's fence becomes one: same count, refused).
- `save_flow.rs`, a table of event sequences against expected effects, covering each case D1's reviews found: save during a save; close and Save during a save with edits made meanwhile; a read held back by a save; an outside change found by the save; a failed save while closing.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test save_flow`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: an edit is held to its slide, and saving has a state machine of its own"
```

---

### Task 3: The text model, read from a slide

**Files:**
- Create: `crates/format/src/rich.rs`, `crates/format/tests/rich_read.rs`
- Modify: `crates/format/src/{lib,parse,model}.rs`

**Interfaces:**
- Produces, in `slides_format`:
  - `pub enum ParaKind { Body, Heading(u8), Bullet { level: u8 }, Numbered { level: u8 } }`
  - `pub struct Para { pub kind: ParaKind, pub spans: Vec<Span> }`. An empty paragraph has no spans. Spans next to each other with the same style are merged, and no span is empty.
  - `pub struct TextFlow { pub place: FlowPlace, pub paras: Vec<Para>, pub source: Vec<ParaSource> }`
  - `pub enum FlowPlace { Region(String), Tag(Range<usize>) }`: the layout region the text is in, or the positioned tag (by its range) holding it.
  - `pub struct ParaSource { pub block: Range<usize>, pub inline: Range<usize> }`: the whole block's bytes (marker included) and the bytes of its inline content.
  - `pub fn text_flows(talk: &Talk, slide: usize) -> Vec<TextFlow>`: each run of editable text on the slide. A region's text is one flow per unbroken run of headings, paragraphs and lists; anything else (a table, code, a picture, a tag, a block quote, a paragraph holding an inline tag other than `<Text>` or an inline picture) ends the run and belongs to no flow.
  - `pub fn uneditable_reason(talk: &Talk, slide: usize, at: usize) -> Option<&'static str>`: for a byte position inside text that's in no flow, the plain sentence to show, such as "Tables can't be edited on the slide yet."
- `Span.style` gains nothing. A hand-wrapped paragraph's line breaks must read as they're drawn: check what the parser and the layout do today with a soft line break, make the model match what's drawn, and say what you found.

- [ ] **Step 1: Write the failing tests**

- `a_title_and_body_are_two_flows`, `a_list_reads_as_bullets_with_levels`, `a_numbered_list_reads_as_numbered`, `a_table_splits_the_text_around_it`.
- `styles_read_from_markdown_and_from_text_tags` (bold, italic, underline, size, colour, a link, inline code, nested).
- `inline_ranges_cover_exactly_the_content`: for each paragraph, the source at `inline` holds no marker, and at `block` holds the marker.
- `text_in_a_box_is_a_flow_of_its_own`, `a_region_marker_starts_a_new_flow`.
- `a_paragraph_with_a_component_in_it_is_in_no_flow`, and `uneditable_reason` names tables, code and quotes.
- `an_empty_slide_has_no_flows`.
- Property: `flows_never_overlap_and_stay_inside_their_slide`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test rich_read`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/format
git commit -m "Format: a slide's text as paragraphs of styled spans"
```

---

### Task 4: Writing the text model back

**Files:**
- Create: `crates/format/src/rich_write.rs`, `crates/format/tests/rich_write.rs`
- Modify: `crates/format/src/lib.rs`

**Interfaces:**
- Consumes: `TextFlow`, `Para`, `ParaKind`, `ParaSource` (Task 3).
- Produces:
  - `pub fn write_inline(spans: &[Span]) -> String`: Markdown for one paragraph's content. Ported from the spike's `spans.rs` and widened to links, inline code, hard breaks and `<Text>` for size, colour and underline. Where `**` or `*` can't close, it falls back to `<Text bold="true">`. Every character Markdown would read as structure is escaped.
  - `pub fn write_flow(talk: &Talk, slide: usize, flow: &TextFlow, paras: &[Para]) -> Change`: the edits that turn the flow's paragraphs into `paras`. A paragraph that's equal to one it had keeps its bytes. One whose spans changed but whose kind didn't has only its `inline` range replaced. Added and removed paragraphs are written with the list markers, numbering, indentation and blank lines the flow's surroundings use.
  - `pub fn write_into_region(talk: &Talk, slide: usize, region: &str, paras: &[Para]) -> Change`: text for a region that has none. The title's is a heading. Another region's goes after a `<Region name="…" />` line, which is added if needed.

- [ ] **Step 1: Write the failing tests**

- Property `what_is_written_reads_back_the_same`: for random paragraphs (kinds, levels, spans with every style, text drawn from an alphabet heavy in Markdown's own symbols, emoji and combining marks), `write_flow` on a random talk then `text_flows` gives the same paragraphs.
- Property `anything_typed_reads_back_as_typed`: plain text of any printable characters, as one paragraph, including text that starts with `#`, `-`, `1.`, `>`, `---`, four spaces, or `<`.
- Property `only_the_changed_paragraphs_bytes_change`: change one paragraph of a flow in a hand-written talk (either line ending, odd spacing, hand-wrapped lines), and every byte outside that paragraph's `block` is the same.
- `typing_a_letter_keeps_the_rest_of_a_hand_written_paragraph`: a table of hand-written paragraphs (wrapped over three lines; `**` touching punctuation; `\*` escapes; a link; inline code; a hard break; `<Text color="red" size="64">` around bold; an unknown attribute on `<Text>`), each with one letter added to its first word. The other spans read back as they were.
- `a_new_paragraph_takes_the_lists_marker`, `numbers_run_on`, `a_paragraph_removed_takes_its_blank_line`.
- `text_goes_into_an_empty_title_as_a_heading`, `text_goes_into_the_right_column_after_its_marker`.
- `bold_that_cannot_close_falls_back_to_a_tag`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test rich_write`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Read the spike's writer first: `git show spike/native-rust:spikes/typing/src/spans.rs`.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-format`, and the property tests once with `PROPTEST_CASES=4000`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/format
git commit -m "Format: styled text written back as Markdown, a paragraph at a time"
```

---

### Task 5: Editing operations

**Files:**
- Create: `crates/format/src/rich_edit.rs`, `crates/format/tests/rich_edit.rs`
- Modify: `crates/format/src/lib.rs`

**Interfaces:**
- Consumes: `Para`, `ParaKind`, `Span`, `Style` (Task 3).
- Produces, all pure:
  - `#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] pub struct Pos { pub para: usize, pub offset: usize }`: a byte offset into a paragraph's text, always on a grapheme boundary.
  - `pub struct Selection { pub anchor: Pos, pub head: Pos }`, with `is_caret`, `start`, `end`.
  - `pub struct TextEdit { pub paras: Vec<Para>, pub selection: Selection, pub typing_style: Option<Style> }`: the style the next typed character takes when the caret sits where nothing is selected.
  - Operations returning a new `TextEdit`: `insert(&self, text: &str)` (newlines split paragraphs), `backspace`, `delete`, `enter`, `soft_break`, `toggle(StyleBit)` for `Bold | Italic | Underline`, `set_size(Option<u32>)`, `set_color(Option<String>)`, `set_kind(ParaKind)` on every paragraph the selection touches, `indent`, `outdent`, `select_all`, `select_word_at(Pos)`, `move_to(Pos, extend: bool)`, `move_by(Motion, extend: bool)` for `Left | Right | WordLeft | WordRight | ParaStart | ParaEnd | FlowStart | FlowEnd`, `paste(&[Para])`, and `copied(&self) -> Vec<Para>`.
  - `style_at(&self) -> StyleState` for the toolbar: for each of bold, italic, underline, size and colour, `All(value) | Mixed | None`, and the paragraphs' kind likewise.
- Rules: Enter in an empty bullet or number turns it into a body paragraph. Enter at the end of a heading starts a body paragraph. Backspace at the start of a bullet outdents it, then makes it a body paragraph, then joins it to the one before. Levels run 0 to 8. Nothing ever leaves an empty span, two neighbours with the same style, or a caret inside a grapheme.

- [ ] **Step 1: Write the failing tests**

- Unit tests for each operation's ordinary case and each rule above.
- `edges_never_lose_or_invent_text`: Backspace at the very start, Delete at the very end, Enter in an empty flow, typing over a selection that spans three paragraphs, a paste of three paragraphs into the middle of a word.
- `carets_never_land_inside_a_character`: motions and deletions over `e` + combining acute, a family emoji joined with zero-width joiners, a flag, and a surrogate-pair emoji.
- `typing_takes_the_style_before_the_caret`, `a_toggle_with_no_selection_styles_what_is_typed_next`.
- `style_at_says_mixed_when_it_is`.
- Properties over random operation sequences: `the_model_stays_well_formed` (no empty spans, merged neighbours, selection in range, on grapheme boundaries, levels in range); `the_text_is_what_the_operations_say` (against a plain-string model of the same operations).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test rich_edit`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Grapheme boundaries come from `unicode-segmentation`, pinned to the version already in `Cargo.lock`.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-format`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/format Cargo.lock
git commit -m "Format: editing a slide's text, as pure operations"
```

---

### Task 6: Where text is on the slide

**Files:**
- Create: `crates/engine/src/text_geometry.rs`, `crates/engine/tests/text_geometry.rs`
- Modify: `crates/engine/src/{deck,flow,slide,layouts,display,elements,lib}.rs`

**Interfaces:**
- Consumes: `text_flows`, `FlowPlace`, `Pos` (Tasks 3 and 5).
- Produces, in `slides_engine`:
  - `pub struct PlacedPara { pub flow: usize, pub para: usize, pub layer: usize, pub rect: Rect }` in slide units, before the layer's rotation.
  - `Deck::text_layout(&mut self, index: usize) -> Arc<TextLayout>`, kept with the slide's layout, with: `paras(&self) -> &[PlacedPara]`; `caret(&self, flow: usize, pos: Pos) -> Option<CaretRect>` (`x`, `top`, `height`, and the layer's `rotate` and centre, so the app can turn it); `hit(&self, x: f32, y: f32) -> Option<Hit>` for a point in slide units, where `Hit { Text { flow, pos } | Uneditable(&'static str) | Placeholder(String) | Nothing }`; `selection_rects(&self, flow, start: Pos, end: Pos) -> Vec<Rect>`; `line_move(&self, flow, pos: Pos, direction: Up | Down | LineStart | LineEnd, wanted_x: Option<f32>) -> Pos`.
  - `Deck::editor_layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>>`: the slide's whole state plus, for each region of its layout with no content, a layer saying "Click to add title" for the title region or "Click to add text" otherwise, in a muted tint of the text colour with a dashed outline. `Deck::layers`, shows, thumbnails and both exports never include them.
  - Alignment: `align=left|center|right` on a `region.<name>` settings line, and an `align` attribute on a positioned tag (added to `PLACEMENT_ATTRS`), override the layout's alignment for that region or box.
- A hit on a point that's turned with its layer is turned back first. A hit inside an empty region gives `Placeholder(region name)`. A hit on a paragraph's empty space to the right of a line gives the end of that line.

- [ ] **Step 1: Write the failing tests**

- `the_caret_at_the_start_is_at_the_paragraphs_left`, `the_caret_moves_right_by_each_characters_width`, `the_caret_after_a_wrap_is_on_the_next_line`, `a_caret_in_centred_text_follows_the_alignment`.
- `a_click_on_a_letter_gives_the_gap_nearest_it`, `a_click_past_the_end_of_a_line_gives_the_lines_end`, `a_click_in_a_turned_box_is_turned_back`.
- `a_click_on_a_table_says_why_it_cannot_be_edited`, `a_click_in_an_empty_region_names_it`, `a_click_on_the_background_is_nothing`.
- `a_selection_over_two_lines_is_two_rects`, `up_and_down_keep_the_column`.
- `geometry_agrees_with_the_model`: for a slide of mixed text, every `Pos` on a grapheme boundary has a caret, and `hit` at each caret gives that `Pos` back.
- `empty_regions_say_click_to_add_text_only_in_the_editor`: in `editor_layers` and in none of `layers`, `layers_at`, `specs` or `thumbnail`.
- `a_region_can_be_centred_from_its_settings_line`, `a_box_can_be_right_aligned`.
- `list_markers_are_not_part_of_the_text`: the caret at offset 0 of a bullet sits after its marker.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test text_geometry`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

The geometry comes from the same shaped text `Flow::draw` draws, so the caret can't disagree with the letters.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS, with no golden image changed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: where a slide's text is, for a caret and a click"
```

---

### Task 7: Typing on the slide

**Files:**
- Create: `crates/app/src/{text_editing,slide_editor}.rs`, `crates/app/tests/ui_typing.rs`
- Modify: `crates/app/src/{document,lib,slide_canvas,workspace_view}.rs`, `crates/app/src/workspace_view/editing.rs`

**Interfaces:**
- Consumes: `text_flows`, `write_flow`, `write_into_region`, `TextEdit` (Tasks 3 to 5), `Deck::text_layout`, `editor_layers` (Task 6), `Document::edit_slide` (Task 2).
- Produces:
  - `text_editing::EditingText`, plain state in `Document`: which slide and flow is being edited and its `TextEdit`. `Document::begin_text(slide, flow, pos)`, `begin_text_in_region(slide, region)`, `text(&self) -> Option<&EditingText>`, `apply_text(&mut self, op: impl FnOnce(&TextEdit) -> TextEdit) -> Result<Vec<usize>, EditRefused>`, `end_text()`. `apply_text` writes the change back through `edit_slide`, then reads the flow again from the new talk and checks it equals the model. If it doesn't, the edit is undone and refused with "That couldn't be typed here, so nothing was changed."
  - A run of typing is one history step: consecutive inserts and deletes in one flow, ended by a caret move, a style change, a pause of two seconds, or anything else.
  - After Undo or Redo of a text step, the text is being edited again where the change was, with the caret where it was before that step.
  - `slide_editor::SlideEditor`, the view over the canvas: it shows `editor_layers`, a caret that blinks (and stops after ten seconds without input), selection rectangles, and takes the mouse and keys. A click on text places the caret, a double click selects the word, a triple click the paragraph, a drag selects, Shift+click extends. A click on a placeholder starts text in that region. A click on something uneditable says why on the status line. Escape, or a click on the desk, ends editing. Ids: `SLIDE_EDITOR_ID`, `CARET_ID`.
  - Keys while text is being edited: letters insert; Enter, Shift+Enter, Backspace, Delete, Tab and Shift+Tab (indent and outdent a bullet or number, and otherwise nothing); arrows, Ctrl+arrows by word, Home and End by line, Ctrl+Home and Ctrl+End by flow, each with Shift to extend; Ctrl+A. The slide-changing keys and the strip's keys don't fire.
  - Before a save, a close, a reload, a slide change, a show or an export, the text being typed is already in the talk: there is no separate buffer to flush.

- [ ] **Step 1: Write the failing tests**

Through real key and mouse events in a test window:
- `clicking_on_text_shows_a_caret_there`, `typing_adds_letters_at_the_caret_and_the_slide_shows_them`.
- `the_markdown_changes_only_in_that_paragraph` (compare the file text before and after).
- `one_undo_takes_back_a_run_of_typing`, `moving_the_caret_starts_a_new_step`, `undo_puts_the_caret_back`.
- `enter_makes_a_new_bullet`, `backspace_at_the_start_joins_paragraphs`, `tab_indents_a_bullet`.
- `a_drag_selects_and_typing_replaces_it`, `a_double_click_selects_a_word`, `shift_arrows_extend`.
- `clicking_click_to_add_title_and_typing_makes_a_heading`, `typing_in_the_right_column_adds_its_marker`.
- `a_new_slide_can_be_typed_into_straight_away`: New slide, click the title placeholder, type, click the body placeholder, type.
- `clicking_a_table_says_it_cannot_be_edited_yet`.
- `escape_ends_editing`, `arrow_keys_move_the_caret_not_the_slide`.
- `text_being_typed_is_in_the_talk_before_a_save` (type, Ctrl+S with no pause, and the file has it), and the same for a close answered with Save and for a show.
- `the_working_copy_follows_typing`, `the_caret_stops_blinking_when_left_alone`, `an_idle_window_with_no_caret_does_nothing`.
- `typing_in_a_turned_box_works`.
- `a_reload_while_typing_ends_editing_without_losing_what_was_typed`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_typing`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, and the timing test from Task 1 with typing on the slide in place of notes.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: click on a slide and type"
```

---

### Task 8: Input methods

**Files:**
- Modify: `crates/app/src/{slide_editor,text_editing}.rs`
- Test: `crates/app/tests/ui_typing.rs`

**Interfaces:**
- Consumes: `SlideEditor`, `EditingText` (Task 7).
- Produces: `SlideEditor` implements GPUI's `EntityInputHandler`, so that the system's input method works: dead keys, compose sequences, and the emoji picker. While a composition is in progress its text is shown underlined at the caret and isn't in the talk; committing it is one insert, and cancelling it leaves nothing. Offsets given to and taken from the system are UTF-16, converted on grapheme-safe boundaries. The candidate window is placed at the caret's position on screen. A composition is ended before anything that ends editing.

- [ ] **Step 1: Write the failing tests**

Driving the `EntityInputHandler` methods directly, as the system would:
- `a_dead_key_then_a_letter_gives_one_character`.
- `composing_text_shows_underlined_and_is_not_in_the_talk`.
- `a_cancelled_composition_leaves_no_text`, and no undo step.
- `a_committed_composition_is_one_undo_step`.
- `an_emoji_from_the_picker_lands_at_the_caret`.
- `utf16_offsets_round_trip_across_emoji_and_accents`.
- `the_candidate_window_sits_at_the_caret`: `bounds_for_range` gives the caret's place in window coordinates.
- `saving_during_a_composition_ends_it_first`.
- `replacing_a_range_the_system_names_replaces_exactly_that`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_typing compos`
Expected: FAIL.

- [ ] **Step 3: Implement it**

The kit's own text field (`gpui-base-0.9.0/src/input`) implements the same trait and shows how GPUI calls it.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: dead keys, compose sequences and emoji while typing on a slide"
```

---

### Task 9: The text tools on the Home tab

**Files:**
- Create: `crates/app/src/text_tools.rs`
- Modify: `crates/app/src/{ribbon,lib,workspace_view}.rs`, `crates/app/src/workspace_view/editing.rs`
- Test: `crates/app/tests/ui_typing.rs`

**Interfaces:**
- Consumes: `TextEdit`'s style operations and `style_at` (Task 5), `Document::apply_text` (Task 7), the alignment settings (Task 6).
- Produces, on the Home tab after the Slides group: a *Text* group with **Bold** (Ctrl+B), **Italic** (Ctrl+I), **Underline** (Ctrl+U), a **size** field with a list (18, 24, 32, 40, 48, 64, 80, 96, and any number from 1 to 512 typed in), and a **colour** button opening the kit's colour picker with the theme's colours first and "Automatic" to clear; and a *Paragraph* group with **Bullets**, **Numbering**, **Indent less**, **Indent more**, and **Align left**, **Centre**, **Align right**. Each has an id, an icon, and a tooltip with its shortcut.
- Each tool shows the state of the selection: pressed when all of it has the style, half-pressed when mixed. The size field shows the size, or is empty when mixed. Sizes shown are what's drawn: text with no size of its own shows its region's size.
- The tools are disabled when no text is being edited. Using one keeps the focus in the text, so typing goes on.
- Alignment applies to the region or box the text is in, as one undoable step ("Align text").

- [ ] **Step 1: Write the failing tests**

- `bold_makes_the_selected_words_bold_and_the_button_shows_it`, the same for italic and underline, by button and by shortcut.
- `a_mixed_selection_shows_half_pressed`.
- `bold_with_no_selection_applies_to_what_is_typed_next`.
- `choosing_a_size_changes_only_the_selected_words`, `typing_a_size_and_pressing_enter_applies_it`, `a_size_out_of_range_is_held_to_the_limits`.
- `choosing_a_colour_changes_the_selected_words`, `automatic_clears_the_colour`.
- `bullets_turn_the_paragraphs_into_a_list_and_back`, `numbering_numbers_them`, `indent_more_and_less`.
- `centre_centres_the_whole_region_and_one_undo_puts_it_back`.
- `the_tools_are_disabled_with_no_text_being_edited`.
- `using_a_tool_keeps_the_caret_in_the_text`.
- `the_markdown_for_a_red_bold_word_is_what_the_design_shows`: `<Text color="…">**word**</Text>` or its equal.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_typing tools`
Expected: FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: bold, size, colour, bullets and alignment from the Home tab"
```

---

### Task 10: Copy and paste

**Files:**
- Create: `crates/app/src/clipboard.rs`, `crates/app/tests/clipboard.rs`
- Modify: `crates/app/src/{slide_editor,slide_strip,lib,workspace_view}.rs`, `crates/app/src/workspace_view/editing.rs`, `crates/format/src/{slide_ops,lib}.rs`
- Test: `crates/format/tests/slide_ops.rs`, `crates/app/tests/ui_typing.rs`, `crates/app/tests/ui_editor.rs`

**Interfaces:**
- Consumes: `TextEdit::copied`, `paste` (Task 5), `insert_slide` (D1), GPUI's clipboard with its metadata entry.
- Produces:
  - Text: Ctrl+C, Ctrl+X and Ctrl+V while text is being edited. What's put on the system clipboard is the plain text, plus the styled paragraphs as JSON in the clipboard's metadata under the app's own name. Pasting takes the styled paragraphs when they're there, and otherwise the plain text, split into paragraphs at newlines, in the style at the caret. A paste is one undo step. A paste larger than 256 KiB is refused with a plain sentence.
  - Slides: Ctrl+C, Ctrl+X and Ctrl+V with the slide list focused, and **Copy**, **Cut** and **Paste** on the slide's menu, copy the current slide's own Markdown and paste it after the current slide, in this talk or another window's. A pasted first slide leaves the talk's own settings behind. `slides_format::slide_text_for_copy(talk: &Talk, index: usize) -> String` and `insert_slide` do the work.
  - Home gets a *Clipboard* group, first: **Paste**, **Cut**, **Copy**, acting on text or on the slide by what has the focus.
  - A slide pasted into a talk that lacks a picture or component it uses shows the usual placeholder, and the status line says so once: "Some pictures on the pasted slide aren't in this talk."
- What's read from the clipboard is untrusted: the JSON is held to 256 KiB and to the model's own rules before it's used, and anything else falls back to plain text.

- [ ] **Step 1: Write the failing tests**

- `copied_words_paste_with_their_styles`, `text_from_another_program_pastes_plain`, `a_paste_with_newlines_makes_paragraphs`, `cut_removes_and_copies`, `a_paste_is_one_undo_step`.
- `a_paste_too_large_is_refused`, `broken_clipboard_data_falls_back_to_plain_text`.
- `a_copied_slide_pastes_after_the_current_one`, `a_slide_pastes_into_another_talk`, `a_copied_first_slide_leaves_the_talks_settings_behind`, `a_cut_slide_is_gone_and_one_undo_brings_it_back`.
- `a_pasted_slide_missing_its_picture_says_so`.
- `paste_goes_to_the_text_when_text_has_the_focus_and_to_slides_when_the_list_has`.
- In the format crate: `a_slides_text_for_copy_pastes_back_as_the_same_slide`, on both line endings and across talks with different ones.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test clipboard`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/format
git commit -m "App: copy and paste text and slides, also between talks"
```

---

### Task 11: Run it for real and write down what happened

**Files:**
- Create: `docs/checks/plan-d2.md`
- Modify: `README.md`, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md`

No new behaviour. Fix what the run finds, each with a test.

- [ ] **Step 1: Build**

Run: `scripts/cargo-safe.sh build --release -p stagepack-studio`

- [ ] **Step 2: Use it in a real window**

On a nested display, with the app's data folder pointed at a scratch folder, on scratch copies of the fixtures. First check the display itself: D1's last run saw the window squeezed to three quarters of its width with clicks offset the same way. Find out why before trusting anything else, and say what it was.
1. New slide of each layout. Click each placeholder and type. Look at the slide and at the Markdown written.
2. Type a paragraph with accented letters through dead keys, an emoji, and Markdown's own symbols. Make words bold, red and larger. Make a bulleted list with two levels and a numbered one. Centre a region.
3. Undo everything a step at a time, and redo it.
4. Save, and compare the file with the original using `git diff --no-index`: only what was typed differs.
5. Type into `fixtures/showcase`'s hand-written slides and compare again.
6. Copy styled words from one slide to another, and a slide from one talk's window to another's.
7. Type fast for thirty seconds on a 500-slide talk. Record the time per keystroke as the app measures it, and how it feels.
8. Kill the app mid-word. The recovery bar brings the text back.
9. Present, and export to PDF and PowerPoint: placeholders don't show, and styled text does.

Look at every screenshot you rely on. Be critical about how typing feels and looks: caret, selection colour, the tools' pressed states.

- [ ] **Step 3: Write `docs/checks/plan-d2.md`**

Like `docs/checks/plan-d1.md`. Say plainly what wasn't exercised: Windows, a real input method beyond dead keys, right-to-left text, a real GPU.

- [ ] **Step 4: Update the README and the roadmap's D2 line**

- [ ] **Step 5: Commit**

```bash
git add docs/checks/plan-d2.md README.md docs/superpowers/plans/2026-11-09-native-rust-roadmap.md
git commit -m "Plan D2 check: what ran, and what to check by hand"
```
