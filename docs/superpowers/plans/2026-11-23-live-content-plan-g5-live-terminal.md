# Plan G5: The Live Terminal Add-on Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With the add-on present and a deliberate yes for that talk, a slide's `<Terminal>` runs a real program and is typed into while presenting; without either, the slide shows the terminal's recording or still exactly as today, and nothing ever starts on its own.

**Architecture:** The add-on is a small separate program, `stagepack-terminal` (a new crate `crates/terminal-helper`), that runs one command in a pseudo-terminal of its own and passes bytes to and from the app over its standard input and output in short binary frames; the app never links a pseudo-terminal library. In the app, `live_terminal` finds the helper, starts it only after a yes, feeds the program's output into the same bounded `slides_terminal::Screen` the replay uses, and draws it the way the replay draws its pictures, so the canvas, the show and every export keep working unchanged. The yes is asked by the app, kept per talk for the session, and remembered only for a folder and the exact command when the owner says *Always for this folder*.

**Tech Stack:** Rust 1.98.0; `portable-pty =0.9.0` in the helper only (pseudo-terminals on Linux, ConPTY on Windows); the app's existing `slides-terminal`, engine and gpui-kit 0.9.0; serde_json for the permissions file.

**Spec:** `docs/superpowers/specs/2026-11-18-live-content-design.md` (Goal G5; "G5. The live terminal add-on"; the anti-goals *A talk reaching your computer* and *Needing anything else*; the risk *The live terminal is the first thing in the app that runs a talk's command. Its yes must be impossible to skip*). `docs/checks/plan-g3.md` and `docs/superpowers/plans/2026-11-19-live-content-plan-g3-decisions.md` (the replay this builds on); `docs/checks/plan-g4.md` (the AI tool, which gains the new settings).

## Global Constraints

- Work on the branch `plan-g5`, cut from `plan-g`.
- No GitHub Actions. Don't push.
- Run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- The engine, exporters, format crate, `slides-terminal` and the helper don't depend on GPUI. Only the helper depends on `portable-pty`.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **Nothing runs without a yes.** No command starts when a talk opens, when a slide is shown in the editor, in a thumbnail, an export, the AI tool, or a test of anything but the live terminal itself. A command starts only when (a) the helper is found, (b) the talk has a yes for that exact command in this session, or the folder is remembered for it, and (c) the presenter reaches its slide in the show or presses *Start* on it in the editor. A talk can't give itself a yes: the yes is kept by the app in its own data folder, never in a talk.
- **The yes shows the command in full,** exactly as it will run (its words, one per line if long) and the folder it runs in, in plain words, with *Run*, *Always for this folder* (folder talks only) and *Don't run*. *Don't run* is the default and Escape. A yes for a packed talk is never remembered. A changed command (any byte) asks again.
- **What runs is what the tag says, and only that.** `run` is split into words (spaces separate; double quotes group; `\"` and `\\` inside quotes; nothing else is special) and the first word is the program, found on the PATH by the helper. No shell is added: `run="bash"` runs bash, `run="ls *"` passes `*` as is. The program runs in the talk's folder for a folder talk, and in a new empty temporary folder, removed afterwards, for a packed or untitled talk. Its environment is the app's own, with `TERM=xterm-256color` and `COLUMNS`/`LINES` set.
- **Limits:** `run` is at most 4,096 bytes and 64 words; the screen is the tag's `cols` by `rows` (default 100 by 30, at most 500 by 200, as recordings); at most 4 live terminals run per window at once; output beyond 4 MB a second is dropped, not queued, and the screen says so once; a recorded session keeps at most the recording limits of G3 (32 MB), after which recording stops and says so.
- **The live screen answers only two questions**, as every terminal must for shells to work: the cursor position (`CSI 6 n` → `CSI row ; col R`) and the primary device attributes (`CSI c` → `CSI ? 1 ; 2 c`). Everything G3 ignores is still ignored: no title, no clipboard, no links, no colour changes, no other answers, no bell.
- **Ending:** leaving the slide in the show stops nothing (the presenter may come back), but ending the show, closing the talk or window, or pressing *Stop* ends what was started. Ending sends the helper an end frame; the helper ends the program's whole process group (Unix) or job (Windows), first politely, then after 2 seconds by force. If the helper hasn't said the program ended 3 seconds after that, the app kills the helper and says, in the status line, *The terminal's program didn't end when asked, so it was stopped.* No program outlives the app: the helper also ends the program when its standard input closes.
- **Keys:** a click on a running terminal in the show gives it the keys, shown by a 3 px accent frame; then every key, the arrows included, goes to the program, except Ctrl+Shift+Escape, which gives the keys back to the show. Escape alone goes to the program. In the editor, typing goes to the terminal only while it has the keys the same way. Pressing a key never starts a command.
- The keyboard rule from D3 holds everywhere else. Every interactive element has an id constant and `test_support()`, and every behaviour has a headless test.
- Tests never touch the owner's real data, cache or Documents folders, never run on a real display, never use the owner's own files, and run only commands they choose themselves (`sh -c '…'`, `cat`, `printf`, `sleep`, on Windows `cmd /c`), never a talk's command they didn't write.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: managing Docker or virtual machines (a tag that says `docker run …` runs that, if Docker is there); a terminal not on a slide; scrollback; copy and paste out of the live screen; changing the give-back key (it's a constant, noted for the owner); packaging the helper (stage E).

Rulings made in the plan:
- **The tag** (my call): `<Terminal run="bash" x y w h />`, with `cols` and `rows`; it may also name `src="assets/demo.cast"`, the recording shown when it doesn't run (without the add-on, before the yes, in exports, thumbnails and the presenter's view). With `run` and no `src`, its still is a plain panel naming the command. A `<Terminal>` with neither is a complaint.
- **The helper is found** at `STUDIO_TERMINAL_HELPER` if set, else beside the app's executable (`stagepack-terminal`, `.exe` on Windows). It's never fetched. "Connected" means found; packaging (stage E) decides whether it's installed.
- **The wire** (my call): frames of `[kind: u8][length: u32 LE][bytes]`, at most 1 MB each. App → helper: `Start` (JSON `{program, args, cwd, cols, rows, env}`), `Input` (bytes), `Resize` (`cols: u16, rows: u16` LE), `End`. Helper → app: `Started`, `Output` (bytes), `Exited` (JSON `{code: Option<i32>, signal: Option<String>}`), `Failed` (one plain sentence, e.g. *There's no program called "bsh" here.*). Anything malformed ends the session. Cost if wrong: the two are always built together, so the wire changes freely.
- **Starting in the show:** on reaching the terminal's slide (or its click, if it has one) after a yes. If the talk has no yes yet, the show asks then, over the slide; *Don't run* shows the recording or still for the rest of the session without asking again. Coming back to the slide shows the program as it is, still running; if it has ended, its last screen and *The program ended* in small text at its foot.
- **In the editor:** a selected `<Terminal run>` has *Start* (asks if need be) and, while running, *Stop* and *Keep as recording*. It never starts on its own in the editor.
- **Keep as recording** writes what the session showed as an asciinema v2 `.cast` (its output with times, input left out) into the talk's `assets` folder through the added-files path, under a name made from the command (`assets/bash.cast`, then `bash-2.cast`), and sets the tag's `src` to it with one byte-range edit, so Undo removes it. This is how a risky demo becomes a safe one.
- **The AI tool** describes `run`, `cols` and `rows`, and its guide says the tool never runs a command and that an AI should give a `src` recording alongside any `run`.

## Review Focus

- **Skipping the yes.** A talk that names the same command as one remembered for another folder; a folder renamed or moved after *Always*; a symlinked folder; the talk's text changed by a byte after *Always*; a packed copy of a remembered folder; two windows of the same talk; a yes given, the window closed and the talk opened again; the AI tool's commands on a talk with `run`; export, thumbnails, the presenter's view and the start screen's previews of that talk. Nothing runs without a yes for that folder and exact command, in this session or remembered. Owned by Task 4's `no_way_round_the_yes`.
- **A command that won't behave.** A program that isn't there; one that exits at once; one that prints 100 MB as fast as it can (`yes`); one that ignores the polite end (`trap '' TERM`); one that starts children that start children (`sh -c 'sleep 1000 & sleep 1000'`); one that prints half a UTF-8 character and stops; one that asks for the cursor position in a loop. The window stays responsive, memory stays bounded, every process is gone 5 s after ending, and the screen and status say what happened in plain words. Owned by Task 1's `every_program_ends_when_told` and Task 2's `a_flood_is_dropped_not_held`.
- **The helper itself misbehaving.** The helper missing, not executable, a different program at that path that prints garbage or nothing, crashing mid-session, a frame claiming 4 GB. The app says *The live terminal couldn't start.* or similar and shows the recording; it never hangs or allocates what a frame claims. Owned by Task 2's `a_broken_helper_is_survived`.
- **Keys going where they shouldn't.** Presenting with a terminal holding the keys: the arrows, Space, Page Down, Escape, F5 and Ctrl+Shift+Escape; clicking elsewhere on the slide; changing slide with the mouse; a terminal ending while it holds the keys; two terminals on one slide. Keys reach exactly one place, and the show is always reachable with Ctrl+Shift+Escape. Owned by Task 5's `the_keys_go_to_one_place`.
- **Ending everything.** End the show, close the window, close the app, open another talk in the window, delete the tag, Undo, change the `run` text while it runs, a crash of the app (the helper's stdin closes). No program is left running. Owned by Task 5's `every_way_of_leaving_ends_the_program` and Task 1's `a_closed_input_ends_the_program`.

---

### Task 1: The helper program

**Files:**
- Create: `crates/terminal-helper/{Cargo.toml,src/main.rs,src/wire.rs,src/run.rs}`; `crates/terminal-helper/src/lib.rs` exporting `wire` so the app can share it
- Modify: `Cargo.toml` (workspace member)
- Test: `crates/terminal-helper/tests/helper.rs` (runs the built `stagepack-terminal` with `env!("CARGO_BIN_EXE_stagepack-terminal")`)

**Interfaces:**
- Produces:
  - Crate `slides-terminal-helper`, binary `stagepack-terminal`, depending on `portable-pty =0.9.0`, `serde`, `serde_json`.
  - `pub mod wire` (no `portable-pty` in it, so the app can depend on the library with `default-features = false`; the pty code sits behind a default feature `run`): `pub enum ToHelper { Start(StartSpec), Input(Vec<u8>), Resize(u16, u16), End }`, `pub enum FromHelper { Started, Output(Vec<u8>), Exited(Exit), Failed(String) }`, `#[derive(Serialize, Deserialize)] pub struct StartSpec { pub program: String, pub args: Vec<String>, pub cwd: PathBuf, pub cols: u16, pub rows: u16, pub env: Vec<(String, String)> }`, `pub struct Exit { pub code: Option<i32>, pub signal: Option<String> }`, `pub const MOST_FRAME: u32 = 1 << 20`, `pub fn write_frame(out: &mut impl Write, kind: u8, bytes: &[u8]) -> io::Result<()>`, `pub fn read_frame(input: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>>` (refuses a length over `MOST_FRAME` before allocating), and `encode`/`decode` for each enum.
  - The binary: reads frames from stdin; on `Start` opens a pty of that size, spawns the program in `cwd` with `env`, answers `Started` or `Failed(sentence)`; pumps output as `Output` frames of at most 64 KB; `Input` writes to the pty; `Resize` resizes it; `End` or end of stdin ends the program (Unix: SIGHUP then SIGTERM to its process group, SIGKILL after 2 s; Windows: close the pseudo-console, then terminate the job after 2 s) and then sends `Exited` and exits 0. When the program exits on its own it sends `Exited` and exits 0.
- [ ] **Step 1: Write the failing tests** (Unix forms; on Windows the same with `cmd /c`, behind `cfg`):
  - `a_program_runs_and_its_output_comes_back`: `Start {program:"sh", args:["-c","printf hello; exit 3"]}` gives `Started`, output containing `hello`, then `Exited {code: Some(3)}`.
  - `input_reaches_the_program`: `cat` echoes `abc\r` sent as `Input`.
  - `resize_is_seen`: `sh -c 'stty size; read x; stty size'` before and after `Resize(120, 40)` prints `30 100` then `40 120`.
  - `a_missing_program_says_so`: program `no-such-program-xyz` gives `Failed` with a sentence naming it, and the helper exits 0.
  - `every_program_ends_when_told`: `sh -c 'trap "" TERM HUP; sleep 1000 & sleep 1000'`; after `End`, `Exited` arrives within 4 s and no process of that group remains (check `/proc` on Linux for the children's pids, which the test reads from the program's own output `echo $$ $!`).
  - `a_closed_input_ends_the_program`: closing the helper's stdin without `End` ends the program the same way.
  - `a_bad_frame_ends_the_session`: a frame of kind 99, and one claiming 4 GB, each end the helper with status 0 and the program ended, having allocated nothing near the claim.
- [ ] **Step 2: Run them to see them fail.** `scripts/cargo-safe.sh test -p slides-terminal-helper`
- [ ] **Step 3: Implement `wire`, then the binary.**
- [ ] **Step 4: Run the tests** — PASS. `scripts/cargo-safe.sh clippy -p slides-terminal-helper --all-targets` clean.
- [ ] **Step 5: Commit** `Terminal helper: run one program in a terminal of its own, spoken to in frames`.

### Task 2: A live session in the app

**Files:**
- Create: `crates/app/src/live_terminal/{mod.rs,helper.rs,session.rs}`
- Modify: `crates/terminal/src/screen.rs` (the two answers, behind an explicit live mode), `crates/app/Cargo.toml`, `crates/app/src/lib.rs`
- Test: `crates/app/tests/live_terminal.rs`, and `crates/terminal/tests/screen.rs` for the answers

**Interfaces:**
- Consumes: `slides_terminal_helper::wire` (Task 1), `slides_terminal::Screen`, the replay's picture drawing (`replay.rs`: factor out what draws a `Screen` to a picture into a function both use, don't copy it).
- Produces:
  - In `slides-terminal`: `pub fn Screen::live(width, height) -> Screen` and `pub fn Screen::take_answers(&mut self) -> Vec<u8>`: a live screen collects the replies to `CSI 6 n` and `CSI c` (`CSI 5 n` → `CSI 0 n` too); a screen made by `new` never does. The G3 test `nothing_in_a_recording_reaches_out` keeps passing unchanged.
  - `pub fn find_helper() -> Option<PathBuf>`: `STUDIO_TERMINAL_HELPER`, else beside `std::env::current_exe()`.
  - `pub fn split_command(run: &str) -> Result<Vec<String>, String>` per the Global Constraints' rule; errors are sentences (*The command to run is empty.*, *The command to run has a quote that isn't closed.*, *The command to run is too long.*).
  - `pub struct Session` with `pub fn start(helper: &Path, spec: StartSpec) -> Session` (spawns the helper with piped stdio and a reader thread; never blocks the caller), `pub fn send_keys(&self, bytes: &[u8])`, `pub fn resize(&self, cols: u16, rows: u16)`, `pub fn end(&self)`, `pub fn state(&self) -> SessionState` (`Starting`, `Running`, `Ended(Exit)`, `Failed(String)`), `pub fn latest_picture(&self) -> Option<Arc<RenderImage>>` drawn off the window's thread at most `MOST_PICTURES_A_SECOND`, `pub fn recorded(&self) -> Option<Vec<u8>>` (the `.cast` so far, or `None` past the 32 MB limit), and `Drop` ending it. The reader feeds output into a `Screen::live`, sends its answers back as `Input`, drops output over 4 MB a second (saying so once on the screen's status), and records events with their times.
  - Key bytes: `pub fn key_bytes(key: &gpui_kit::Keystroke) -> Option<Vec<u8>>` mapping printable text, Enter (`\r`), Backspace (`\x7f`), Tab, Escape, the arrows (`ESC [ A`…; `ESC O A` in application-cursor mode if the screen says so), Home/End/PageUp/PageDown/Delete, and Ctrl+letter (`0x01`–`0x1a`); it returns `None` for Ctrl+Shift+Escape.
