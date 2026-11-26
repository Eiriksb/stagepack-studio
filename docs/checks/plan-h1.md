# Plan H1: what ran, and what to check by hand

The run was on the `plan-h1` branch, in a debug build, on a 16-core Linux laptop, with Rust 1.98.0, on 2026-11-25. The branch hasn't been pushed or merged. **Nothing here has run on Windows.**

## What was built

A slide can answer clicks in the show, and a talk's own component can keep a state and take events.

- **Routing.** A click, drag or wheel turn on an interactive thing goes to it, and the show doesn't advance. Interactive things are a `<Button>`, a running live terminal, a model, and a talk's own component that says `#[component(interactive)]`. Everywhere else a click advances as before, and the keyboard and a clicker are unchanged. A press that starts on one thing and is let go of off it neither acts nor advances. An interactive thing is outlined faintly (1 px, the accent colour at 40 %) under the pointer, never in the slide's bitmap, so never in an export. The pointer hides after 2 seconds of rest on a slide that has no interactive thing. The presenter window's current slide takes presses the same way and both windows show one state; the next-slide preview takes none.
- **`<Button>`.** `label`, `look` (`primary`, `secondary`, `quiet`), `id`, the placement attributes, and exactly one action: `step`, `run` with `in`, or `do` with `on` and an optional `value`. It is pressed with a left click only, looks 15 % darker while held, and acts on release over it. A button with no action or more than one, an `on` or `in` that names nothing it can act on, and a `step` past the slide's last click each give a notice on the slide and in the AI tool's `check`.
- **Typed text.** `run` types its text and Enter into the live terminal on the slide whose `id` is `in`, starting it first. The terminal's command goes through the G5 gate. The typed text has a yes of its own on the host: the whole text is shown, matched exactly, asked again if it changes, *Always for this folder* for folder talks only, and a yes for the shell never covers it. Text with control characters other than a tab, over 4,096 bytes, or with the characters `unrunnable` refuses, is refused with a notice and never typed. A terminal that has ended, was refused or is past the limit of 4 gives one line on the status bar saying why.
- **Interface v2.** `ABI_VERSION` is 2. A module that says 1 is loaded and drawn as before; 3 or more is still refused. `RenderInput` gains `state` and `events`, `RenderOutput` gains `state`, `ComponentSchema` gains `interactive`, and `Element` gains `Hit`. `ctx.events()`, `ctx.state()` and `ctx.set_state()` are the toolkit's side. State is at most 64 KiB, lives while its slide is shown, and is never written to the talk. Events are gathered to 60 drawings a second per component, at most 64 to a drawing, with every Down, Up and Action kept and only the last Move. Editing the talk keeps the state of a component whose tag and settings are unchanged. Only a component placed on the slide itself is interactive.
- **The editor.** A selected button or interactive component has *Try* (and Space), as a video has *Play*; leaving the selection stops it. `run` and `step` do nothing there and the button's form says *Runs in the show.* A `do` works while its target is tried.
- **The guide and the example.** A chapter in `docs/guide/components.md`, the *Buttons* section of the AI guide, a paragraph in `docs/guide/features.md`, a bullet in the README, and `fixtures/components/components/counter.rs` with a slide of a counter and two buttons (`do="add" value="1"` and `do="reset"`).

## How to try it

1. Build the app: `scripts/cargo-safe.sh build -p stagepack-studio`, and the helper if you want a button to type into a terminal: `scripts/cargo-safe.sh build -p slides-terminal-helper`, with `STUDIO_TERMINAL_HELPER=$PWD/target/debug/stagepack-terminal`.
2. Copy `fixtures/components` to a scratch folder, open it, and choose to build its components. The counter slide is the one before the last.
3. Press F5, reach the counter slide, click the plus, then *Add 1* and *Reset*.

## Tests on Linux

