# Plan H1: Clickable slides and buttons — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the show, a click on an interactive thing goes to that thing rather than advancing the slide. This plan adds:
- a built-in `<Button>` that types a command into a live terminal, sends an action to another interactive thing, or goes to a click step;
- a talk's own components that react to the pointer and keep state, through component interface v2.

**Architecture:**
- **One hit-test list per slide.** Each slide gets one list of interactive things, `Interactive` (built from the deck's layers). The audience window and the presenter's current slide both route a press through it before the show's own click.
- **Shared state.** Each interactive thing's state lives in `Presentation`, so both windows show the same state.
- **Buttons** are a native built-in tag with a new `ButtonOn` on `LayerSpec`.
- **Interface v2.** A component's state and events are new fields in the existing JSON. A component built against v1 runs as before.

**Tech Stack:** Rust 1.98, GPUI through gpui-kit 0.9.0, wasmtime =50.0.1 (the components sandbox), and the existing engine, toolkit and app crates.

**Spec:** `docs/superpowers/specs/2026-11-25-live-demos-design.md`, project 1 ("Clickable slides and buttons"), together with the "Constraints" and "The yes" for typed commands on the host.

## Global Constraints

- A click, drag or wheel turn on an interactive thing goes to it, and the show does not advance. Everywhere else, a click advances as today. The keyboard and a clicker behave exactly as today: Space, Enter and the arrows still move the show while a button or component is under the pointer.
- **What counts as interactive:**
  - a `<Button>`;
  - a running live terminal, whose routing already exists;
  - a model in the show, which already takes drags;
  - a talk component whose schema says `interactive: true`.
  - Maps are added in plan H3.
- **The presenter window's current slide** routes presses the same way as the audience window, and both show one state. The next-slide preview stays a still picture and takes no clicks.
- **Hover outline.** On hover in the show, an interactive thing gets a faint outline: 1 px, the theme's accent colour at 40 % opacity. It is never drawn into the slide's bitmap, so it is never exported.
- **The pointer.** It is shown on a slide that has an interactive thing. On any other slide in the show, it hides after 2 s without movement.
- **`<Button>` attributes:**
  - `label` (needed);
  - `look`, with the choices `primary` (the default), `secondary` and `quiet`;
  - one action, given by exactly one of:
    - `run` with `in`;
    - `do` with `on`, and an optional `value`;
    - `step`;
  - `id`, so other things can name it;
  - the usual placement attributes.
- **Button refusals.** A button with no action, or with more than one, draws as a button and shows the notice *This button has no action.* or *This button has more than one action: run, do or step.* respectively. Pressing it does nothing.
- **A button is pressed with a left click only.** It looks pressed (darker by 15 %) while the pointer is down on it, and acts on release over it.
- **`run`/`in`:**
  - The button types the `run` text followed by `\r` into the live terminal on the same slide whose `id` is `in`, starting that terminal first if it isn't running.
  - **The terminal's own command** goes through the existing G5 gate.
  - **The typed text** has a yes of its own, on the host:
    - It is asked on the first press, and the whole text is shown.
    - The text is matched exactly.
    - *Always for this folder* is offered for folder talks only.
    - It is asked again if the text changes.
    - A yes for the terminal's shell never covers typed text.
  - **The texts typed text may not contain:** control characters other than tab, more than 4,096 bytes, or the hidden or odd characters `unrunnable` already refuses. Such a text is refused with a notice and never typed.
- **`do`/`on`/`value`:**
  - `do` sends a named action, with `value` (or an empty string), to the interactive component with that `id` on the same slide.
  - An `on` that names nothing, or names something that isn't interactive, gives the notice *This button's `on` names nothing it can act on.*
- **`step`:** a whole number N from 0 to the slide's step count. It goes straight to click step N of the current slide. Anything out of that range gives the notice *This button's step is past the slide's last click.*
- **Every `id` a button names is checked** (`in`, `on`, and a `step` range) as a notice on the slide. The same notices appear in the AI tool's `check`.
- **Interface v2:**
  - `ABI_VERSION` becomes 2. A module whose `abi_version()` returns 1 is still loaded and rendered exactly as now. A module saying 3 or more is still refused as `NewerVersion`.
  - `RenderInput` gains `state: Option<serde_json::Value>` and `events: Vec<Event>`, both `#[serde(default)]`.
  - `RenderOutput` gains `state: Option<serde_json::Value>`, `#[serde(default)]`.
  - `ComponentSchema` gains `interactive: bool`, `#[serde(default)]`.
  - `Element` gains the variant `Hit { name: String, child: Box<Element> }`.
- **v2 state:**
  - It is at most 64 KiB once serialized. An answer with more is a render error: *This component's state is over 64 KiB.*
  - It lives while its slide is shown, and leaving the slide clears it.
  - It is never written to the talk.
- **v2 events** are:
  - `Event::Pointer { kind: PointerKind, x: f32, y: f32, hit: Option<String> }`, with `PointerKind { Down, Move, Up, Wheel { dy: f32 } }`, in the component's own units with its box's top-left at 0,0;
  - `Event::Action { name: String, value: String }`.
- **Event delivery:**
  - Events are coalesced to at most 60 renders a second per component, keeping every Down, Up and Action, and only the last Move.
  - At most 64 events go to one render. A Move is dropped before anything else.
  - The existing limits apply to each render: 250 ms, 64 MiB of memory, 4 MiB of output.
- **The editor.** A selected button, or a selected interactive component, gets *Try* (and Space while selected), as a video gets *Play*. Interaction happens only while trying, and leaving the selection stops it. A button's `run` and `step` do nothing in the editor, and the button says so in its form: *Runs in the show.* A `do` works in the editor while its target is being tried.
- **Rules:**
  - No real decks, clients or organisations in commits or docs.
  - Stage by name, never `git add -A`, and never stage `docs/superpowers/plans/2026-11-09-roadmap.md`, `.superpowers/`, `.claude/` or `target/`.
  - Use `scripts/cargo-safe.sh` for cargo, never `cargo fmt`, never `pkill`, and never run the GUI on the real display.
  - Never touch `~/.local/share/stagepack-studio`, `~/.cache/stagepack-studio`, `~/Documents`, `~/.local/lib` or `~/.local/bin`.
  - No GitHub Actions.
  - The commit trailer is `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A press that starts on an interactive thing and is released off it, or the other way round.** Neither acts and neither advances the show. Task 1 pins this with `a_press_that_leaves_the_button_does_nothing`.
2. **A button on a slide shown in both windows.** One press acts once, not once per window, and the state is the same in both windows. Task 4 pins this with `a_press_in_the_presenter_window_acts_once_and_both_windows_agree`.
3. **A button whose `run` names a terminal that is ended, refused, or at the limit of 4 live terminals.** The press says why in one notice line on the status bar, and nothing is typed anywhere else. Task 4 pins this with `a_run_into_a_terminal_that_cant_start_says_why`.
4. **A v2 component that traps, runs out its time or returns bad state while events are flooding in.** The component shows its problem, the show keeps moving, and later events are dropped rather than queued without limit. Task 5 pins this with `a_component_that_fails_under_events_shows_its_problem_and_the_show_moves_on`.
5. **Editing the talk while a slide with state is shown** (the file is saved in another editor). State is kept for a component whose tag range and props are unchanged, and cleared otherwise. A button whose target vanished gets its notice. Task 5 pins this with `an_edit_keeps_state_only_for_an_unchanged_component`.

---

### Task 1: Routing clicks to interactive things in the show

**Files:**
- Create: `crates/app/src/interactive.rs`. It holds the slide's list of interactive things and the hit test shared by both show windows.
- Modify:
  - `crates/app/src/present_view.rs`: `AudienceView::press`, `drag`, `let_go`, the `on_click` guard, and `PresenterView::press`. Add the hover outline and the pointer hiding.
  - `crates/app/src/present.rs`: add `Session::go_to_step`.
  - `crates/app/src/lib.rs`: add `mod interactive;`.
- Test: `crates/app/tests/ui_interactive.rs`.

**Interfaces:**
- **Produces:**
  - `pub(crate) enum InteractiveKind { Button, Terminal(LiveId), Model(usize), Component }`.
  - `pub(crate) struct InteractiveThing { source: Range<usize>, rect: Rect, rotate: f32, kind: InteractiveKind, id: Option<String> }`. It is in slide units.
  - `pub(crate) fn interactive_at(things: &[InteractiveThing], at: Point<f32>) -> Option<&InteractiveThing>`. The topmost thing wins, and it uses `inside_turned`.
  - `pub(crate) fn interactive_on(document: &Document, slide: usize, moment: Moment) -> Vec<InteractiveThing>`. Only layers shown at that moment count.
  - `impl Session { pub fn go_to_step(&mut self, shape: &mut impl TalkShape, step: u32) -> Moved }`. It clamps to the slide's steps, and moving back counts as `BackedUp`.
  - `impl Presentation { pub fn go_to_step(&mut self, step: u32, cx) }`. It sets `reveal` and `slide_began` as `next` does.
  - `Presentation` also gains `pressed: Option<(Range<usize>, PressedIn)>`, with `enum PressedIn { Audience, Presenter }`, so a press is owned by one window.
- **Consumes:** `press_for_keys`, `model_at`, `inside_turned` and `LiveTerminals` (all existing).

- [ ] **Step 1: Write the failing tests** in `ui_interactive.rs`. Use `rehearse`, `start_show` and the raw mouse events from `support/typing.rs`. Until Task 2 exists, a talk component built in the test with `interactive: true` (hand-written WAT, as `ui_present.rs` builds its `Clock`) stands in as the interactive thing.
  - `a_click_on_an_interactive_thing_does_not_advance`: the counter still says `1 / 2`.
  - `a_click_beside_it_still_advances`: `2 / 2`.
  - `a_press_that_leaves_the_button_does_nothing`: press on it, move 300 px off it, release. The counter stays `1 / 2` and the thing got no Up event inside its box.
  - `keys_still_move_the_show_with_the_pointer_over_it`: `right` moves to `2 / 2`.
  - `the_presenter_current_slide_routes_presses_too`: a press on the thing in the presenter window reaches it, and the next-slide preview takes none.
  - `the_hover_outline_is_drawn_only_in_the_window_and_never_in_the_slide_bitmap`: the outline element is found by its id `INTERACTIVE_HOVER_ID`, and the slide's layer pixels are unchanged.
  - `the_pointer_hides_after_two_seconds_on_a_plain_slide_and_stays_on_an_interactive_one`: use the test clock (`cx.executor().advance_clock`).
- [ ] **Step 2:** Run `scripts/cargo-safe.sh test -p stagepack-studio --features test-hooks --test ui_interactive`. Expected: it fails to compile, because `interactive` is missing.
- [ ] **Step 3: Implement it.**
  - `press_for_keys` keeps giving keys to a terminal, but is now called through `interactive_at`.
  - The model drag path is unchanged.
  - The click guard is `swallow_click || pressed.is_some()`.
- [ ] **Step 4:** Run the test file, then `--test ui_present --test ui_live_terminal --test ui_model`, which must still pass unchanged. Then run `clippy -p stagepack-studio --all-targets -- -D warnings`. Expected: all pass and clippy is clean.
- [ ] **Step 5: Commit**, staging the files named above.

### Task 2: The `<Button>` tag

**Files:**
- Modify:
  - `crates/engine/src/builtins.rs`: add `button_component()`, a hand-written `NativeComponent`, because it takes `id`, and `BUTTON_TAG`.
  - `crates/engine/src/display.rs`: add `ButtonOn` to `LayerSpec`.
  - `crates/engine/src/slide.rs`: in `positioned_layer`, fill `button`.
  - `crates/engine/src/deck.rs`: add `buttons_of` and `Deck::buttons_at`.
  - `crates/app/src/document/large.rs`: add `Document::buttons_at`.
  - `crates/app/src/inspector/labels.rs`: add `BUILT_INS`, `LEFT_OUT`, `NEEDED`, `CHOICES` and a meaning for `Button`.
  - `crates/app/src/ai_tool/describe.rs`: add `BUILT_IN_ORDER`.
  - `crates/app/src/ai_tool/check.rs`: add `tags_on`, and a `button_problem` arm. The default arm must not catch it.
  - `crates/app/src/selection.rs`: in `name_of`, Button displays as *Button*.
  - `crates/app/src/palette.rs`: add `LISTED_BUILT_INS` and `default_attrs`.
  - `crates/app/src/insert_tools.rs`: add an Insert tab tool, *Button*.
  - `crates/app/src/interactive.rs`: buttons join `interactive_on`.
  - `crates/engine/src/slide.rs`: `id` joins `PLACEMENT_ATTRS`, so it never reaches a component as a prop. It is already a reserved name for `#[component]`, so no component can rely on getting it. `TagPlace` gains `id: Option<String>`, which `LayerSpec` carries as `id`. Two tags with the same `id` on a slide give the notice *Two things on this slide have the id "<id>".*, and buttons naming it then act on neither.
- Test: `crates/engine/tests/builtins.rs`, `crates/app/tests/ai_tool.rs`, and `crates/app/tests/ui_interactive.rs`.

**Interfaces:**
- **Produces:**
  - `pub struct ButtonOn { pub label: String, pub look: ButtonLook, pub action: Result<ButtonAction, ButtonFault>, pub id: Option<String> }`.
  - `pub enum ButtonAction { Run { text: String, terminal: String }, Do { action: String, target: String, value: String }, Step(u32) }`.
  - `pub enum ButtonFault { NoAction, ManyActions, BadStep }`.
  - `pub enum ButtonLook { Primary, Secondary, Quiet }`.
  - `Deck::buttons_at(index, Moment) -> Vec<ButtonLayer>`, with `ButtonLayer { layer: usize, source: Range<usize>, on: ButtonOn }`.
  - The engine draws the button: a `Rounded` shape with radius 8, the label centred in the theme's body font, fill and text colours from the theme for each look, and padding 16 by 10.
- **Consumes:** `InteractiveThing` (Task 1).

- [ ] **Step 1: Write the failing tests.**
  - `a_button_draws_its_label_in_each_look` (engine): there are three looks, each layer holds a Fill and a Text whose text is the label, and the three fills differ.
  - `a_button_reads_one_action` (engine): `run="ls" in="t"` gives `Run`, `do="next" on="m"` gives `Do` with an empty `value`, and `step="2"` gives `Step(2)`.
  - `a_button_without_or_with_two_actions_has_its_notice` (engine): the notice texts are exactly those in Global Constraints.
  - `an_id_is_read_as_placement_and_a_duplicate_has_its_notice` (engine): a talk component on a slide with `id="c"` doesn't receive `id` among its props.
  - `check_reports_what_a_button_names_that_isnt_there` (ai_tool): an `in` with no terminal, an `on` with no interactive component, and a `step="9"` on a slide with 2 clicks give three problems, with the exact sentences.
  - `describe_lists_the_button` (ai_tool): `describe --json` contains `Button` with `label` needed and `look` choices `primary`, `secondary` and `quiet`.
  - `insert_button_adds_a_button_tag_and_selects_it` (ui_interactive).
- [ ] **Step 2:** Run `scripts/cargo-safe.sh test -p slides-engine --test builtins` and `-p stagepack-studio --features test-hooks --test ai_tool --test ui_interactive`. Expected: they fail.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4:** Run the same tests, then the whole engine and app suites. Exports must be unchanged except that a button draws as its still. The slidev add-on's export of an unknown tag already turns it into a picture, so check that with its existing tests. Run `clippy --workspace --all-targets -- -D warnings`. Expected: all pass and clippy is clean.
- [ ] **Step 5: Commit.**

### Task 3: The yes for typed text on the host

**Files:**
- Modify:
  - `crates/app/src/live_terminal/permission.rs`: add `TypeKey`, `may_type`, and remembering typed text.
  - `crates/app/src/live_terminal/ask.rs`: add the question for typed text.
- Test: `crates/app/tests/live_terminal_yes.rs` (where `may_run` is tested) and, for the dialog, `crates/app/tests/ui_live_terminal.rs`.

**Interfaces:**
- **Produces:**
  - `pub(crate) struct TypeKey { text: String, terminal_command: Vec<String>, folder: Option<PathBuf> }`.
  - `pub(crate) fn may_type(talk: &TalkPlace, text: &str, terminal_command: &[String], session: &SessionYes, remembered: &RunPermissions) -> Decision`.
  - `RunPermissions::remember_typed(&TalkFolder, text, terminal_command)`.
  - `RunQuestion` gains `typed: Option<String>`. When `typed` is set, the dialog's title is *Type this into the terminal?*, and its body shows the text in the monospace font, then the line *into* followed by the terminal's command.
- **Consumes:** `unrunnable` (existing). Nothing presses a button yet, so this task's tests call `may_type` and the dialog directly.

- [ ] **Step 1: Write the failing tests.**
  - `typed_text_is_asked_once_per_text`: the first press asks, *Run*, and a second press doesn't ask. A button whose text differs by one character asks.
  - `a_yes_for_the_shell_does_not_cover_typed_text`.
  - `always_remembers_typed_text_for_a_folder_talk_only`: a packed talk offers no *Always*.
  - `typed_text_with_control_characters_or_over_4096_bytes_is_refused`: the exact notices are *This button's text has a control character.* and *This button's text is over 4,096 bytes.*
  - `a_changed_terminal_command_asks_again_for_the_same_text`.
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3: Implement it.** Store typed texts in the same `terminal-permissions.json` under a new key, `typed`. A file without that key reads as empty, and old app versions ignore it, since they read only their known keys. Check this against the file's current reader before relying on it.
- [ ] **Step 4:** Run the tests, `live_terminal_yes` and `ui_live_terminal` whole, and clippy. Expected: all pass and clippy is clean.
- [ ] **Step 5: Commit.**

### Task 4: Buttons act in the show

**Files:**
- Modify:
  - `crates/app/src/interactive.rs`: add `fn press_button(...)`.
  - `crates/app/src/present_view.rs`: on release over the same button it was pressed on, act.
  - `crates/app/src/workspace_view/running.rs`: start a terminal on a button's behalf through the existing `try_start_live`.
- Test: `crates/app/tests/ui_interactive.rs` and `crates/app/tests/ui_live_terminal.rs`.

**Interfaces:**
- **Consumes:**
  - `ButtonAction` (Task 2);
  - `Presentation::go_to_step` (Task 1);
  - `LiveTerminals::get(&LiveId)` and `Session::send_keys(&[u8])`;
  - `try_start_live`;
  - `may_type` and the typed-text question (Task 3).
- **Produces:** `pub(crate) fn act(show: &mut Presentation, button: &ButtonLayer, cx) -> Acted`, with `enum Acted { Done, Asked, Refused(String) }`. `Refused` carries the one-line sentence the status line shows.

- [ ] **Step 1: Write the failing tests.**
  - `a_step_button_goes_to_its_step`: from `1 / 3`, a button with `step="2"` gives step 2 of slide 1. The counter says the same slide, and the layers with `click <= 2` are shown.
  - `a_run_button_types_its_text_into_the_terminal_after_the_yes`:
    - The talk is `<Terminal id="t" run="cat" …/>` plus `<Button run="hello" in="t" …/>`.
    - The button is pressed; the terminal's yes is answered *Run*, then the button's yes is answered *Run*.
    - Within 2 s, the screen holds `hello`.
    - Use the real helper with `support/helper.rs`, as the G5 tests do. In the test, answer the yes with `SessionYes::allow_for_tests`.
  - `a_second_press_types_again_without_asking`.
  - `a_run_into_a_terminal_that_cant_start_says_why`: with 4 terminals already live, the status line says *This button's terminal can't start: 4 are already running.*, and nothing is typed.
  - `a_press_in_the_presenter_window_acts_once_and_both_windows_agree`: a `step` button pressed in the presenter window. The place moves once, and both windows show it.
  - `a_do_button_sends_its_action` lands with Task 5's test component. Put a `#[ignore = "Task 5"]` test name here and remove the ignore in Task 5.
  - `a_run_button_on_a_packed_talk_asks_every_time_it_is_opened`: no *Always*, as Task 3 decided.
- [ ] **Step 2:** Run the test files. Expected: they fail.
- [ ] **Step 3: Implement it.** Typed text is `run` + `"\r"`, sent with `send_keys`. A terminal that is starting gets the text queued, and the existing 1 MiB queue cap applies.
- [ ] **Step 4:** Run the tests, `ui_live_terminal` whole, and clippy. Expected: all pass and clippy is clean.
- [ ] **Step 5: Commit.**

### Task 5: Component interface v2: events and state

**Files:**
- Modify:
  - `crates/toolkit/src/interface.rs`: add `ABI_VERSION = 2`, the new fields and `Event`.
  - `crates/toolkit/src/element.rs`: add `Element::Hit`.
  - `crates/toolkit/src/props.rs`: add `Ctx::events()`, `Ctx::state::<T: DeserializeOwned + Default>() -> T` and `Ctx::set_state<T: Serialize>(&T)`. `finish` puts the set state in `RenderOutput.state`.
  - `crates/toolkit-macros/src/lib.rs`: `#[component(interactive)]` sets `interactive: true` in the schema, and makes the emitted `abi_version()` return 2. A component without it keeps returning 1, so components already built are unchanged.
  - `crates/components/src/sandbox.rs`: check the 64 KiB state limit in and out. `Measure` and `keep_pictures_in_assets` gain `Hit`.
  - `crates/engine/src/elements.rs`: `element_node` gains `Hit`, and records hit rectangles.
  - `crates/engine/src/deck.rs` and `crates/engine/src/timeline.rs`: a layer of an interactive component is laid out from `(Moment, state hash)`. Add a `state: u64` to `Moment` and `LayerKey`, which is 0 for anything not interactive.
  - `crates/app/src/interactive.rs`: per-component state and event queues in `Presentation`, cleared on leaving the slide, with coalescing and the limits from Global Constraints.
- Test:
  - `crates/toolkit/tests/interface_v2.rs`;
  - `crates/toolkit/tests/interface_v1.rs`: `the_version_is_one` becomes `a_v1_module_still_runs`, and every v1 JSON round trip stays byte-exact;
  - `crates/components/tests/sandbox.rs`;
  - `crates/app/tests/ui_interactive.rs`.

**Interfaces:**
- **Produces:**
  - `Event`, `PointerKind`, `Element::Hit` and `ComponentSchema.interactive`, as in Global Constraints.
  - `Ctx::events() -> &[Event]`.
  - `Ctx::state<T>() -> T`, which returns the default when the state is absent or doesn't parse.
  - `Ctx::set_state<T>(&T)`.
  - `#[component(interactive)]`.
  - In the app: `Presentation::send_to(source: &Range<usize>, Event)`.
- **Consumes:** `InteractiveThing` (Task 1) and `ButtonAction::Do` (Task 2).

- [ ] **Step 1: Write the failing tests.**
  - `v2_round_trips` (interface_v2): an input with `state` and three events, and an output with `state`, round-trip byte-exact. An input without them parses with `state: None` and empty `events`.
  - `a_v1_module_still_runs` (sandbox): the existing `fixtures/v1/score` module renders the same output as before.
  - `a_module_saying_three_is_refused` (sandbox): `NewerVersion(3)`.
  - `state_over_64_kib_is_refused` (sandbox): the exact error sentence.
  - `a_counter_counts_clicks_on_its_plus` (ui_interactive):
    - Build a counter component whose element has `Hit { name: "plus" }`.
    - Two presses on the plus give a text of `2`, and a press beside the plus inside the component gives no change.
    - The show doesn't advance.
    - Build it with the bundle the same way `support/components.rs` does. If the bundle isn't there, the test skips with the same message the other bundle tests give.
  - `leaving_the_slide_clears_state`: next, then previous, gives `0`.
  - `a_do_button_sends_its_action`: remove Task 4's ignore. A `<Button do="add" on="c" value="5">` makes the counter show `5`.
  - `events_are_coalesced_to_sixty_renders_a_second`: 1,000 Moves in 1 s of test clock give at most 61 renders, and the last Move is delivered.
  - `a_component_that_fails_under_events_shows_its_problem_and_the_show_moves_on`: a WAT module that traps on its second render. The problem element shows, `right` still advances, and the queue holds at most 64 events.
  - `an_edit_keeps_state_only_for_an_unchanged_component`.
- [ ] **Step 2:** Run `scripts/cargo-safe.sh test -p toolkit -p toolkit-macros -p slides-components -p slides-engine`, and the app's `ui_interactive`. Expected: they fail.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4:** Run the tests, then `scripts/test-components.sh`, since the bundle's toolkit changed. Then run the workspace suite and clippy. Expected: all pass and clippy is clean.
- [ ] **Step 5: Commit.**

### Task 6: *Try* in the editor

**Files:**
- Modify:
  - `crates/app/src/slide_editor/mod.rs`: add `trying: Option<(PlayingHere, usize)>`, and `TRY_ID = "interactive-try"`, `TRY_DONE_ID`.
  - `crates/app/src/slide_editor/selecting.rs`: add `try_or_stop` and `trying`, and extend `keep_playing_only_what_holds`.
  - `crates/app/src/slide_editor/render.rs`: the *Try* button over the selection, and Space through a new `INTERACTIVE_CONTEXT`.
  - `crates/app/src/slide_editor/mouse.rs`: presses go to the thing being tried before selection.
  - `crates/app/src/inspector/form.rs`: a button's form shows *Runs in the show.* under `run` and `step`.
- Test: `crates/app/tests/ui_interactive.rs`.

**Interfaces:** consumes Task 5's `send_to`, which is moved or duplicated so the editor holds a state store of its own, and Task 2's `ButtonAction`.

- [ ] **Step 1: Write the failing tests.**
  - `try_lets_the_counter_count_in_the_editor_and_selection_still_works_after`.
  - `space_tries_and_stops_a_selected_component`.
  - `a_run_or_step_button_does_nothing_in_the_editor_and_its_form_says_so`.
  - `a_do_button_tried_in_the_editor_reaches_a_target_being_tried`.
  - `selecting_something_else_stops_trying_and_clears_its_state`.
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4:** Run the tests, `ui_video` and `ui_model` (whose Play and Turn must be unchanged), and clippy. Expected: all pass and clippy is clean.
- [ ] **Step 5: Commit.**

### Task 7: The guide, the example and the record

**Files:**
- Modify:
  - `docs/guide/components.md`: a new chapter, "Reacting to clicks: `ctx.events` and `ctx.state`", with `#[component(interactive)]`, `Element::Hit` and the limits.
  - `docs/guide/writing-talks-for-ai.md`: add `<Button>`.
  - `docs/guide/features.md`: a paragraph on clickable slides and buttons.
  - `README.md`: one bullet in *What it does*.
- Create:
  - `fixtures/components/components/counter.rs`, interactive;
  - a slide in `fixtures/components/slides.md` with the counter and two buttons (`do="add" value="1"` and `do="reset"`);
  - `docs/checks/plan-h1.md`, in the shape of plan-g5's record.
- Test: the existing check that every example in the guide builds and checks clean (`scripts/test-components.sh` and `ai_tool`).

- [ ] **Step 1:** Write the chapter and the example. The example must build with the bundle and check clean.
- [ ] **Step 2:** Run `scripts/test-components.sh`, then `scripts/test-all.sh --quick`, and note the counts from the output.
- [ ] **Step 3: Write `docs/checks/plan-h1.md`.** It covers:
  - what was built;
  - the test counts from Step 2;
  - the hand checks:
    - present the example talk on a real screen, and click the counter and the buttons;
    - a real `run` button in a terminal;
    - the presenter window;
    - the pointer hiding;
    - on Windows, the same steps.
  - "## The final review", left for the controller.
- [ ] **Step 4: Commit.**
