# Plan A: what ran, and how to check Windows

Run on the `native-rust` branch, on a 16-core Linux laptop, in debug builds, on 2026-11-10. Nothing here has run on Windows.

## Tests on Linux

`scripts/cargo-safe.sh test --workspace` passed at commit a5702ea: 266 tests, 0 failed, 0 ignored, no warnings.

| Crate | Tests |
| --- | --- |
| slides-engine | 151 (builtins 81, flow 29, deck 18, render 11, text 8, files 4) |
| slides-format | 60 (parse 35, edit 18, split 7) |
| stagepack-studio (app) | 49 (ui_slides 24, document_state 20, watcher 4, ui_layout 1) |
| toolkit | 6 |

## Real-window check on Linux

The controller checked commit 41d907e in a debug build, with real key events on a nested X display:

- Right pressed twice reached slide 3 of 7.
- Ctrl+M opened the Markdown panel.
- An outside edit to `slides.md` reached the panel, and the app stayed on slide 3.

Earlier, a real-window run in Task 12 showed a slide's heading change after an outside edit, with no restart.

At commit a5702ea, on the same kind of display, Right pressed five times reached slide 6 of 7. Replacing the picture on that slide and saving `slides.md` with unchanged text showed the new picture, and the app stayed on slide 6.

## Not exercised

- The folder picker.
- A drag-resize of the window.
- Anything at all on Windows. No Windows machine has run this code. The one GitHub Actions run was cancelled before it reported anything, and the project no longer uses GitHub Actions.

## Optional Windows check, by hand

On a Windows PC with Rust 1.98.0 and the MSVC build tools installed:

1. `git clone -b native-rust https://github.com/Eiriksb/stagepack-studio.git`
2. `cargo test -p slides-format -p toolkit -p slides-engine`. If only golden tests fail, the images differ slightly from Linux, so send back which ones.
3. `cargo run --release -p stagepack-studio -- fixtures\showcase`, then confirm the slides match the images in `crates/engine/tests/golden/`, that Left and Right change slides, and that Ctrl+M opens the Markdown panel.
