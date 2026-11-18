# Plan D4: Arrange and Animate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move, resize and turn things and a layout's areas with the mouse and the keyboard; select several at once; align, distribute and reorder them, with snapping; set click steps, entrances, one-at-a-time lists and slide transitions from the Animate tab; and change a slide's layout from a picker.

**Architecture:** A gesture shows a temporary override in the engine (a place for a tag or a region that isn't in the source) and writes one byte-range `Change` when the mouse is let go, through the same `Document` history as every other edit. A thing's place is its tag's `x y w h rotate` attributes; a layout area's place is words on its `region.<name>:` settings line, which the engine already reads. Click steps and entrances are the `click` and `enter` values in the same two places, and a transition is the slide's `transition:` setting. Front-to-back order is source order, so reordering moves a tag's own lines. Snapping, aligning, distributing and step renumbering are pure functions with property tests; the window only feeds them and draws.

**Tech Stack:** Rust 1.98.0, gpui-kit 0.9.0 with gpui-fast 0.1.13, gpui-component widgets (`DropdownButton`, `Button`, `Popover`, context menu), proptest, and the existing crates.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`: goals 5 and 7, the move, resize and rotate part of 3, "Layouts and moving things", "One source of truth" (a drag shows a temporary override and makes one source edit), and the anti-goals *Rewriting the user's text*, *Editing the wrong thing*, *Code in the way* and *Losing work*. `docs/superpowers/plans/2026-11-16-native-rust-plan-d3-decisions.md` has D3's rulings and its list "For the start of plan D4".

## Global Constraints

- Work on the branch `plan-d4`, cut from `main`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx` and `slides-components` don't depend on GPUI.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **A visual edit changes only the bytes of what was edited.** Moving, resizing or turning changes the values of `x`, `y`, `w`, `h` and `rotate` that differ, and adds those that are missing. Reordering moves a tag's own lines and the blank lines that keep it apart. A click step, an entrance or a transition changes that one value. No other byte of the talk changes, on any slide. Property-tested for every operation.
- **What's written reads back as what was meant**, and every number written is a whole number where the value is whole and has one decimal otherwise.
- **If the app can't be sure what a gesture maps to, it refuses and says so.**
- **Undo is exact.** One drag, one resize, one turn, one run of arrow-key nudges, one align or distribute of many things, one reorder, one Animate choice, one layout change: one undo step each. A gesture that ends where it began, or is cancelled, makes no step.
- **The keyboard rule from D3 holds:** while the list of slides or the notes have the keyboard, nothing on the slide is selected, being typed in, or waiting as a new text box. A selected layout area counts as a thing selected.
- A talk is untrusted input. Nothing in one may crash the app. A panic during an edit becomes a refused edit.
- Nobody is made to see Markdown, a tag name with angle brackets, a settings key, or Rust. Every label and message is plain words. A layout's region is called an *area* in words a person sees.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless GPUI test.
- An idle window does no work, and nothing redraws while a thing sits selected.
- One frame of a drag on a slide of sixty things costs under 6 ms at the median before drawing, in the release timing test. A keystroke keeps its 8 ms budget.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: templates (D5); grouping and cropping; resizing several things at once; a rubber-band selection drawn on the empty slide; motion paths, exits and timings; an animation pane; a rename offer for a clashing file name; decoding pictures off the window's thread; faster typing in very large tables; sizing a new component to what it draws.

Rulings made in the plan:
- **Front-to-back order is source order among placed things.** A layout's areas always sit under them, as the engine draws them today. Cost: text in an area can't be brought in front of a picture; a text box can.
- **One thing selected aligns to the slide; several align to each other** (to the box that holds them all). Distribute needs three.
- **Pictures and icons keep their proportions when a corner is dragged**, and other things don't; Shift does the opposite. Shift while turning steps by 15 degrees. Alt switches snapping off.
- **Several things are picked with Shift or Ctrl and a click**, and Ctrl+A picks every thing on the slide when a thing is selected. There's no rubber band yet.
- **A layout area is picked by its frame.** While its text is being typed in, the frame is drawn; a press on the frame's line selects the area. A press on its text still types, as in D2.
- **A selected area can't be deleted, copied or reordered.** Delete says so. *Put back* in the form returns it to where the layout has it.
- **A thing lying in the text (a table, a hand-written picture) is lifted out when dragged:** a tag at the top level gets a place written on it; anything else is wrapped in a placed `<Box>` at the end of the slide. One undo step, with the move.
- **Animating a thing gives it the step after the slide's last.** Taking a step away closes the gap, so no click ever shows nothing. Steps taken by a one-at-a-time list or by a component that steps are never renumbered.
- **A thing dragged off the slide keeps 16 units on it**, so it can't be lost.
- **Arrow keys with a thing selected move it** one unit, ten with Shift. Page Up and Page Down still change the slide. Ctrl+D already duplicates the thing.
- **The Home tab's History group goes in front of Slides**, so Undo and Redo show at 1,280 pixels (left as a question by D3).
- **The form panel can be hidden** from the View tab. It starts shown.

## Review Focus

- **Hand-written places.** A tag with `x` and no `w`; single-quoted, unquoted, decimal, negative, repeated or non-numeric values (`x="abc"`); a tag over several lines; a `region.body:` line with words in any order, repeated or unknown words, tabs, or no words at all. A move changes only the values that differ, and every other byte stays. Owned by Task 2's `placing_changes_only_the_values_that_differ`.
- **Turned things.** Things at 37, 90, 180, 359.9 and -360 degrees: the corner opposite a dragged handle stays where it is to within half a unit, the outline and handles sit on the thing, and aligning uses the box the turned thing covers. Owned by Task 6's `the_opposite_corner_stays_put_at_any_turn` and Task 8's `aligning_uses_the_turned_bounds`.
- **A gesture that doesn't finish.** Escape in the middle of a drag; the button let go outside the window; the window losing the keyboard; Undo, Page Down or an outside reload while the button is down; a press and release with no movement. No step is made, the override is gone, and the slide is as it was. Owned by Task 5's `an_unfinished_drag_leaves_nothing`.
- **Click steps a person wrote.** Gaps, repeats, `click="0"`, `click="x"`, `click="999"`, a one-at-a-time area beside numbered things, a component that steps. After any Animate choice no click shows nothing, and things that weren't chosen keep their bytes unless their step had to close a gap. Owned by Task 9's `no_click_shows_nothing_after_any_choice`.
- **Things at the extremes.** A thing 1 unit wide, one of 3,840, one mostly off the slide, sixty exactly on top of each other, one covering the whole slide. Each can be selected (by Tab if not by a click), its handles can be grabbed, and it can't be made smaller than 8 units or lost off the slide. Owned by Task 4's `every_thing_can_be_reached` and Task 6's `a_thing_never_gets_smaller_than_eight_units`.

---

### Task 1: Split the three large files

**Files:**
- Create: `crates/app/src/document/{mod,errors,open,reading,core,slides,files,clipboard,selecting,inserting,tables,typing,components,saving}.rs`, `crates/app/src/slide_editor/{mod,keys,selecting,typing,ime,mouse,marks,render}.rs`, `crates/app/src/inspector/{form,commit,colors,widgets,render}.rs`
- Delete: `crates/app/src/document.rs`, `crates/app/src/slide_editor.rs` (their contents move)
- Modify: `crates/app/src/inspector.rs`, `crates/app/tests/support/typing.rs`, the `ui_*.rs` tests that carry their own copies of the helpers

**Interfaces:**
- No public signature changes, and no behaviour changes. Every `pub` path that worked before still works (`stagepack_studio::document::Document`, `stagepack_studio::slide_editor::SELECTION_OUTLINE_ID`, and so on).
- Each new file is under 900 lines. Private fields that other modules of the same folder need become `pub(super)`.
- Produces in `tests/support/typing.rs`, lifted from the copies in `ui_select.rs`, `ui_inspector.rs`, `ui_insert.rs`, `ui_palette.rs`, `ui_tables.rs` and `ui_icons.rs`, which then use these:
  - `pub fn go_to(typing: &Typing, cx: &mut TestAppContext, index: usize)`
  - `pub fn thing(typing: &Typing, cx: &mut TestAppContext, start: &str) -> ElementBox`
  - `pub fn middle(typing: &Typing, cx: &mut TestAppContext, element: &ElementBox) -> Point<Pixels>`
  - `pub fn selected(typing: &Typing, cx: &TestAppContext) -> Option<(usize, Range<usize>)>`
  - `pub fn outline(typing: &Typing, cx: &mut TestAppContext) -> Option<Bounds<Pixels>>`
  - `pub fn bounds_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<Bounds<Pixels>>`
  - `pub fn click_on(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>)`

- [ ] **Step 1: Record the starting point**

Run: `scripts/cargo-safe.sh test --workspace 2>&1 | grep "^test result"` and keep the totals (1,862 passed, 13 ignored).

- [ ] **Step 2: Move `document.rs` into `document/`, one concern per file, and commit**

Run: `scripts/cargo-safe.sh test -p stagepack-studio`
Expected: PASS, the same number of tests.

- [ ] **Step 3: Move `slide_editor.rs` into `slide_editor/`, and commit**

- [ ] **Step 4: Move the inspector's form, commit, colour, widget and render code into `inspector/`, and commit**

- [ ] **Step 5: Lift the test helpers, and commit**

- [ ] **Step 6: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and `scripts/cargo-safe.sh clippy --workspace --all-targets`
Expected: the totals from Step 1, no warnings. `wc -l` shows no file in `crates/app/src` over 900 lines except `workspace_view.rs`.

Commit messages: `App: the open talk's code is one file per concern`, and the same shape for the other three.

---

### Task 2: Arrange operations in the format crate

**Files:**
- Create: `crates/format/src/arrange_ops.rs`, `crates/format/tests/arrange_ops.rs`
- Modify: `crates/format/src/{lib,slide_ops,edit}.rs`

**Interfaces:**
- Produces, in `slides_format`:
  - `pub enum Target { Tag(Range<usize>), Region(String) }`: a top-level tag by its whole source range, or a layout's region by name.
  - `pub struct ValueEdit<'a> { pub target: Target, pub name: &'a str, pub value: Option<&'a str> }`. `None` removes the value.
  - `pub fn set_values(talk: &Talk, slide: usize, edits: &[ValueEdit]) -> Option<Change>`: one `Change` for all of them. On a tag it sets or removes the attribute as `set_tag_attr` does. On a region it sets or removes the `name=value` word on the slide's `region.<name>:` settings line, adding the word, the line or the settings block as needed, and removing the line when its last word goes (and the block when its last line goes, as `remove_setting` does). A value that is already what's asked makes no edit. `None` if a target isn't there, a name or value can't be written (`tag_text`'s rules; a region word's value is `[A-Za-z0-9.+-]+`), two edits name the same target and name, or the slide wouldn't read back as before except for those values.
  - `pub fn region_value<'a>(slide: &'a Slide, region: &str, name: &str) -> Option<&'a str>`: the word the engine would use.
  - `pub enum Order { Forward, Backward, Front, Back }`
  - `pub fn reorder(talk: &Talk, slide: usize, tag: &Range<usize>, order: Order) -> Option<Change>`: moves a top-level placed tag's own lines past the next placed top-level tag in that direction, or past all of them. The moved tag's bytes and every other block's bytes are the same; only the blank lines between blocks may change, and a `<Region />` line, the text of a region and `<Notes>` are never crossed in a way that changes which region a block reads into. `Some(empty)` when it's already there. `None` if the tag isn't a placed top-level tag or the slide wouldn't read back as the same blocks in the new order.
  - `pub fn can_reorder(talk: &Talk, slide: usize, tag: &Range<usize>, order: Order) -> bool`: whether `reorder` would move anything.
  - `pub fn lift(talk: &Talk, slide: usize, block: &Range<usize>, place: (f32, f32, f32, f32)) -> Option<Change>`: gives a block lying in a region a place. A top-level tag gets `x y w h` written on it. Any other block (a table, a `![]()` picture, a paragraph, a tag inside a list) is removed as `delete_block` does and written at the end of the slide's content, as `insert_block` does, inside `<Box x=".." y=".." w=".." h="..">`, blank line, the block's own bytes with any list indentation taken off, blank line, `</Box>`. `None` for a block inside another tag, or if the result wouldn't read back as the old blocks without it followed by the box holding it.
  - `pub fn set_layout(talk: &Talk, slide: usize, layout: &str) -> Option<Change>`: sets the slide's `layout:` setting, or removes it for `content`. Nothing else changes: `region.` lines for regions the new layout lacks stay.
  - `pub fn set_transition(talk: &Talk, slides: &[usize], transition: Option<&str>) -> Option<Change>`: sets or removes `transition:` on each slide named, in one `Change`.
  - `pub fn number_text(value: f32) -> String`: `"120"` for 120.0, `"120.5"` for 120.5, rounded to one decimal; never `-0`.
- All wrapped against panics as `write_flow` is.

- [ ] **Step 1: Write the failing tests**

`arrange_ops.rs`, property tests over `gen_talk()` (extend it with region lines: words in any order, repeated and unknown words, tabs, an empty `region.body:`; and placed tags with missing, single-quoted, unquoted, decimal, negative, repeated and non-numeric place values):
- `placing_changes_only_the_values_that_differ`: for any set of edits to `x y w h rotate click enter` on any targets, the result is `None` or the reparsed values equal what was asked, every value not named and every byte outside the tags and region lines named is the same, and an edit asking for the value already there adds no `SourceEdit`.
- `setting_then_undoing_is_exact`.
- `removing_the_last_word_removes_the_line`, and the block with it when it was the only line.
- `reordering_keeps_every_blocks_bytes`: the multiset of block texts is the same, the placed tags' order differs only by the one moved, and each flowing block reads into the same region as before.
- `forward_then_backward_is_where_it_began` (the source is byte-equal).
- `lifting_keeps_the_blocks_own_bytes`: the lifted block's text is in the new source, in a box at the place asked, and every other block's bytes are the same.
- Named: `a_region_with_no_line_gets_one`, `a_slide_with_no_settings_gets_a_block`, `two_edits_to_one_value_are_refused`, `a_repeated_word_changes_the_one_the_engine_uses`, `front_of_three`, `reorder_does_not_cross_a_region_marker_wrongly` (`<Region name="right" />` between two placed tags), `lifting_a_table_out_of_a_list_is_refused_or_exact`, `lifting_a_top_level_tag_writes_its_place`, `content_layout_removes_the_setting`, `a_transition_on_every_slide_is_one_change`, `crlf_talks_keep_crlf`, `number_text_is_plain` (`0`, `-0.04` → `0`, `12.25` → `12.3`, `1e9`).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-format --test arrange_ops`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

Make `slide_ops.rs`'s private `set_region_value` the one writer of region words and give it a remove; `set_align` goes through it.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-format`, and `PROPTEST_CASES=4000 scripts/cargo-safe.sh test -p slides-format --test arrange_ops`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/format
git commit -m "Format: a thing or an area can be placed, reordered, lifted out and given a step"
```

---

### Task 3: The engine shows a place that isn't written yet, and says where the areas are

**Files:**
- Modify: `crates/engine/src/{deck,slide,layouts,text_geometry,timeline,lib}.rs`
- Test: `crates/engine/tests/{deck,flow,text_geometry,timeline}.rs`

**Interfaces:**
- Consumes: `slides_format::Target`.
- Produces, in `slides_engine`:
  - `pub struct Placing { pub target: Target, pub rect: Rect, pub rotate: f32 }`
  - `impl Deck`:
    - `pub fn set_placing(&mut self, slide: usize, placing: Vec<Placing>)`: until cleared, `specs`, `layers`, `editor_layers` and `text_layout` of that slide lay each target out at the place given, as if its source said so. The source, `slide_version`, thumbnails, `layers_at` and what's exported don't change. A target that isn't on the slide is ignored. A layer whose place moved and whose size and turn didn't is not drawn again: its picture is reused at the new position.
    - `pub fn clear_placing(&mut self)`; `pub fn placing(&self) -> Option<(usize, &[Placing])>`. Any `update`, `update_parsed`, `update_text` or `set_files` clears it.
    - `pub fn fixed_steps(&mut self, slide: usize) -> u32`: the steps taken by one-at-a-time regions and by components that step, which no `click` value can renumber.
  - `pub fn layouts() -> &'static [LayoutDef]`, in the order title, content, two-columns, section, blank.
  - `pub struct RegionBox { pub name: String, pub rect: Rect, pub rotate: f32, pub moved: bool, pub empty: bool, pub is_list: bool }`: a region of the slide's layout as placed (with any placing applied); `moved` when the slide's settings place or turn it; `empty` when nothing reads into it; `is_list` when its content has a list or more than one block, so one at a time means something.
  - `impl TextLayout`: `pub fn regions(&self) -> &[RegionBox]` (every region of the layout, in the layout's order); `pub fn region(&self, name: &str) -> Option<&RegionBox>`; `pub fn region_of_flow(&self, flow: usize) -> Option<&str>` (the region a text flow reads into, `None` for text in a placed tag).
  - `ElementBox` gains `pub placed: bool` (a top-level tag with a place, so it can be moved without lifting) and `pub keeps_shape: bool` (a picture or an icon).

- [ ] **Step 1: Write the failing tests**

- `deck.rs`: `a_placing_shows_without_changing_the_source` (specs and `text_layout().element()` give the new rect; `source()` and `slide_version` are the same; after `clear_placing` they give the old one, and the layers are pixel-equal to before); `a_placing_of_a_region_moves_its_text`; `an_update_clears_the_placing`; `a_moved_layer_is_not_drawn_again` (the `Arc<Layer>` pixmap pointer or a draw counter shows no new render for a pure move, and one for a resize); `a_placing_for_a_missing_target_is_ignored`; `placing_never_panics` (property: any rect and turn, including NaN and infinity, on any target).
- `text_geometry.rs`: `every_region_of_the_layout_is_listed` (two-columns gives title, left, right, with `empty` right); `a_moved_region_says_so`; `a_flow_knows_its_region`; `a_picture_keeps_its_shape_and_a_shape_does_not`; `a_tag_in_the_text_is_not_placed`.
- `timeline.rs`: `fixed_steps_counts_a_one_at_a_time_list` (three items → 3), and 0 for a slide of numbered tags.
- `flow.rs`: `layouts_are_listed_in_order`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-engine` and the golden-image tests with it.
Expected: PASS, no golden image changed.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: a place can be shown before it's written, and the layout's areas are listed"
```

---

### Task 4: Selecting several things, and a layout's areas

**Files:**
- Modify: `crates/app/src/{selection,history}.rs`, `crates/app/src/document/{selecting,clipboard,core}.rs`, `crates/app/src/slide_editor/{selecting,mouse,marks,render,keys}.rs`, `crates/app/src/inspector/{form,render}.rs`, `crates/app/src/{notes_panel,workspace_view}.rs`
- Test: `crates/app/tests/ui_arrange_select.rs` (new), `crates/app/tests/ui_select.rs`, `crates/app/tests/ui_insert.rs`

**Interfaces:**
- Produces, in `stagepack_studio::selection`:
  - `pub enum Thing { Block(Range<usize>), Region(String) }`
  - `pub struct Selection { pub slide: usize, pub things: Vec<Thing> }`: in the order picked; never empty; no thing twice.
- Produces, on `Document`:
  - `pub fn selection(&self) -> Option<&Selection>`. `selected()` stays and is `Some` only when exactly one `Thing::Block` is selected, so every D3 caller works as before.
  - `pub fn select_also(&mut self, slide: usize, thing: Thing) -> bool`: adds the thing, or takes it out if it's there; a thing on another slide replaces the selection.
  - `pub fn select_region(&mut self, slide: usize, region: &str) -> bool`
  - `pub fn select_all_things(&mut self) -> bool`: every element of the current slide, not its areas.
  - `pub fn selected_boxes(&mut self) -> Vec<(Thing, Rect, f32)>`: each selected thing's rectangle and turn, from the layout.
  - `delete_selected`, `cut_selected`, `copy_selected` and `duplicate_selected` act on every selected block in one step (copied text is the blocks in source order, a blank line between); with only areas selected they return `Err(EditRefused::AreaStays)`, whose words are *A layout's area can't be deleted or copied. Choose another layout to change the areas.*
  - A step in the history remembers the whole selection before and after, and Undo and Redo bring it back.
- In the window:
  - A click with Shift or Ctrl on a thing calls `select_also`. Ctrl+A in `SELECTION_CONTEXT` is `SelectAllThings`. Tab and Shift+Tab go through the things and then the areas.
  - While text in a region is being typed, the region's frame is drawn (id `region_frame_id(name)`, a dashed line). A press within 5 pixels of that line, and not on text of another thing, selects the region. A selected region shows the same outline as a thing.
  - Each selected thing has an outline; `SELECTION_OUTLINE_ID` is the last picked one, and the others are `selection_outline_id(n)`.
  - The status line says *Shape selected*, *3 things selected*, *Title area selected* (`title`, `body`, `left`, `right` are *Title*, *Main text*, *Left column*, *Right column*).
  - The form shows, for several things, *3 things selected* and no fields; for an area, its name, the Place fields (written through `set_values` on the region) and a *Put back* button (`INSPECTOR_PUT_BACK_ID`) that removes `x y w h rotate` in one step, disabled when the area isn't moved.
  - The notes taking the keyboard lets go of the selection, as the list of slides does.

- [ ] **Step 1: Write the failing tests**

`ui_arrange_select.rs`:
- `shift_click_adds_and_takes_away`, `ctrl_a_selects_every_thing_and_no_area`, `a_click_without_shift_selects_one_again`.
- `delete_takes_every_selected_thing_in_one_step`, `copy_and_paste_of_two_things_brings_both`, `duplicate_of_two_things_selects_the_copies`, `undo_brings_back_the_things_and_the_selection`.
- `a_press_on_an_areas_frame_selects_it` (typing in the body, then a press on the frame line), `a_press_on_its_text_still_types`, `tab_reaches_the_areas_after_the_things`.
- `delete_on_an_area_says_why_and_changes_nothing`, `an_areas_place_fields_write_its_settings_line`, `put_back_removes_the_place_in_one_step`.
- `every_thing_can_be_reached` (Review Focus): on a slide with a thing 1 unit wide, one covering the whole slide, sixty at one place and one mostly off the slide, Tab visits each exactly once, and a click on the 1-unit thing selects it.
- `the_notes_taking_the_keyboard_lets_go`; and in `ui_insert.rs` extend `the_list_never_has_the_keyboard_while_a_thing_is_selected` and `the_list_taking_the_keyboard_ends` with a selected area and with two things selected.
- `nothing_redraws_while_two_things_sit_selected`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_arrange_select`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: several things can be selected, and so can a layout's area"
```

---

### Task 5: Moving with the mouse and the arrow keys

**Files:**
- Create: `crates/app/src/arrange.rs` (pure geometry), `crates/app/src/slide_editor/arranging.rs`, `crates/app/src/document/arranging.rs`, `crates/app/tests/ui_move.rs`, `crates/app/tests/arrange.rs`
- Modify: `crates/app/src/slide_editor/{mouse,keys,render}.rs`, `crates/app/src/inspector/fields.rs`, `crates/app/tests/edit_speed.rs`

**Interfaces:**
- Consumes: Task 2's `set_values`, `number_text`; Task 3's `Deck::set_placing`; Task 4's `Selection`.
- Produces, in `stagepack_studio::arrange` (no GPUI):
  - `pub struct Placed { pub rect: Rect, pub rotate: f32 }`
  - `pub fn bounds(placed: &Placed) -> Rect`: the upright box a turned rectangle covers.
  - `pub fn kept_on_slide(rect: Rect) -> Rect`: moved so at least 16 units of it are on the 1920 by 1080 slide in each direction, and within the engine's limits (-3840 to 7680).
  - `pub const MOVE_STARTS: f32 = 4.0` (pixels the pointer travels before a press becomes a drag).
- Produces, on `Document`:
  - `pub fn preview_places(&mut self, places: &[(Thing, Placed)])` and `pub fn drop_preview(&mut self)`: through `Deck::set_placing`. Only placed things and areas are shown.
  - `pub fn place_things(&mut self, label: &'static str, places: &[(Thing, Placed)], run: EditRun) -> Result<Vec<usize>, EditRefused>`: one `set_values` change writing only what differs from what the layout has now, each number through `number_text` after rounding positions and sizes to whole units. Nothing differing is `Ok(vec![])` and no step. Labels: `MOVE`, `RESIZE`, `TURN`, `NUDGE` (*Move*, *Resize*, *Turn*, *Move*).
- In the window:
  - A press on a selected thing or area (or one that the press selects), moved more than `MOVE_STARTS`, drags every selected thing by the same amount. Each frame calls `preview_places`; letting go calls `drop_preview` then `place_things(MOVE, …, EditRun::New)`. Escape, the window losing the keyboard, or any other edit arriving ends the drag with `drop_preview` and nothing written. A drag never starts from a press that put the caret in text.
  - Shift held while dragging keeps the move to one direction, whichever is larger.
  - `SELECTION_CONTEXT` binds left, right, up, down to `Nudge { dx, dy }` of 1 unit and shift-arrows to 10. A run of nudges within 1 second of each other is one undo step (`EditRun::Continue`). Nudging a thing that isn't placed says *Drag it out of the text first.*
  - The form's Left and Top fields take -3840 to 7680.

- [ ] **Step 1: Write the failing tests**

- `arrange.rs` (properties): `bounds_hold_every_corner`; `kept_on_slide_keeps_sixteen_units` and is the identity for a rect already inside.
- `ui_move.rs`:
  - `a_drag_moves_a_shape_and_writes_only_x_and_y` (source differs only in those two values); `a_drag_is_one_undo_step`; `a_drag_of_two_things_moves_both_by_the_same_amount`; `a_drag_of_an_area_writes_its_settings_line`; `shift_keeps_the_move_straight`.
  - `the_slide_shows_the_thing_under_the_pointer_while_dragging` (the outline's bounds follow, and `source()` is unchanged until release).
  - `an_unfinished_drag_leaves_nothing` (Review Focus): Escape; release outside the window; the window deactivated; Ctrl+Z, Page Down and an outside reload with the button down; a press and release with no movement. After each: no new undo step, `Deck::placing()` is `None`, the source is byte-equal.
  - `a_small_wobble_is_a_click_and_writes_nothing` (3 pixels).
  - `arrow_keys_nudge_by_one_and_shift_by_ten`; `a_run_of_nudges_is_one_undo_step` and a pause makes two; `a_thing_cannot_be_dragged_off_the_slide`; `a_negative_left_can_be_typed_in_the_form`.
  - `left_and_right_change_the_slide_only_when_nothing_is_selected`.
  - `nothing_redraws_after_a_drag_ends`.
- `edit_speed.rs`, ignored, release: `a_frame_of_a_drag_on_a_slide_of_sixty_things_is_quick`: 200 calls of `preview_places` followed by `editor_layers` and `text_layout`, median under 6 ms.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_move --test arrange`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`
Expected: PASS. Report the timings. If the drag frame can't meet 6 ms, say what it is and where the time goes; don't loosen the test without saying.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a thing is moved by dragging it or with the arrow keys"
```

---

### Task 6: Handles: resizing and turning

**Files:**
- Modify: `crates/app/src/arrange.rs`, `crates/app/src/slide_editor/{arranging,marks,render,mouse}.rs`, `crates/app/src/inspector/commit.rs`
- Test: `crates/app/tests/ui_resize.rs` (new), `crates/app/tests/arrange.rs`

**Interfaces:**
- Produces, in `arrange`:
  - `pub enum Handle { TopLeft, Top, TopRight, Right, BottomRight, Bottom, BottomLeft, Left, Turn }`, `Handle::ALL`.
  - `pub const MIN_SIZE: f32 = 8.0`
  - `pub fn handle_point(placed: &Placed, handle: Handle, turn_gap: f32) -> (f32, f32)`: where the handle sits on the slide, turned with the thing; `Turn` is `turn_gap` units beyond the middle of the top edge.
  - `pub fn resized(start: &Placed, handle: Handle, to: (f32, f32), keep_shape: bool) -> Placed`: the thing with that handle dragged to `to` (slide units). The opposite corner or edge stays where it is on the slide at any turn. Neither side goes under `MIN_SIZE` or over 3840, and the rectangle never flips. With `keep_shape`, a corner keeps the starting proportions; an edge handle never does.
  - `pub fn turned_to(start: &Placed, to: (f32, f32), stepped: bool) -> Placed`: turned so the top points from the centre to `to`; in whole degrees from -180 to 180, or steps of 15 when `stepped`.
- In the window:
  - One thing or area selected, and not being typed in, shows eight square handles and a round turn handle: ids `handle_id(Handle)`, 9 pixels across, the turn handle 24 pixels beyond the top edge. A thing that isn't placed (it lies in the text) shows none. A press within 6 pixels of a handle takes it before anything under it.
  - Dragging a handle previews and writes as a move does, with `RESIZE` or `TURN`. Corners keep the shape for `ElementBox::keeps_shape`, and Shift does the opposite. Shift while turning steps by 15 degrees. A turn of 0 removes `rotate`.
  - The pointer shows the resize or the turn it would do.
  - Committing an icon's *Size* in the form writes `w` and `h` to that size in the same step, where the icon has them.

- [ ] **Step 1: Write the failing tests**

- `arrange.rs` (properties over any start rectangle, turn, handle and pointer):
  - `the_opposite_corner_stays_put_at_any_turn` (Review Focus): within 0.5 unit, at turns including 37, 90, 180, 359.9 and -360.
  - `a_thing_never_gets_smaller_than_eight_units` (Review Focus), nor flips, for any pointer.
  - `keeping_the_shape_keeps_the_proportions` to within 1 percent; `an_edge_handle_changes_one_side_only`.
  - `handles_sit_on_the_turned_thing`: `handle_point` of each corner equals the engine's turned corner.
  - `turning_points_the_top_at_the_pointer`; `stepped_turns_are_multiples_of_fifteen`.
- `ui_resize.rs`: `dragging_a_corner_resizes_and_writes_what_changed`; `a_picture_keeps_its_shape_and_shift_frees_it`; `a_shape_is_free_and_shift_keeps_it`; `the_turn_handle_writes_rotate`; `turning_back_to_zero_removes_it`; `a_turned_shape_resizes_from_its_own_corner` (rotate 37: the opposite corner's place on the slide is the same before and after); `an_area_resizes_and_its_text_wraps_in_the_preview`; `two_things_selected_show_no_handles`; `a_thing_in_the_text_shows_no_handles`; `escape_during_a_resize_leaves_nothing`; `a_one_unit_thing_can_still_be_grabbed`; `a_resize_is_one_undo_step`; `an_icons_size_moves_its_width_and_height`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_resize --test arrange`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and `PROPTEST_CASES=4000 scripts/cargo-safe.sh test -p stagepack-studio --test arrange`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a thing is resized and turned by its handles"
```

---

### Task 7: Snapping and guides

**Files:**
- Create: `crates/app/src/snap.rs`, `crates/app/tests/snap.rs`, `crates/app/tests/ui_snap.rs`
- Modify: `crates/app/src/slide_editor/{arranging,marks,render}.rs`

**Interfaces:**
- Produces, in `stagepack_studio::snap` (no GPUI):
  - `pub struct Guide { pub vertical: bool, pub at: f32, pub from: f32, pub to: f32 }`: a line on the slide, in slide units.
  - `pub struct Snapped { pub dx: f32, pub dy: f32, pub guides: Vec<Guide> }`
  - `pub fn snap_move(moving: Rect, others: &[Rect], within: f32) -> Snapped`: the smallest shift, at most `within` in each direction, that puts the moving box's left, centre or right on the slide's left edge, centre or right edge or on another box's left, centre or right, and the same for top, middle and bottom. Each direction is decided on its own. A guide is given for each line met, running the length of both boxes. Nothing within reach gives 0 and no guide.
  - `pub fn snap_edge(edge: f32, vertical: bool, others: &[Rect], within: f32) -> Option<(f32, Guide)>`: the same for one edge being resized.
- In the window: `SNAP_PIXELS = 6.0`, turned into slide units by the slide's scale. A move snaps the box holding everything dragged; a resize snaps the edges that handle moves, for a thing that isn't turned. Things being dragged aren't in `others`. Guides are drawn over the slide while the button is down (id `SNAP_GUIDE_ID`, the theme's ring colour, 1 pixel). Alt held snaps to nothing. Arrow keys never snap.

- [ ] **Step 1: Write the failing tests**

- `snap.rs` (properties): `a_snap_never_moves_further_than_within`; `after_a_snap_a_line_really_is_shared` (to 0.01); `nothing_near_means_no_move_and_no_guide`; `the_nearest_line_wins`; `snapping_twice_changes_nothing_more`. Named: `the_slides_centre`, `left_edge_to_anothers_right_edge`, `centres_line_up`.
- `ui_snap.rs`: `a_drag_near_the_slides_centre_lands_on_it` (written `x` is exactly the centred value); `a_guide_shows_while_snapped_and_goes_on_release`; `alt_switches_snapping_off`; `a_resize_snaps_its_moving_edge_to_another_thing`; `a_turned_thing_snaps_by_the_box_it_covers_when_moved_and_not_when_resized`; `arrow_keys_do_not_snap`; `things_dragged_together_do_not_snap_to_each_other`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test snap --test ui_snap`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and the drag timing test from Task 5 (sixty things to snap to).
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a thing being moved snaps to the slide and to the things around it"
```

---

### Task 8: Align, distribute and order

**Files:**
- Create: `crates/app/src/arrange_tools.rs`, `crates/app/tests/ui_arrange.rs`
- Modify: `crates/app/src/{arrange,ribbon,lib,workspace_view}.rs`, `crates/app/src/workspace_view/editing.rs`, `crates/app/src/document/arranging.rs`, `crates/app/src/slide_editor/{mouse,render}.rs`, `crates/app/tests/arrange.rs`, `crates/app/tests/ui_layout.rs`

**Interfaces:**
- Consumes: Task 2's `reorder`, `can_reorder`; Task 5's `place_things`, `bounds`.
- Produces, in `arrange`:
  - `pub enum AlignTo { Left, Centre, Right, Top, Middle, Bottom }`
  - `pub fn aligned(things: &[Placed], to: AlignTo) -> Vec<Placed>`: one thing against the slide; several against the box that holds all their `bounds`. Each moves so its own `bounds` meets the line; sizes and turns don't change.
  - `pub fn distributed(things: &[Placed], across: bool) -> Option<Vec<Placed>>`: three or more, with equal gaps between their `bounds` along the direction, the outermost two staying put, in their order along it. `None` for fewer than three.
- Produces, on `Document`: `pub fn align_things(&mut self, to: AlignTo)`, `pub fn distribute_things(&mut self, across: bool)`, `pub fn reorder_selected(&mut self, order: Order)`, each `-> Result<Vec<usize>, EditRefused>` and one step (`ALIGN`, `DISTRIBUTE`, `ORDER`: *Align*, *Distribute*, *Change order*); and `pub fn arrange_here(&self) -> ArrangeHere { pub count: usize, pub placed: usize, pub can: [bool; 4] }` for the tools' enabled state. New refusals and their words: `EditRefused::NeedsThree` (*Select three or more things to space them out.*), `EditRefused::InTheText` (*Drag it out of the text first.*), `EditRefused::AreaOrder` (*A layout's area always sits behind the things on the slide.*).
- Produces, in `arrange_tools`: actions `AlignThings { to }`, `DistributeThings { across }`, `ReorderThing { order }`; `pub fn arrange_group(here: &ArrangeHere, cx: &App) -> AnyElement`: one *Arrange* dropdown (`ARRANGE_ID`) whose menu has *Bring to front*, *Bring forward*, *Send backward*, *Send to back*, a line, the six aligns (*Align left* … *Align bottom*), a line, *Space out across*, *Space out down*. An item that can't act is disabled.
- In the window:
  - The Home tab's groups are, in order: Clipboard, History, Slides, Text, Paragraph, Arrange, Table.
  - A right-click on a thing selects it (keeping a selection it's part of) and opens a menu: *Cut*, *Copy*, *Paste*, *Duplicate*, *Delete*, a line, then the four order items. The menu closing gives the keyboard back to the slide with the thing still selected.
  - The View tab has a *Form* toggle (`TOGGLE_INSPECTOR_ID`, action `ToggleInspector`); hidden, the slide takes the room.
  - Reordering works on one selected thing. Several, or an area, are refused in words.

- [ ] **Step 1: Write the failing tests**

- `arrange.rs` (properties): `aligning_uses_the_turned_bounds` (Review Focus: after `aligned`, every thing's `bounds` shares the line, to 0.01, at any turn); `aligning_twice_changes_nothing_more`; `aligning_keeps_sizes_and_turns`; `distributing_makes_the_gaps_equal` and keeps the two ends; `distributing_fewer_than_three_is_none`.
- `ui_arrange.rs`: `one_thing_aligns_to_the_slide`; `three_things_align_to_each_other_in_one_step`; `space_out_across_makes_equal_gaps`; `space_out_with_two_things_is_disabled_and_says_why`; `bring_forward_moves_its_lines_and_draws_it_above` (the layer order from `specs` changes and the two tags' bytes are the same); `to_front_and_to_back`; `the_front_thing_cannot_come_forward` (disabled); `an_area_cannot_be_reordered_and_says_why`; `a_right_click_opens_the_things_menu_and_keeps_it_selected`; `delete_from_the_menu_takes_the_thing_and_not_the_slide`; `the_keyboard_rule_holds_after_the_menu_closes`; `the_form_can_be_hidden_and_shown`; `undo_and_redo_show_at_1280` (in `ui_layout.rs`: both buttons are inside the ribbon's visible bounds).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_arrange --test arrange`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: things can be aligned, spaced out and brought forward or sent back"
```

---

### Task 9: The Animate tab

**Files:**
- Create: `crates/app/src/animate.rs` (pure), `crates/app/src/animate_tools.rs`, `crates/app/src/document/animating.rs`, `crates/app/tests/animate.rs`, `crates/app/tests/ui_animate.rs`
- Modify: `crates/app/src/{ribbon,lib,workspace_view}.rs`, `crates/app/src/slide_editor/marks.rs`

**Interfaces:**
- Consumes: Task 2's `set_values`, `set_transition`, `region_value`; Task 3's `Deck::fixed_steps`, `RegionBox::is_list`, `TextLayout::region_of_flow`; `slides_engine::{Enter, Transition}`.
- Produces, in `stagepack_studio::animate` (no GPUI):
  - `pub struct Stepped { pub target: Target, pub click: Option<u32>, pub each: bool }`: what one target on the slide has now; only a valid number (1 to 999) counts as a click.
  - `pub enum Choice { Animate, Remove, Earlier, Later, Each(bool) }`
  - `pub fn chosen(all: &[Stepped], fixed: u32, chosen: &[Target], choice: Choice) -> Option<Vec<(Target, Option<u32>)>>`: the new click for every target whose click changes (`None` removes it).
    - `Animate`: each chosen target with no click gets the step after the largest in use (clicks and `fixed`), all the same step.
    - `Remove`: the chosen lose their click; then any step above `fixed` that nothing has is closed by lowering the clicks above it.
    - `Earlier` / `Later`: the chosen move one step, down to 1 or up to one past the largest; then gaps close as for `Remove`.
    - `Each(on)` is for one region: on removes its numbered click. (The window writes `click=each` or removes it.)
    - `None` when nothing would change. Clicks at or under `fixed` are never renumbered.
  - `pub fn steps_shown(all: &[Stepped], fixed: u32) -> u32`: the slide's steps.
- Produces, on `Document`: `pub fn animate_here(&mut self) -> AnimateHere { pub targets: Vec<Target>, pub click: Option<u32>, pub steps: u32, pub enter: Option<Enter>, pub each: Option<bool>, pub can_each: bool, pub transition: Transition, pub why_not: Option<EditRefused> }` and `pub fn animate(&mut self, choice: Choice)`, `pub fn set_entrance(&mut self, enter: Enter)`, `pub fn set_slide_transition(&mut self, transition: Transition, all: bool)`, each `-> Result<Vec<usize>, EditRefused>` and one step (`ANIMATE`, `TRANSITION`: *Change animation*, *Change transition*). The targets are the selected placed things and areas; with nothing selected and text in an area being typed, that area. New refusal `EditRefused::NothingToAnimate` (*Select something on the slide first.*); a thing in the text gives `InTheText`.
- Produces, in `animate_tools`: `pub fn animate_tab(here: &AnimateHere, has_talk: bool, cx: &App) -> Vec<AnyElement>` with groups:
  - *Transition*: a dropdown (`TRANSITION_ID`) *None*, *Fade*, *Push*, and *Apply to all slides* (`TRANSITION_ALL_ID`).
  - *Appear*: a dropdown (`ENTRANCE_ID`) *No animation*, *Appear*, *Fade in*, *Fly up*. Choosing an entrance on a thing with no click animates it first, in the same step. *No animation* removes both `click` and `enter`.
  - *Order*: *Earlier* (`EARLIER_ID`), *Later* (`LATER_ID`), and words *Click 2 of 3* (`STEP_LABEL_ID`), or *Not animated*.
  - *List*: a toggle *One at a time* (`ONE_AT_A_TIME_ID`), enabled when one area with `is_list` is the target.
- In the window: with the Animate tab open, each animated thing and area on the slide carries a small numbered badge at its top left corner (`step_badge_id(n)`), the one-at-a-time area a badge reading *1–3*. Badges aren't drawn on any other tab, in thumbnails, when presenting or in exports.

- [ ] **Step 1: Write the failing tests**

- `animate.rs` (properties over any slide of up to twelve targets with any clicks, any `fixed`, any chosen set and choice):
  - `no_click_shows_nothing_after_any_choice` (Review Focus): after applying the result, every step from `fixed + 1` to `steps_shown` has at least one target.
  - `what_was_not_chosen_keeps_its_order`: unchosen targets keep their relative order of steps, and any whose click didn't need to close a gap isn't in the result.
  - `animate_then_remove_is_where_it_began`; `earlier_then_later_is_where_it_began` when no gap closed.
  - `fixed_steps_are_never_renumbered`.
  - Named: `the_first_animation_is_click_one`, `two_chosen_share_a_step`, `removing_the_middle_of_three_closes_the_gap`, `earlier_at_click_one_does_nothing`, `a_gap_a_person_wrote_is_left_until_a_choice_touches_it`.
- `ui_animate.rs`: `the_tab_has_its_tools_and_says_nothing_is_on_the_way`; `choosing_fade_in_writes_click_and_enter_in_one_step`; `no_animation_removes_both`; `later_swaps_with_the_next_step`; `an_area_being_typed_in_is_the_target`; `one_at_a_time_writes_click_each_on_the_areas_line_and_is_off_for_one_paragraph`; `a_transition_is_written_for_this_slide`; `apply_to_all_writes_every_slide_in_one_step`; `none_removes_the_setting`; `nothing_selected_disables_the_tools_and_says_why`; `a_table_in_the_text_says_drag_it_out_first`; `badges_show_only_on_the_animate_tab`; `the_show_plays_what_was_chosen` (`timeline_of` gives the steps, clicks and entrances chosen); `hand_written_click_zero_reads_as_not_animated`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test animate --test ui_animate`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and `PROPTEST_CASES=4000 scripts/cargo-safe.sh test -p stagepack-studio --test animate`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: the Animate tab sets click steps, entrances, lists one at a time and transitions"
```

---

### Task 10: Dragging a thing out of the text

**Files:**
- Modify: `crates/app/src/document/arranging.rs`, `crates/app/src/slide_editor/{arranging,mouse}.rs`
- Test: `crates/app/tests/ui_lift.rs` (new), `crates/app/tests/ui_tables.rs`

**Interfaces:**
- Consumes: Task 2's `lift`; Task 5's drag.
- Produces, on `Document`: `pub fn lift_selected(&mut self, to: Placed) -> Result<Vec<usize>, EditRefused>`: one step (`MOVE_OUT`, *Move out of the text*) that lifts the one selected block to that place and leaves it selected. Refused with `EditRefused::CouldNotBeMade` where `lift` is `None`.
- In the window: a drag that starts on a selected block that isn't placed (a table, a picture or shape in the text) shows a dashed outline of its size following the pointer, with snapping, and on release calls `lift_selected` with its drawn size at the place it was dropped. The slide's text closes up. Escape leaves it where it was. Several things selected with one in the text: the drag is refused with *Drag it out of the text first.* A table is dragged by its border, as it's selected.
- A table lifted into a box can still be typed in, and its row and column tools still work.

- [ ] **Step 1: Write the failing tests**

`ui_lift.rs`: `a_table_dragged_out_is_in_a_box_where_it_was_dropped` (the table's own bytes are in the source unchanged, inside a `<Box>` with whole-number `x y w h`); `the_text_closes_up_and_nothing_else_changes`; `it_is_one_undo_step_and_undo_is_exact`; `the_lifted_table_can_be_typed_in_and_gains_a_row`; `a_shape_tag_in_the_text_gets_its_place_written_and_no_box`; `a_hand_written_picture_is_lifted_in_a_box`; `once_lifted_it_has_handles_and_moves_as_any_thing`; `escape_leaves_it_in_the_text`; `a_thing_inside_a_component_is_refused_in_words`; `the_lifted_thing_stays_selected`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_lift`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

If typing in a table inside a `<Box>` doesn't work today, make it work in the engine's and the format crate's flow code with tests there, and say so in the report.

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates
git commit -m "App: a table or picture in the text is dragged out to a place of its own"
```

---

### Task 11: The layout picker

**Files:**
- Create: `crates/app/src/layout_picker.rs`, `crates/app/tests/ui_layouts.rs`
- Modify: `crates/app/src/{ribbon,lib,workspace_view,slide_strip}.rs`, `crates/app/src/document/slides.rs`

**Interfaces:**
- Consumes: Task 2's `set_layout`; Task 3's `layouts()`.
- Produces, on `Document`: `pub fn set_layout(&mut self, layout: &str) -> Result<Vec<usize>, EditRefused>` (one step, `CHANGE_LAYOUT`, *Change layout*; the current layout again is no step) and `pub fn layout_here(&self) -> &'static str`.
- Produces, in `layout_picker`: `pub fn layout_tiles(current: Option<&str>, on_pick: impl Fn(&'static str, &mut Window, &mut App) + 'static, cx: &App) -> AnyElement`: five tiles (`layout_tile_id(name)`), each a small 16:9 drawing of the layout's areas made from `LayoutDef`'s rectangles, with its name under it (*Title*, *Title and content*, *Two columns*, *Section*, *Blank*), the current one marked. Action `SetLayout { layout: &'static str }`.
- In the window:
  - The Home tab's Slides group has a *Layout* button (`LAYOUT_ID`) opening the tiles for the current slide, and the *New slide* arrow shows the same tiles in place of its list of names.
  - The slide list's menu has *Layout* with the five names.
  - After a change, text keeps its place where the new layout has an area of the same name; text of an area the new layout lacks shows in its main area, as the engine already does. The status line says *Layout changed to Two columns.*

- [ ] **Step 1: Write the failing tests**

`ui_layouts.rs`: `the_picker_shows_five_tiles_and_marks_the_current_one`; `picking_two_columns_writes_the_setting_and_nothing_else`; `picking_title_and_content_removes_the_setting`; `the_current_layout_again_makes_no_step`; `it_is_one_undo_step`; `a_moved_area_keeps_its_place_through_a_change_and_back` (`region.body:` stays); `text_in_the_right_column_shows_in_the_main_area_under_title_and_content` (drawn, nothing lost from the source); `new_slide_from_a_tile_adds_that_layout`; `the_slide_lists_menu_changes_the_layout`; `a_talk_with_an_unknown_layout_marks_no_tile`; `the_tiles_draw_the_layouts_areas` (each tile has as many area rectangles as the layout has regions).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_layouts`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a slide's layout is chosen from a picker"
```

---

### Task 12: The real window, the record and the guides

**Files:**
- Create: `docs/checks/plan-d4.md`
- Modify: `README.md`, `docs/guide/*.md` where they describe the editor, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (the D4 line gains its file names), `crates/app/tests/typing_random.rs`

**Interfaces:**
- No code interfaces. `typing_random.rs`'s random session gains the new operations: select also, move, nudge, resize, turn, align, distribute, reorder, animate, lift, change layout, and Undo and Redo among them, checking after each that the talk reads back, that Undo to the start gives the first bytes, and that the keyboard rule holds.

- [ ] **Step 1: Extend the random session and run it**

Run: `PROPTEST_CASES=2000 scripts/cargo-safe.sh test -p stagepack-studio --test typing_random`
Expected: PASS. A failure is a bug to fix in the code it points at, with a named test.

- [ ] **Step 2: Run the app in a real window**

On a scratch display (Xvfb, with `XDG_DATA_HOME` and `XDG_CACHE_HOME` set to scratch folders, never the real ones), with screenshots to `target/check/d4-run/`: build a slide with a title, a picture, three shapes and a table; drag, resize and turn each; snap one to the centre; select three and align and space them; bring one forward; drag the table out of the text; move and resize the title area and put it back; animate three things, reorder their steps, set a list one at a time and a transition; change the layout; present and step through; save, reopen, export to PowerPoint and PDF and open both. Look at every screenshot.

- [ ] **Step 3: Write `docs/checks/plan-d4.md`**

What ran and what was seen, the timings, what couldn't be checked, a Windows hand-check list and a Linux one (Wayland, display scaling, a real mouse's double-click, desktop PowerPoint showing the animations), and known issues.

- [ ] **Step 4: Update the README and the guides**

- [ ] **Step 5: Verify and commit**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and the release timing tests.
Expected: PASS.

```bash
git add docs/checks/plan-d4.md README.md docs/guide docs/superpowers/plans/2026-11-09-native-rust-roadmap.md crates/app/tests/typing_random.rs
git commit -m "Docs: what plan D4 was checked with"
```