- [ ] **Step 1: Write the failing tests.**
  - In `crates/terminal/tests/screen.rs`: `a_live_screen_answers_two_questions` (`CSI 6 n` after moving to row 3 col 5 gives `ESC [ 3 ; 5 R`; `CSI c` gives `ESC [ ? 1 ; 2 c`; OSC 52, OSC 0, `CSI > c`, DECRQSS give nothing) and `a_recording_screen_never_answers`.
  - In `crates/app/tests/live_terminal.rs` (a shared test helper `helper_program()` finds `stagepack-terminal` in the target folder beside the test binary and, if it isn't there, builds it once with `std::env!("CARGO")` `build -p slides-terminal-helper`; tests set `STUDIO_TERMINAL_HELPER` to it):
    - `a_session_shows_what_the_program_prints`: `sh -c 'printf "\033[31mred\033[0m"; sleep 5'` — the session's screen has `red` in red within 3 s.
    - `keys_reach_the_program`: `cat`; `send_keys(b"hi\r")`; the screen shows `hi` twice.
    - `split_command_reads_words_and_quotes`: `a "b c" d\"e` → `["a","b c","d\"e"]`… exact table of 8 cases including empty, unclosed quote, 65 words, 4,097 bytes.
    - `a_flood_is_dropped_not_held`: `sh -c 'yes | head -c 200000000'`; the app process's resident memory grows under 100 MB and the session reaches `Ended` within 30 s.
    - `a_broken_helper_is_survived`: `STUDIO_TERMINAL_HELPER` pointing at `/bin/true`, at a script printing random bytes, and at a missing path: the session reaches `Failed` with a sentence within 3 s, no thread is left (count as `replay::threads_running` does).
    - `ending_a_session_ends_its_program`: `drop(session)` of a `sleep 1000` leaves no such process within 5 s.
    - `a_session_is_recorded`: `printf one; sleep 0.2; printf two` gives a `.cast` that `slides_terminal::read` reads with the two outputs in order.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests**, then `-p slides-terminal` and `--test ui_terminal` — PASS.
- [ ] **Step 5: Commit** `App: a live terminal session through the helper, drawn as a replay is drawn`.

### Task 3: The tag, its still and its settings

**Files:**
- Modify: `crates/engine/src/{slide.rs,deck.rs,display.rs,terminal_screen.rs}` (the `run`, `cols`, `rows` attributes and the still), `crates/app/src/inspector/labels.rs` (labels and meanings), `crates/app/src/export.rs` (the notice), `docs/guide/writing-talks-for-ai.md`
- Test: `crates/engine/tests/terminal.rs`, `crates/app/tests/ai_tool.rs`, `crates/app/tests/ui_terminal.rs`

**Interfaces:**
- Produces: `TerminalOn` gains `pub run: Option<String>`, `pub cols: u16`, `pub rows: u16` (equality includes them); with `run` and no `src`, the layer is a plain panel of the box with the words *Live terminal* and the command (cut to one line with `…`), in the theme's mono font; with `src`, the recording's last screen as now. `Deck::terminal_problem` says: neither `src` nor `run` — *This terminal has no recording and no command to run.*; a `run` that `split_command` refuses — that sentence (move `split_command` into `slides-terminal` so the engine can call it, and have Task 2's re-export it); `cols`/`rows` out of range — *A terminal can be 1 to 500 columns and 1 to 200 rows.* The export notice counts live terminals drawn as their recording or panel: *2 live terminals are shown as their recording or a still.*
- [ ] **Step 1: Write the failing tests.**
  - `a_live_terminal_without_a_recording_shows_its_command` (engine): the layer's items include the text `Live terminal` and `bash`; no file is read (the deck's `TalkFiles` is a spy).
  - `a_live_terminal_with_a_recording_shows_the_recording` (engine): same still as `<Terminal src>` alone.
  - `a_bad_live_terminal_says_why` (engine): each of the three sentences.
  - `describe_names_run_cols_rows` (ai_tool): with meanings; `run`'s meaning says the AI tool never runs it and that the owner is asked first.
  - `nothing_runs_from_the_ai_tool` (ai_tool): a talk with `<Terminal run="sh -c 'touch ran'">`: `check`, `outline`, `describe` and `picture` all succeed and no `ran` file appears; the guide's new example (with `src` and `run`) checks clean.
  - `exports_show_a_live_terminal_as_its_still` (ui_terminal or export tests): PDF and PowerPoint succeed, the notice is said, nothing runs.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `-p slides-engine --test terminal`, `-p stagepack-studio --test ai_tool --test ui_terminal` — PASS.
- [ ] **Step 5: Commit** `Engine: a terminal that runs a command, shown as its recording or a still until it does`.

### Task 4: The yes

**Files:**
- Create: `crates/app/src/live_terminal/permission.rs`, `crates/app/src/live_terminal/ask.rs` (the dialog view)
- Test: `crates/app/tests/live_terminal_yes.rs`

**Interfaces:**
- Produces:
  - `pub struct RunPermissions` kept in `terminal-permissions.json` in the app's data folder (as `BuildPermissions`): `load(file)`, `default_file()`, `always(folder: &Path, command: &[String]) -> bool` (by the folder's canonical path and the exact words), `remember(folder, command) -> io::Result<()>`.
  - `pub struct SessionYes` held by the window's talk (not by the talk's files): `pub fn allowed(&self, command: &[String]) -> bool`, `pub fn allow(&mut self, command)`, `pub fn refuse(&mut self, command)`, `pub fn refused(&self, command) -> bool`. A new talk opened in the window gets a new one; nothing carries over between windows.
  - `pub fn may_run(talk: &TalkPlace, command: &[String], session: &SessionYes, remembered: &RunPermissions) -> Decision` with `Decision { Run, Ask, Refused }` — `Run` only from the session's yes or (folder talk only) the remembered permission; the one function every start goes through.
  - The dialog: ids `RUN_ASK_ID`, `RUN_YES_ID`, `RUN_ALWAYS_ID` (absent for packed and untitled talks), `RUN_NO_ID`; it shows *This talk wants to run a program on your computer.*, the program and its arguments each on its own line in the mono font, *in* and the folder (or *in a new empty folder* for a pack), and *It can do anything you can do. Run it only if you trust this talk.*; *Don't run* is focused and Escape chooses it.
