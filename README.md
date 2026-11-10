# Stagepack Studio

A native desktop app for editing Markdown slide talks, written in Rust with GPUI. "Stagepack Studio" is a working name.

It is being rebuilt as a fully native app: it reads a talk's files directly, draws the slides itself and writes edits back to the Markdown. It does not use a web view. The design is in `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md` and the plans are under `docs/superpowers/plans/`.

Right now the app opens a talk's folder and shows its slides, drawn by the engine from `slides.md` and the pictures and fonts beside it. The arrow and Page keys move between slides, Ctrl+M shows the talk's Markdown, and saving `slides.md` updates the window. F5 presents the talk, and Shift+F5 rehearses it with a presenter window of notes, next slide, timer and counter. Talks can be packed into one file, and exported to PDF and PowerPoint. What has and hasn't been checked is in `docs/checks/plan-b.md`. Slides can't be edited in the app yet.

## Building

Use `scripts/cargo-safe.sh` in place of `cargo` on a desktop you are working on. A GPUI build can use more memory than the desktop can spare, and the script runs cargo in its own memory-limited systemd scope so only the build is killed, not your session.

```bash
scripts/cargo-safe.sh test --workspace
scripts/cargo-safe.sh run
```
