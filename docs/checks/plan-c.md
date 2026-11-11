# Plan C: what ran, and how to check Windows and PowerPoint

The run was on the `plan-c` branch at commit 6e9d8bc, on a 16-core Linux laptop, with Rust 1.98.0, in a release build, on 2026-11-11. It found one thing to fix, in the example talk, which is fixed at d23a628, and the tests were run there. The branch hasn't been merged or pushed yet. Nothing here has run on Windows.

## Tests on Linux

At d23a628, `scripts/cargo-safe.sh test --workspace` passed: 879 tests, 0 failed, 7 ignored, no warnings. `scripts/cargo-safe.sh clippy --workspace --all-targets` gave no warnings.

| Crate | Tests |
| --- | --- |
| slides-components | 51, and 4 ignored (sandbox 27, source_check 18, examples 2 and 1 ignored, compile 1 and 3 ignored, unit 3) |
| slides-engine | 358, and 1 ignored (builtins 84, native 45, packed 42, chart 37 and 1 ignored, flow 29, deck 26, talk_components 25, moments 15, timeline 13, files 12, canvas 11, render 11, text 8) |
| slides-format | 60 (parse 35, edit 18, split 7) |
| slides-pdf | 18 |
| slides-pptx | 107 (elements 36, write 31, parts 20, timing 10, fonts 9, unit 1) |
| stagepack-studio (app) | 247, and 2 ignored (ui_slides 67, ui_present 45, document_state 40, present 33, builds 30 and 2 ignored, export 16, watcher 12, unit 3, ui_layout 1) |
| toolkit | 38 (component 21, interface_v1 17) |
| toolkit-macros | 0 |

Six of the ignored tests need the compiler bundle. The seventh, `charts_for_a_look`, writes pictures for a person to look at. `scripts/test-components.sh` builds the bundle and runs all seven, and all seven passed.

## The bundle

`scripts/make-bundle.sh target/bundle` made a bundle of 468 MB (489,820,554 bytes). Nearly all of it is the compiler's own libraries in `lib` (441 MB). The toolkit's prebuilt libraries are 27 MB. The release executable is 97 MB, and `scripts/cargo-safe.sh build --release -p stagepack-studio` took 3 minutes 40 seconds from a warm cache.

## Real-window check on Linux

The talk was a scratch copy of `fixtures/components`, outside the repository. The app was the release build, on a nested X display (`Xephyr :80`, 1280x800, no window manager), with `STUDIO_BUNDLE` set to the bundle and `XDG_DATA_HOME` set to a scratch folder, so that *Always build this folder* wrote nothing to the real data folder. Key presses and clicks went in through the X test extension, and a small script logged every `rustc` process ten times a second. Every result below is from a screenshot that was looked at, under `target/check/c/`, which is not committed. Xephyr stayed up for the whole run.

- **Opened with no builds:** the components' slides showed dashed boxes saying *Score needs building*, and the bar said some components need building, with *Build components* and *Always build this folder*. No `rustc` ran, and the talk had no `build` folder.
- **Build components:** the bar said *Building components…*. Five `rustc` processes ran, one after another, and 1.0 s after the click all five components were built and in the index. The next screenshot, 0.5 s later, showed both scores drawn and the bar gone.
- **Always build this folder:** once everything is built, the bar is gone, and *Always build this folder* with it. So it was pressed after the next change to `score.rs`, which brought the bar back. It built the change and wrote the folder into the scratch data folder's `build-permissions.json`. Then `score.rs` was changed again and saved. With screenshots every 0.03 s, the bar said *Building components…* 0.15 s after the save, and the slide showed the new text 0.37 s after it. That was one measurement.
- **The slide resizes:** while the bar is showing, the slide is drawn smaller to make room for it, so the slide jumps in size when a build starts and again when it ends. That wasn't changed.
- **A type error:** a line assigning a string to a `u32` was added to `score.rs` and saved. The panel said `components/score.rs:6: mismatched types`, the bar offered *Build components* again, and the slide kept the last good build. Taking the line out again cleared the panel without a new build, as the source was the same as the build's.
- **The show:** Shift+F5 opened the audience and presenter windows. Right went through the score, the callout and the three lines of `reveal`, one per click, and the counter and the next-step thumbnail followed. On the `clock` slide, two screenshots 71 s apart showed the hand at about 45° and then about 110°, which is a turn and a bit, in both windows.
- **The building chart:** the example talk's chart showed all four bars at once, as it had no `build="true"`. That was the fix in d23a628, which has a test. Adding it to the scratch copy while the show ran took effect at once. The chart then started with only its axes, and each click added one bar, Q1 to Q4.
- **Memory and processor on the `clock` slide:** over 70.7 s in the show, with both windows drawing the clock, the app used 62.3 s of processor time, 0.88 of one core, and 51.4 s of that was in the kernel. Xephyr used 0.11 of a core. Its memory stayed at 335 MB from start to end, which was also its peak. On a still slide of the same show, the app used 0.05 of a core. This display draws in software (the log says DRI3 was unavailable), so this says nothing about a real GPU, and whether the windows here draw at a fixed rate wasn't found out. The cost on a real screen is still an open question.
- **The packed talk, with `STUDIO_BUNDLE` unset:** there was no bundle beside the executable either. The packed file opened with no bar and no prompt, and its scores were drawn. F5 showed the talk, the three lines one per click, the chart building, and the clock turning about 6° a second. No `rustc` ran.
- **A component that never returns:** `components/spin.rs`, with `loop {}`, on a slide of its own. Opening the folder didn't build it, though the folder was by then in *Always build*, as opening a talk never builds. After *Build components*, the slide showed a dashed box saying *Spin: took too long*, and the panel said the same. The app stayed responsive, and used 0.012 of a core over 10 s on that slide.

