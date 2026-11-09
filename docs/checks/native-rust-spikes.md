# Native Rust spikes: results

Throwaway builds on the `spike/native-rust` branch, answering the go-or-no-go questions in `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`. Measured on a 16-core Linux laptop with 30 GB of memory, in release builds, on 2026-11-09.

## Spike 1: drawing

| Measure | Limit | Result |
| --- | --- | --- |
| Full raster, 1920×1080, p50 | at most 150 ms | 25.3 ms |
| Full raster, 3840×2160, p50 | at most 500 ms | 210.8 ms |
| Picture check: rotated text sharp, `12%` bold, red and larger, emoji in colour | looks right | yes |

### One bitmap composed per frame (what the plan asked for)

| Run | Limit on frame time, p95 | Result |
| --- | --- | --- |
| Drag, 1920×1080 | at most 17.5 ms | **42.2 ms, fails** |
| Fade, 1920×1080 | at most 17.5 ms | **43.6 ms, fails** |
| Drag, 3840×2160 | at most 34 ms | **83.3 ms, fails** |
| Fade, 3840×2160 | at most 34 ms | **124.0 ms, fails** |

### Each layer cached as its own GPUI image (`--draw sprites`)

| Run | Limit on frame time, p95 | Result |
| --- | --- | --- |
| Drag, 1920×1080 | at most 17.5 ms | 16.8 ms |
| Fade, 1920×1080 | at most 17.5 ms | 16.8 ms |
| Drag, 3840×2160 | at most 34 ms | 16.8 ms |
| Fade, 3840×2160 | at most 34 ms | 16.8 ms |
| GPU memory growth over 3000 drag frames | at most 64 MB | 0 MB (886 MB throughout) |

GPU memory is the measure that counts, because GPUI's images live there. A control run proved it. At 640×360 over 1500 frames:

| Run | GPU memory, early to late | Process memory |
| --- | --- | --- |
| Releasing each old image | 877 to 877 MB | 217 to 217 MB |
| Deliberately never releasing them | 1197 to 3501 MB | 216 to 220 MB |

Process memory barely notices the leak, so it can't be used to check for one. Frame times here are when GPUI painted, not when the screen showed the frame, so an occasional dropped frame could hide below the 95th percentile.

A screenshot of the running window shows the slide drawn correctly.

**Verdict for spike 1: yes,** with each layer cached as its own image, so the CPU does no work per frame. This display runs at 60 Hz, so 16.8 ms is the fastest it can show.

Not covered: this machine has no 4K screen, so "4K" is a 3840×2160 image shown in a 1280×720 window. Rotating or resizing a layer by dragging, and typing, each redraw and re-upload one layer repeatedly, and neither was measured. The one-bitmap failure is for a simple build with no dirty rectangles, so it doesn't prove that a CPU path could never work.

## Spike 2: compile loop

The component is the 98-line fixture in `spikes/compile/fixtures/score.rs`, and each save changes one string in it. Times cover `rustc` only. Loading the result in Wasmtime adds a little more.

| Measure | Limit | Result |
| --- | --- | --- |
| Save to compiled, `opt-level=1`, p95 | goal 2000 ms, fails over 5000 ms | 279 ms |
| Save to compiled, `opt-level=0`, p95 | same | 167 ms |
| Save to compiled, `opt-level=2`, p95 | same | 284 ms |
| Compiled component renders in Wasmtime with no imports | yes | yes |
| Compiler bundle on disk | at most 480 MB | 465 MB |
| Compiles and renders in a sandbox with no Rust, no home folder and no network | yes | yes, 272 ms p95 |
| Folder named `my talk æøå 100%` | works | works, after a fix |

Findings for the real build:
- Rust 1.98 keeps the standard library's metadata in separate `.rmeta` files, so the bundle needs those as well as the `.rlib` files. They add 77 MB.
- The bundle needs the compiled `serde_derive` macro as well as ours, because the `serde` library refers to it.
- LLVM's linker treats `%` in its output path as a random-name placeholder, so a folder like `100%` breaks it. The app must link in a private scratch folder and move the result into place.
- Wasmtime 50 needs its `anyhow` feature for its errors to convert to `anyhow`.
- The toolkit here is small. A real one will take longer to link, but this leaves plenty of room.

### Windows

GitHub's Windows runner built the bundle and ran the loop: 375 ms p95 for a save, `render_ok` true, and a 399 MB bundle in a 137 MB zip. That runner has Rust installed, so it doesn't prove the clean-machine case. [The zip is on this Actions run](https://github.com/Eiriksb/stagepack-studio/actions/runs/42661592540).

**Owner's check: not done.** On 2026-11-09 the owner chose to move on without it, so no clean Windows machine has run this. The steps stay here for whoever does it.

1. On the Windows PC, use a user account with no Rust. `where rustc` should find nothing.
2. Download `spike-compile-windows` from the Actions run above, and unzip it to a folder with a space in its name.
3. Double-click `run-check.cmd`, and send back the JSON line it prints.
4. If Windows says a DLL is missing, send its name.

Expected: `render_ok` true, and `compile_ms_p95` at most 5000.

**Verdict for spike 2: yes on Linux. Unproven on a clean Windows machine.**

## Spike 3: typing on the slide

| Measure | Limit | Result |
| --- | --- | --- |
| Styled text writes back byte for byte, and only the edited paragraph changes (CRLF file with odd spacing) | yes | yes |
| Markdown symbols typed as text stay text | yes | yes |
| Random spans survive writing and parsing | yes | yes, 2,000 cases |
| Fallback to `<Text bold="true">` because `**` can't close there | recorded | 101 of 2,000 random paragraphs, using an alphabet heavy in Markdown symbols |
| Per-word colour, typing inside a styled word, and emoji reach the Markdown | yes | yes |
| Caret offsets across `æ🎉x` between UTF-16 and bytes | correct | correct |

### Real key events in the window

Driven by XTest on a nested display with the Norwegian keyboard layout. The talk was in a folder named `my talk æøå 100%`.

| Step | Result |
| --- | --- |
| Typed `æøå` | Appeared in the text box |
| Dead diaeresis, then `a` | Gave `ä` |
| Shift+Left twice, then Ctrl+B, Ctrl+1 and Ctrl+2 | Only those two letters became bold, red and larger |
| Ctrl+S | Line 7 became `First paragraph, with **bold** words. æø<Text color="red" size="64">**åä**</Text>`, and every other line stayed byte-identical |

**Owner's check on the real desktop: not done.** The owner opened the window once on 2026-11-09, nothing was saved, and they chose to move on. So a real keyboard and pasting an emoji are unproven. The steps stay here for whoever does it.

Run:

```bash
scripts/cargo-safe.sh run --release -p spike-typing --bin spike-typing -- spikes/typing/fixture.md
```

1. Type `blåbær`.
2. Press the dead acute key and then `e`. It should give `é`.
3. Paste `🎉`. It should appear in colour, and the caret should land after it.
4. Select a word with Shift+arrows, then press Ctrl+B, Ctrl+1 and Ctrl+2. Only that word should change.
5. Press Ctrl+S, then run `git diff --no-index spikes/typing/fixture.original.md spikes/typing/fixture.md`. Only the first paragraph should differ.

Limits of this spike, for the editor plan: the text box edits one single-line paragraph. A hand-wrapped paragraph would lose its later lines on save, and links, code spans, images and hard breaks aren't parsed yet. The 101-of-2,000 figure comes from running that test alone.

**Verdict for spike 3: yes from the tests and the scripted run. Unproven on a real keyboard.**
