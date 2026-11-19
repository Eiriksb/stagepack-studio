# Plan G3: Terminal Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A recorded terminal session in a talk plays on its slide with its own timing, pauses at marks until the next click, and shows its last screen wherever it can't play: in the editor, thumbnails and exports, where PDF gets real text.

**Architecture:** A new crate, `slides-terminal`, reads an asciinema v2 recording (`.cast`) into a checked, bounded list of timed events, and keeps a terminal screen in Rust that the events are fed to; it draws nothing and knows nothing of windows. The engine treats `<Terminal>` as a placed thing whose layer is the screen at a moment, turned into ordinary display items (filled cells and text in the bundled JetBrains Mono), so the still, thumbnails, PDF and every editing tool work unchanged. The app plays it: a player on a background thread steps the screen through time and hands the window a drawn picture of the newest screen, laid over the layer exactly as video frames are. A recording is a small text file of the talk, held like a chart's data, not a large file.

**Tech Stack:** Rust 1.98.0; `serde_json =1.0.154` and `unicode-width =0.2.3` (in Cargo.lock); the engine's text shaping and tiny-skia renderer; gpui-kit 0.9.0 with gpui-fast 0.1.13 (`paint_image`, `drop_image`); proptest.

**Spec:** `docs/superpowers/specs/2026-11-18-live-content-design.md` (Goal G3; "G3. Terminal replay"; the anti-goals *A talk reaching your computer*, *Lock-in*, *Losing work*). `docs/superpowers/plans/2026-11-19-live-content-plan-g2-decisions.md` and `docs/checks/plan-g2.md` have what plan G2 built and left.

## Global Constraints

