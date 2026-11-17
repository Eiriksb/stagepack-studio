# Plan D4: what ran, and what to check by hand

The run was on the `plan-d4` branch, starting from commit 737dfc3, on a 16-core Linux laptop, with Rust 1.98.0, in a release build, on 2026-11-17. It found two things to fix, each fixed with a test that failed before its fix, in a49c415 and 81f4d4a; the random session's new parts are 3c3eaa7. The worse of them edited the wrong slide: with a layout's area selected, Page Down to the next slide, nothing shown as selected there, and *Align right* from the *Arrange* menu moved that slide's title over its list. The other made a table read as a paragraph after notes were typed. Both are under *What it found*. The fix to the first was seen again in the window on a release build with it. The branch hasn't been pushed or merged yet. Nothing here has run on Windows.

## Tests on Linux

At the run's last commit, `scripts/cargo-safe.sh test --workspace` passed: 2,132 tests, 0 failed, 19 ignored, no warnings. The run added four of them: the two new parts of the random session, and a test for each fix. None of the timing tests that can fail on a loaded computer failed. `scripts/cargo-safe.sh clippy --workspace --all-targets` gave no warnings.

| Crate | Tests |
| --- | --- |
| slides-components | 64, and 5 ignored (sandbox 36, source_check 19, compile 4 and 4 ignored, unit 3, examples 2 and 1 ignored) |
| slides-engine | 500, and 2 ignored (builtins 89, packed 58, text_geometry 57 and 1 ignored, deck 55, talk_components 46, native 45, chart 39 and 1 ignored, flow 30, files 17, moments 16, timeline 16, canvas 11, render 11, text 9, unit 1) |
| slides-format | 325 (rich_edit 58, rich_write 45, slide_ops 43, parse 36, rich_read 35, element_ops 32, arrange_ops 21, edit 18, table_ops 18, split 13, reparse 6) |
| slides-pdf | 21 (pdf 17, unit 4) |
| slides-pptx | 109 (elements 36, write 33, parts 20, timing 10, fonts 9, unit 1) |
| stagepack-studio (app) | 1,068, and 12 ignored (ui_typing 104, ui_slides 85, document_state 67, ui_insert 64, ui_present 50, ui_editor 48, ui_select 47, clipboard 40, recovery 40, saving 37, ui_inspector 37, present 33, arrange 27, builds 26 and 2 ignored, ui_tables 26, ui_animate 25, ui_arrange_select 25, ui_arrange 24, ui_move 24, added_files 22, unit 21, ui_resize 21, ui_lift 20, ui_palette 20, export 16, history 16, watcher 15, animate 14, ui_icons 14, ui_layouts 14, ui_snap 14, snap 11, save_flow 8, edit_panics 3, typing_random 3, typing_bytes 2, ui_layout 2, arrange_place_props 1, arrange_select_props 1, icons 1, edit_speed 10 ignored) |
| toolkit | 45 (component 24, interface_v1 20, doc 1) |
| toolkit-macros | 0 |

Of the 19 ignored tests, seven need the compiler bundle and weren't run, one writes pictures for a person to look at, and eleven are the release timing tests, ten of them below.

The random editing session, `crates/app/tests/typing_random.rs`, has three parts now:

