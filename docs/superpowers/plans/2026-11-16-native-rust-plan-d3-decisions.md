# Plan D3: decisions made while building

The calls made during plan D3 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. They are in the order they were made, so a later one can replace an earlier one. The final review triaged every smaller finding, and those it wanted fixed were fixed in its fix wave, so some entries below are closed. `docs/checks/plan-d3.md` has what ran.

## Questions for the owner

Each of these changed what a person sees, and each was decided without asking. Say so if one is wrong.

- **One finding that matters is left open.** A right-click on a slide in the list, with a shape selected or being typed in, leaves the shape showing as selected while the list has the keyboard, and Delete then takes the slide. The plan allows one fix wave and this was found by the check of it. The fix is a line and four tests.
- **The Home tab no longer fits at 1,280 pixels.** The clearer *Delete slide* and *Duplicate slide* labels push Undo and Redo behind the arrow at the row's end. Moving the History group in front of *Slides* would show them again.
- **Ctrl+D, the arrow keys and the Page keys with a thing selected** still act on the slide. Whether they should act on the thing is D4's question.

- **The form panel is always shown**, 300 pixels wide at the right, with one line of help when nothing is selected. It first appeared only when a thing was selected, which moved the slide under the pointer in the middle of a double-click. The design lists the inspector as a fixed part of the window. A way to hide it can come with D4.
- **The window can't be narrower than 1,024 pixels.** It was 800. The fixed panel needs the room.
- **A new text box isn't written to the talk until its first visible character is typed.** Until then it's an outline and a caret. Escape, Save or a click elsewhere leaves nothing behind. Its size can't be set in the panel before it has a character, and it can't start with a blank line or a space.
- **Nothing is dragged yet.** A new thing lands in the middle of the slide at a fixed size, and is moved by typing numbers under *Place*. Dragging is D4, as the roadmap has it.
- **A number field takes a comma as the decimal mark.** `1,5` is 1.5. So `1,000` and `1.000` both read as 1.
- **A table isn't placed by hand.** It's plain Markdown at the end of the slide's main text. An empty cell the app makes is two spaces, so typing gives `| x |`.
- **With a table selected and no cell clicked**, only *Row below* and *Column right* work, adding at the end. The other four say *Click in a cell first.*
- **The Home tab's slide tools say *Delete slide* and *Duplicate slide*.**
- **The status line always keeps its height**, so the slide no longer jumps when a message comes and goes. It costs one line of slide height.
- **A file added to a talk is saved only if the saved text uses it.** One file is 32 MB at most, and a drop or a paste takes 20 pictures at most.
- **Text inside a component can be typed on the slide only if the component says it takes content** and its build is loaded and working. Any other component is selected as a whole.
- **Pasting a component brings its source and its build, and builds nothing.** A paste can claim its build belongs to its source, as a talk's own files can when opened. That can't be checked without compiling. The build still runs only in the sandbox.

## For the start of plan D4

- **Three files are too large to hold in mind:** `document.rs`, `slide_editor.rs` and `inspector.rs`. Split them before building on them.
- **A drag needs a temporary override on the deck.** Writing a place through the talk costs 8.7 ms on a slide of sixty things and makes an undo step each time.
- **There's no multi-selection, no arrow-key nudging, and no handles.** The outline is drawn by the window over the slide, turned with its thing, so handles can go there.
- **A component inserted at the fixed size can stretch.** A Callout's typed words land at the bottom of its box. Sizing a new thing to what it draws belongs with resizing.
- **An icon's Size field doesn't move its Width and Height.**
- **A file whose name is taken by a different file** gets a number on the way in, but a clash that appears later blocks saving until the picture is taken off the slide or the other file is renamed. A rename offer is wanted.
- **Pictures are read and decoded on the window's thread**, and so are large copies and pastes. A large picture freezes the window for a second or more.
- **Typing on a slide that holds a very large table is slow:** 60 to 70 ms a key with a full 20 by 51 table. The 8 ms budget is met for 6 by 8.
- **Aligning a selected component that takes content writes `align` onto its tag**, and tables inside such a component can't be typed in.
- **A component's load time isn't counted against its slide's drawing budget.**

## Rulings