- Work on the branch `plan-g3`, cut from `plan-g`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx`, `slides-components`, `slides-video`, `slides-model` and the new `slides-terminal` don't depend on GPUI.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **A recording is data.** Playing one can only put characters and colours on its own screen. A sequence that would set a window's title, write to the clipboard, open or name a link, change the terminal's colours for other programs, ask the terminal a question (and so expect an answer), resize anything but its own screen, or ring a bell does nothing. Nothing in a recording is ever sent anywhere, run, or read as a path. A file that isn't a recording, or is damaged at any byte, shows a plain panel and one plain sentence; it never crashes, hangs, or allocates what the file claims.
- **Limits, checked before anything is kept:** a recording is 32 MB at most (the talk's ordinary file limit); a screen 500 columns by 200 rows at most; 1,000,000 events at most; one event's output 1 MB at most; a recording 24 hours long at most; scrollback isn't kept.
- **Nothing waits on the window's thread** for a recording beyond reading the talk's file as any data file is read: stepping the screen through time and drawing its picture happen off it. An idle window does no work: with nothing playing, nothing redraws and no thread steps a screen.
- **The still is the screen as the recording ends**, laid out by the engine as display items, so it is the same on every machine and a PDF has real, selectable text.
- **Nothing is lost.** A recording is a file of the talk like a chart's data: it goes through saving, the working copy, recovery, packs and copy-and-paste as those do.
- **A visual edit changes only the bytes of what was edited**, and Undo is exact.
- A keystroke keeps its 8 ms budget and a drag frame its 6 ms on a talk holding ten recordings.
- The keyboard rule from D3 holds; in the show, the keys run the show, never the recording.
- Nobody is made to see Markdown, a tag name with angle brackets, a settings key, a file path, or a format's name without a plain word beside it.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless test.
- Tests never touch the owner's real data, cache or Documents folders, never use the owner's own files, and never run on a real display. Test recordings are made by the tests.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: recording inside the app (G5's add-on records); typing into a recording; scrollback or scrolling back through a recording; speed settings; sound (the bell); a recording in PowerPoint as anything but its last screen; asciinema's version 1 or 3 formats.

Rulings made in the plan:
- **A recording is written `<Terminal src="assets/run.cast" x y w h />`**, with its `click` and `enter` as for any placed thing. The screen's size is the recording's own (its header's width and height); the cells are sized to fill the box, keeping their shape, and centred.
- **The screen is the crate's own** (my call): a small, bounded emulator of the sequences recordings use (text, the cursor, erasing, scroll regions, the alternate screen, colours in 16, 256 and full colour, bold, faint, italic, underline, inverse, wide characters), written in Rust with an explicit list of what's ignored, rather than a general terminal library whose every feature would need checking against the rule above. Cost if wrong: a recording that uses a rarer sequence shows it wrong, and that sequence is added.
- **Timing:** each event plays at its own time from the start; a gap longer than a second is shortened to a second. **A mark** (asciinema's `"m"` event) stops the replay there until the next click: a recording's marks are its slide's steps, as a model's views are. With no marks it plays through once and holds its last screen.
- **In the editor a recording shows its last screen.** A selected recording has *Play*, which plays it in place from the start, through its marks without stopping, and *Stop*; it never plays on its own while editing.
- **In the show** it starts when its slide shows (or on its own click, if it has one) and starts again from the beginning each time the slide is shown. Going back a step from a mark shows the screen at the mark before.
- **The presenter's window shows the last screen**, as it shows a video's still.
- **PDF gets the last screen as real text; PowerPoint gets it as a picture**, and the export says once how many recordings became their last screen.
- **Input events (`"i"`) are ignored**; output (`"o"`) is drawn; a resize (`"r"`) resizes the screen within the limits.

## Review Focus

- **A file that isn't a usable recording.** An empty file; a picture named `.cast`; a header that isn't JSON, lacks a width or height, or says 100,000 columns; lines that aren't arrays; times that go backwards, are NaN, negative or past the 24 hours; a single line of 30 MB; a million tiny events; output that is half of a UTF-8 character, or not UTF-8 at all; escape sequences cut off at the end of an event or the file; a CSI with 10,000 parameters or a parameter of 10^30; OSC strings that never end. None crashes, hangs, allocates what the file claims, or takes more than 2 seconds. Owned by Task 1's `any_bytes_read_or_say_why` and Task 2's `any_output_keeps_the_screen_sound`.
- **Sequences that try to reach out.** Title (OSC 0/1/2), clipboard (OSC 52), links (OSC 8), colour changes (OSC 4/10/11/104), queries (DA, DSR, CPR, DECRQSS, XTVERSION, window reports CSI t), the bell, and anything unknown. Each does nothing visible beyond its own screen and nothing is answered. Owned by Task 2's `nothing_in_a_recording_reaches_out`.
- **Playing and leaving.** Play in the editor, then select something else, change slide, delete it, Undo, close the window, start the show; in the show, click through the marks, go back, leave the slide, come back, jump to another slide. Every player thread ends, every picture is released, nothing steps a screen for a slide that isn't shown, and a recording that came back starts from the beginning. Owned by Task 4's `every_way_of_leaving_stops_the_replay`.
- **A long recording.** A recording of two hours with 500,000 events: opening the talk lays out its last screen off the window's thread without waiting the window; going back a step to a mark doesn't replay two hours on the window's thread. Owned by Task 2's `a_long_recording_is_stepped_quickly` and Task 4's `going_back_to_a_mark_is_quick`.
- **A recording and the rest of the talk.** Copy and paste of a slide with a recording into another talk (the file comes too); a crash after inserting one and the next start; save in each form; Save As; a recording whose file is gone; a hand-written `<Terminal>` with no `src`, a `src` outside the talk, a `src` that's a video. Owned by Task 5's `a_hand_written_terminal_reads_or_says_why` and `a_recording_is_kept_saved_and_brought_back`.

---

### Task 1: Reading a recording

**Files:**
- Create: `crates/terminal/{Cargo.toml,src/lib.rs,src/cast.rs}`
- Modify: `Cargo.toml` (workspace member)
- Test: `crates/terminal/tests/cast.rs`

**Interfaces:**
- Produces:
  - `pub struct Recording { pub width: u16, pub height: u16, pub events: Vec<Event>, pub length: Duration, pub marks: Vec<Mark> }`; `pub struct Event { pub at: Duration, pub what: What }`; `pub enum What { Output(String), Resize(u16, u16), Mark(String) }` (input events are dropped while reading); `pub struct Mark { pub at: Duration, pub label: String }`.
  - `pub fn read(bytes: &[u8]) -> Result<Recording, CantPlay>`: asciinema v2 — the first line a JSON object with `version: 2`, `width`, `height` (and optional `timestamp`, `title`, `env` ignored); each further non-empty line a JSON array `[time, code, data]`; blank lines skipped. Times are seconds as numbers, never decreasing (a line whose time goes backwards is damage). Every limit of the Global Constraints checked as it reads, before anything is kept.
  - `pub fn played(recording: &Recording) -> Vec<Event>`: the events at their played times — each gap longer than one second shortened to one second — so `Recording.length` and `Mark.at` are played times too (`read` fills them so).
  - `pub enum CantPlay { NotARecording, Damaged, TooLarge, KindNotPlayed, CouldNotRead }` with plain-word `Display`: *This file isn't a terminal recording.*, *This terminal recording is damaged.*, *This terminal recording is too large to play here. Recordings of up to 32 MB, 24 hours and screens of 500 by 200 can be played.*, *This kind of terminal recording can't be played here. Recordings made by asciinema in its second format can.*, *This terminal recording couldn't be read.*
  - `pub struct RecordingFacts { pub width: u16, pub height: u16, pub length: Duration, pub marks: usize }` and `pub fn facts(bytes: &[u8]) -> Result<RecordingFacts, CantPlay>`, from one pass that keeps no output.
  - Behind a `make` feature, for tests elsewhere: `pub fn make_test_cast(width: u16, height: u16, lines: &[(f64, &str, &str)]) -> Vec<u8>` and `pub fn make_demo_cast() -> Vec<u8>` (a prompt, a command typed a character at a time, coloured output, a mark, a second command, a full-screen program in the alternate screen that exits).

- [ ] **Step 1: Write the failing tests** in `crates/terminal/tests/cast.rs`: `a_made_recording_reads_back` (width, height, events, the length with a 3-second gap played as 1, two marks with their labels and played times); `input_and_unknown_codes_are_left_out`; `version_one_and_three_are_kinds_not_played`; `any_bytes_read_or_say_why` (Review Focus: a property over random bytes, over a made recording with any byte changed, any line removed or repeated, or cut at any length — `read` and `facts` return within 2 seconds, never panic, and no allocation exceeds 64 MB, by a counting allocator); named cases: a header of 100,000 columns; times NaN, negative, decreasing, past 24 hours; a 30 MB line; 1,000,001 events; output that is half a UTF-8 character (kept as the replacement character, not damage); `facts_agree_with_read`; `each_refusal_reads_in_plain_words`.
- [ ] **Step 2: Run them to see them fail.** `scripts/cargo-safe.sh test -p slides-terminal --features make`. Expected: the crate doesn't exist.
- [ ] **Step 3: Write `read`, `facts`, `played` and the makers.** Read line by line from the bytes with the limits checked before a line is parsed; parse each line with `serde_json` into a bounded value (a line over 1 MB of output is TooLarge before it's parsed).
- [ ] **Step 4: Run the tests to see them pass**, then the workspace and clippy.
- [ ] **Step 5: Commit.** `git add Cargo.toml Cargo.lock crates/terminal` then `git commit -m "Terminal: a recording is read and checked"`.

### Task 2: The screen

**Files:**
- Create: `crates/terminal/src/{screen.rs,parse.rs,player.rs}`
- Test: `crates/terminal/tests/{screen.rs,player.rs}`

**Interfaces:**
- Consumes: Task 1's `Recording`, `Event`, `What`, `played`. As built (Task 1's review): `Event.at` is the recorded time; `played(&Recording) -> Vec<Duration>` gives one played time per event, in order, copying no text; `Mark { at: Duration, event: usize }` with `at` the played time; `Recording::label(&Mark) -> &str`.
- Produces:
  - `pub struct Screen` with `Screen::new(width: u16, height: u16)`, `feed(&mut self, bytes: &str)`, `resize(width, height)`, `width()`, `height()`, `cell(col, row) -> &Cell`, `cursor() -> Option<(u16, u16)>` (None when hidden), `rows() -> impl Iterator<Item = &[Cell]>`. `pub struct Cell { pub text: char, pub wide: Wide, pub style: Style }`, `pub enum Wide { Narrow, Left, Right }`, `pub struct Style { pub fg: Colour, pub bg: Colour, pub bold, faint, italic, underline, inverse: bool }`, `pub enum Colour { Default, Indexed(u8), Rgb(u8, u8, u8) }`, and `pub fn rgb(colour: Colour, foreground: bool) -> [u8; 3]` with a fixed 256-colour palette and default colours (light grey on near-black).
  - What it does: printable text with wrapping at the right edge (pending wrap as terminals do), wide characters as two cells by `unicode-width`, combining marks joined to the cell before; CR, LF, BS, HT (stops every 8), the cursor moves (CUU/CUD/CUF/CUB/CUP/HVP/CHA/VPA/CNL/CPL), save and restore (DECSC/DECRC, CSI s/u), erase in display and line (ED/EL, modes 0-2, ED 3 doing nothing as there's no scrollback), insert and delete lines and characters (IL/DL/ICH/DCH/ECH), scroll up and down (SU/SD, IND/RI/NEL), scroll regions (DECSTBM), the alternate screen (modes 47, 1047, 1049), cursor visibility (mode 25), autowrap (mode 7), SGR (0-9, 21-29, 30-37, 38;5;n, 38;2;r;g;b, 39, 40-47, 48;…, 49, 90-97, 100-107), RIS. Everything else — every OSC, DCS, APC, PM, SOS string, every query, the bell, other modes — is consumed and does nothing. A CSI keeps at most 32 parameters, each clamped to 65,535; a string sequence's content is dropped as it's read, so an unending one costs no memory.
  - `pub struct Player` with `Player::new(recording: Arc<Recording>)`, `screen_at(&mut self, at: Duration) -> &Screen` (steps forward from where it is; going back starts again from the nearest kept snapshot), `next_stop(after: Duration) -> Option<Duration>` (the next mark), `stops() -> &[Duration]`, `last() -> Screen` (the screen as it ends). It keeps a snapshot of the screen every 10 seconds of played time and at every mark, at most 1,000 snapshots, so going back is quick.

- [ ] **Step 1: Write the failing tests**: `screen.rs` — `text_wraps_and_scrolls`; `colours_and_styles_are_kept` (16, 256, full colour, bold, inverse); `wide_and_combining_characters_take_their_cells`; `erasing_and_inserting_move_the_right_cells`; `a_scroll_region_scrolls_only_itself`; `the_alternate_screen_comes_and_goes`; `nothing_in_a_recording_reaches_out` (Review Focus: each named sequence fed in, the screen's cells unchanged but for what it printed around it, and the screen offers nothing to send — it has no output at all); `any_output_keeps_the_screen_sound` (property: random bytes and random sequences fed in any split across events — the cursor stays on the screen, every row has width cells, a wide character always has its right half, no panic, within the time and the 64 MB). `player.rs` — `a_recording_plays_at_its_own_times`; `marks_are_stops`; `going_back_is_the_same_as_going_forward` (the screen at t reached forward and reached after going back are equal, for many t); `a_long_recording_is_stepped_quickly` (Review Focus: a made two-hour recording of 500,000 events — `last()` within 2 s in release, and `screen_at` going back to any mark within 50 ms in release; ignored in debug).
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Write the parser, the screen and the player.** The parser is a state machine over chars (ground, escape, CSI, string) with no allocation beyond its bounded parameters.
- [ ] **Step 4: Run the tests to see them pass**, then the workspace and clippy.
- [ ] **Step 5: Commit.** `git commit -m "Terminal: a screen a recording is played on, and its player"`.

### Task 3: A recording on a slide

**Files:**
- Modify: `crates/engine/{Cargo.toml,src/elements.rs,src/builtins.rs,src/slide.rs,src/display.rs,src/deck.rs,src/lib.rs}`, `crates/pdf`, `crates/pptx` where they list what became pictures
- Create: `crates/engine/src/terminal_screen.rs`
- Test: `crates/engine/tests/terminal.rs`

**Interfaces:**
- Consumes: Tasks 1-2's `read`, `Recording`, `Screen`, `Player`, `CantPlay`.
- Produces:
  - The built-in tag `Terminal` with `src` (a talk's `.cast` file), placed like any other thing, its natural size the screen's cells at 18 px a row, keeping its shape.
  - `pub fn screen_items(screen: &Screen, rect: Rect) -> Vec<DisplayItem>`: a filled rectangle for the background, one fill per run of cells sharing a background colour, one `Text` item per run of cells sharing a style (JetBrains Mono regular, bold or italic, the cell's size fitted to the box and centred, kept on whole pixels), underline as a stroke, the cursor as a block when shown. The same items draw the still and every frame of the replay, so they look the same.
  - `LayerSpec.terminal: Option<TerminalOn { src, rect, rotate, stops: usize }>` and the layer's items being the last screen. A recording's marks add their count to its slide's steps, as a model's views do.
  - `Deck::terminal_problem(source range) -> Option<CantPlay or the file's problem>`, `Deck::terminals()` for the app, and a panel with the problem's sentence where a recording can't be read, as a model's panel shows.
  - Reading the file and laying out its last screen happen in the still jobs plan G1 made, off the window's thread; the layout before the still arrives shows a plain dark panel of the box's size.
  - Exports: PDF draws the layer's items, so its text is real; PowerPoint draws the layer as a picture; each says once *N terminal recordings were exported as their last screen.* (*1 terminal recording was …*).

