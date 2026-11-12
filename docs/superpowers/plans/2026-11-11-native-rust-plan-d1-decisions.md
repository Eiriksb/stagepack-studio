# Plan D1: decisions made while building

The calls made during plan D1 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. They are in the order they were made, so a later one can replace an earlier one. The final review triaged every smaller finding, and those it wanted fixed were fixed in its fix wave, so some entries below are closed. `docs/checks/plan-d1.md` has what ran.

## For the start of plan D2

The final review found three things D2's typing needs before it starts, and D1 doesn't:

- An incremental reparse with a text-only deck update. A full parse of a 500-slide talk takes 30 to 40 ms, and every edit does one.
- An `edit_slide` check that requires every other slide's bytes to be the same before and after. Counting slides isn't exact once people type code fences on a slide.
- Splitting the window's save and working-copy state out of the view into two small plain pieces, `SaveFlow` and `WorkingCopies`. It holds twelve fields now and D2 adds text being typed.
- New slides are blank, with nothing to click on. Empty regions need a "Click to add text" placeholder.

## Rulings

- **Setup.** No linked worktree, as for plans A to C. Cost if wrong: The owner's uncommitted roadmap edit shares the checkout, and it stays unstaged.
- **Setup.** A subagent's commit keeps the attribution line its own session gives it.
- **Setup.** The Insert and Animate tabs exist from D1 with one line saying their tools are on the way. The owner asked for categorised tool tabs, and dead buttons would mislead. Cost if wrong: Two nearly empty tabs until D3 and D4.
- **Setup.** Ctrl-m becomes New slide, as in PowerPoint, and the Markdown panel moves to ctrl-shift-m. Cost if wrong: A rebinding.
- **Setup.** A hidden slide is left out of a PDF and written with show="0" in PowerPoint. That's what PowerPoint itself does. Cost if wrong: A setting in one export.
- **Pre-flight.** In T3, a reload from disk that changes the text always empties the history, since its ranges no longer fit. Cost if wrong: Undo doesn't cross an outside change.
- **Pre-flight.** In T1, set_notes makes "</Notes" in notes harmless by writing "<\/Notes" only if the parser reads that back as the same text; otherwise it inserts a zero-width space after "<" and documents it. Notes are plain words for a speaker, and exactness matters less than never breaking the slide. Cost if wrong: An invisible character in a note that contains that tag.
- **Task 1.** Notes are stored escaped and read unescaped (\\ and \< and whatever else the parser trips on), replacing the pre-flight ruling. Neither form it named parses, and people who don't code will type anything into notes. Cost if wrong: Hand-written notes containing \\ or \< read with one character fewer.
- **Task 1.** An operation adds a blank line to the break it writes where a slide of only key: value lines would otherwise be read as a settings block. An edit must never change what someone else's slide means. Cost if wrong: One blank line beside such a slide.
- **Task 1.** An unclosed code fence, and a later slide's own title entry travelling with the talk's settings once it has been first, are left and documented. Cost if wrong: Rare surprises on files that were already odd.
- **Task 1.** Document::edit (Task 3) refuses any edit that turns a slide that parsed into one that doesn't, and says so. A safety net under every visual edit, for cases like 65 nested quote marks typed into notes. Cost if wrong: An edit is refused that a person could have fixed by hand.
- **Task 1.** Document::edit (Task 3) also refuses an edit that changes the number of slides by anything other than what its command expects (0, +1 or -1). The review showed a notes removal merging two slides with no parse error. Cost if wrong: None, each command knows its count.
- **Task 1.** When notes are all that keeps two lists apart, removing them leaves an empty Notes element. No blank line can keep the lists apart. Cost if wrong: An empty element stays in the file.
- **Task 3.** History::record takes the slide before and after, undo and redo take the talk's current text, and extend_last merges notes typed in a row. The plan's signatures couldn't show where an undone move came from. Cost if wrong: None.
- **Task 4.** Save As asks its kind (Talk folder or Packed talk) in the app's own dialog before the system's Save dialog. The system dialog can't offer file types through GPUI. Cost if wrong: One extra click.
- **Task 5.** Each window keeps its working copy in its own slot under the talk's key, offered newest first, instead of a pending/ folder. It also keeps two windows on one talk from overwriting each other. Cost if wrong: A window opening a talk already open elsewhere is offered that window's live copy.
- **Task 6.** Ctrl-z inside the notes field uses the field's own undo, and the ribbon's Undo uses the talk's history. A text field's undo is what people expect while typing in it, and the typing still lands as one notes edit. Cost if wrong: Two undo behaviours depending on focus.
- **Task 8.** The button reads "Hide slide" and "Show slide", against Task 6's brief. On its own, "Show" beside the Slide Show tab reads as starting a show. Cost if wrong: A label.
- **Final.** Every Critical and Important finding, the working-copy and close minors, and the review's cheap usability items are fixed in one wave. Cost if wrong: None, each is small.
- **Final.** Three things are left for the start of plan D2 and recorded there: an incremental reparse with a text-only deck update (a full parse of a 500-slide talk is 30 to 40 ms per edit), an edit_slide net that requires every other slide's bytes to be unchanged, and splitting the window's save and working-copy state into SaveFlow and WorkingCopies. D2's typing needs all three and D1 doesn't. Cost if wrong: D2 starts with three preparatory tasks.
- **Final.** A format-crate panic during an edit becomes a refused edit. Nothing in a talk may crash the app, and the property tests' generator missed real shapes. Cost if wrong: A bug shows as a refusal instead of a crash report.
- **Final.** Typing and then clearing a note on a talk that ends in a bare --- with no final newline leaves one line ending behind. It's inside the slide being edited, each step still undoes exactly, and telling it from one that was already there isn't possible after the clamp. Cost if wrong: Such a talk reads unsaved after a note is typed and deleted.
- **Final.** File keeps two items, "Open talk folder…" and "Open talk file…". The system dialog picks files or folders, never both. Cost if wrong: One more menu item.