## The exported files

The app's Export and *Save as packed talk* ask where to save through the system's Save dialog, which the nested display can't show. So the scratch talk was exported and packed from a throwaway crate outside the repository, which made a deck the way the app's export does and called `write_pdf`, `write_pptx` and `pack_folder`. That crate resolved its own dependency versions, not the repository's `Cargo.lock`.

- **PDF:** 7 pages of 960 x 540 pt. `pdftotext` finds every slide's text in order, the chart's axis numbers and labels included. All seven pages looked right as pictures, with each slide in its finished state: all three lines, all four bars, and the clock at its start.
- **PPTX, by reading its XML:** the chart's slide has its four bars as `<p:sp>` shapes, each `prstGeom` `rect` filled `2563EB`. Their heights are 258, 388, 323 and 517 units, in proportion to 120, 180, 150 and 240. There's no picture of the chart. The clock is an `ellipse` and a freeform (`custGeom`) for its hand. The bars drawn by hand are 4 `rect` shapes, with their labels as text. The only picture in the file is the callout's 48x48 icon. The export said that `Reveal`, `Clock` and `Chart` change as the talk plays, so PowerPoint shows them in their last state, and none of the slides has animations.
- **PPTX, in LibreOffice 24.2.7:** it converted to a 7-page PDF with the same text, and the pages looked the same as our PDF's, bars, badges and clock included. The callout's title *Remember* broke onto two lines, *Remembe* and *r*. LibreOffice doesn't load the fonts embedded in a `.pptx`, so it drew the text in a wider font than Inter, in a box sized for Inter. Whether PowerPoint keeps it on one line is left to the Windows check.
- **The packed file:** it holds `slides.md`, `talk.json`, the CSV, the five sources and their five builds, and `build/index.json`.

## Not exercised

- Anything at all on Windows. No Windows machine has run this code.
- `scripts/make-bundle.ps1` has never even been parsed, as this machine has no PowerShell. It's untested.
- Real PowerPoint. It hasn't opened a freeform shape or a chart, so whether the chart's lines and pie slices can be selected and recoloured is not known. The export has only been opened in LibreOffice.
- The form on screen for a component's arguments. It isn't built yet, and is plan D3. This plan delivers the schema and the fields a form will show.
- The Cargo file for a code editor. Its checks run only when it's written, so a planted `.cargo/config.toml` or `build.rs` that arrives later is not caught. The app and the guide say to use a code editor only on a talk you trust. *Set up for a code editor* wasn't used in this run.
- The native Save dialog, through Export and *Save as packed talk*.
- Building with a real GPU, and the processor cost of an animated component there.
- The line and pie charts in the app. Only the bar chart was shown in the run, though the tests draw all three.

## Windows and PowerPoint check, by hand

The `plan-c` branch isn't pushed yet. This check starts once it has been merged into `main` and pushed. It needs a Windows PC with Rust 1.98.0, the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`), the MSVC build tools and desktop PowerPoint, and a second Windows account with no Rust installed.

1. `git clone https://github.com/Eiriksb/stagepack-studio.git`, and work in that folder on `main`.
2. `cargo test --workspace`. If only golden tests fail, the images differ slightly from Linux, so send back which ones.
3. Build the bundle: `powershell -ExecutionPolicy Bypass -File scripts\make-bundle.ps1 -Out target\bundle`. This script has never run, so send back any error it gives, word for word. Then, in a Command Prompt in the same folder, `set STUDIO_BUNDLE=%CD%\target\bundle` and `cargo test --workspace -- --ignored`, and say whether those seven pass.
4. `cargo build --release -p stagepack-studio`. Make a folder such as `C:\StudioCheck`, and copy `target\release\stagepack-studio.exe` into it, and `target\bundle` into it as `C:\StudioCheck\bundle`. Copy `fixtures\components` to `C:\StudioCheck\components`.
5. Sign in to the account with no Rust. Check that `rustc --version` in a new Command Prompt says it's not recognised. Run `C:\StudioCheck\stagepack-studio.exe C:\StudioCheck\components`. The bar should offer *Build components*. Press it, and say how many seconds it takes for the scores on slide 2 to appear, or what the panel says instead. Then press F5, and Right through the talk: the three lines come one per click, the clock's hand turns, and the chart adds a bar per click. Press Escape.
6. In the same folder, make `C:\StudioCheck\charts\slides.md` holding:

   ```markdown
   # Lines

   <Chart kind="line" labels="Q1, Q2, Q3, Q4" values="120, 180, 150, 240" title="Sales" />

   ---

   # Slices

   <Chart kind="pie" labels="North, South, East" values="5, 3, 2" title="Share" />
   ```

   Open it with `stagepack-studio.exe C:\StudioCheck\charts`, and use Export, then PowerPoint, to write `charts.pptx`. Do the same for `C:\StudioCheck\components`, to write `components.pptx`.
7. Open both in desktop PowerPoint. Say whether PowerPoint offers to repair either file. In `charts.pptx`, click the line and say whether it can be selected on its own and given another colour, then do the same for one pie slice. In `components.pptx`, say whether the chart's bars are separate shapes that can be recoloured, whether the clock's hand shows, and whether the callout's title *Remember* is on one line.
