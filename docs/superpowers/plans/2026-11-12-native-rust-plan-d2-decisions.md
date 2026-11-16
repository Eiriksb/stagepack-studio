# Plan D2: decisions made while building

The calls made during plan D2 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. They are in the order they were made, so a later one can replace an earlier one. The final review triaged every smaller finding, and those it wanted fixed were fixed in its fix wave, so some entries below are closed. `docs/checks/plan-d2.md` has what ran.

## Questions for the owner

- **How an empty line is spelled.** An empty line typed on a slide is written as a line holding `&nbsp;`. It was `**<Text></Text>**` until the final review, which is still read. Nothing has been released, so no talk holds the old spelling unless it was made with a build of this branch. A paragraph that really is one no-break space is written `&#160;`. If a tag of its own is wanted instead, this is the time to say so.
- **Slides don't cross between two separately started copies of the app on Linux with their styles.** GPUI keeps a clipboard entry's extra data inside the process on X11 and Wayland. A slide still crosses as plain Markdown. Styled words cross as plain words. Whether Windows carries the extra data is a hand check.
- **`align` is a name a component's argument can't have.** A component written before D2 with an `align` argument must rename it.
- **Ctrl+V of ordinary text with the slide list focused makes a slide of it.** It's one Undo away, and it's what lets Markdown from another editor be pasted as a slide.

## For the start of plan D3

- **Structural edits tidy their neighbours.** Typing and deleting characters change only the paragraph typed in. Joining two list items, turning a list on or off, changing a paragraph's kind or its level, and deleting across items write the whole list again, and that can change the indentation and markers of items that weren't touched (a tab-nested item becomes two spaces). D3 adds more structural edits, so this is the place to make the whole-list write keep each untouched item's own bytes.
- **The offer stays while typing in an empty place.** *Click to add text* is still drawn under the caret. The outline and the words are one layer. The engine needs to draw a place being typed in without its words.
- **The canvas is redrawn once per change, not once per frame.** Typing far faster than a person can queues retired images until the next paint, and memory rises past a gigabyte. Refreshing the canvas once per frame ends it.
- **Rasterising isn't in the 8 ms budget.** The timing test stops before drawing. Rasterising a changed slide adds about 3.9 ms at the median and up to 17 ms. A slide near the 32 KB limit types slowly.
- **Pictures and components don't travel with a pasted slide.** They show as placeholders with a notice until D3 builds adding a file to a talk.
- **A line of three backticks with more backticks after** is read as the start of code and swallows slide breaks. It's in `split.rs`, and D3's code editing will meet it.
- **A hand-written `- &nbsp;` item with items under it** makes its list refuse typing since the new empty-line spelling.
- **Tables, code, quotes, and text mixed with a component or picture** can't be typed in. A click says so.
- **Lists are drawn at the left.** The alignment tools are off for a place holding only a list.

## Rulings