## Smaller findings and notes

- **Task 1.** A one-line <Notes>hi</Notes> isn't read as notes by the parser, so set_notes adds a second element beside it.
- **Task 1.** Remove_notes has an unreachable branch; 65 or more nested quote or list markers typed into notes hit the parser's nesting limit.
- **Task 1.** Import order in slide_ops.rs; one extra parse per notes removal.
- **Task 2.** A long pass of thumbnail() calls can make the current slide's layers the oldest and evict them; evict thumbnails before layers. Carry to Task 6, which makes the thumbnail pass.
- **Task 2.** Deck::thumbnail panics on an index out of range; shown_slides() scans every slide several times a frame, cache it per update. Carry to Task 6.
- **Task 2.** No test for hiding or removing a slide during a show; the building-thumbnail test would pass if steps were ignored; slide_xml_shown_or_not is an awkward name; no thumbnail test with pictures or components.
- **Task 2.** Sandbox a_schema_that_never_returns_is_stopped failed once more under full parallel load. A flaky timing test worth loosening in a final wave.
- **Task 3.** Undoing past 64 MiB of redo text drops the whole undo stack at once (trim pops undo entries that free nothing); drop the far end of redo first. Fix in the final wave.
- **Task 3.** The notes typing run joins on the label string; no test for the saved marker inside a typing run; new test lines aren't rustfmt-formatted.
- **Task 3.** Each keystroke clones and diffs the whole text. Note for D2, where typing on the slide uses the same path.
- **Task 3.** Document::move_slide and set_notes pass indices straight to the format crate; UI callers must only pass real indices (check in Task 6), and a reload over unsaved edits marks the text saved, which Task 4 handles before calling reload.
- **Task 4.** Saving a packed talk on Windows is reasoned, not tested: expected to work on NTFS on Windows 10 1809 or later, and to fail with a message on FAT or exFAT drives. For the by-hand Windows check.
- **Task 4.** Quitting the whole app isn't intercepted, only closing a window; the app has no Quit item. workspace_view.rs is about 1,250 lines.
- **Task 4.** Repack_with_slides replaces talk.json only if the pack has one; workspace_view.rs grew by about 450 lines and saving could be its own module (Task 6 moves what it touches).
- **Task 5.** A working copy has no Windows ACL handling beyond the user's own data folder; the dispatch asked only for Unix modes.
- **Task 4.** The fix agent was stopped by the owner after committing a2a7d17 and before writing its fix report; the controller ran the workspace tests on it (1044 passed, 0 failed, 8 ignored) and the re-review works from the diff.
- **Task 4.** A packed talk gets no outside-change check before it's repacked.
- **Task 5.** Two app processes on one talk aren't guarded by a lock file; key folders from the layout before slots are never swept (only from earlier builds of this branch).
- **Task 6.** Older tests changed for the new layout: ctrl-shift-m for Markdown, canvas scale and Markdown position checks, and Present found on the Slide Show tab. Saving's window code moved to workspace_view/saving.rs, editing commands to workspace_view/editing.rs.
- **Task 6.** A doubled doc comment on rows_shown; a refused edit runs deck.update twice and redraws the touched slides' thumbnails.
- **Task 7.** It also fixed a Task 6 bug: scrolled past about row 25, the list took its one-row measuring call for what's on screen and redrew thumbnails forever. Regression test added.
- **Task 7.** Two wall-clock tests in slides-components (a_compile_that_fills_the_disk_is_stopped, a_schema_that_never_returns_is_stopped) fail now and then under full parallel load. Task 8 loosens them.
- **Task 8.** New slides are blank and all five layouts look the same until D2 gives empty regions a "Click to add text" placeholder. This is the first thing D2 must fix.
- **Task 8.** The system Save dialog and the "Packed talk" choice in the window weren't pressed for real; workspace_view::init sits beside bind_keys and three older test files still call bind_keys.
- **Task 8.** Slide_canvas computes the slide's edge twice and tests the stored rectangle, not the paint; a four-digit slide number likely spills past its column; the 50 s bound in the disk test is close to vacuous.
- **Task 8.** Looks: thumbnail text is tiny, the New slide chevron sits apart from its button, the title hugs the Export menu, the notes box is tall for one line.
- **Final.** Deck::update matches slides by index, so adding or deleting slide 3 of 500 redraws every later thumbnail; when every slide is hidden a show plays them all but PowerPoint gets them all hidden; a killed save leaves a temp file beside the talk; a one-line <Notes>hi</Notes> isn't read as notes; two app processes on one talk aren't guarded; starting the app with no file opens a folder chooser and Cancel quits.
- **Final.** The answer bars cover the top 16 px of the slide, and about 50 px when both show; the "Saved" status line coming and going still moves the slide by one line.
- **Final.** A pack that opens but whose slides.md can't be read is written over without asking (the packed version of the folder check).
- **Final.** Undo, redo, recovery, loading the changed file, opening and reloads still parse outside catch_unwind, and Deck::update_parsed runs unguarded after a candidate is accepted. For D2.
- **Final.** On the nested test display in the last session, clicks landed at three quarters of their x position and screenshots were squeezed to three quarters of their width, in this build and in one from before plan D. Nothing in the fix diff touches window size or scale. Not seen on a real desktop, and not checked on one.
- **Final.** Panic_in_next_change is a doc-hidden public test hook compiled into the release library; nothing in the app calls it.
