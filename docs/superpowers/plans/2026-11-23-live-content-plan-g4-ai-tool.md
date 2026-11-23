# Plan G4: The AI Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Without opening a window, `stagepack-studio` can tell an AI what a talk may hold, check a talk and say every complaint by slide, give its outline, and draw any slide at any click step as a PNG, both as commands and as a Model Context Protocol server over standard input and output, with a guide for the AI shipped in the app.

**Architecture:** A new module tree `crates/app/src/ai_tool/` in the app crate, so it reuses the app's headless pieces (opening a folder or a pack, the 3D drawer's stills, the always-build permissions, the bundled compiler) without GPUI. `main.rs` sends a first argument that names a command to it and never starts GPUI, recovery or a window for one. Each command is a plain function from a talk path and options to a value that prints as plain text or JSON; the MCP server is a small hand-written JSON-RPC 2.0 loop over newline-delimited stdio that calls the same functions and opens the talk afresh for every call, since the AI edits its files between calls.

**Tech Stack:** Rust 1.98.0; `serde`/`serde_json` (workspace); `base64 =0.22.1` (already the app's); tiny-skia 0.11.4's `Pixmap::encode_png`; the engine's `Deck`; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-18-live-content-design.md` (Goal G4; "G4. A tool for an AI"; the anti-goals *A talk reaching your computer* and *Needing anything else*). `docs/checks/plan-g3.md` and `docs/superpowers/plans/2026-11-19-live-content-plan-g3-decisions.md` have what G3 built and left.

## Global Constraints

- Work on the branch `plan-g4`, cut from `plan-g`.
- No GitHub Actions. Don't push.
- Run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx`, `slides-components`, `slides-video`, `slides-model` and `slides-terminal` don't depend on GPUI, and `ai_tool` uses nothing of GPUI or `gpui_kit` either (a test greps for it).
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **No window, ever, for a command.** A command never calls `gpui_kit::application()`, never starts the recovery sweep, and works with `DISPLAY` and `WAYLAND_DISPLAY` unset.
- **Reads and writes only inside the talk.** The tool reads the talk it's pointed at (through `FolderFiles` or `PackedFiles`, so a link can't lead out of it) and the app's own build permissions file, read-only. It writes nothing anywhere except: the talk's `build` folder when it builds (below), and, for the `picture` command only, the one file named by `--out`. The MCP server writes nothing but its replies on standard output; its log, if any, goes to standard error.
- **Compiles nothing unless the folder is one the owner said to always build** (`BuildPermissions::always`) and this copy of the app has its compiler (`builds::bundled_compiler()`). A packed talk is never built. Otherwise components are checked as they are built now, and an unbuilt or out-of-date one is reported as the window reports it.
- **No network.** The server talks over its standard input and output only; nothing opens a socket.
- **A broken talk or a broken request never kills the tool.** Laying out a slide is guarded as the window guards it (`catch_unwind`); a slide that fails says so and the others are still checked. A bad request gets a JSON-RPC error and the server reads the next line.
- **Limits:** one line of input to the server is 4 MB at most; a picture is 64 to 3,840 pixels wide (default 1,920); a slide number is 1 to the talk's slide count; a step is 0 to the slide's steps.
- Every message a person or an AI reads is a plain sentence. Slides are numbered from 1, in the order of the text, hidden ones included and marked hidden.
- Tests never touch the owner's real data, cache or Documents folders, never use the owner's own files, and never run on a real display: every test that runs the program sets `HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_CONFIG_HOME` to a temporary folder and removes `DISPLAY` and `WAYLAND_DISPLAY`.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: editing a talk for the AI (it writes the files itself); an HTTP or socket transport for MCP; MCP resources, prompts or sampling; packaging the command for each system (stage E); a live terminal (G5).

Rulings made in the plan:
- **The commands** (my call), each taking a talk as a folder or a `.sptx` file:
  - `stagepack-studio describe [TALK] [--json]`: what a talk may hold — the layouts and their regions, the talk's settings, a slide's settings, the placement settings every placed thing takes, and every built-in tag with each setting's name, kind, whether it's needed, its label in the form panel, what it means, and its choices. Given a talk, its own built components too, from their schemas.
  - `stagepack-studio check TALK [--json]`: every complaint, by slide, as the window would show it. Exit status 0 when there are none, 1 when there are some.
  - `stagepack-studio outline TALK [--json]`: each slide's number, layout, first words, steps, whether it's hidden and whether it has notes.
  - `stagepack-studio picture TALK SLIDE [--step N] [--width W] --out FILE.png`: the slide at step `N` (default: its last, the whole slide) as a PNG.
  - `stagepack-studio guide`: prints the guide for the AI.
  - `stagepack-studio mcp TALK`: the MCP server for that one talk.
  - `stagepack-studio help` (and `--help`, `-h`): what each does.
  - Exit status 2 for a talk that can't be opened or arguments that can't be read, with one plain sentence on standard error. A first argument that isn't a command name opens a window as now, so to open a folder called `check`, write `./check`.
- **MCP by hand** (my call): JSON-RPC 2.0, one message per line, the methods `initialize`, `notifications/initialized`, `ping`, `tools/list` and `tools/call`. Protocol versions `2025-06-18`, `2025-03-26` and `2024-11-05` are known; `initialize` answers with the client's if it's one of those, else `2025-06-18`. Tools: `describe`, `check`, `outline`, `picture` (returns an `image` content of `image/png`, base64, and a `text` content naming the slide, step and its complaints) and `guide`. A tool that fails returns `isError: true` with a plain sentence; an unknown method is error `-32601`, bad JSON `-32700`, bad params `-32602`. A batch (a JSON array) is answered with `-32600`. The talk is opened afresh for each call. Cost if wrong: an assistant that needs another method adds it.
- **A picture's moment** is `Moment { step, time: 0.0 }` drawn with every layer as it settles (as the PDF draws), so a box that fades in at click 1 is fully drawn at step 1 and absent at step 0. Videos are their stills, models are drawn as stills by `model_view::draw_still`, recordings are their last screen, all as the PDF export does.
- **What the window shows, for check:** for each slide, its parse error if it has one, `Deck::notices_whole`, and for each `<Video>`, `<Model>` and `<Terminal>` on it the deck's `video_problem`/`model_problem`/`terminal_problem` for that tag's source; for the talk, `Deck::component_problems` and any build problems. Each prefixed in text by `Slide N: `.
- **What each setting means** is written next to its label, in `inspector/labels.rs`, in one table the form panel already reads, so describing a built-in and showing it in the form come from the same place; a test fails when a built-in's schema has a setting with no meaning. The settings the form leaves out (a file's `src`, a model's `view`/`views`) are described too, since an AI writes them.
- **The guide** lives at `docs/guide/writing-talks-for-ai.md` and is put in the program with `include_str!`. Every example in it is checked by a test to open with no complaints.

## Review Focus

- **A path that isn't a talk.** Nothing at the path; a folder with no `slides.md`; a `slides.md` that isn't UTF-8 or is 0 bytes; a damaged `.sptx`; a picture named `.sptx`; a path with spaces or that isn't UTF-8; a folder whose `assets/x.png` is a link to `/etc/passwd`. Each gives exit status 2 or a complaint in plain words, never a panic, and nothing outside the talk is read. Owned by Task 1's `a_path_that_isnt_a_talk_says_why` and Task 3's `a_link_out_of_the_talk_is_a_complaint`.
- **Requests that aren't right.** A line that isn't JSON; a line of 5 MB; a batch; no `id`; an unknown method; `tools/call` for an unknown tool; `slide` of 0, -1, 10^12, `"3"` or a float; `step` past the slide's steps; `width` of 0 or 100,000; standard input closed in the middle of a line. Each gets the error the ruling says, the server reads on, and closing standard input ends it with status 0. Owned by Task 5's `any_line_gets_an_answer_or_none`.
- **The talk changing between calls.** The AI writes `slides.md` between two `check` calls, or deletes it, or leaves it empty; each call sees the files as they are then. Owned by Task 5's `each_call_reads_the_talk_afresh`.
- **Components and the always-build rule.** A talk with an unbuilt component and no permission is reported, not built, and `build` is untouched; the same folder with permission (written to the scratch data folder) is built once and then checked clean; a packed talk with the same component is never built. Owned by Task 3's `builds_only_where_always_built`.
- **Big talks.** A talk of 300 slides is checked in under 10 seconds in a debug build; a talk with a 400 MB video file checks, outlines and draws its slide without reading the video into memory (peak growth under 64 MB, by the counting allocator the app tests already use, or by `/proc/self/status` `VmHWM` in a child run). Owned by Task 3's `a_big_talk_is_checked_quickly` and Task 4's `a_slide_with_a_big_video_is_drawn_from_its_still`.

---

### Task 1: Commands without a window, and the outline

**Files:**
- Create: `crates/app/src/ai_tool/mod.rs` (the command line), `crates/app/src/ai_tool/talk.rs` (opening a talk headless), `crates/app/src/ai_tool/outline.rs`
- Modify: `crates/app/src/main.rs`, `crates/app/src/lib.rs`
- Test: `crates/app/tests/ai_tool.rs` (runs the built program; a helper `run(args, home) -> Output` sets the scratch folders and removes the display variables)

**Interfaces:**
- Produces:
  - `pub fn run_command(args: &[OsString]) -> Option<i32>` in `ai_tool`: `None` when `args[1]` isn't a command name (so `main` opens a window as now), else the command's exit status after it has printed. `main.rs` calls it first and `std::process::exit`s with its status.
  - `pub struct OpenTalk { pub deck: Deck, pub files: Arc<dyn TalkFiles + Send + Sync>, pub place: TalkPlace }` where `pub enum TalkPlace { Folder(PathBuf), Packed(PathBuf) }`, and `pub fn open_talk(path: &Path) -> Result<OpenTalk, String>`: a folder through `FolderFiles` and its `slides.md`, a file through `open_packed`; its fonts loaded as `export::make` loads them (`Fonts::with_system_fallback` plus the talk's own); `deck.draw_models_with(Arc::new(model_view::draw_still))`. The error is one plain sentence naming the path, e.g. *There's no talk at /x/y.* or */x/y has no slides.md, so it isn't a talk.*
  - `pub struct OutlineSlide { pub slide: usize, pub layout: String, pub first_words: String, pub steps: u32, pub hidden: bool, pub notes: bool }` (Serialize) and `pub fn outline(talk: &mut OpenTalk) -> Vec<OutlineSlide>`. `first_words` is the slide's first 80 characters of text, whitespace collapsed, cut at a character boundary with `…` when cut.
  - Plain-text forms: `1. Title — "Hello there" (2 clicks, notes)`; hidden slides end `, hidden)`.
- Consumes: `export::make`'s font loading (factor it into a shared `pub(crate) fn talk_fonts(...)` if it's not already reusable), `document::open`'s `read_talk_text`, `model_view::draw_still`, `model_view::wait_closed` (called before exit).

- [ ] **Step 1: Write the failing tests** in `crates/app/tests/ai_tool.rs`:
  - `outline_lists_each_slide`: a scratch talk of three slides (title layout "Hello", a two-column slide with one `<Box click="1">`, a hidden slide with notes) gives, in text, three lines starting `1. `, `2. `, `3. `, the second saying `1 click`, the third `hidden` and `notes`; with `--json`, an array of three objects whose `steps` are `[0, 1, 0]` and `hidden` `[false, false, true]`. Exit status 0.
  - `a_path_that_isnt_a_talk_says_why`: for a missing path, an empty folder, a folder whose `slides.md` is the bytes `[0xff, 0xfe]`, a file of random bytes named `x.sptx`, and a path with a space: exit status 2, one line on standard error, nothing on standard output, no `panicked` in standard error.
  - `a_command_opens_no_window`: `outline` with `DISPLAY`/`WAYLAND_DISPLAY` unset succeeds, and the scratch `XDG_DATA_HOME` has no `stagepack-studio` folder afterwards (no recovery, no working copies).
  - `help_names_every_command`: `help`, `--help` and `-h` print the names `describe`, `check`, `outline`, `picture`, `guide`, `mcp`; exit status 0. An unknown option to a command (`outline TALK --frobnicate`) is status 2 with a sentence naming it.
- [ ] **Step 2: Run them to see them fail.** `scripts/cargo-safe.sh test -p stagepack-studio --test ai_tool` — FAIL (no commands).
- [ ] **Step 3: Implement `run_command`, `open_talk`, `outline`, and the dispatch in `main.rs`.** Arguments are read by hand (no new dependency): the command, then positionals and `--name value` / `--flag` options; anything else is status 2.
- [ ] **Step 4: Run the tests.** Same command — PASS. Then `scripts/cargo-safe.sh test -p stagepack-studio` — PASS.
- [ ] **Step 5: Commit** `App: commands without a window, starting with a talk's outline`.

