# Plan D1: what ran, and what to check by hand

The run was on the `plan-d1` branch, starting from commit 178099c, on a 16-core Linux laptop, with Rust 1.98.0, in a release build, on 2026-11-11. It found five things to fix, each fixed in its own commit with a test, and the run was repeated in part on the release build with the fixes, at bf56d1e. The tests were run at c285066. The branch hasn't been pushed or merged yet. Nothing here has run on Windows.

## Tests on Linux

At c285066, `scripts/cargo-safe.sh test --workspace` passed: 1,108 tests, 0 failed, 8 ignored, no warnings. `scripts/cargo-safe.sh clippy --workspace --all-targets` gave no warnings.

| Crate | Tests |
| --- | --- |
| slides-components | 64, and 5 ignored (sandbox 36, source_check 19, compile 4 and 4 ignored, examples 2 and 1 ignored, unit 3) |
| slides-engine | 398, and 1 ignored (builtins 85, packed 49, native 45, talk_components 42, chart 39 and 1 ignored, deck 39, flow 29, moments 15, timeline 13, files 12, canvas 11, render 11, text 8) |
| slides-format | 92 (parse 35, slide_ops 32, edit 18, split 7) |
| slides-pdf | 20 (pdf 16, unit 4) |
| slides-pptx | 108 (elements 36, write 32, parts 20, timing 10, fonts 9, unit 1) |
| stagepack-studio (app) | 380, and 2 ignored (ui_slides 80, document_state 53, ui_present 50, ui_editor 41, present 33, recovery 31, builds 26 and 2 ignored, saving 19, export 16, watcher 15, history 12, unit 3, ui_layout 1) |
| toolkit | 46 (component 24, interface_v1 20, doc 2) |
| toolkit-macros | 0 |

The eight ignored tests are the same as in Plan C: seven need the compiler bundle, and one writes pictures for a person to look at.

Two wall-clock tests in `slides-components` failed now and then with the whole workspace running in parallel on a loaded machine. `a_compile_that_fills_the_disk_is_stopped` took 28.5 s against a bound of 20 s, and `a_schema_that_never_returns_is_stopped` took 15.9 s against 2 s, most of that compiling the test's module. Each still asserts the outcome only its limit gives: the disk limit's message, and the clock's interrupt. Their bounds are now 50 s, under the compile's own 60 s time limit, and 30 s.

## Real-window check on Linux

The app was the release build, on a nested X display (`Xephyr :80`, 1280x800, no window manager), with `XDG_DATA_HOME` set to a scratch folder, so the working copies went there and not to the real data folder. The talks were scratch copies of `fixtures/showcase` and `fixtures/components` outside the repository, and a generated talk of 500 slides. Key presses, clicks, drags and the mouse wheel went in through the X test extension. Every result below is from a screenshot that was looked at. They are under `target/check/d1-run/`, which is not committed, and the last whole window is `target/check/d1-final.png`. The display draws in software, as the log says DRI3 is unavailable, so the timings say nothing about a real GPU.

### How it looks

The window reads as an editor of the PowerPoint kind: the File and Export menus and the title on top, the Home, Insert, Animate, Slide Show and View tabs under them, the slide list down the left, the slide on a grey desk, and the notes under it. Home has a *Slides* group (New slide with its list of five layouts, Duplicate, Delete, Hide slide) and a *History* group (Undo, Redo), each with its caption. Slide Show has *From the start* and *Rehearse*, and View toggles the slide list, the notes and the Markdown. Insert and Animate hold one line: *Tools for this tab are on the way.* They stay that way until plans D3 and D4.

What looked wrong, and was fixed:

- **The slide had no edge.** A white slide on a desk of 245 grey barely showed where it ended. The editor now draws a one-pixel line in the theme's border colour just outside the slide. A show draws none. (bf56d1e)
- **Slide numbers from 100 on broke over two lines**, *22* above *5*, as the number's column had room for two digits. It has room for three now, and a fourth would go into the padding. (1fff451)
- **On a hidden slide the button read *Show***, a tab away from *Slide Show*, and read like starting the show. The button and the list's menu now say *Hide slide* and *Show slide*. (55a1fa0)

What looks wrong and wasn't changed:

- **A new slide is blank.** The five layouts insert a slide whose title and text are empty, so a new *Title* slide and a new *Blank* slide look the same, white, in the list and on the desk. There is nothing to click and type into until plan D2, which brings typing on the slide. Placeholders such as *Click to add a title* belong with it.
- **A bar at the bottom shrinks the slide.** The recovery bar, the bar about a change from outside and the status line all take room from the slide, so it jumps in size when they come and go, as in Plan C.
- **The title sits against the Export menu**, rather than centred as in PowerPoint.
- **The list's menu shows Backspace for Delete**, where the Delete key works too.
- The window sits 2 pixels right of the display's edge, as there is no window manager. That's the display, not the app.