- **Setup.** No linked worktree, as for the earlier plans. Cost if wrong: The owner's uncommitted roadmap edit shares the checkout, and it stays unstaged.
- **Setup.** The plan wasn't held for the owner's review before it ran. The owner asked to continue and has run every earlier plan this way. Cost if wrong: Rework of whatever the owner would have changed. The plan's own rulings are at its top.
- **In the plan.** A new thing is put in the middle of the slide at a fixed size, 40 units down and right for each thing already exactly there. Cost if wrong: Things are placed by numbers until D4.
- **In the plan.** A picture is written as a placed `<Picture>` tag, never as `![]()`. Cost if wrong: None. A hand-written `![]()` can still be selected and deleted.
- **In the plan.** A table is plain Markdown in the slide's main region, not placed. Cost if wrong: A table can't be moved on its own.
- **In the plan.** Files added to a talk are named for what they were, made safe, with a number if the name is taken by different bytes. The same bytes twice are one file. Cost if wrong: A file name a person didn't choose.
- **In the plan.** A file is saved only if the saved text uses it, and one file is 32 MB at most. Cost if wrong: A larger picture is refused.
- **In the plan.** One click selects a box or shape that holds text, and a second types in it. A click on a region's text still types at once. Cost if wrong: One more click to type in a box.
- **In the plan.** A shape's `kind` is a choice of six, and any other value still draws a rectangle. Cost if wrong: None.
- **Pre-flight.** On Ctrl+V with no caret, the richest thing wins: a copied thing, then a slide, then this app's text, then a picture, then file paths, then plain text as a slide in the slide list. Cost if wrong: A reorder.
- **Task 1, made twice and reversed.** The slide splitter keeps its new fence rule: an opening fence line is indented three columns at most, and a closing one up to three more than its opener. The controller first ruled to go back to opening at any indentation. The implementer showed that only a `---` at column 0 starts a slide, and that a line at column 0 ends any list item and its fence, so the limit can't lose a break, where the old rule did. Cost if wrong: None found.
- **Task 3.** Text inside a component that can't be typed in gives the component as the thing clicked. A thing thinner than 16 units is picked as if 16 thick, so a line can be clicked. Cost if wrong: None seen.
- **Task 5.** The status line keeps its height always. Cost if wrong: One line of slide height for everyone.
- **Task 6.** A number field takes a comma as the decimal mark. A value with both a dot and a comma is refused. Cost if wrong: `1,500` reads as 1.5.
- **Task 7.** A new text box isn't written until its first character is typed. The first design removed an empty box afterwards by folding history steps. That broke on a second insert, on Save and on a change in the panel. Empty regions already work this way. Cost if wrong: The panel can't size a text box before it has a character.
- **Task 6, replacing a workaround.** The form panel is always shown. Mapping later presses of a double-click through the slide's old place broke twice. Cost if wrong: 300 pixels less slide width for everyone.
- **Task 7.** While a text box waits for its first character, input with nothing visible in it is ignored. Cost if wrong: A text box can't start with a blank line or a space.
- **Task 6.** The window is 1,024 pixels wide at least. The Markdown panel shrinks to 240 before the slide gives way, and the slide's column is never under 240. Cost if wrong: The window can't be made narrower.
- **Task 9.** A cell the app makes empty is two spaces, and typing in a cell of only spaces keeps one each side. A hand-written cell keeps its own padding. Cost if wrong: An emptied hand-written cell comes back as two spaces.
- **Task 10.** With a table selected and no caret, only adding at the end works. A table's size is said one way everywhere, counting body rows and not the heading. Cost if wrong: One more click to delete the last row.
- **Task 11.** Only Text, Box and Shape, and a loaded component that says it takes content, hold text that can be typed on the slide. A component is loaded when its slide is drawn, when one is selected, or when the list of components opens. Never when the talk opens. The first build of this task made any placed tag holding text typeable, so a click on a component that didn't draw its words typed into the slide behind it, and it ran every component when the talk opened. Cost if wrong: The list of components can take up to a second to fill the first time.
- **Task 11.** A box of text is remembered by where its tag starts, followed through edits, never by counting boxes. Cost if wrong: None.
- **Final.** The Home tab's slide tools are labelled *Delete slide* and *Duplicate slide*. Cost if wrong: Longer labels in a row that already scrolls.
- **Final.** One fix wave took the three Important findings, the smaller ones the review named, and the ledger's timing tests. What's for D4 is above. Cost if wrong: None, each is small.

## Smaller findings and notes

- **Task 1.** An unclosed fence opened one to three columns in, inside a list item, swallows the slide break after it. The splitter can't see lists. It was so before this plan.
- **Task 2.** Two or more blank lines after a slide's opening lines leave two before what follows typed text. Removing an attribute from a tag written over several lines inside a quote takes one space only.
- **Task 3.** Within one layer, a thin line's picking slack can put it above a small thing lying over it.
- **Task 4.** Two clashing names whose new names chain make recovery give up and say so. A working copy holding only an added file and no text stays for 30 days. Each keep of the text reads the kept files again to compare. Several clash messages overwrite each other. A reload from outside asks when the only unsaved thing is an added file.
- **Task 5.** After a reload from outside, the selection can move to a neighbour with the same bytes.
- **Task 6.** A panel first shown while squeezed keeps the squeezed width when the window grows. The panel is drawn again on every keystroke on the slide. A press exactly on a text's left edge can miss.
- **Task 7.** A few zero-width characters still write a text box that looks empty. Text box is offered on a slide where nothing more can be written.
- **Task 8.** The icon grid has no scrollbar, and the keyboard can pick only the first match.
- **Task 9.** A cell holding only a no-break space gets its padding changed by typing. Inline code with a backslash before a pipe can't be typed in a cell.
- **Task 10.** The sentence for a cell holding more than text can be said for the wrong cell in a turned box. The Home tab's Table group is off the screen at 1,280 pixels until the row is scrolled; the panel has the same tools.
- **Task 11.** The list of components can take a quarter of a second more than its one-second budget.
- **Task 12.** A paste decodes and checks every carried file before deciding which the pasted text uses. Two copies of the app on one folder have no lock between them, as before.
- **Task 13.** In the real window, Delete took a whole slide while a thing on it showed as selected, when the list of slides had the keyboard. The fix is one rule: while the list of slides has the keyboard, nothing on the slide is selected or being typed. The run's first launch showed on the real desktop for a few seconds, with scratch data folders set; nothing was written to the real ones.
- **Final.** Fixed in the wave: a press on the list of slides ends the typing; the slide tools' labels; a saved picture read from the talk; a space on a selected shape; the timing test's tables filled (a full 6 by 9 table is 5.2 to 6.0 ms a key, a full 20 by 51 about 65 ms); the picture dialog's sentence and its cap of 20; timing checks loosened; two fence tests; a packed talk saved but not read back.
- **Final.** Not fixed: the right-click route above; Save As to a pack that can't be read back says the save failed though the file was written; a slide of sixty things is at the keystroke budget; a hand-written table near 10,000 cells takes over half a second a key; typing at the very start of a selected table's first cell lets go of the table; the list's focus backstop never fires in the headless tests, so nothing tests it; one compile test in the components crate failed once on a loaded computer.
- **Final.** The disk filled during the build, from the build cache. Stale test programs were deleted from `target/debug/deps` twice.