### Task 2: What a talk may hold

**Files:**
- Create: `crates/app/src/ai_tool/describe.rs`
- Modify: `crates/app/src/inspector/labels.rs` (a meaning beside each label, and the left-out settings described)
- Test: `crates/app/tests/ai_tool.rs`, and a unit test in `labels.rs`

**Interfaces:**
- Produces:
  - `pub fn meaning(tag: &str, argument: &str) -> Option<&'static str>` in `inspector::labels`, from the same table as the labels (the table's rows grow a third column; the form panel keeps reading labels from it unchanged).
  - `#[derive(Serialize)] pub struct Description { pub layouts: Vec<LayoutInfo>, pub talk_settings: Vec<SettingInfo>, pub slide_settings: Vec<SettingInfo>, pub placement: Vec<SettingInfo>, pub tags: Vec<TagInfo> }` with `LayoutInfo { name, regions: Vec<String>, main }`, `SettingInfo { name, meaning, values: Option<Vec<String>> }`, `TagInfo { tag, built_in: bool, takes_content: bool, settings: Vec<FieldInfo> }`, `FieldInfo { name, kind, needed: bool, label: Option<String>, meaning: Option<String>, choices: Vec<String> }`.
  - `pub fn describe(talk: Option<&mut OpenTalk>) -> Description`. Layouts from `slides_engine::layouts()`; talk settings from `slides_format::TALK_SETTINGS`; slide settings (`layout`, `transition`, `hidden`, and any other key `slide_ops` reads — the implementer lists them from that file); placement from `slides_engine::PLACEMENT_ATTRS` with the values `slide_ops` accepts (`enter`: `appear`, `fade`, `fly-up`); built-in tags from `builtin_registry()` schemas, labels and meanings from `labels.rs`, choices as the form shows them. With a talk, its own components from `Deck::components()` and `component_schema`, `built_in: false`, meanings `None`.
  - Text form: a heading per part, a line per item, `name (kind, needed) — meaning`.
- [ ] **Step 1: Write the failing tests.**
  - In `labels.rs`: `every_built_in_setting_has_a_meaning` — for every tag of `builtin_registry()` and every prop of its schema, `meaning(tag, prop)` is `Some` and not empty.
  - In `ai_tool.rs`: `describe_without_a_talk_lists_every_built_in` — `describe --json` has a `tags` entry for each of `Shape Box Icon Picture Video Model Terminal Chart`; `Video`'s settings include `src` (needed) and `loop`; `layouts` names the five layouts with their regions; `placement` names `x y w h rotate click enter align`, `enter` with values `["appear","fade","fly-up"]`.
  - `describe_with_a_talk_adds_its_components` — the components fixture talk (`fixtures/components`, copied to scratch with its `build`) gives a `score` tag with `built_in: false` and its arguments.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Write the meanings and `describe`.** Each meaning is one plain sentence in the app's own voice (e.g. Video `loop`: *Plays the video again from the start when it ends.*; Model `turn`: *`auto` turns the model slowly on its own while the slide shows.*; Terminal `src`: *The terminal recording to play, an asciinema `.cast` file in the talk.*).
- [ ] **Step 4: Run the tests** and the inspector's own tests (`--test ui_inspector` or whichever covers labels) — PASS.
- [ ] **Step 5: Commit** `App: describe what a talk may hold, from the form panel's own table`.

### Task 3: Checking a talk

**Files:**
- Create: `crates/app/src/ai_tool/check.rs`
- Test: `crates/app/tests/ai_tool.rs`

**Interfaces:**
- Produces:
  - `#[derive(Serialize)] pub struct Checked { pub slides: Vec<SlideProblems>, pub talk: Vec<String> }`, `SlideProblems { slide: usize, hidden: bool, problems: Vec<String> }` (only slides with problems listed), and `pub fn check(talk: &mut OpenTalk, permissions: &BuildPermissions) -> Checked`.
  - Building first, only when `talk.place` is a folder, `permissions.always(folder)`, and `bundled_compiler()` is `Some`: `builds::build_components(folder, &compile)`, its problems in `talk`, then the deck's components refreshed (`Deck::refresh_components`).
  - Each slide's problems, in this order: its parse error, `notices_whole`, then each video/model/terminal tag's problem (walk the slide's blocks as `export::videos_on` and friends do). A slide whose layout panics gives the one problem *This slide couldn't be laid out.* and the next slide is checked.
  - Text form: `Slide N: <problem>` lines, then the talk's, then a last line `No problems found.` or `N problems on M slides.`; exit status 0 or 1.
- Consumes: `OpenTalk` (Task 1), `permissions::BuildPermissions::{load, default_file}`.
- [ ] **Step 1: Write the failing tests.**
  - `check_says_each_problem_by_slide`: a talk whose slide 2 has `<Picture src="assets/missing.png" />` and slide 3 `<Video src="assets/clip.mp4" />` where `clip.mp4` is a text file; `check` exits 1, has a `Slide 2: ` line and a `Slide 3: ` line, each with the sentence the window shows for that case (take the sentences from the engine/app, don't retype them); `--json` has those two slides only. A clean talk exits 0 with `No problems found.`.
  - `a_link_out_of_the_talk_is_a_complaint` (unix only): `assets/x.png` a symlink to a file outside the scratch talk; `check` complains about slide 1's picture and the outside file's bytes appear nowhere in the output.
  - `builds_only_where_always_built`: the components fixture with its `build` folder removed. With no permission: `check` reports the component as not built and `build` doesn't exist afterwards. With `build-permissions.json` written into the scratch `XDG_DATA_HOME/stagepack-studio/` naming the folder, and only if the compiler bundle is found (skip with a printed reason otherwise, as the components tests do): `check` exits 0 and `build/` exists. A `.sptx` of the same talk without its build: never builds.
  - `a_big_talk_is_checked_quickly`: 300 slides of text, a box and a chart each, `check` under 10 s.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement `check` and the command.**
- [ ] **Step 4: Run the tests** — PASS.
- [ ] **Step 5: Commit** `App: check a talk without a window, as the window would`.

### Task 4: A picture of a slide

**Files:**
- Create: `crates/app/src/ai_tool/picture.rs`
- Test: `crates/app/tests/ai_tool.rs`

**Interfaces:**
- Produces:
  - `pub struct Picture { pub png: Vec<u8>, pub slide: usize, pub step: u32, pub steps: u32, pub problems: Vec<String> }` and `pub fn picture(talk: &mut OpenTalk, slide: usize, step: Option<u32>, width: u32) -> Result<Picture, String>`. `slide` is 1-based; out-of-range slide, step or width is an `Err` sentence such as *This talk has 12 slides, so there's no slide 13.* The pixmap is `width` wide and `width * 9 / 16` high, the deck's layers at `Moment { step, time: 0.0 }` composed over the theme's background at scale `width / 1920`, encoded with `Pixmap::encode_png`. `problems` is that slide's `check` problems.
  - The command writes `png` to `--out` with `write_atomically`, prints *Slide 3, click 1 of 2, written to out.png.* and the slide's problems, exit 0 (1 if it had problems, as `check`).
- [ ] **Step 1: Write the failing tests.**
  - `a_picture_is_the_slide_at_its_step`: a slide with a red full-slide `<Shape kind="rect" fill="#ff0000" x="0" y="0" w="1920" h="1080" click="1" />` on a white background: at `--step 0` the PNG's centre pixel is the background, at `--step 1` (and with no `--step`) it's red; width 640 gives a 640×360 PNG (decode with the `image` dev-dependency).
  - `bad_numbers_are_said_plainly`: slide 0, slide 99, step 5 on a one-step slide, width 10 and 100000: status 2, one sentence each, no file written.
  - `a_slide_with_a_big_video_is_drawn_from_its_still`: a sparse 400 MB file `assets/big.mp4` (made with `set_len`, not written) and `poster="assets/still.png"`; the picture succeeds, shows the still's colour in the video's box, and the child's `VmHWM` (Linux) stays under 256 MB.
  - `models_and_recordings_are_drawn` : a model made by `slides-model`'s `make` feature and a recording by `slides-terminal`'s `make_demo_cast`; the PNG isn't blank in either box (some pixel differs from the background). If the model drawer can't get a device, the model's box shows its plain panel and the test only checks that the command succeeded.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement `picture` and the command.**
- [ ] **Step 4: Run the tests** — PASS.
- [ ] **Step 5: Commit** `App: draw a slide at a click step as a PNG, without a window`.

### Task 5: The MCP server

**Files:**
- Create: `crates/app/src/ai_tool/mcp.rs`
- Test: `crates/app/tests/ai_tool_mcp.rs` (spawns `stagepack-studio mcp TALK` with piped stdio and the scratch environment; a helper sends a line and reads one line back with a 30 s timeout)

**Interfaces:**
- Produces:
  - `pub fn serve(talk: &Path, input: impl BufRead, output: impl Write) -> io::Result<()>`; the `mcp` command calls it with locked stdin/stdout. Reads lines up to 4 MB (a longer line is answered `-32700` with `id: null` and skipped to its end without being held); blank lines ignored; ends at end of input.
  - `initialize` result: `protocolVersion` per the ruling, `capabilities: { tools: {} }`, `serverInfo: { name: "stagepack-studio", version: env!("CARGO_PKG_VERSION") }`, `instructions`: two sentences saying which talk it serves and to call `guide` first.
  - `tools/list`: the five tools, each with a `description` and an `inputSchema` (`picture`: `slide` integer ≥ 1 required, `step` integer ≥ 0, `width` integer 64–3840; the others no arguments). `describe` includes the talk's own components.
  - `tools/call`: opens the talk with `open_talk` each time and runs the Task 1–4 function; the text content is the same text the command prints; `picture` gives `[{type:"image", data, mimeType:"image/png"}, {type:"text", text}]`. A call's panic is caught and returned as `isError: true`, *Something went wrong drawing this talk.*
  - Notifications (no `id`) get no answer.
- [ ] **Step 1: Write the failing tests.**
  - `an_assistant_can_list_and_call_the_tools`: `initialize` with `2025-06-18` answers it; `tools/list` names `describe check outline picture guide`; `tools/call outline` returns text with `1. `; `tools/call picture {slide:1, width:320}` returns an image that base64-decodes to a 320×180 PNG.
  - `any_line_gets_an_answer_or_none`: each bad input from the Review Focus line gets its error code (or none for a notification), and a `ping` after all of them still answers `{}`. Closing stdin ends the process with status 0 within 5 s.
  - `each_call_reads_the_talk_afresh`: `check` clean; then the test adds `<Picture src="assets/missing.png" />` to `slides.md`; `check` now has the complaint; then empties `slides.md`; `outline` answers (no slides, or one empty slide — whatever `open_talk` gives) without an error from the server.
  - `the_server_writes_nothing`: after a session of every tool, the scratch talk's files and the scratch home are byte-for-byte as before (compare a listing with sizes and hashes).
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement `serve`.**
- [ ] **Step 4: Run the tests** — PASS.
- [ ] **Step 5: Commit** `App: an MCP server for one talk, over standard input and output`.

### Task 6: The guide for the AI, and the record

**Files:**
- Create: `docs/guide/writing-talks-for-ai.md`, `crates/app/src/ai_tool/guide.rs`, `docs/checks/plan-g4.md`
- Test: `crates/app/tests/ai_tool.rs`

**Interfaces:**
- Produces: `pub const GUIDE: &str = include_str!("../../../../docs/guide/writing-talks-for-ai.md");` printed by `guide` and returned by the MCP `guide` tool.
- The guide says, in plain words for an AI: a talk is a folder with `slides.md` and `assets/`; slides are separated by `---`; the settings block and its keys; layouts and regions and how content goes to a region; text, lists, tables, code, notes; placing a tag with `x y w h` on the 1920×1080 slide; clicks and `enter`; each built-in tag with one example; the talk's own components and that the tool won't build them unless the owner said so; and the working loop — write the files, `check`, look with `picture` at each step, fix, repeat. It points to `describe` for the full list rather than copying it.
- [ ] **Step 1: Write the failing tests.**
  - `every_example_in_the_guide_checks_clean`: each fenced block marked `markdown` in `GUIDE` is written as the `slides.md` of a scratch talk (with the small `assets` files it names made by the test: a PNG, a `.cast`, a CSV; a video or model example uses a `poster` and the test accepts only *can't be played*-style complaints for it if any — list them), and `check` finds no problems.
  - `the_guide_names_every_built_in`: every tag of `builtin_registry()` appears in `GUIDE` as `<Tag`.
  - `guide_prints_the_guide`: `guide` prints exactly `GUIDE`.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Write the guide and `guide.rs`.**
- [ ] **Step 4: Run the whole suite**: `scripts/cargo-safe.sh test --workspace` — PASS, with the totals written in the record.
- [ ] **Step 5: Write `docs/checks/plan-g4.md`**: what was built, the commands with an example of each, how to connect an assistant (`claude mcp add stagepack -- stagepack-studio mcp /path/to/talk`, and the JSON for other clients), what ran and passed, and the hand checks for the owner: connect Claude Code to a scratch talk, ask it to make a three-slide talk with a chart and look at each slide, and see the pictures match the window; the same on Windows from a fresh clone, where `picture` and `mcp` must work from PowerShell. Leave a `## The final review` heading with *To be written by the controller.*
- [ ] **Step 6: Commit** `Docs: a guide for an AI writing a talk, and plan G4's record`.
