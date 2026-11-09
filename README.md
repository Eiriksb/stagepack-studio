# Stagepack Studio

A native desktop app for editing Markdown slide talks, written in Rust with GPUI. "Stagepack Studio" is a working name.

It is being rebuilt as a fully native app: it reads a talk's files directly, draws the slides itself and writes edits back to the Markdown. It does not use a web view. The design is in `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md` and the plans are under `docs/superpowers/plans/`.

Right now the window shows a title bar and an empty canvas area, and the engine is the next piece of work.

## Building

Use `scripts/cargo-safe.sh` in place of `cargo` on a desktop you are working on. A GPUI build can use more memory than the desktop can spare, and the script runs cargo in its own memory-limited systemd scope so only the build is killed, not your session.

```bash
scripts/cargo-safe.sh test --workspace
scripts/cargo-safe.sh run
```
