# Stagepack Studio

A native desktop editor for Slidev `.sptx` talks. "Stagepack Studio" is a working name.

This is early work, a walking skeleton on Linux: it opens a talk in a window through a `stagepack serve` sidecar, shows the slides in a web view with the Studio add-on, edits an element's position from a native inspector, and saves back into the `.sptx`.

- `crates/protocol`: the bridge protocol, checked against the golden fixtures of the Studio add-on. Set `STUDIO_SRC` (or fill in `dev.env`) to point at that checkout before running `cargo test`.
- `crates/sidecar`: runs `stagepack serve --json` and reads its events.
- `crates/app`: the window (GPUI) and the talk's state machine.

Use `scripts/cargo-safe.sh` in place of `cargo` on a desktop you are working on, and `scripts/dev-home.sh` to build `target/dev-home`, the runtime the app needs. `docs/checks/plan-1-linux.md` is the end-to-end check.

The design and the implementation plans live under `docs/superpowers/`.
