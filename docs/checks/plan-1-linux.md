# Plan 1: Linux end-to-end check

2026-11-09, app code at fa9e7da, debug build through `scripts/cargo-safe.sh run`.

**Result: the scripted pass passed on the real fixture.** It covers hand-check steps 1 to 5, 8 to 10, 11 except Cancel, and 14 (Ctrl+S from the inspector field in the first run, from the page in the second). It does not cover steps 6 and 7 (Free position and its Undo), or steps 12 and 13, which need the installed Stagepack. The hand check below is still to do.

## Scripted pass (nested display)

XTest input and screenshots in `Xephyr :77 -screen 1280x800`, with `STUDIO_DEBUG_IPC=1`, no window manager. `XDG_DATA_HOME` points at a scratch folder, so serve doesn't claim the talk in the installed Stagepack. "Log" means the app's stderr.

`target/checks/my talk æøå 100%.sptx`, a copy of the rebuilt `target/fixtures/hello.sptx`:

| Step | Result |
| --- | --- |
| 1. Copy the fixture | Done |
| 2. Open, wait for `ready` | `ready` in the log |
| 3. Slide 2 (Right arrow), click the Box | `slide`, then `selection`; inspector shows `<div>`, X 100, Y 100, W 200, H 120 |
| 4. X field, Ctrl+A, type 140, Enter | `edited` and `result`; the Box's left edge moved from 104 to 145 on screen |
| Undo, then Redo (title bar) | Each gives `history`, `edited` and `result`. After Undo the Box is at 104 and the X field reads 100; after Redo it is at 145 and the field reads 140 |
| 5. Ctrl+S, focus in the X field | A `result` (finish-edit), then `Saved` from serve; `slides.md` has `v-drag="[140,100,200,120]"` |
| 6. WM_DELETE | A `result` (finish-edit), no prompt, exit 0 |
| 7. `scripts/check-saved.sh` | `saved talk check: ok` |
| Second run, clean talk: double-click the paragraph, End, type ` typed without escape`, Ctrl+S from the page, no Escape | `save shortcut`, `edited`, `result`, `Saved`; `unzip -p` of `slides.md` has `This paragraph sits in the normal flow of the slide. typed without escape` |
| WM_DELETE | Exit 0, and `unzip -p` still shows those words |
| Third run: double-click the paragraph, type ` typed for the close`, WM_DELETE with no Ctrl+S | `edited` and `result`, then the save-changes dialog (screenshot). **Don't Save**: exit 0, the file's sha256 unchanged |

No `stagepack`, `node`, app, WebKit or Xephyr process was left after any run.

## Hand check (owner)

- [ ] 1. Copy `target/fixtures/hello.sptx` to `/tmp/…/my talk æøå 100%.sptx`.
- [ ] 2. Open it: `STUDIO_DEV_HOME=target/dev-home scripts/cargo-safe.sh run -p stagepack-studio -- "<path>"`.
- [ ] 3. Go to slide 2 and click the Box. The inspector shows `<div>` and X 100.
- [ ] 4. Type 140 in X and press Enter. The Box moves, and the title shows `•`.
- [ ] 5. Press Undo, then Redo. The Box moves back, then forward again.
- [ ] 6. Click the paragraph. **Free position** shows; click it and the paragraph becomes positioned.
- [ ] 7. Undo it.
- [ ] 8. Click into the page so it has the keyboard, then press Ctrl+S. The `•` goes away.
- [ ] 9. Close the window, with no prompt.
- [ ] 10. Run `scripts/check-saved.sh "<path>"`: `saved talk check: ok`.
- [ ] 11. Reopen the talk, make an edit and close. The save prompt appears. **Cancel** keeps the window. **Don't Save** closes, and the file's mtime is unchanged.
- [ ] 12. Open the same file with the installed Stagepack first (`~/.local/share/stagepack/stagepack open "<path>"`), then in the app. The app shows `This presentation is already open in Stagepack. Close it there, then open it here.`
- [ ] 13. The reverse order: open the file in the app first, then with the installed Stagepack. Stagepack opens the talk in its window on the app's server, and there is no second copy (no new folder appears in `~/.local/share/stagepack/runtimes/*/work`).
- [ ] 14. Double-click text on the slide and type some words. Press Ctrl+S without pressing Escape, and close the window. Reopen the talk: the words are there.