### What it did

On a scratch copy of the showcase, 7 slides:

- **Each layout, duplicate, hide, delete, drag and notes:** New slide's list added a Title, a Title and content, a Section, a Two columns and a Blank slide. Duplicate copied slide 1, Hide slide faded the copy in the list with a crossed eye beside its number, Delete took out an empty slide, slide 1 was dragged to between slides 4 and 5, with the blue line showing where it would land, and *Remember to smile.* was typed into its notes. The title said `• Showcase · 4 / 12`.
- **Undo and Redo:** ten Undos took the talk back to `Showcase · 1 / 7`, with no unsaved mark and Undo disabled. Ten Redos brought back all twelve slides, the hidden copy and the notes.
- **Save:** `git diff --no-index` of the original and the saved `slides.md` showed one hunk at the top, holding the new slides, `hidden: true` and the notes. Everything after it was the same.
- **Typed notes kept spaces:** in that diff the notes line ended in two spaces. The notes field sends its words after every key, and a space typed between two words was left outside the words the next key replaced, so it stayed at the end, and a new line left an indented blank line. Notes are now written trimmed, as the parser reads them, and the same typing saved `Say hi, then smile.` with nothing after it. (d13affd)
- **Killed with `kill -9`:** two slides were added and the app killed 2 s later. The talk on disk was byte for byte as saved. Opened again, a bar said *Changes from last time weren't saved*, with *Bring them back* and *Discard*. *Bring them back* gave the 14 slides, with the unsaved mark.
- **Changed from outside:** with those changes unsaved, a slide was appended to `slides.md` from a shell. Within 2 s a bar said *This talk was changed outside the app*, with *Keep my changes* and *Load the changed file*. *Keep my changes* and Save wrote the 14 slides, without the appended one.
- **Save As:** Ctrl+Shift+S opened the app's own dialog, *Keep the talk as a folder of files, or pack it into one file to send*, with *Cancel*, *Talk folder* and *Packed talk (.sptx)*. The system's Save dialog that follows can't be shown on the nested display, so it was cancelled there. The packed talk was written by what runs once the dialog answers, `Document::save_as_job(SaveTarget::Packed(..)).run()`, from a scratch test that was not committed. It held `slides.md`, `talk.json` and `assets/photo.png`.
- **The packed talk:** after closing the app, it was opened with the `.sptx` as its argument. It showed all 14 slides, the first hidden. Deleting slide 2 and Ctrl+S wrote the pack again, and the status line said *Saved showcase.sptx.* Unpacked, `slides.md` differed only by that slide's five lines, and the picture and `talk.json` were byte for byte the same.
- **A show skips hidden slides:** with slide 1 hidden, F5 showed slide 2. Left stayed on it, and Right went to slide 3.

On a scratch copy of the components example, the editor offered *Build components*, and after it the scores, the callout, the lines, the clock and the bars showed in the slide and in the list.

### A talk of 500 slides

A generated talk of 500 slides, each with a title, a list or a box, and notes, all five layouts in turn.

- **Opening:** the window, the first slide and the first six pictures in the list were all on screen in the first screenshot that found the window, 0.45 to 0.55 s after starting the app, three times on each build. The app then held 243 MB.
- **Scrolling the list:** 800 wheel clicks in 4.5 s went from the top to slide 500. Halfway down, the rows showed their numbers and grey frames, with their pictures drawn a moment later. At the bottom, the last six pictures were already drawn in the first screenshot. The app used 1.1 cores while scrolling. Afterwards it held 295 to 299 MB, and scrolling the whole list down and up twice more left it at 300 MB. It felt smooth, with the pictures catching up.
- **One Undo:** from the click on Undo after a Delete to the slide on screen changing took 0.15 to 0.21 s, measured by screenshots 35 ms apart, and a Delete the same. Clicking another slide in the list, which only draws a slide, took 0.11 to 0.18 s by the same measure, so most of it is drawing. In a scratch test of the talk alone, with no window, Delete took 16 to 34 ms and Undo 15 to 32 ms.
- **Idle:** with the list or the slide focused, over 20 s, the app used 0.007 to 0.008 of a core, and Xephyr none. With the notes focused it used 0.11 of a core, because the blinking caret drew the whole window twice a second, which in software drawing is expensive. The caret now holds still ten seconds after the last key, click or focus, as GTK's does, and starts again on the next. With the fix, the notes focused and left for 11 s, it used 0.008 of a core over the next 20 s. (83035b6) On the components talk, on a still slide and on the clock's slide, it used about 0.015 of a core.