- **Typing**, as before: 250 steps of typing, styles, lists, Undo and Redo on each of eight hand-written talks, five seeds each.
- **Arranging**, new: 160 steps on each of three hand-written talks of placed things written every way by hand (single quotes, no quotes, a tag over several lines, a turn of -360, 359.9 and 37, a thing 1 unit high and one 3,840 wide, `click="x"`, a step of 9 with gaps before it, a CRLF talk, areas placed by a slide's settings), with a table, a picture and a list in the text. Each step is one of: go to a slide, pick a thing or an area, pick one more, pick every thing, let go, a drag, a run of arrow keys, a handle dragged, the turn handle dragged, align, space out, bring forward or send back, an Animate choice, an entrance, a transition on one slide or all, a thing lifted out of the text, a layout changed, a drag shown and let go of with Escape, a letter typed in an area, typing ended, Undo, Redo. After each it checks that a place written reads back as the place asked for (position and size to a whole unit, turn to a tenth of a degree), that no other slide's bytes changed, that a refused step changed neither the talk nor the history, that no slide stopped being readable, and that the typing model and the talk agree. At the end, Undo all the way gives the first bytes and Redo all the way the last.
- **The window**, new: 70 presses, clicks and keys in a headless window on each of two talks, five seeds each: a click on a thing with or without Shift or Ctrl, a double click, a drag to anywhere, a right-click and Escape, a click anywhere on the slide, letters, the arrow keys, Escape, Delete and Backspace, Undo and Redo, a click on a row of the list of slides, a row pressed and moved 7 pixels, a right-click on a row, a click in the notes, Ctrl+A, Tab, Page Up and Down, a new text box, Ctrl+D. After each, if the list of slides or the notes have the keyboard, nothing on the slide may be selected, typed in or waiting as a new text box, and every slide must still read. At the end, Undo all the way gives the first bytes.

`PROPTEST_CASES=2000 scripts/cargo-safe.sh test -p stagepack-studio --test typing_random` ran the arranging with 2,001 seeds (667 a talk), about 320,000 steps, and passed in 24 minutes. Its first run, with the default five seeds, failed at seed 1, step 125: an area dragged on one slide was written on another (see *What it found*).

The release timing tests, `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`, three times, on the 500-slide talk, each timing the work up to the talk's text and layout being changed, before the slide is drawn:

| What | Median | Slowest |
| --- | --- | --- |
| A frame of a drag of a slide's sixty things | 0.14 to 0.17 ms | 0.17 to 0.70 ms |
| The same, every frame snapping | 0.13 to 0.14 ms | 0.21 to 0.54 ms |
| The same, on the Animate tab, with its step badges | 0.39 to 0.41 ms | 0.56 to 0.78 ms |
| A keystroke on a slide | 2.41 to 2.47 ms | 6.1 to 6.6 ms |
| A keystroke on the Animate tab | 2.31 to 2.44 ms | 5.1 to 6.6 ms |
| A keystroke with three pictures waiting to be saved | 2.40 to 2.50 ms | 4.6 to 6.4 ms |
| A keystroke in the notes | 0.93 to 1.06 ms | 4.9 to 5.9 ms |
| A keystroke in a table cell | 4.5 to 5.8 ms | 14.7 to 24.1 ms |
| *Apply to all slides*, a transition on all 500 | 59 to 66 ms | 82 to 87 ms |
| Delete of sixty things at once | 81 to 115 ms | 131 to 164 ms |
| Duplicate of sixty things | 69 to 81 ms | 96 to 130 ms |
| Paste of sixty things | 78 to 82 ms | 83 to 130 ms |

The plan's budgets are 6 ms for a frame of a drag and 8 ms for a keystroke, on the median; every median is well within them. The release build took 47 seconds from a warm cache, and the executable is 111 MB.

## The scratch display, and the owner's data

The run used a nested X display, `Xephyr :84` at 1,600 by 900, with no window manager. Every launch went through one script that refuses to start unless that display is running, and starts the app with `DISPLAY=:84`, `WAYLAND_DISPLAY` taken away, and `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` set to folders under `target/check/d4-run/`, so the working copies, caches and settings went there. The app never showed on the desktop's own display. The talk was a scratch copy of `fixtures/showcase` under `target/check/d4-run/talks/`. The real data folders, `~/.local/share/stagepack-studio` and `~/.cache/stagepack-studio`, were listed with every file's time before the first launch and again after the last: the two lists are the same, and nothing in them is newer than 2026-11-09. The display draws in software, so the timings say nothing about a real GPU.

Keys, clicks, drags and window sizes went in through the X test extension from small programs written for the run (D3's, and a Python one with a key held down across a drag). A picture was given to the clipboard as `image/png` by D3's program. The pointer's shape was read from the display's cursor (the X fixes extension), as screenshots of the display don't show it. Screenshots are under `target/check/d4-run/`, which is not committed; every one was looked at. The system's dialogs can't be shown on the nested display, so Open, Save As and Export weren't pressed: the talk was exported with `run_export`, the code behind the Export menu, from a scratch test that was not committed, and the files were drawn with `pdftoppm` and, for PowerPoint, LibreOffice.

## Real-window check on Linux

### What it did

On the scratch showcase, 7 slides, in one session:

- **A slide built:** *New slide*'s caret opened five tiles, *Title*, *Title and content*, *Two columns*, *Section* and *Blank*, each drawing its areas; *Title and content* added slide 6 with *Click to add title* and *Click to add text*. *Arrange and animate* typed in the title. A picture pasted with the Paste tool landed in the middle, 400 by 300, selected, with eight square handles on its corners and edges and a round one above it. *Rectangle*, *Ellipse* and *Rounded rectangle* from Shapes landed stepped aside from each other. A 3 by 3 table from the grid filled with Tab; a Tab in the last cell added an empty row.
- **Dragged, resized and turned, each of the four:** a drag moved the thing under the pointer, and the panel's *Left* and *Top* changed when it was let go (not during the drag). While dragging, thin lines showed what it snapped to: an edge of another thing, and the slide's middle across and down. A corner handle resized from the corner across, which stayed put; the picture's corner kept its 4 by 3 (400 by 300 to 516 by 387) and the shapes' didn't. An edge handle changed one side. The turn handle turned the thing in whole degrees (58, 30, 16); with Shift pressed during the turn, in steps of 15 (60). The outline and the handles followed each turned thing, on it.
- **Snapped to the middle:** the ellipse dragged near the middle of the slide showed a line across and a line down through the slide's middle, and was written with its middle on the slide's, `x` 720 and `y` 380 for 480 by 320.
- **Three selected, aligned and spaced out:** a click and two Shift-clicks selected the three shapes, each with its own outline and no handles; the panel and the status line said *3 things selected*. *Arrange*, *Align middle* lined their turned boxes up; *Space out across* spaced them out. The menu's order items were greyed, with *Select one thing to bring it forward or send it back.* under them; with one selected, *Space out* was greyed with *Select three or more things to space them out.*
- **Brought forward:** a right-click on the ellipse selected it and opened *Cut, Copy, Paste, Duplicate, Delete*, then *Bring to front, Bring forward, Send backward, Send to back*. *Bring forward* drew it in front of the rounded rectangle, and the *Arrange* menu then greyed *Bring to front* and *Bring forward*.
- **The table dragged out of the text:** a click in a cell and Escape selected the table, *Table selected*. Dragged, a dashed outline showed where it would go, and when let go it was written into `<Box x="-20" y="626" w="1720" h="248">` at the end of the slide, selected as *Table in a box*, with the box's form and the table's tools together in the panel. The main text area, now empty, offered *Click to add text*.
- **The title area moved, resized and put back:** a click in the title drew its frame; a press on the frame's line selected *Title area*, with handles and a form of *Left, Top, Width, Height, Turn* and *Put back*. Dragged to the bottom of the slide it was written `region.title: x=195 y=940`, and resized from a corner `x=195 y=897 w=1335 h=183`; *Put back* took the line away and it was where the layout has it. Delete with the area selected said *A layout's area can't be deleted or copied. Choose another layout to change the areas.*
- **Animated:** on the Animate tab, *Fade in* for the picture, *Appear* for the ellipse and *Fly up* for the rectangle gave them the badges 1, 2 and 3 at their top-left corners. *Later* on the picture swapped it with the ellipse, and *Order* read *Click 2 of 3*. Three paragraphs typed in the main text area, then *Appear* chosen from the menu while the caret was there, animated the area (step 4), and selected it. *One at a time* made its badge *1–3*. *Push* from the transition menu was written `transition: push`.
- **The layout changed:** *Layout* on the Home tab opened the tiles with *Title and content* marked; *Two columns* wrote `layout: two-columns`, the text went into the left column, and the status line said *Layout changed to Two columns.*
- **Saved, closed, opened again:** Save, then the app ended and started again on the folder: slide 6 was as left, with the transition and the badges 1, 2, 3. `git diff --no-index` of the original and the saved `slides.md` showed one hunk, the added slide.
- **Presented:** F5, then to slide 6: it came in pushed from the right. At its start the shapes not animated and the table showed; click 1 brought the ellipse at once, click 2 faded the picture in, click 3 flew the rectangle up, and click 4 went to slide 7. The table's box, later in the talk, stayed drawn over the shapes at every step, as in the editor.
- **Exported:** the PDF has 8 pages, page 6 slide 6 as it ends, every thing turned and in its place. The PowerPoint file, drawn by LibreOffice, shows the same; its slide 6 has a `push` transition, three click entrances, appear, fade and fly in, on the ellipse, the picture and the rectangle, the turns as `rot`, the shapes as `rect`, `ellipse` and `roundRect`, the picture, and the table as a table.
- **Idle:** with a shape selected, the app used 0.007 of a core over 20 seconds, and 265 MB.

### The hand checks the reviews asked for

- **The Appear menu while typing:** with the caret in the main text area, the Appear menu opened over the slide and *Appear* chosen from it animated that area, which then showed as selected. The typing's target was kept.
- **The pointer's shape:** a corner handle shows the diagonal resize arrows, an edge handle the left-right or up-down ones, the turn handle a pointing hand, and the thing itself and the empty slide the plain arrow. A menu lying over a handle shows the plain arrow everywhere on it, also right over the handle.
- **Right-clicks, and Delete after each:** on a thing, the menu above, and after Escape, Delete took only that thing. On the empty slide, a menu of *Paste* alone, and after Escape, Delete did nothing: the slide stayed. On a thing with the list of slides holding the keyboard (a row clicked first), the thing was selected, the menu opened, and after Escape, Delete took only the thing, leaving 8 slides.
- **New slide and its caret:** the two sit side by side with no line between them, but the pointer over *New slide* lights only it, and a press on the caret frames only the caret, so they read as two buttons, not one.
- **A drag let go outside the window:** a shape dragged down past the slide, the notes and the window's bottom edge, and let go there, went back where it was: no step was made (Redo, from an earlier Undo, stayed on) and nothing was written.

### The Home tab's width

At 1,280 pixels the Home tab shows *Paste*, *Cut* and *Copy*; *Undo* and *Redo*; *Save*; *New slide* and its caret, *Layout*, *Duplicate slide*, *Delete slide* and *Hide slide*; *Bold*, *Italic*, *Underline*, the size, the colour and *Clear formatting*; then *Bullets* and *Numbering*, and *Decrease indent* half hidden under the arrow, with the *Paragraph* label cut to *Par*. *Increase indent*, the three alignments and *Arrange* are past the arrow. It's the same with a caret in text. At 1,440 the whole *Paragraph* group shows and *Arrange* is past the arrow, its icon half shown. At 1,600 everything shows, with no arrow; with a table selected, the Table group's *Row above* starts past the arrow there.

### What it found

- **Arranging reached a slide that wasn't shown.** A layout's area selected, then Page Down: the area stayed selected on the first slide, unseen, and the Home tab's *Arrange* menu was on. *Align right* then wrote the first slide's title area's place on the slide shown: in the window, slide 2's title moved over its list (Undo brought it back). The same path reached *Space out* and the order items. For a thing rather than an area, aligning wrote nothing, as the thing isn't on the slide shown; the order items, read from the code, would have moved it on the slide that wasn't shown. The random session found it as an area's drag written on another slide. Aligning, spacing out, reordering, lifting and moving now act only on what's selected on the slide shown, so the menu is off on another slide and comes back on with the selection on its own slide. Test: `arranging_never_reaches_a_slide_not_shown` in `ui_arrange.rs`. (a49c415) Seen again in the window on the release build with it: the menu was greyed and nothing was written.
- **Notes ending a line in `|` made a table after them read as a paragraph.** Notes whose last line is a quote or a list item ending in `|` (`> a|`, `- a|`) made the `markdown` crate read a table after `</Notes>` as a paragraph, so the slide showed the table's text as one paragraph. It was the random case of `notes_operations_keep_the_slides_other_blocks` seen once earlier in the plan; the saved case reproduced it. A `|` ending a line of typed notes is now written `\|`, and read back as `|`. Test: `notes_with_a_line_ending_in_a_bar_keep_the_table_after_them` in `crates/format/tests/slide_ops.rs` (81f4d4a); the saved case is kept in `crates/format/tests/slide_ops.proptest-regressions`. Hand-written notes ending a line in `|` before a table, and a quote ending in `|` straight before a table in a slide's text, still read that way.

### What looks wrong and wasn't changed

- **Shift held before pressing the turn handle doesn't turn.** With Shift or Ctrl down, a press adds or takes away the thing under the pointer, before any handle: on the empty slide it let go of the selection, and where the handle lay over the picture, it added the picture and then dragged both. Shift pressed after the press steps by 15 degrees, as the plan has it.
- **A layout change leaves an area's animation behind.** The main text was shown one at a time as `region.body`; *Two columns* names its main area `left`, so the text went there and the setting no longer reached it: the badge went, and the show had all three paragraphs at the start. Changing back brings it again. The plan keeps such lines on purpose.
- **A step badge on a turned thing sits off the thing**, at the corner of the upright box the thing covers. The badge on an area covers the first letter or two of its text.
- **Handles of a thing mostly off the slide are drawn outside it**, over the notes and the status line, while the thing and its outline are cut off at the slide's edge.
- **The panel's numbers don't follow a drag**; they change when it's let go. An area's form shows its place fields empty until it's moved.
- **The right-click menu on a thing shows no keys for *Duplicate* and *Delete*** when opened with the list of slides holding the keyboard; opened otherwise it shows *Ctrl+D* and *Backspace*.
- **Several things selected look alike**: a thin grey outline each, no handles, and on things of the same colour it's hard to tell which are picked.
- **Things spaced out by their turned boxes can overlap**, as the boxes of turned things are much bigger than the things.
- **A typed `- ` doesn't start a list**: it's written `\- First point`.
- **Ctrl+V, after Escape twice to end typing in the title, pasted nothing**; the Paste tool did.
- **Shift+F5 rehearses from the first slide**, and the Slide Show tab has *From the start* and *Rehearse* only: there's no way to present from the current slide.

## Not exercised

- **Anything at all on Windows.**
- **The system's dialogs**: Open, Save As, Export, *Picture* and *Choose file…*.
- **Wayland, a real GPU, scaling above 100 %, a touch screen, and a real mouse** (double-clicks and drags came from the X test extension, with fixed pauses).
- **Desktop PowerPoint**: the PowerPoint file was drawn by LibreOffice, which doesn't play the animations; that they're there was read from the file.
- **The 500-slide talk in the window**: its speed was measured by the release timing tests only.
- **Alt switching snapping off, Ctrl+A, Tab through things, and the arrow keys in the window**: covered by headless tests only.
- **The compiler bundle's seven tests**, not run.

## Windows check, by hand

The branch hasn't been pushed or merged yet. Once it's in `main`, this check starts from a clone of `main`, on a Windows PC with Rust 1.98.0 and the MSVC build tools, Git for Windows, desktop PowerPoint, and the bundle as in `docs/checks/plan-c.md`.

1. `git clone https://github.com/Eiriksb/stagepack-studio.git`, then `cd stagepack-studio`, then `cargo test --workspace`. If only golden tests fail, send back which ones.
2. `cargo build --release -p stagepack-studio`. Make `C:\StudioCheck`, copy `target\release\stagepack-studio.exe` into it and the bundle in as `C:\StudioCheck\bundle`, and copy `fixtures\showcase` to `C:\StudioCheck\showcase` and again to `C:\StudioCheck\original`.
3. Run `C:\StudioCheck\stagepack-studio.exe C:\StudioCheck\showcase`. Add a *Title and content* slide from *New slide*'s caret. Say whether *New slide* and its caret look like one control or two.
4. Type a title, add a picture with *Picture*, three shapes from *Shapes* and a 3 by 3 table. Drag each, resize each from a corner and an edge, and turn each from the round handle, once with Shift pressed after the press. Send screenshots with a turned thing selected and during a drag showing a snapping line. Say whether the outline and handles sit on each thing, and what the pointer looks like over a corner, an edge, the round handle and the thing.
5. Drag a shape near the slide's middle: lines should show through the middle, and it should land there. Hold Alt while dragging: no lines and no snapping.
6. Shift-click two more shapes, then from the Home tab's *Arrange* menu, *Align middle* and *Space out across*. Right-click one shape and *Bring forward*. Arrow keys should move a selected shape by one unit, Shift with them by ten.
7. Click in a table cell, press Escape, and drag the table: it should be moved out of the text into a box. Click in the title, press on its frame's line, drag it, resize it, then *Put back* in the panel.
8. On the Animate tab, animate three things with *Appear*, *Fade in* and *Fly up*, use *Later* on one, type three lines in the main text and set *One at a time*, and set *Push*. With the caret in an area, choose from the Appear menu: say whether it animated that area.
9. Select an area or a shape, press Page Down, and open *Arrange*: it should be greyed, and nothing on the slide shown should move.
10. Right-click a thing, the empty slide, and a thing just after clicking a slide in the list: after Escape, Delete should take only the thing, or, on the empty slide, do nothing.
11. Drag a shape and let go outside the window: it should go back, and no step be made.
12. Ctrl+S, close, open again, F5, step through slide 6, then export to PDF and PowerPoint. In desktop PowerPoint, play slide 6: say whether it's pushed in, whether the three things come in on clicks one, two and three as appear, fade and fly in, whether the list comes a point at a time, and whether the turned things are turned the same way.
13. In a Command Prompt in `C:\StudioCheck`, `git diff --no-index original\slides.md showcase\slides.md`: only the added slide should differ.
14. At 125 %, 150 % and 200 % scaling, at 1,280 pixels wide and at the narrowest: send a screenshot of the Home tab, and one with a turned shape selected. Say which tools are past the arrow, and whether the handles are whole and on the thing.
15. D3's Windows check, steps 4 to 19 of `docs/checks/plan-d3.md`, where they're still open.

## Linux check, by hand

Each item under both an X11 session and a Wayland session.

1. A real mouse's double click on a shape types in it; a drag of a thing, a handle and the round handle each follows the pointer smoothly; letting go outside the window makes no step.
2. The pointer's shape over a corner, an edge and the round handle, and over a menu lying on a handle.
3. At 125 %, 150 % and 200 % scaling: the handles, the snapping lines and the step badges sit on the things, and the Home tab's tools past the arrow can be reached.
4. Present on a second screen with the presenter view, step through slide 6, and see the push and the three entrances.
5. The PowerPoint file in LibreOffice Impress: slide 6's transition and its three animations in the animation panel.
6. Shift held before pressing the round handle (see above); say whether that's how it should be.
7. D3's Linux check, items 1 to 8 of `docs/checks/plan-d3.md`.

## Known after the run

Found in this run and not changed are under *What looks wrong and wasn't changed*. These are known from the plan's tasks, and weren't changed:

- **Not in this plan:** grouping and cropping; resizing several things at once; a rubber band drawn on the empty slide to select; motion paths, exits and timings; an animation pane; presenting from the current slide.
- **Front to back is the order in the talk**: a layout's areas always sit under the placed things, so text in an area can't be brought in front of a picture; a text box can.
- **A selected area can't be deleted, copied or put in front or behind.**
- **Notes ending a line in `|` written by hand before a table**, and a quote ending in `|` straight before a table, still make the table read as a paragraph. The fix covers notes typed in the app.
- **D3's known issues**, in `docs/checks/plan-d3.md`, still stand, but for the notes before a table, fixed here for typed notes.