- **Setup.** No linked worktree, as for the earlier plans. Cost if wrong: The owner's uncommitted roadmap edit shares the checkout, and it stays unstaged.
- **Setup.** A subagent's commit keeps the attribution line its own session gives it.
- **Setup.** Alignment belongs to the region or box, written on its settings line or as an attribute, not to each paragraph. Markdown has no paragraph alignment. Cost if wrong: One region can't mix centred and left paragraphs.
- **Setup.** A turned text box is edited as it lies, with the caret turned with it. Showing it upright while typing waits for D4. Cost if wrong: Typing at an angle until D4.
- **Setup.** A slide pasted into another talk brings its text. Pictures and components the other talk lacks show as placeholders with a notice. Cost if wrong: A pasted slide can need its pictures added again.
- **Setup.** Text is edited as a plain model of paragraphs and spans, written back after every change through the Document's history, with the engine answering where things are. One layout, one history, nothing to flush before a save. Cost if wrong: Each keystroke costs a write, a reparse of one slide and a layout of it.
- **Setup.** Text the model can't hold exactly isn't editable in D2, and a click says so. The design says to refuse when unsure. Cost if wrong: Those wait for D3.
- **Pre-flight.** The history step for a text edit carries the flow's place and the selection before and after, kept beside the step in Document. Cost if wrong: None.
- **Pre-flight.** The size shown for text with no size of its own comes from the engine's layout. Cost if wrong: An empty size field for unstyled text.
- **Task 1.** Undo, redo, recover and take_theirs return a Result, and a reload can say Unreadable. A panic guard needs somewhere to report. Cost if wrong: None.
- **Task 2.** The save flow has two more effects, TakeRead and LetGo, and the working copies are a GPUI entity, since their timers need one. Cost if wrong: A harder unit test.
- **Task 3.** A soft and a hard line break both read as a line break, because both are drawn as one. A break can be written in either form as long as it reads back the same. Cost if wrong: An edited hand-wrapped paragraph changes how its break is spelled, which shows in a diff and not on the slide.
- **Task 3.** A paragraph holding a `<Text>` with an attribute the model doesn't hold, or a link with a title, isn't editable. Replacing its text would drop the attribute without a word. Cost if wrong: Such paragraphs can't be typed in until the model holds them.
- **Task 4.** Writing a flow returns nothing for a refusal and an empty change for nothing to change. The plan's signature couldn't say "refused", and the design wants refusals said. Cost if wrong: A signature different from the plan.
- **Task 4.** The writer's last fallback, writing a whole flow from scratch, is dropped: it refuses instead. It never won, and it was the only path that could rewrite untouched markup freely. The second, writing the flow again with each old item's shape passed in, stays. Cost if wrong: A few more refusals.
- **Task 4, replaced after Task 6.** Empty paragraphs and a break at a paragraph's end were to live only in the editor while typing. Task 6 showed that where things are drawn can only come from the talk as written: a paragraph the editor holds and the source doesn't can't carry a caret. So they are stored, in a spelling that reads back exactly, and drawn as an empty line. Cost if wrong: A deliberate empty line shows as a line of its own in the Markdown.
- **Task 5.** Ctrl+Right goes to the start of the next word, as on Windows and Linux. Cost if wrong: A habit from macOS doesn't fit.
- **Task 5.** Backspace removes a whole cluster, such as a letter and an accent typed as two. Cost if wrong: Typists of Indic scripts and Thai retype the base letter.
- **Task 5.** Control characters other than tab and newline are dropped on insert, paste and typing. Direction overrides and isolates are dropped too. Cost if wrong: Text copied with them loses them.
- **Task 5.** Pasting into an empty paragraph takes the pasted paragraph's kind. A style chosen before typing survives a change of kind or level and Enter; only moving the caret or selecting drops it. Cost if wrong: A habit.
- **Task 6.** `align` stays a reserved name. There's no public release, so no built component in the wild can have it, and one rule for placement names beats a special case. Cost if wrong: A private component with an `align` argument must rename it.
- **Task 6.** A click anywhere inside a text place goes to the nearest paragraph, as in boxes. Cost if wrong: None.
- **Task 7.** In a title, Enter makes a line break, the same as Shift+Enter. A body paragraph can't be turned into a heading where it would become the title; that's refused with a sentence. Cost if wrong: A two-paragraph title has to be written by hand.
- **Task 7.** Redo puts the caret where it was after the step, as every editor does. The brief's "before" was a slip. Cost if wrong: None.
- **Task 9.** The ribbon's row of tools scrolls sideways when it doesn't fit. Collapsing groups is later work. Cost if wrong: Undo is hidden until scrolled to at 800 pixels.
- **Task 9.** The alignment tools are off, with a tooltip saying why, when a place holds only list items. The engine draws lists at the left. Cost if wrong: None until lists can be aligned.
- **Task 9.** A colour from the sliders is applied however the picker is closed. A size equal to the text's own size clears the size. Aligning a place as it already is writes nothing. Cost if wrong: A habit.
- **Task 10.** With the slide list focused, plain clipboard text within the size limit pastes as a slide when no slide data is there. It goes through the same one-slide check. Goal 9 wants paste between talks, and this is the only way across two copies of the app on Linux. Cost if wrong: Ctrl+V of unrelated text in the list makes a slide of it, one Undo away.
- **Task 10.** A paste never sets a talk-wide setting, also into an empty talk. One rule everywhere beats "it's only colours and fonts". Cost if wrong: A pasted first slide loses the title and theme lines it came with.
- **Final.** An empty line is written `&nbsp;`, replacing `**<Text></Text>**`, which is still read. People write it by hand, and every Markdown viewer shows it as a blank line. Cost if wrong: A second spelling to keep reading.
- **Final.** What's left after the fix for typing in hand-indented lists, structural edits tidying their neighbours, is recorded and not fixed in D2. Cost if wrong: A diff with more lines than the edit after joining or re-levelling items in a hand-indented list.
- **Final.** One fix wave took the Critical finding, the three Important ones and the cheap hardening. The rest are below. Cost if wrong: None, each is small.