- [ ] **Step 1: Write the failing tests**: `a_recording_lays_out_as_its_last_screen` (the items' text is the screen's text, row by row, with its colours); `the_screen_fills_its_box_keeping_its_shape`; `a_pdf_has_the_screens_text` (the exported PDF's text holds a word printed by the recording); `a_recording_that_cant_be_read_is_a_panel_saying_why`; `marks_are_steps`; `ten_recordings_lay_out_off_the_window_thread`.
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Write the tag, the items, the layer, the still job and the exports' line.**
- [ ] **Step 4: Run the tests to see them pass**, then the workspace and clippy.
- [ ] **Step 5: Commit.** `git commit -m "Engine: a terminal recording is laid out on its slide as its last screen"`.

### Task 4: Playing it

**Files:**
- Create: `crates/app/src/replay.rs`
- Modify: `crates/app/src/{slide_canvas.rs,present_view.rs,slide_editor/*,main.rs}` as the video player and the model Turner are wired
- Test: `crates/app/tests/ui_terminal.rs`

**Interfaces:**
- Consumes: Task 2's `Player`; Task 3's `TerminalOn`, `screen_items`, the slide's steps.
- Produces:
  - `replay::Replay`: one per playing recording, on a background thread of its own that steps a `Player` and draws the newest screen with the engine's renderer at the box's device size, handing the window a picture no oftener than 60 a second and only when the screen changed. The window's thread takes the newest picture with no wait and lays it over the layer, as video frames are laid; a picture it no longer needs is released.
  - In the show: starts when the slide shows (or on its click), from the beginning; stops at each mark until the next click; the show's back step to a mark shows the screen at that mark (from the player's snapshots, off the window's thread); leaving the slide ends the thread and releases every picture; coming back starts again.
  - In the editor: `TERMINAL_PLAY_ID` (*Play*) and `TERMINAL_STOP_ID` (*Stop*) for a selected recording; it plays through its marks without stopping and holds its last screen; selecting something else, changing slide, the text changing, the window losing focus or the show starting stops it.
  - The presenter's window shows the last screen.

- [ ] **Step 1: Write the failing tests** in `ui_terminal.rs`: `the_show_plays_a_recording_and_stops_at_its_marks`; `a_click_goes_on_from_a_mark`; `going_back_to_a_mark_is_quick` (Review Focus: on the long recording, the window's frame during the back step under 16 ms and the mark's screen shown within 200 ms); `play_in_the_editor_plays_through_and_holds_the_end`; `every_way_of_leaving_stops_the_replay` (Review Focus: every way listed there — no thread left, pictures made equals pictures released, nothing stepped for a slide not shown, and a slide come back to starts from the beginning); `an_idle_talk_of_recordings_does_no_work` (ten recordings, none playing: no frames asked for, no thread); `the_presenter_shows_the_last_screen`; `the_window_never_waits_for_the_replay` (a player slowed by a test hook: every window frame under 50 ms).
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Write `Replay` and wire it into the show and the editor** the way `video_player` and `model_view::Turner` are.
- [ ] **Step 4: Run the tests to see them pass**, then the workspace and clippy.
- [ ] **Step 5: Commit.** `git commit -m "App: a terminal recording plays in the show and the editor"`.

### Task 5: Putting a recording on a slide

**Files:**
- Modify: `crates/app/src/{added_files.rs,insert.rs,inspector*,workspace_view/inserting.rs,ribbon*}`, `crates/app/src/document/*` where a data file is added
- Test: `crates/app/tests/ui_terminal.rs`, `crates/app/tests/export.rs`

**Interfaces:**
- Consumes: Task 1's `facts`, `CantPlay`; Task 3's tag.
- Produces:
  - `FileKind::Recording` (`.cast`, the `assets` folder, the ordinary 32 MB limit), going through every path a data file does: added, saved, kept in the working copy, brought back, packed, copied between talks.
  - *Terminal recording* on the Insert tab (`INSERT_TERMINAL_ID`), with a file dialog; a dropped or pasted `.cast` does the same. `Document::insert_terminal` writes `<Terminal src x y w h />` at the screen's natural size fitted to the slide, as one step, selected, on the slide shown when it was asked for. A file that isn't a recording, or can't be played, is refused before it's added, with its sentence.
  - The form: the place fields and one line of facts (*80 by 24, 3 min 20 s, 2 stops*), read off the window's thread.
- [ ] **Step 1: Write the failing tests**: `a_chosen_recording_is_put_on_the_slide`; `a_dropped_or_pasted_recording_is_too`; `a_file_that_isnt_a_recording_is_refused_saying_why`; `the_form_says_what_the_recording_is`; `a_recording_is_kept_saved_and_brought_back` (Review Focus: each save form, Save As, the working copy after a crash, a pack, copy and paste into another talk); `a_hand_written_terminal_reads_or_says_why` (Review Focus: no `src`, a `src` outside the talk, a video's `src`, a file gone — each a panel and a notice, moving and deleting touch only its bytes, Undo exact); `exports_say_how_many_recordings_became_their_last_screen`.
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Write the kind, the insert, the drop and paste, the form and the export line.**
- [ ] **Step 4: Run the tests to see them pass**, then the workspace and clippy.
- [ ] **Step 5: Commit.** `git commit -m "App: a terminal recording is put on a slide from the Insert tab, a drop or a paste"`.

### Task 6: The real window, the record and the guides

**Files:**
- Create: `docs/checks/plan-g3.md`
- Modify: `README.md`, `docs/guide/components.md` where it lists what a slide can hold, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (the G3 line gains its file names), `crates/app/tests/{typing_random,edit_speed}.rs`

- [ ] **Step 1: Extend the timing tests and the random session**: `edit_speed.rs` gains, ignored and in release, a keystroke and a drag frame on a talk of ten slides each holding a recording, opening that talk, and the long recording playing for 300 frames with the window thread's share of a frame at the median and 99th percentile; `typing_random.rs`'s window run gains inserting a recording, playing it in the editor, and the show's clicks through its marks.
- [ ] **Step 2: Run the app in a real window**, on a scratch display with a scratch home as plan G2's record describes, with made recordings (the demo, the long one, one using the alternate screen and full colour): insert each, play each, present with clicks through the marks and back, save as a pack, reopen, kill and bring back, copy a slide with one into another talk, export to PDF (check the text can be selected) and PowerPoint.
- [ ] **Step 3: Write `docs/checks/plan-g3.md`**, with a hand-check list for a real Linux display and for Windows, including a recording made by `asciinema rec` on the owner's own computer.
- [ ] **Step 4: Update the README and the guides.**
- [ ] **Step 5: Verify and commit.** `git commit -m "Docs: what plan G3 was checked with"`.
