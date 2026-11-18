# Plan D4: decisions made while building

The calls made during plan D4 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. They are in the order they were made. The final review triaged every smaller finding, and those it wanted fixed were fixed in its fix wave. `docs/checks/plan-d4.md` has what ran.

## Questions for the owner

Each of these changed what a person sees, and each was decided without asking. Say so if one is wrong.

- **The Home tab no longer fits at 1,280 or 1,440 pixels.** It gained *Layout* and *Arrange*. At 1,280 *Decrease indent* is half hidden, and *Increase indent*, the three alignments and *Arrange* are behind the arrow at the row's end. At 1,440 only *Arrange* is. At 1,600 everything shows. Undo and Redo show at every width, since the History group moved forward. Ways out: icons without words for some tools, a tab of its own for arranging, or the paragraph tools in two rows. None was chosen.
- **A new thing in the text is moved by dragging it out.** A table, or a picture or shape written in the text, becomes a thing with a place when it's dragged: a tag gets its place written on it, anything else is wrapped in a box. A lifted table keeps the width it had in the text.
- **A box that holds one thing is picked first, and the thing inside by a second click.** The form then shows the inner thing. Arrow keys, Align and Animate with the inner thing selected act on the box.
- **One thing selected aligns to the slide, and several align to each other.** Spacing out needs three.
- **Pictures and icons keep their proportions when a corner is dragged**, and other things don't. Shift does the opposite. Shift while turning steps by 15 degrees. Alt switches snapping off.
- **Several things are picked with Shift or Ctrl and a click, or Ctrl+A.** There's no rubber band on the empty slide.
- **A layout's area is picked by a press on its frame**, which shows while its text is typed in, or by Tab. A press on its text types. A selected area can't be deleted, copied or reordered, and *Put back* in the form returns it to where the layout has it.
- **A layout's areas always sit behind the things placed on the slide.** Order is the order in the talk, among placed things only.
- **Arrow keys with a thing selected move it**, one unit, ten with Shift. Left and Right change the slide only when nothing is selected. Page Up and Page Down always do.
- **A thing can't be dragged, resized, aligned or spaced out off the slide**: 16 units of it stay on.
- **Animating gives a thing the click after the slide's last, and taking a click away closes the gap.** *Earlier* and *Later* swap with the neighbouring click. An empty area can't be animated: *Type something in it first.*
- **A layout change keeps an area's place and animation lines for areas the new layout lacks**, so they come back when the layout does. Until then they do nothing, so a list shown one at a time under *Title and content* shows whole under *Two columns*.
- **New slide is a button and a small arrow beside it**, the arrow opening the layout tiles. They look like two buttons.
- **The form panel can be hidden** from the View tab. It starts shown.
- **Positions and sizes are written as whole numbers.** A thing of an odd width snapped to the slide's centre lands half a unit off it.

## For the start of plan D5

- **What a layout is, is written in several places.** The engine's list, the format crate's own list of names and areas, the app's names for layouts and for areas, and the default layout's name in three crates. A new layout or area missing from one of them is refused when placed. Make one source first.
- **The format crate copies the engine's reading of an area's settings line** (which of two repeated words counts, what a click may be). One property test in the engine holds the two together; extend it if a template adds words.
- **Templates will have placed boxes and empty areas**, where this plan's last two fixes were: a box and what it holds moving as one, and an empty area not counting as animated.
- **A selection can stay on a slide that isn't shown.** Every new operation must act only on the selection of the slide shown, as arranging does.
- **The random session's window run should gain each new tool** as it's added.
- **There's no way to present from the current slide**, only from the start.

## Rulings

