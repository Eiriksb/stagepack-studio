# Stagepack Studio

A native desktop app for editing Markdown slide talks, written in Rust with GPUI. "Stagepack Studio" is a working name.

It is being rebuilt as a fully native app: it reads a talk's files directly, draws the slides itself and writes edits back to the Markdown. It does not use a web view. The design is in `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md` and the plans are under `docs/superpowers/plans/`.

Right now the app opens a talk's folder and shows its slides, drawn by the engine from `slides.md` and the pictures and fonts beside it. The arrow and Page keys move between slides, Ctrl+Shift+M shows the talk's Markdown, and saving `slides.md` in another editor updates the window. F5 presents the talk, and Shift+F5 rehearses it with a presenter window of notes, next slide, timer and counter. Talks can be packed into one file, and exported to PDF and PowerPoint. What has and hasn't been checked is in `docs/checks/plan-b.md`.

The window is an editor of the PowerPoint kind, with tool tabs across the top, the list of slides down the left and the notes under the slide. Slides can be added in five layouts, duplicated, hidden from the show, deleted, and dragged to a new place, and every change can be undone. Each edit changes only the bytes of what was edited in `slides.md`. Save and Save As write a talk folder or a packed talk, the app asks before saving over a change made outside it, and unsaved changes are kept in the app's data folder and offered back after a crash. What has and hasn't been checked of this is in `docs/checks/plan-d1.md`.

Text is typed straight onto the slide: a click in a title, a body or a box puts a caret there, and an empty place on a new slide offers *Click to add title* or *Click to add text*. Words can be made bold, italic, underlined, coloured and sized, paragraphs made into bulleted or numbered lists with levels, and a place's text aligned, from the Home tab, whose row of tools scrolls sideways in a narrow window. Dead keys and input methods are taken as the system gives them. Text and whole slides can be cut, copied and pasted, also between talks, and Markdown copied from another editor pastes as a slide. A run of typing is one undo step, and typing rewrites only the paragraph typed in. Tables, code, quotes and text mixed with components or pictures can't be typed in yet, and a click there says so. What has and hasn't been checked of this is in `docs/checks/plan-d2.md`.

A talk can have components of its own: small pieces of Rust in its `components` folder, used on a slide by their tag, like the ready-made `<Chart>`. The app builds them to WebAssembly with the compiler bundle that ships with it, only when asked, and runs them in a sandbox. How to write one is in `docs/guide/components.md`, with examples in `fixtures/components`. What has and hasn't been checked of this is in `docs/checks/plan-c.md`.

## Building

Use `scripts/cargo-safe.sh` in place of `cargo` on a desktop you are working on. A GPUI build can use more memory than the desktop can spare, and the script runs cargo in its own memory-limited systemd scope so only the build is killed, not your session.

```bash
scripts/cargo-safe.sh test --workspace
scripts/cargo-safe.sh run
```
