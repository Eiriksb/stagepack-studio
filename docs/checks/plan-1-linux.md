# Plan 1: Linux end-to-end check

2026-11-09, at 6369405, debug build through `scripts/cargo-safe.sh run`.

**Result: the core loop works, but not on the fixture as it stands.** The fixture's Box doesn't appear on slide 2, so the pass on the real fixture stopped at step 3. The whole loop passed on a copy with one line of the fixture corrected.

## Scripted pass (nested display)

XTest input and screenshots in `Xephyr :77 -screen 1280x800`, with `STUDIO_DEBUG_IPC=1`, no window manager. "Log" means the app's stderr.

### The fixture as it stands: `target/checks/my talk æøå 100%.sptx`

| Step | Result |
| --- | --- |
| 1. Copy the fixture | Done |
| 2. Open, wait for `ready` | `ready` in the log |
| 3. Slide 2, click the Box | **Failed.** Right arrow gave `slide` and slide 2 shows, but the Box isn't on it: no pixel of its colour in the screenshot, and clicks where it should be gave no `selection`. A click on the paragraph did (`selection`, inspector `Text`), so clicks reach the page. |
| 4 to 7 | Not reachable. `check-saved.sh` fails at its first check, as the file was never saved. |

Closing it exited 0 with no prompt, and the file was unchanged.

### One line corrected: `target/checks/my talk æøå 100% array.sptx`

The same talk with `v-drag="[100,100,200,120]"`.

| Step | Result |
| --- | --- |
| 2. Open, wait for `ready` | `ready` in the log |
| 3. Slide 2, click the Box | `selection`; inspector shows `<div>`, X 100, Y 100, W 200, H 120 |
| 4. X field, Ctrl+A, type 140, Enter | `edited` and `result`; the Box moved, the title shows `•` |
| 5. Ctrl+S, focus in the X field | `Saved` from serve; `slides.md` has `v-drag="[140,100,200,120]"` |
| 6. WM_DELETE | No prompt, exit 0 |
| 7. `check-saved.sh` | **Fails its first check**: Studio writes the bracket form, the script looks for `v-drag="140,100,200,120"`. With only that string changed to the bracket form, in a scratch copy: `saved talk check: ok`. |
| Second run: edit, WM_DELETE | The save-changes dialog appears (screenshot). **Don't Save**: exit 0, mtime and content unchanged. |

No `stagepack`, `node`, app or Xephyr process was left after any run.

### Why the Box is missing

`v-drag="100,100,200,120"` isn't a position to Slidev. Vue compiles it to `[_directive_drag, 100,100,200,120]`, so the directive's value is `100`, which Slidev reads as the name of a `dragPos` frontmatter entry. There is none, so Slidev places the element itself. Slidev's position form for the directive is `v-drag="[100,100,200,120]"`, and that is what Studio writes (`formatPos(pos, 'attr')` in `client/md/drag.ts`).

Opening the same fixture straight at `/2` in Chromium, on a plain `serve` with Studio, also ended Slidev: it wrote the `dragPos` it had worked out, and crashed in `invalidateModule` (`Cannot read properties of undefined (reading '_clientModule')`). Seen once. The app loads `/1` first and didn't hit it in two runs on this fixture.

## Hand check (owner)

Steps 3 to 10 need the fixture and the first check of `check-saved.sh` corrected first (see above).

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