- [ ] **Step 1: Write the failing tests.**
  - `the_yes_is_asked_once_per_session`: `may_run` is `Ask`, then `Run` after `allow`, `Refused` after `refuse`; a different command is `Ask` again; a new `SessionYes` is `Ask`.
  - `always_is_for_one_folder_and_one_command`: remembered for folder A and `["bash"]`: A with `["bash"]` is `Run`; A with `["bash","-l"]`, B with `["bash"]`, a symlink to A (canonical, so `Run` — the same folder) and a packed copy of A are `Ask`; A after being renamed is `Ask`.
  - `a_talk_cant_give_itself_a_yes`: a `terminal-permissions.json` written inside the talk folder changes nothing; only the scratch data folder's file counts.
  - `the_dialog_shows_the_whole_command` (headless GPUI): every word and the folder appear; *Always for this folder* is absent for a pack; Escape and *Don't run* refuse; Enter with nothing else focused refuses.
  - `no_way_round_the_yes`: with the helper set to a test helper that records every `Start` it gets into a scratch file, open a folder talk with `<Terminal run="sh -c 'touch ran'">` in a headless window, show its slide in the editor, make its thumbnail, export PDF and PowerPoint, open the presenter's view, open a second window of it: the record file stays empty. (Starting from the show is Task 5's.)
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests** — PASS.
- [ ] **Step 5: Commit** `App: the yes a talk needs before it runs a program, never kept in the talk`.