- `scripts/test-components.sh` (with the compiler bundle): every step passed. `slides-components` compile 4 passed and examples 1 passed (the six examples, `counter.rs` among them, build, load and draw), `builds` 2 passed, `ai_tool` 31 passed (the guide's examples check clean), `ui_interactive` 26 passed (including the counter built with the bundle), the WebAssembly reader build 1 passed, the Slidev add-on 3 passed and the round-trip 3 passed.
- `scripts/test-all.sh --quick`: clippy ok and packaging ok. The workspace and components steps each had failures on a loaded machine, the first run racing a build of the other steps. Two were mine: the palette tests (`ui_palette`) counted five components in the example talk, and now count six. After that fix, run alone: `ui_palette` 20 passed, `ai_tool` 31 passed, `talk_components` 46 passed, and `a_guest_that_loops_while_loading_is_stopped` (`addon_host`) 2 passed. That test failed once on its first rerun at 61 s of load and passed alone in 0.21 s; it times a guest that loops and has nothing to do with this plan.
- `a_big_talk_is_checked_quickly` and `a_picture_of_a_model_ends_cleanly_and_soon` (`ai_tool`) failed once in the full run at 116 s and passed in the run of `ai_tool` alone (31 passed in 2.9 s).
- `builds`, run with the bundle: 2 passed, after the example talk's 6 components were counted.

- The last `scripts/test-all.sh --quick` of Task 7 was not clean from one end to the other: the failures above were rerun alone, not in a clean full run. The fixes after the final review ran it again; its counts are below.

- After the final-review fixes, run on 2026-11-26: `scripts/test-components.sh` passed every step but the Slidev round-trip: `ui_interactive` 31 passed with the bundle, the counter tests among them, and `ai_tool` 31 passed. The round-trip's 3 tests failed because a stale `xtask` build in `target/` named a folder that no longer exists. After a rebuild of `xtask` they passed (3 passed). `ui_live_terminal` 26 passed, `live_terminal_yes` 40 passed, `slides-engine` and `slides-components` 694 passed, and clippy, warnings as errors, was clean.
- `scripts/test-all.sh --quick` after the fixes: clippy ok and packaging ok (59 passed). The workspace step had 2,941 passed and 5 failed, and the components step 75 passed and 2 failed. So this run wasn't clean from one end to the other either. Failed tests, rerun alone:
  - `a_guest_that_loops_while_loading_is_stopped` failed at 15.2 s in the full run and passed alone in 0.22 s.
  - `a_big_talk_is_checked_quickly` failed in the full run and on its first rerun at 27 s. It then passed alone in 1.9 s.
  - Four tests in `slides-model`'s `draw` failed: `a_closed_drawer_says_so`, `draw_never_blocks_the_caller`, `only_the_newest_picture_of_a_scene_is_drawn` and `two_drawers_do_not_fight`, with *the drawer's thread never ended* or *the drawer never said whether it can draw*. So did `a_picture_of_a_model_ends_cleanly_and_soon` (`ai_tool`). They failed again alone. Run alone, the whole `draw` file hangs on asking the graphics device for an adapter, though 13 of its tests passed in the full run. `slides-model` depends on nothing these fixes touched, so this is the machine's graphics device. It has to be run again on a machine whose device answers.

## Found in the final review

- **A component froze on a repeated input.** A component given exactly what it was given the time before (a counter's *Reset* pressed a third time, or a `do` to a component that keeps nothing) was never told it had been drawn, so it took nothing more until its slide was left. A second rehearsal of the same talk, or the editor's *Try* before a show, could start the same way. Fixed: every feed is answered, feeds are told apart across shows and the editor, and a show forgets its components when it ends.
- **Duplicate ids acted on the first match.** A button whose `in` or `on` named an `id` two things share typed into, or sent to, the first. Fixed: it acts on neither, and the status line, the slide's notice and `check` say *Two things on this slide have the id "…".*
- **`run` reached a terminal not yet shown.** A button could start and type into a `<Terminal click="2">` the audience couldn't see yet. Fixed: nothing is asked or typed, and the status line says *This button's terminal isn't on the slide yet.*
- A press on a button takes the keys back from a terminal that had them. That is meant, and the AI guide's *Buttons* section says so.

## Hand checks for the owner

On a scratch display, with a scratch copy of the example talk:

1. Present the example talk (F5) on a real screen. On the counter slide, click the plus: the count goes up and the slide doesn't advance. Press *Add 1* and *Reset*: the count follows. Click beside the counter: the show advances. Space and the arrows still move the show with the pointer over a button.
2. A real `run` button in a terminal: a slide with `<Terminal id="shell" run="bash" …/>` and `<Button label="List" run="ls" in="shell" …/>`. Pressing the button asks for the terminal's command, then for the typed text (shown whole); after both, `ls` and Enter are typed. A second press doesn't ask again. Change the text in the file: it asks again.
3. The presenter window (Shift+F5 with two screens): press a button and click the counter in the presenter's current slide. It acts once, and both windows show the same count. The next-slide preview takes no clicks.
4. The hover outline: a faint outline appears on the button, the counter's plus and a running terminal, and an export (PDF, PowerPoint, a picture) shows none.
5. The pointer hiding: on a plain slide, it hides after about 2 seconds without moving (see the owner items below), and on a slide with a button it stays.
6. The editor: select the counter and press *Try* (or Space), click the plus, select a `do` button and press it. Selecting something else stops it. A `run` or `step` button shows *Runs in the show.*
7. On Windows, the same steps.

## Owner items

- The real pointer can't be hidden yet: gpui-fast 0.1.13 has no hidden cursor style. The 2 second hiding policy is tested but isn't visible on screen.
- A tab character is allowed in a button's typed text, but in a shell a tab triggers completion, so the text that runs can differ from what was shown and said yes to. The owner decides whether to refuse tabs.
- Windows: the same hand checks.

## The final review

(Left for the controller.)