## Not exercised

- **Anything at all on Windows.** No Windows machine has run this code.
- **Saving a packed talk on Windows** is reasoned and not tested. Writing the pack replaces a file the app has open. On NTFS on Windows 10 1809 or later this should work as on Linux. On FAT or exFAT drives, such as most USB sticks, Windows is expected to refuse, and Save then fails with a message, leaving the pack as it was and the talk unsaved. Save As to another drive still works.
- **The system's Save dialog**, behind Save As and Export. The nested display can't show it. Save As was run through the code behind the dialog instead.
- **A real second screen.** The show was on the one nested display.
- **A real GPU.** Every timing above is with software drawing.
- **Quitting the app with unsaved changes**, other than closing its window. The app has no Quit item, and quitting isn't intercepted.
- **A working copy's protection on Windows.** On Linux its folder and files are the user's alone. On Windows it relies on the user's own data folder.

## Windows check, by hand

The work is on the `plan-d1` branch, which hasn't been pushed or merged yet. Once it is pushed, this check starts from a clone of that branch; once it's merged, from `main`. It needs a Windows PC with Rust 1.98.0 and the MSVC build tools, Git for Windows, two screens if there are two, and a USB stick formatted FAT32 or exFAT.

1. `git clone https://github.com/Eiriksb/stagepack-studio.git`, then `cd stagepack-studio` and `git checkout plan-d1`.
2. `cargo test --workspace`. If only golden tests fail, the images differ slightly from Linux, so send back which ones.
3. `cargo build --release -p stagepack-studio`. Make `C:\StudioCheck`, copy `target\release\stagepack-studio.exe` into it, and copy `fixtures\showcase` to `C:\StudioCheck\showcase` and again to `C:\StudioCheck\original`.
4. Run `C:\StudioCheck\stagepack-studio.exe C:\StudioCheck\showcase`. Say whether anything looks cut off, cramped or out of line in the tabs, the slide list, the slide or the notes, and send a screenshot.
5. Use New slide's arrow to add one slide of each layout. Duplicate a slide, use Hide slide on one, Delete one, drag one to another place in the list, and type a sentence into the notes. Press Undo until it greys out, and check the talk is as it was. Press Redo until it greys out, and check everything is back. Press Ctrl+S. In a Command Prompt in `C:\StudioCheck`, run `git diff --no-index original\slides.md showcase\slides.md`, and send back what it prints. Only what was edited should differ.
6. Add a slide, wait 2 seconds, and end the app in Task Manager (*End task*). Start it again on the same folder. A bar should offer the changes back. Press *Bring them back*, and say whether the slide is there.
7. With that change unsaved, open `C:\StudioCheck\showcase\slides.md` in Notepad, add a line at the end, and save it. Say whether the app's bar asks about it, and what *Keep my changes* then Ctrl+S leaves in the file.
8. Press Ctrl+Shift+S, then *Packed talk (.sptx)*. This is the first time the system's Save dialog is used: say whether it shows, and save as `C:\StudioCheck\showcase.sptx`. Close the app, start it on `C:\StudioCheck\showcase.sptx`, delete a slide and press Ctrl+S. Say whether it saved, and whether the app opens the file again with that slide gone.
9. Copy `showcase.sptx` to the FAT32 or exFAT stick, open it from there, delete a slide and press Ctrl+S. Saving there is expected to fail: send back the message, word for word. Close the app and open the file on the stick again: it should be as it was before the failed save. Then use Save As to save it to `C:\StudioCheck`, and say whether that works.
10. Hide slide 2, then press F5. The show should go from slide 1 to slide 3. With two screens, Shift+F5 should put the audience on one and the presenter on the other. Press Escape.
11. In PowerShell, make a talk of 500 slides:

    ```powershell
    New-Item -ItemType Directory C:\StudioCheck\big
    (1..500 | ForEach-Object { "# Slide $_`n`n- Point one`n- Point two`n" }) -join "`n---`n`n" | Set-Content -Encoding utf8 C:\StudioCheck\big\slides.md
    ```

    This hasn't been tried, as the machine the plan ran on has no PowerShell. Open it, and say how many seconds it takes to show the slide list, and the app's memory in Task Manager. Scroll the list to the end fast, and say how it feels and the memory then. Delete a slide and press Undo, and say whether the wait is noticeable.
12. Click in the notes, then leave the app alone for 20 seconds. Say what Task Manager shows for its CPU.