## Smaller findings and notes

- **Task 1.** A picture replaced on disk shows only after the next save or reload. A refused undo says *That change couldn't be made*. A keystroke on the 500-slide talk fell from 23.7 ms to 0.75 ms with the incremental reparse.
- **Task 2.** `follow` in saving.rs turns three effects into no-ops, so a misrouted one would vanish. WorkingCopies has no tests of its own.
- **Task 3.** The format crate's table of each layout's regions is a second copy of the engine's. A test holds them together over every layout.
- **Task 4.** An edited paragraph's own escapes are made the plainest (`\\w` becomes `\w`). A paragraph put between two list items has no blank line after it. In a list written out whole, spaces after an untouched marker can close up, and an untouched item can be numbered again when the blank line before it is taken away.
- **Task 5.** A double-click at a paragraph's end takes the punctuation before it. Each edit costs time in step with the paragraph's length.
- **Task 6.** The notice about `align` inside a paragraph is a plain search, so it can come for inline code or an escaped tag. An empty heading (`#` alone) now takes a whole line. A paragraph ending in a line break gets an empty last line. A textless ellipse says it's *a box that holds more than text*.
- **Task 7.** A tidy-up folded into a step that didn't make the flow can leave a blank line. A slide that fails to lay out ends the typing without tidying. The slowest single keystroke goes past 8 ms; the median is about a third of it.
- **Task 8.** Text being composed is drawn as a label over the slide at the caret, not in the line. The candidate window is placed from where the composition starts. On Linux, a dead key then Ctrl+S saves nothing, as the system makes a letter of the two. X11 input methods aren't told when the caret moves.
- **Task 9.** A colour named by hand shows no swatch. Indent is offered for body paragraphs, where it does nothing. Tab onto a tool past the arrow doesn't scroll the row to it.
- **Task 10.** A slide pasted into text over the limits does nothing, without a word. Plain text starting `title: …` and `---` pastes as visible words. A blank line before a leading `---` makes the paste refused. The sentence about missing pictures also comes for a picture that can't be read. A failed clipboard write can't be known, as GPUI doesn't say.
- **Task 11.** D1's "squeezed window" was the screenshot helper reading three-byte pixels as four. Seven defects found in the real window were fixed in eight commits. No screenshot shows the new sentence for pictures and icons.
- **Final.** Fixed in the wave: typing in a hand-indented list; a paste the slide can't hold; Tab on a first bullet; an empty line's spelling; a slide's notices and thumbnail guarded; Save As and Export settling typing done while their dialog was up; a heading pasted outside a title; one list-level limit; a test holding the talk-wide settings to the engine's; colour emoji from the bundled font first; a link starting a paragraph with a bracket in its code.
- **Final.** Found by the check of the fix wave and left, as the plan allows one wave: emptying a nested item in a hand-indented list re-indents its neighbours; a list holding a hand-written `- &nbsp;` item with items nested under it refuses every key (it could be typed in before the new spelling; nothing is lost); the paste-speed test has 50 ms to spare and could fail on a loaded computer, where 250 ms would tell the cases apart as well; with the emoji font first, symbols the bundled fonts lack (✔ ♥ ☎) are drawn as colour emoji on screen, as they already were in a PDF.
- **Final.** Older than D2, found by the reader's property test at 1,000 cases: a line starting with three backticks and holding more backticks is taken as the start of code, which Markdown doesn't allow, so slide breaks are swallowed until the next line of three backticks.
- **Final.** Not fixed: a trailing newline in pasted text leaves an empty line; Redo is lost by clicking away after an Undo that left a text empty; a slide near 32 KB types slowly; the paste size check counts a styled paste's text and not its markup, so a paste right at the limit can still be refused by the writer with the plainer sentence; Tab on an item under an empty item is refused once; the emoji font is found by its family name, so a computer's own copy of the same font could be the one used.
