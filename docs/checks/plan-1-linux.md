# Plan 1: Linux end-to-end check

2026-11-09, app code at 6369405, debug build through `scripts/cargo-safe.sh run`.

**Result: the scripted pass passed on the real fixture, steps 1 to 7 and the save-changes dialog.** The hand check below is still to do.

## Scripted pass (nested display)

XTest input and screenshots in `Xephyr :77 -screen 1280x800`, with `STUDIO_DEBUG_IPC=1`, no window manager. "Log" means the app's stderr. A first pass stopped at step 3 because the fixture's `v-drag` had no brackets, so the Box never appeared. The fixture is corrected, and everything below is from the re-run on it.

`target/checks/my talk æøå 100%.sptx`, a copy of the rebuilt `target/fixtures/hello.sptx`:

| Step | Result |
| --- | --- |
| 1. Copy the fixture | Done |
| 2. Open, wait for `ready` | `ready` in the log |
| 3. Slide 2 (Right arrow), click the Box | `slide`, then `selection`; inspector shows `<div>`, X 100, Y 100, W 200, H 120 |
| 4. X field, Ctrl+A, type 140, Enter | `edited` and `result`; the Box's left edge moved from 104 to 145 on screen; the title shows `•` |
| Undo, then Redo (title bar) | Each gives `history`; the Box goes back to 104, then forward to 145 |
| 5. Ctrl+S, focus in the X field | `Saved` from serve; the `•` goes away; `slides.md` has `v-drag="[140,100,200,120]"` |
| 6. WM_DELETE | No prompt, exit 0 |
| 7. `scripts/check-saved.sh` | `saved talk check: ok` |
| Second run: edit X to 150, WM_DELETE | The window stays and the save-changes dialog appears (screenshot). **Don't Save**: exit 0, mtime and content unchanged. |

After Undo the X field still showed 140, with the Box back at 100, because the field keeps focus and a focused field ignores the page's echo.

No `stagepack`, `node`, app, WebKit or Xephyr process was left after any run.

## Hand check (owner)

- [ ] 1. Copy `target/fixtures/hello.sptx` to `/tmp/…/my talk æøå 100%.sptx`.
- [ ] 2. Open it: `STUDIO_DEV_HOME=target/dev-home scripts/cargo-safe.sh run -p stagepack-studio -- "<path>"`.
- [ ] 3. Go to slide 2 and click the Box. The inspector shows `<div>` and X 100.
- [ ] 4. Type 140 in X and press Enter. The Box moves, and the title shows `•`.
- [ ] 5. Press Undo, then Redo. The Box moves back, then forward again.
- [ ] 6. Click the paragraph. **Free position** shows; click it and the paragraph becomes positioned.
- [ ] 7. Undo it.
- [ ] 8. Press Ctrl+S. The `•` goes away.
- [ ] 9. Close the window, with no prompt.
- [ ] 10. Run `scripts/check-saved.sh "<path>"`: `saved talk check: ok`.
- [ ] 11. Reopen the talk, make an edit and close. The save prompt appears. **Cancel** keeps the window. **Don't Save** closes, and the file's mtime is unchanged.
- [ ] 12. Open the same file in plain Stagepack first (`$STAGEPACK_SRC/dist/linux/stagepack open`), then in the app. The busy message shows.