### Task 5: Running in the show and the editor

**Files:**
- Modify: `crates/app/src/{present_view.rs,slide_canvas.rs}`, `crates/app/src/slide_editor/{mod.rs,render.rs}`, `crates/app/src/workspace_view.rs` (ending on close), `crates/app/src/live_terminal/mod.rs`
- Test: `crates/app/tests/ui_live_terminal.rs`

**Interfaces:**
- Consumes: `Session` (Task 2), `may_run` and the dialog (Task 4), `TerminalOn.run` (Task 3).
- Produces:
  - `pub struct LiveTerminals` owned by the window: at most 4 sessions, keyed by the tag's source range and command; `start(...)` goes through `may_run` and the dialog; `end_all(&mut self) -> Vec<String>` (the sentences for any that wouldn't end), called on ending the show, closing the talk or the window, and opening another talk in it.
  - Canvas: a running session's picture is laid over its layer as a replay's is; while it has the keys, the 3 px accent frame.
  - Show: starts on reaching the slide or its click; *Don't run* keeps the still; a click on it gives it the keys; key routing per the Global Constraints; Ctrl+Shift+Escape gives them back; the presenter's view shows the still.
  - Editor: `TERMINAL_START_ID`, `TERMINAL_END_ID` (*Stop*) and `TERMINAL_KEEP_ID` (*Keep as recording*, Task 6 fills it) on a selected `<Terminal run>`; ending on deselecting the slide is not done — it runs until *Stop*, closing, or the tag's removal or its `run` changing.
  - Status line: *The terminal's program didn't end when asked, so it was stopped.*; *The live terminal couldn't start: <sentence>*; *The live terminal add-on isn't installed, so its recording is shown.* (said once per talk when a `run` slide is shown and the helper isn't found).
- [ ] **Step 1: Write the failing tests** (headless GPUI, the real helper through `STUDIO_TERMINAL_HELPER`, `sh -c` commands):
  - `the_show_asks_then_runs`: reaching the slide shows the dialog; *Run* starts it and its output appears in the layer's picture; leaving and coming back shows it still running; ending the show ends it.
  - `dont_run_keeps_the_still_for_the_session`: *Don't run*, leave, come back: no dialog, the still, nothing started.
  - `the_keys_go_to_one_place`: click gives keys (frame drawn); Right Arrow and Space reach `cat` (seen on its screen) and the slide doesn't change; Ctrl+Shift+Escape gives them back and Right Arrow then moves the show; with two terminals, a click on the second moves the keys to it.
  - `every_way_of_leaving_ends_the_program`: for each of end show, close window, open another talk in the window, delete the tag then Undo, change `run` while running, *Stop*: the `sleep 1000` process is gone within 5 s and no session remains.
  - `without_the_add_on_the_recording_shows`: `STUDIO_TERMINAL_HELPER` pointing at a missing path and no helper beside the test binary: the slide shows its recording, the status says so once, no dialog.
  - `at_most_four_run`: five `run` terminals on one slide, all allowed: four run, the fifth shows its still and the status says why.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests**, then `--test ui_terminal --test ui_present` (or whichever cover the show's keys) — PASS.
- [ ] **Step 5: Commit** `App: a live terminal runs in the show and the editor, typed into, and ended with the talk`.

### Task 6: Keep as recording, and the record

**Files:**
- Modify: `crates/app/src/live_terminal/session.rs`, `crates/app/src/slide_editor/*` (the *Keep as recording* action), `crates/app/src/added_files.rs` if needed, `docs/guide/writing-talks-for-ai.md`
- Create: `docs/checks/plan-g5.md`
- Test: `crates/app/tests/ui_live_terminal.rs`

**Interfaces:**
- Produces: *Keep as recording* takes `Session::recorded()`, adds it as `assets/<stem>.cast` (stem from the program's file name, made unique), and sets `src` on the tag by one byte-range edit; Undo removes both the `src` and (as for any added file) the file from what's saved. Past the 32 MB limit the button says *This session is too long to keep.* and is disabled.
- [ ] **Step 1: Write the failing tests.**
  - `a_session_kept_becomes_the_recording`: run `printf hello`, *Keep as recording*: the tag has `src="assets/printf.cast"`, the still now shows `hello` without running anything, Save writes the file, Undo takes the `src` away.
  - `a_kept_recording_plays_like_any_other`: the kept file plays in the editor with *Play* (G3's) and reads with `slides_terminal::read`.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the whole suite**: `scripts/cargo-safe.sh test --workspace --no-fail-fast` — record the totals.
- [ ] **Step 5: Write `docs/checks/plan-g5.md`**: what was built; how to try it (build the helper, set `STUDIO_TERMINAL_HELPER` or put it beside the app, a scratch talk with `<Terminal run="bash" src=…>`); what ran; the hand checks for the owner: a real shell in a show on a scratch display, typing, Ctrl+Shift+Escape, a full-screen program (`htop` or `vim`), *Keep as recording*, `docker run --rm -it alpine sh` if Docker is there, and all of it on Windows with PowerShell and ConPTY from a fresh clone. Leave `## The final review` with *To be written by the controller.*
- [ ] **Step 6: Commit** `Docs: keep a live session as a recording, and plan G5's record`.
