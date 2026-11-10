# Plan B: what ran, and how to check Windows and PowerPoint

Run on the `plan-b` branch at commit ecde000, on a 16-core Linux laptop, in debug builds, on 2026-11-10. Nothing here has run on Windows. The branch has not been pushed.

## Tests on Linux

`scripts/cargo-safe.sh test --workspace` passed: 573 tests, 0 failed, 0 ignored, no warnings.

| Crate | Tests |
| --- | --- |
| slides-engine | 234 (builtins 81, native 41, flow 29, packed 29, deck 18, timeline 13, render 11, text 8, files 4) |
| slides-format | 60 (parse 35, edit 18, split 7) |
| slides-pdf | 8 |
| slides-pptx | 86 (write 24, elements 23, parts 20, timing 10, fonts 9) |
| stagepack-studio (app) | 179 (ui_slides 54, ui_present 37, document_state 36, present 31, export 16, watcher 4, ui_layout 1) |
| toolkit | 6 |

## Real-window check on Linux

The talk was `fixtures/show`: three slides, with a `fade` entrance on a box (`click="1"`), `click=each` with `fly-up` on a list, a `fade` transition into slide 2, a `push` transition into slide 3, and notes on every slide. The app was a debug build, on a nested X display (`Xephyr :80`, 1280x800, no window manager), driven by XTest key events. A screenshot after every press showed the counter where it should be, so the run was clean. Screenshots are under `target/check/`, which is not committed.

- **Shift+F5 (rehearse):** opened two windows, "Audience" and "Presenter", each 640x800, side by side. The audience window showed the slide, letterboxed in black. The presenter window showed the current slide, the next step, the notes, a timer and a counter.
- **Press by press:** Right pressed six times went 1/3 (box on slide 1), 2/3 (slide 2, empty list), 2/3 (first point), 2/3 (second), 2/3 (third), 3/3. On each press the audience and presenter views matched. The presenter's next-step thumbnail was the box shown (slide 1), the list one item further, and so on. The last thumbnail read "End of the talk". The notes followed the slide. The timer ran, and the counter showed the slide, not the click.
- **Left** from the last slide returned to 2/3 with the whole list shown. Escape closed both present windows and left the editor window.
- **Fade entrance, mid-way:** 0.07 s screenshots, starting with the key press, caught the box at partial strength. The fill was (255, 251, 235) and then (254, 246, 213), on the way to its final (254, 243, 199), and the text was at about half strength (176, 179, 186, against 59, 69, 84 at the end). So the fill and the text fade together, below the atlas.
- **Fade transition, mid-way:** the audience and presenter views both showed slide 1 and slide 2 blended, in the same frame.
- **Push transition, mid-way:** the audience view showed the old slide partly slid off to the left and the new one partly in from the right.
- **Fly-up:** the list's points appeared one per click in the right places. A frame in the middle of the movement was not caught, so the flight itself was not seen.
- **F5 (present on one screen):** on this one-screen display it opened a single window, 1280x800 at the screen's corner, with the slide letterboxed in black, and no presenter window. Right showed the box. Escape closed it. With no window manager, this display can't show whether full screen took effect: the editor window was already the size of the screen. The sign that it worked is only that the black bars filled the screen.
- **Editing while the app ran:** moving the box in `slides.md` while the app was open updated the window with no restart.

## The exported files

`fixtures/show` was exported from a throwaway crate outside the repository, which used `slides-engine`, `slides-pdf` and `slides-pptx` only, the way the app's export does, to `show.pdf` and `show.pptx`.

- **PDF:** `pdfinfo` reports 3 pages of 960 x 540 pt. `pdftotext` finds every heading, bullet and box text, in order. All three slides rendered correctly as PNGs, with the box shown (a PDF has no clicks, so it holds the finished slide).
- **PPTX, in LibreOffice:** it opened with `-env:UserInstallation=file://<temp>` and converted to a PDF of 3 pages, with the same text and the same layout as the PDF, on all three slides. LibreOffice drew the text in a different sans font than the PDF did, and the bullets in blue where the PDF had black ones. That the file has the embedded fonts was not checked in LibreOffice.
- **PPTX, by reading its XML:** slide 1 has one click entrance (preset 10, fade). Slide 2 has three click entrances (preset 2, fly in, from the bottom) and `<p:fade/>` as its transition. Slide 3 has `<p:push dir="l"/>`. Each slide has notes in the right slide. The package has 4 embedded font files.
- LibreOffice was used only to open the file. Whether the animations play was not checked: it was not run as a slide show.

## Not exercised

- Two real screens. The nested display has one, so the two windows shared one screen and overlapped the editor.
- Full screen with a real window manager.
- Anything at all on Windows. No Windows machine has run this code.
- Real PowerPoint, including whether the exported animations and transitions play. The export has only been opened in LibreOffice.
- The native file dialogs: Open, Save, and the export's save dialog.
- Whether a push's first frame hitches on a real GPU. The nested display used software rendering (the log says DRI3 was unavailable), so this was not measured.

## Optional Windows and PowerPoint check, by hand

On a Windows PC with Rust 1.98.0, the MSVC build tools and desktop PowerPoint installed. The branch hasn't been pushed, so first push it, or copy the folder over, to reach that PC.

1. `git clone -b plan-b https://github.com/Eiriksb/stagepack-studio.git`
2. `cargo test --workspace`. If only golden tests fail, the images differ slightly from Linux, so send back which ones.
3. `cargo run --release -p stagepack-studio -- fixtures\show`. Press Shift+F5 with two screens connected and confirm the audience window is on one and the presenter window, with the notes, next slide, timer and counter, is on the other. Press Right through every click, and confirm the fade, the fly-up and the push play. Press Escape to end. Then press F5 and confirm the show fills the screen.
4. Use Export, then PowerPoint, to write `show.pptx`, and Export, then PDF, to write `show.pdf`. Open both.
5. In PowerPoint, check the three slides' fonts and shapes. Play the show from slide 1 and confirm the box fades in on the first click, the list's points fly in one per click, and the slides change with a fade and a push. Check the speaker notes. Tell me which animation, if any, doesn't play, or if PowerPoint offers to repair the file. That PowerPoint plays the animations has not been checked.
