# Plan D5: what ran, and what to check by hand

The run was on the `plan-d5` branch, starting from commit 4f59e8a, on a 16-core Linux laptop, with Rust 1.98.0, in a release build, on 2026-11-18. It found two things to fix in the window, each fixed with a test that failed before its fix, in 2eb57fb and b58b7d9: the start screen's tiles didn't line up, and on the *Tour* template's chart slide the words *Click to add text* lay over the chart. Both are under *What it found*, and both fixes were seen in the window on a release build with them. The random session's new part is in the same commit as this record. The branch hasn't been pushed or merged yet. Nothing here has run on Windows.

## Tests on Linux

At the run's last commit, `scripts/cargo-safe.sh test --workspace` passed: 2,257 tests, 0 failed, 19 ignored, no warnings. The run added two of them, a test for each fix. None of the timing tests that can fail on a loaded computer failed. `scripts/cargo-safe.sh clippy --workspace --all-targets` gave no warnings.

| Crate | Tests |
| --- | --- |
| slides-components | 64, and 5 ignored |
| slides-engine | 512, and 2 ignored (packed 66, files 20 and flow 31 among them, with D5's new talk held in memory and written out whole) |
| slides-format | 328 (layouts 3 new in D5) |
| slides-pdf | 21 |
| slides-pptx | 109 |
| stagepack-studio (app) | 1,178, and 12 ignored (ui_typing 104, ui_slides 85, document_state 67, ui_insert 64, recovery 50, ui_present 50, ui_editor 48, ui_select 47, clipboard 40, saving 37, ui_inspector 37, ui_start 36, present 33, ui_animate 29, arrange 27, builds 26 and 2 ignored, ui_tables 26, ui_arrange_select 25, ui_move 25, ui_resize 25, ui_arrange 24, ui_lift 24, added_files 22, unit 21, ui_palette 20, ui_untitled 17, ui_layouts 16, export 16, history 16, watcher 15, animate 14, templates 14, ui_icons 14, ui_snap 14, save_flow 13, untitled 12, snap 11, edit_panics 3, typing_random 3, typing_bytes 2, ui_layout 2, arrange_place_props 1, arrange_select_props 1, icons 1, edit_speed 10 ignored) |
| toolkit | 45 |
| toolkit-macros | 0 |

Of the 19 ignored tests, seven need the compiler bundle and weren't run, one writes pictures for a person to look at, and eleven are the release timing tests, ten of them below.

The random editing session, `crates/app/tests/typing_random.rs`, still has its three parts: typing, arranging, and the window. The window's part now has four starts, each with five seeds: the two hand-written talks it had, and twice a start on the start screen, in a window whose working copies, build permissions and Documents folder are in a scratch folder. There a template is picked at random by clicking its tile, the window is made the active one, and the same 70 presses, clicks and keys follow. At a step picked at random, the new presentation is saved for the first time: Escape, Ctrl+S, the packed or the folder button at random, and the Save dialog answered with a file in the scratch Documents folder or a new folder beside it; the talk must then be neither untitled nor unsaved, and the steps go on. After every step the checks are as before: while the list of slides or the notes have the keyboard, nothing on the slide is selected, typed in or waiting as a new text box, and every slide still reads. At the end the talk is saved again if it has changed, and what's at the place it was saved to, opened again from the disk, has the same text as the window; then Undo all the way gives the first bytes, the template's for a new presentation. A check of the check, the comparison made to fail on purpose, failed at seed 3 on the *Ocean* template saved packed, so the new part does run.

`PROPTEST_CASES=1000 scripts/cargo-safe.sh test -p stagepack-studio --test typing_random` passed in 15 minutes: the arranging with 1,002 seeds (334 a talk), the typing with five seeds a talk, and the window with five seeds for each of its four starts. It found nothing. It ran before the two fixes; the whole workspace, with the random session at its default seeds, ran after them.

The release timing tests, `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`, three times, on the 500-slide talk, each timing the work up to the talk's text and layout being changed, before the slide is drawn:

| What | Median | Slowest |
| --- | --- | --- |
| A frame of a drag of a slide's sixty things | 0.15 to 0.18 ms | 1.1 to 1.3 ms |
| The same, every frame snapping | 0.18 ms | 0.47 to 1.55 ms |
| The same, on the Animate tab, with its step badges | 0.34 to 0.40 ms | 0.46 to 2.5 ms |
| A keystroke on a slide | 2.56 to 2.80 ms | 7.5 to 9.0 ms |
| A keystroke on the Animate tab | 2.47 to 2.58 ms | 6.5 to 12.2 ms |
| A keystroke with three pictures waiting to be saved | 2.46 to 2.60 ms | 5.7 to 8.4 ms |
| A keystroke in the notes | 1.01 to 1.06 ms | 6.3 to 10.4 ms |
| A keystroke in a table cell | 5.2 to 6.3 ms | 12.7 to 18.1 ms |
| *Apply to all slides*, a transition on all 500 | 62 to 67 ms | 71 to 90 ms |
| Delete of sixty things at once | 97 to 109 ms | 122 to 146 ms |
| Duplicate of sixty things | 91 to 101 ms | 105 to 128 ms |
| Paste of sixty things | 94 to 99 ms | 108 to 138 ms |

The plan's budgets are 6 ms for a frame of a drag and 8 ms for a keystroke, on the median; every median is within them; the slowest single keystrokes, up to 18 ms in a table cell, are over the budget, as in D4. Nothing in this plan changes those paths. The release build took 72 seconds from a warm cache, and 20 seconds after the two fixes; the executable is 115 MB.

## The scratch display, and the owner's data

The run used a nested X display, `Xephyr :85` at 1,600 by 900, with no window manager. Every launch went through one script that refuses to start unless that display and a scratch message bus (below) are running, and starts the app with `DISPLAY=:85`, `WAYLAND_DISPLAY` and `STUDIO_BUNDLE` taken away, `HOME` set to a scratch home under `target/check/d5-run/home`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `XDG_DOCUMENTS_DIR` set to folders in it, and a `Documents` folder there named in its `user-dirs.dirs`. So the working copies, the build permissions, the caches and the Documents folder the app suggests were all scratch ones. The app never showed on the desktop's own display.

The real data folders, `~/.local/share/stagepack-studio` and `~/.cache/stagepack-studio`, were listed with every file's time and size before the first launch and again after the last: the two lists, of 25,134 entries each, are the same, and nothing in them is newer than 2026-11-09. The modification times of the two folders and of the real `~/Documents` are the same before and after, and so is the list of what's directly in `~/Documents`.

The system's file dialogs go through the desktop's portal, which would show them on the real desktop. So the app was given a message bus of its own, with nothing on it but a stand-in for the portal's file chooser, a small Python program written for the run: it writes down each Open or Save request, with the folder and name the app suggested, and answers it with a path read from a file, or as cancelled. Open, Save As, the close prompt's *Save* and Export were all pressed in the window and ran the app's own code from start to end; only the system's dialog itself was replaced. What the real dialogs look like, and whether they start where the app asks, is in both hand checks.

Keys, clicks and drags went in through the X test extension from D4's small Python program, and a picture was given to the clipboard as `image/png` by D3's program. Windows were raised, given the keyboard and closed (as a window manager's close button does) by small programs written for the run. Screenshots are under `target/check/d5-run/`, which is not committed; every one was looked at. The display draws in software, so the timings say nothing about a real GPU. The exported PDF was drawn with `pdftoppm`, and the PowerPoint file was turned into a PDF by LibreOffice, run with a scratch profile, and drawn the same way.

## Real-window check on Linux

### What it did

In two sessions of the app, with a crash between them:

- **Started with no talk:** a window titled *Stagepack Studio* showed in 0.1 seconds (three starts), with the tiles' pictures drawn within 0.4 seconds. *New presentation* over five tiles, *Light*, *Dark*, *Warm*, *Ocean* and *Tour*, each a picture of its first slide with its name and a line under it (*White, with dark text*, *Deep navy, with light text*, *Cream, with dark brown text*, *Deep blue, with white text and gold*, *A short talk that shows what a slide can hold*), then *Open a talk file…* and *Open a talk folder…*. The first tile had the keyboard's ring; Right moved it to *Dark*. The pointer over a tile lights it. The Export menu's two items are greyed and do nothing.
- **A talk from each template:** a click on *Light* showed *Untitled presentation · 1 / 1* in the same window, its one slide with *Click to add title* and *Click to add text*, Save on, Undo off. Ctrl+N opened a start screen in a new window each time; *Dark*, *Warm* and *Ocean* became *Untitled presentation 2*, *3* and *4*, and *Tour* *5*, with 8 slides. A title typed on each showed a dot before the name. On *Dark* the caret is white, *Click to add text* a muted blue-grey, a selection of words a muted blue, a new rectangle's outline and handles white, and the step badge a white square with its number; on *Ocean* the caret and the dashed frame of the area being typed in are white. Each stands out from its slide.
- **The tour:** all eight slides, stepped through with Page Down: a title, a list, two columns, a picture and a shape, a table, a chart, a section slide, and how to save and present. F5 presented it from the start: slide 2 came with its list hidden and showed a point a click; the right arrow went on through the eight, staying on the last.
- **A picture, a packed save, closed and opened again:** on a new tour, a picture pasted with the Paste tool landed in the middle, selected. Ctrl+S asked *Keep the talk as a folder of files, or pack it into one file to send.* with *Talk folder* and *Packed talk (.sptx)*; *Packed* asked the dialog for `Untitled presentation 2.sptx` in the scratch Documents folder, the stand-in answered `Tour.sptx`, and the title became *Tour · 1 / 8* with Save off and *Saved Tour.sptx.* on the status line. Closed, it went without asking. A new start screen's *Open a talk file…* asked for one file, and `Tour.sptx` opened in that window with the picture where it was.
- **A crash, and the talks brought back:** with *Light*, *Dark*, *Warm* and *Ocean* edited and *Tour* not, the app was ended with `kill -9` and started again the same way. The start screen listed four under *Not saved last time*, newest first: *Untitled presentation 4* (*An ocean talk*), *3* (*A warm talk*), *2* (*A dark talk*) and *Untitled presentation* (*A light talk*), each *1 slide*, *Today 12:09* and so on, with *Open* and *Discard*. The tour, never edited, wasn't listed.
- **Discarded:** *Discard* on the warm one asked *Discard this presentation? It can't be brought back.* with *Cancel* and *Discard*; *Discard* took it off the list, and its working copy was gone from the scratch data folder.
- **Brought back:** *Open* on the dark one showed it in that window, unsaved, with its title, its rectangle and its *Fade in*, as *Untitled presentation · 1 / 1*, the lowest number free.
- **Two copies of the app:** with the dark one open and edited in the first copy, a second copy started on the same display listed only the ocean and the light ones. Ended, it left the first's working copy alone.
- **The success test's ten slides, from *Light*:** a title slide (*Our first year*); a list of three points shown one at a time, with *Appear* and *One at a time* on the Animate tab and a *Fade* transition; a pasted picture; a rectangle, an ellipse and an arrow from Shapes, dragged apart, the rectangle on click 1 with *Appear* and the ellipse on click 2 with *Fly up*; a 3 by 4 table typed in with Tab; a two-column slide; a section slide; a numbered list; a slide with words and an icon found by searching *heart*; and a closing title slide. *From the start* on the Slide Show tab presented it: the list a point a click, the rectangle on the first click and the ellipse on the second, the arrow there from the start, every slide in its place, ten in all.
- **Saved as a folder:** Ctrl+S, *Talk folder*: the dialog was asked for *Untitled presentation 2* in Documents and answered with a new folder, `Our first year`. It holds `slides.md` and `assets/pasted-picture.png`, and nothing else; the title became *Our first year · 7 / 10*, and the build permissions file holds that folder under *always build*. The text is the ten slides as typed, each thing with its place, click and entrance.
- **Exported:** *PowerPoint…* and *PDF…* from the Export menu each asked the dialog for *Our first year* with their ending, in Documents, and wrote the files, with *Exported Our first year.pptx.* on the status line. The PDF has 10 pages of 960 by 540 points, each the slide as it ends. The PowerPoint file, drawn by LibreOffice, shows the same ten slides in its own font; read from the file, slide 2 has a fade transition and three click entrances, slide 4 an appear and a fly-in, and slide 5 a real table.
- **The close prompt on an untitled talk:** the brought-back dark one, edited, asked *Save before closing?* with *Cancel*, *Don't save* and *Save*. *Save*, then *Packed*, asked the dialog for `Untitled presentation.sptx`; cancelled, the window stayed, with *The presentation wasn't saved, so its window was left open.* Asked again, *Don't save* closed it and took its working copy away. The saved talk's window then closed without asking, and the app ended with its last window.
- **Idle:** with two windows open and nothing selected, the app used 0.009 of a core over 20 seconds, and 335 MB.

### What it found

- **The tiles didn't line up.** A line under a tile that takes two lines (*Ocean*'s and *Tour*'s) pushed the other three tiles down by half a line, so the row's pictures and names were at two heights. The row now lines its tiles up at the top. Test: `the_tiles_line_up_at_the_top` in `crates/app/tests/ui_start.rs`. (2eb57fb) Seen again in the window after the fix.
- **On the tour's chart slide, *Click to add text* lay over the chart.** The slide's main text area was empty, and the chart was placed over it, so the editor drew the offer to add text at the chart's top left, half under it. The slide now says *Click the chart, then change its numbers in the panel beside the slide.*, and the chart sits below that line. Test: `no_offer_to_add_text_lies_under_a_thing_on_the_tour` in `crates/app/tests/templates.rs`; the slide's golden picture was written again and looked at. (b58b7d9) Seen again in the window's list of slides after the fix.

### What looks wrong and wasn't changed

- **Four of the five tiles are plain colour.** The colour templates' slide is an empty title slide, and a picture of it has no words, so *Light*'s tile is a white box and the others a panel of colour; only the name says what it is.
- **A colour from the talk's theme is called *None*.** A rectangle on *Dark* is drawn in the accent colour and an arrow on *Light* has a dark outline 6 wide, but the panel says *None* beside each swatch.
- **With the caret in an empty, centred area, *Click to add text* stays**, and the caret sits in the middle of the words, as in *Click to |add text*, until a letter is typed.
- **Words a person sees that aren't plain:** the icon's form names it *location-heart*; a picture's form shows its file's name, *photo.png* or *pasted-picture.png*; the Save As choice says *Packed talk (.sptx)*.
- **A brought-back presentation takes the lowest number free**, so *Untitled presentation 2* before the crash came back as *Untitled presentation*, while another copy's start screen lists the others by the names they had (*Untitled presentation 4*).
- **The question before discarding doesn't name the presentation**: *Discard this presentation?* only.
- **The File menu has no *Close* or *Quit***; a window is closed by its own close button.
- **When a list is shown one at a time, the Order group's line reads *One at a time***, the same words as the List group's button beside it.
- **A file is written with the name given**: an export answered with a name ending in `.pptx` from the PDF item wrote a PDF with that name. The real dialogs may hold the ending; the hand checks ask.

## Not exercised

- **Anything at all on Windows.**
- **The system's real dialogs**: Open, Save As, Export and *Picture*. The app's side of each was run against a stand-in that answered for the dialog; *Picture* wasn't pressed, and pictures came by the clipboard.
- **A computer with no Documents folder**, where Save As and Export should suggest the home folder. Covered by headless tests only.
- **An installer, a double-click on a talk file, and the start menu**: plan E.
- **Wayland, a real GPU, scaling above 100 %, two screens, a touch screen, and a real mouse.**
- **Desktop PowerPoint**: the PowerPoint file was drawn by LibreOffice, which doesn't play the animations; that they're there was read from the file.
- **A talk with components made from a template and saved as a folder, then built**: that such a folder is set to *Always build* was seen in the permissions file; the build itself is covered by headless tests only.
- **The compiler bundle's seven tests**, not run.

## Windows check, by hand

The branch hasn't been pushed or merged yet. Once it's in `main`, this check starts from a clone of `main`, on a Windows PC with Rust 1.98.0 and the MSVC build tools, Git for Windows, desktop PowerPoint, and the bundle as in `docs/checks/plan-c.md`.

1. `git clone https://github.com/Eiriksb/stagepack-studio.git`, then `cd stagepack-studio`, then `cargo test --workspace`. If only golden tests fail, send back which ones.
2. `cargo build --release -p stagepack-studio`. Make `C:\StudioCheck`, copy `target\release\stagepack-studio.exe` into it and the bundle in as `C:\StudioCheck\bundle`.
3. Double-click `C:\StudioCheck\stagepack-studio.exe`. Send a screenshot of the start screen. Say whether the five tiles line up, whether each shows its picture, and whether the words under them are whole.
4. Pick *Dark* and type a title. Say whether the caret, the dashed frames and *Click to add text* can be seen. Insert a rectangle: say whether its outline and handles can be seen. Ctrl+N, pick *Ocean*, and do the same.
5. Ctrl+N, pick *Tour*, step through its eight slides and press F5. Press *Picture* on the Insert tab: say whether the system's Open dialog shows. Pick a photo.
6. Ctrl+S. Choose *Packed talk*: say whether the system's Save dialog shows, whether it starts in your Documents folder, and whether the name it offers is *Untitled presentation*, with a number when more than one is open, ending in `.sptx`. Save it as `C:\StudioCheck\Tour.sptx`. Close the window, Ctrl+N from another, and open `Tour.sptx` with *Open a talk file…*: the photo should be there.
7. Pick *Warm*, type a title, wait five seconds, and end the app from Task Manager (*End task* on its process). Start it again: the start screen should list it under *Not saved last time* with its words and the time. *Open* it, then close it with *Don't save*, and start the app again: it shouldn't be listed. Make another, crash it the same way, and *Discard* it from the list.
8. Start a second copy of the app while the first has an unsaved new presentation open and edited: the second's start screen shouldn't list it.
9. From *Light*, build ten slides with text, a picture, three shapes, a table and click animations on a list and on two shapes. Present it, save it as a talk folder under `C:\StudioCheck`, and export to PowerPoint and PDF. Open both. In desktop PowerPoint, play it: say whether the list comes a point a click and the shapes on clicks one and two, whether the table is a table, and whether anything is missing or out of place.
10. Close a new presentation with edits and press *Save*, then *Cancel* in the system dialog: the window should stay and say why.
11. On a Windows account whose Documents folder has been moved (to OneDrive, say), say where Save As and Export start.
12. D4's Windows check, steps 3 to 15 of `docs/checks/plan-d4.md`, where they're still open.

## Linux check, by hand

Each item under both an X11 session and a Wayland session.

1. The start screen, its five tiles and their pictures, at 100 % and 200 % scaling.
2. The real Save dialog from Ctrl+S on a new presentation, from the close prompt's *Save*, and from Export: say whether it starts in Documents with the name *Untitled presentation*, and whether the packed form's ending is kept when the name is changed. The Open dialog from *Open a talk file…* and *Open a talk folder…*.
3. On an account with no Documents folder (`xdg-user-dirs` pointing at nothing), say where Save As and Export start.
4. A crash before the first save, by `kill -9`, and the talk brought back from *Not saved last time*.
5. Present the ten-slide talk on a second screen with the presenter view.
6. The PowerPoint file in LibreOffice Impress: slide 2's transition and its animations in the animation panel.
7. D4's Linux check, items 1 to 7 of `docs/checks/plan-d4.md`.

## The design's first success test

The design's first test for the first public release is someone with no coding knowledge. Line by line, at the end of this plan:

- **They download one installer and run it.** Not yet. There is no installer: the app is built from the source with Rust, and is one executable of 115 MB that opens, edits, presents, saves and exports without the compiler bundle, as this run showed with the bundle taken away. Plan E owes the installers for Windows and Linux, per user and with no administrator rights, with the bundle, fonts, icons and licences in them, and a way to start the app that isn't a terminal.
- **They choose *New presentation* or double-click a talk.** *New presentation* now holds, and was seen in the window: the app starts on a start screen with five templates, and a click on one gives a presentation ready to type in, with nothing asked first. Ctrl+N and *New presentation…* in the File menu do the same in a new window. Opening a talk holds from the start screen's two buttons and the File menu. A double-click on a talk file doesn't yet: plan E owes registering `.sptx` with the system so that a double-click opens it in the app.
- **They build a ten-slide talk with text, pictures, shapes, a table and click animations.** Holds, and was seen in the window from the *Light* template, as above. The slip that showed: a click a few pixels outside a layout's area typed nothing and said nothing, so a line meant for the section slide was lost until it was typed again.
- **They present it, save it, and export it to PowerPoint for a colleague.** Holds, and was seen in the window: presented from the start with every click, saved for the first time as a folder (and, for the tour, packed), and exported to PowerPoint and PDF through the app's own Export, with the system's dialog answered by a stand-in. Whether the PowerPoint file plays its animations in desktop PowerPoint is in the Windows check.
- **They never see Markdown, Rust, a terminal or a missing-dependency error.** Holds on this path, but for the terminal: nothing in the run showed Markdown or Rust unless asked for (Ctrl+Shift+M still shows the talk's Markdown), and no message asked for anything to be installed, with the compiler bundle taken away. The few words that aren't plain are under *What looks wrong and wasn't changed*. The app was started from a terminal; plan E owes the installed app's own launcher.

So of the five lines, three hold and were seen to hold in a real window on Linux; the installer, the double-click and the launcher are plan E's; and none of it has been seen on Windows.

The coder's test, briefly: opening a folder and editing `slides.md` in an editor, with the slide updating on save, has held since plan B, and the start screen now opens a folder too. Writing a component from the guide, seeing it within seconds and its compile errors with file and line has held since plan C, with the bundle; a folder saved from the app starts with *Always build this folder* on. Handing the talk to someone without Rust holds through the `build` folder and the packed talk, and placing the component from the palette with its form through plan D3. Plan E owes the bundled compiler in the installer, so that a coder who installs the app doesn't fetch it separately, and running both tests from the installers.

## Known after the run

Found in this run and not changed are under *What looks wrong and wasn't changed*. These are known from the plan's tasks, and weren't changed:

- **Not in this plan:** changing a talk's colours or fonts after it's made; templates from a file or the internet; saving a talk as a template; presenting from the current slide; installers and the file type.
- **The Home tab doesn't fit at 1,280 pixels**, the window's width when it opens; the tools past *Hide slide* are behind the arrow.
- **On a white slide, selection outlines and snapping lines were near-black.** (Softened after the final review, in ef6a8f8: a mark is darkened or lightened only as far as it takes to stand out.)
- **A talk opened by its path that couldn't be opened** showed technical words on the window's overlay. (Fixed after the final review, in 49ca3e0: it says the start screen's plain sentence.)
- **D4's known issues**, in `docs/checks/plan-d4.md`, still stand.

## The final review

A reviewer read the whole branch at 4c170c6 in a copy of its own, ran the plan's six focused suites (142 tests), the engine's pack-and-folder property at 3,000 cases, and the random window session on twelve new seeds for each of its four starts, and wrote three throwaway tests to prove what it suspected. It traced the life of an untitled presentation for lost work: the order of the write, the talk being marked saved, its working copy being cleared and the window closing; edits made while a save runs; a save that fails; the app killed in the middle of a save; a presentation brought back; two windows; two copies of the app. It found no way to lose a person's edits, nothing read or written outside the talk and the app's own data folder, and nothing compiled without a yes. It asked for four fixes, and the one fix wave made them:

- **A template's picture could be lost for good.** The Tour's picture deleted, then a first save, then Undo: the line came back naming a file that had never been written anywhere. A template's file the first save didn't write is now kept with the talk, so Undo finds it and the next save that uses it writes it. (7f6cf8b)
- **A picture added and taken back within two seconds left a folder holding only that picture**, which later start screens counted as *1 couldn't be read.* with nothing to press. Such a folder is now cleared when the window closes, isn't counted, and is removed at the next start if a crash left it. (d26ea30)
- **A talk opened by its path that couldn't be opened showed file names** on the window. It says the start screen's plain sentence. (49ca3e0)
- **Marks on a white slide were near-black.** A mark that doesn't stand out is now darkened or lightened only as far as it must be. (ef6a8f8)

The wave also made a copy left behind by a crash during a bring-back go with the one brought back, took line breaks out of listed names, and kept the pack-and-folder test from failing by chance on a reserved file name. (4367996)

At 4367996, `scripts/cargo-safe.sh test --workspace` passed: 2,264 tests, 0 failed, 19 ignored, and clippy gave no warnings. The random session at 500 cases passed, and the release timing tests passed, with a drag frame at 0.18 to 0.39 ms and the slowest keystroke, in a table cell, at 4.5 ms at the median. A second reviewer then checked the wave, ran the five suites it touched, and found every finding met and nothing new broken. The window wasn't run again after the wave, so its fixes have been seen in tests only.

Known after the final review, and not changed:

- **Between a working copy's folder being made and its lock being taken, or on a disk that can't lock files, a second running copy of the app starting at that instant can remove a folder that holds no text yet.** Nothing a person could get back is in it, and the window's next keep writes it whole again.
- **A component pasted into a folder that is already always built is built without a further yes**, as the design has it for such folders. It should be settled before the compiler ships in an installer.
- **Working copies over 30 days old are removed at launch**, never-saved presentations among them, with nothing said.
- **The Home tab doesn't fit at 1,280 pixels**, the width the window opens at.
- The smaller findings and the notes for plan E are in `docs/superpowers/plans/2026-11-18-native-rust-plan-d5-decisions.md`.