- **Setup.** No linked worktree, as for the earlier plans. Cost if wrong: the owner's uncommitted roadmap edit shares the checkout, and it stays unstaged.
- **Setup.** The plan wasn't held for the owner's review before it ran. The owner said to go ahead and has run every earlier plan this way. Cost if wrong: rework of whatever the owner would have changed. The plan's own rulings are at its top.
- **Task 1.** The split of the three large files wasn't read by a reviewer. It was checked by program: every line of the old files is in the new ones, visibility aside. Cost if wrong: the commit that moved the tests' shared helpers was read by nobody but its author.
- **Task 2.** *Bring forward* then *Send backward* gives back the same bytes only where blank lines alone lie between the two things. Otherwise the order is restored and the thing sits on the other side of the text between. Cost if wrong: none to what's drawn.
- **Task 3.** A place shown before it's written is ignored for a thing lying in the text. Cost if wrong: none; a lift shows a dashed outline.
- **Task 3.** An area turned and shown one at a time counts as one click, as the engine plays it.
- **Task 4.** A press on an area's own text types even where the text touches its frame. The form calls an area *Main text area*, *Title area* and so on.
- **Task 4.** Several copies made at once step aside together, by one step, and keep their arrangement.
- **Task 5.** Several things dragged together stop together at the slide's edge. A selection with one thing in the text isn't dragged at all, and says *Drag it out of the text first.*
- **Task 6.** A very small thing's handles are drawn apart so each can be pressed. A side a handle doesn't change may stay under 8 units. The turn handle shows an open hand, as there's no turning pointer. Taking an icon's Size away leaves its width and height.
- **Task 7.** Things lying in the text and empty areas aren't snapped to. A box with no width snaps by its one line. On a corner that keeps its shape, only the side the pointer drives snaps. Eight earlier drag tests hold Alt, as their drags now land on real lines.
- **Task 8.** The Home tab keeps its File group. Reordering needs one thing selected: *Select one thing to bring it forward or send it back.* The order items are checked against the talk as a menu opens.
- **Task 9.** *Earlier* and *Later* are off for a thing that isn't animated. A slide with 999 clicks refuses another: *This slide has as many clicks as it can have.* While typing in a text box or an area, that box or area is what the Animate tab acts on.
- **Task 10.** As in the questions above: the box first, the thing inside by a second click. A table in a box with other things says *Drag the box it's in instead.*
- **Task 12.** Its four commits were reviewed by the final reviewer, not on their own. Cost if wrong: one fewer read of the two fixes among them.
- **Final.** One fix wave took the three Important findings and the small ones the review wanted fixed now. Cost if wrong: none, each is small.

## Smaller findings and notes

- **Found by the real-window run and fixed:** aligning, spacing out and reordering acted on a selection left on a slide that wasn't shown, so *Align right* could move another slide's title. Notes typed with a last line ending in `|` made the table after them read as a paragraph.
- **Found by the final review and fixed:** after Ctrl+A on a slide with a lifted table, nothing could be moved or aligned. An entrance chosen in an empty area gave the next thing animated a click that showed nothing. Shift held before a press on a handle let go of the thing. A right button let go of in the middle of a drag ended the drag. Undo of a layout change didn't select the area again.
- **Found by the random session in the fix wave and fixed:** an area turned back upright lost its selection.
- **Not fixed, small:** a step badge on a turned thing sits off it, and an area's badge covers its first letters. Handles of a thing mostly off the slide are drawn over the notes. An area's place fields are empty until it's moved, and the form's numbers follow a drag only when it's let go. Several things selected are hard to tell apart on things of one colour. A right-click menu opened while the list of slides has the keyboard shows no keys beside its items. At the slide's edge a snap line just outside wins over one just inside, and neither is taken.
- **Not fixed, in the tests:** the test that many things deleted at once write what one at a time would takes 50 seconds and doesn't say which path it took. Clearing a guide when a drag frame has nowhere to put the thing couldn't be reached in a test.
- **Hand-written talks:** a hand-written `\|` in notes now reads as `|`, as `\-` already does. Notes written by hand whose last line ends in `|` before a table still break the table; the fix covers notes typed in the app.
- **Seen in the run, from earlier plans:** a typed `- ` doesn't start a list. Ctrl+V after Escape twice pasted nothing, where the Paste tool did.
- **The disk:** the final reviewer's own copy of the branch took 28 GB while it ran and was removed afterwards.
