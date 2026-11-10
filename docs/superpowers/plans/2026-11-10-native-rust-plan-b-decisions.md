# Plan B: decisions made while building

The calls made during plan B without asking the owner, and the smaller findings left for later. Each decision says what it costs if it's wrong. The final review triaged every smaller finding, and those it wanted fixed were fixed in its fix wave, so some entries below are closed. `docs/checks/plan-b.md` has what ran.

## Rulings

- **Setup.** No linked worktree; Task 1 creates branch plan-b in this checkout. The approved plan's Global Constraints name it, and a worktree would rebuild the 31 GB GPUI cache. Cost if wrong: The owner's checkout sits on the feature branch.
- **Task 1.** LayerKey also fingerprints each layer's item positions and sizes. A click=each item with the same text and rect could otherwise reuse stale pixels when its list grows and the indent moves. Cost if wrong: An occasional extra redraw.
- **Task 2.** On a split title region only the first piece is the title placeholder. A slide has one title. Cost if wrong: One line in slide.rs.
- **Task 2.** A table over 10,000 cells exports as text with a notice. Padding short rows would be quadratic for a hostile talk. Cost if wrong: An enormous table isn't a native table.
- **Task 2.** NativeContent::Shape gains inset and centered. PowerPoint text must sit where the app draws it, and Task 6 builds on this. Cost if wrong: Two fields.
- **Task 3.** Unpack only writes slides.md, talk.json and paths under components/, assets/ and build/. A planted .git/config could run commands later. Cost if wrong: A pack with other files can't be unpacked.
- **Task 3.** Windows-hostile names are refused on every platform. Talks must stay portable, and the design ships on Windows. Cost if wrong: A file with such a name can't be packed.
- **Task 6.** Bullet::None writes buNone only in a placeholder, and Dot in a placeholder inherits the master's bullet. My dispatch wording was too strong, the code matches the JS reference output byte for byte, and a text box has no bullet to turn off. Cost if wrong: A list placed in the title region shows no bullet in PowerPoint.
- **Task 6.** Text writers take &mut TextCtx and shape_xml returns Option. The link callback is FnMut, and tables and pictures have their own writers. Cost if wrong: None.
- **Task 8.** Exported hyperlinks are limited to http, https and mailto, and anything else becomes a plain run with a notice. A talk is untrusted and could otherwise plant file: or UNC targets in a file someone sends on. Cost if wrong: Unusual link schemes lose their link in PowerPoint.
- **Task 8.** FolderFiles refuses absolute, prefixed, backslashed and .. paths before touching the disk. Canonicalising first could reach a network share on Windows. Cost if wrong: None.
- **Task 8.** A .. that stays inside the talk is resolved by text rather than refused. Packed files accept it from Task 3's fix, and the two kinds of talk must read paths alike; a path that climbs out is still refused. Cost if wrong: None.
- **Task 9.** A font named only in a buFont isn't collected or warned about. It's the bullet character's font, Arial, which the person never chose and PowerPoint always has. Cost if wrong: None.
- **Task 9.** A family named in a different case is embedded under the font's own name, with the slide's typeface unchanged. The alias renaming in the JS exists for web font names this app doesn't produce. Cost if wrong: PowerPoint might not match a hand-typed lowercase family.
- **Task 10.** The Stagepack message and button show in the window's problem area rather than a dialog. A dialog can't open while the window is being built, and the message is the window's whole content. Cost if wrong: A different presentation of one sentence.
- **Task 10.** File > Open opens the talk in a new window. It avoids resetting the watcher and a stale reload. Cost if wrong: An extra window.
- **Task 12.** ScreenInfo.id is the screen's index. DisplayId only converts to u64 and the plan fixes u32. Cost if wrong: None, the index maps straight back to the display.
- **Final.** The show asks only about the slides around it, through a small TalkShape trait, and Session stays pure. Laying out every slide at F5 froze the UI on an ordinary photo talk. Cost if wrong: The trait is one seam and easy to swap for a whole-talk snapshot.
- **Final.** Pictures get a 512 MiB byte budget with least-recently-used eviction, and Deck drops the laid slides furthest away to release them. Laid.specs holds the same Arc<Pixmap> so the cache alone can't free anything. Cost if wrong: Walking back over slides re-decodes.
- **Final.** A JPEG goes into the PDF as its own bytes and one picture is embedded once, no downsampling. 98 MB against 19.6 MB for the same talk. Cost if wrong: The fallback path is the existing pixmap one.
- **Final.** A picture outside assets/ is reported by name at Save as packed, the allow-list doesn't grow. Unpack refuses unknown top-level names by design. Cost if wrong: The person keeps seeing a sentence they can act on.
- **Final.** Click="5" alone stays five presses in the app, PowerPoint collapses to distinct clicks. The app is literal and PowerPoint has no empty clicks. Cost if wrong: Plan C revisits this once components add steps.
- **Final.** Temp files from a killed export are left alone. A stray hidden file, nothing damaged. Cost if wrong: A sweep at start-up is a small later change.
- **Final.** The LibreOffice font difference is a LibreOffice limit (24.2.7 doesn't load fonts embedded in a pptx, Inter isn't installed here). Blue bullets were a real defect, fixed. Cost if wrong: Whether PowerPoint loads the fonts stays in the Windows hand-check.
- **Final.** Opening a pack that holds a hidden name below an allowed folder is refused outright, not only unpacking. The rule sits in safe_part and one name check is simpler than two. Cost if wrong: A hand-zipped pack with a stray dot file gets an error naming the file, and nothing is damaged.
- **Final.** Numbers get buClr as well as bullets, as the JS writes it. A number can differ from its first run's colour, a link for instance. Cost if wrong: No cost.
- **Final.** The re-review's three minors and its out-of-scope note are fixed, not parked. A space in a mail link, a CMYK JPEG's colours, a test that no longer reached its cap, and slides.md read without the size and regular-file rule. Cost if wrong: Each a few lines, and the last is the same hole as Important 7.

## Smaller findings left for later

- **Task 1.** Slide.rs import order and long LayerSpec literals; a click=each fallback can duplicate notices; pieces past 999 share click 999 with no notice; no tests for a bad click on a region line or a non-list block's source range.
- **Task 2.** A shape or stack inside a nested list keeps the list's level, so it would be indented in the export only.
- **Task 2.** Paragraph level can exceed DrawingML's 8; carried into Task 6, which must clamp lvl to 0..=8.
- **Task 2.** Flow.rs casts a table row index to i16; the 32 KiB slide cap keeps it far below overflow.
- **Task 3.** Metadata and File::open re-resolve a path after the walk, so a file swapped for a link in that window is followed.
- **Task 3.** Two zip names that decode to the same text silently keep the later one; a leading / reads from a pack but not a folder; a nested assets/.git or components/.cargo still unpacks.
- **Task 4.** Face_bytes copies each font twice; a -0.0 baseline can split a glyph run; no test for an unreadable font face; poppler-based tests pass silently when poppler is absent.
- **Task 5.** Package part names don't reject .. or empty segments; theme tests only use the default theme; slides-engine dependency unused until Task 6; the seen set in every_part_is_well_formed checks nothing.
- **Task 6.** A linked run with a newline calls ctx.link per piece, which duplicates relationships; an empty-paragraph literal is duplicated; a magic floor in shapes.rs; table margins copy constants from flow.rs.
- **Task 7.** Notes trim strips U+0085 where JS doesn't; notes runs are written by hand rather than through text.rs.
- **Task 8.** Text.rs doc says link is asked once per run; no test of identical bytes under two names, of a linked run with a newline, or of Override entries; an element that can't be placed is dropped silently; a picture's stored type trusts its extension; shape_xml and picture_xml are test-only wrappers.
- **Task 8.** A safe link with leading spaces is written with them; a .. through a symlink resolves differently from the OS, though it stays contained.
- **Task 9.** If a family's regular face fails but others succeed, orphan font parts remain; a family in different case isn't renamed; tests/fonts.rs has a vacuous clause for three lengths.
- **Task 10.** No test that opening a folder starts watching; a mistyped folder falls through to a packed-file error with no path; a huge font in a folder is read before the cap; timeline_of and notes_of index unchecked (carried into Task 12); a talk titled CON suggests a reserved name; the suggested file name is unasserted; test fixtures repeat; workspace_view.rs is 628 lines.
- **Task 11.** Nothing tests opacity or run grouping below the atlas, so Task 13 must watch a fade; a stale doc comment on images().
- **Task 12.** Starting a show lays out every slide on the UI thread, decoding every picture, and PictureCache has no total limit, so F5 stalls and a crafted talk could exhaust memory (for the final review).
- **Task 12.** The transition clock starts before the incoming slide is drawn; a redundant into_iter; present_view and workspace_view import each other; no test that two canvases don't share images.
- **Task 13.** Docs/checks/plan-b.md Windows step 1 clones a branch that isn't pushed yet; the preamble covers it, step 1 alone doesn't. Carry into the final fix wave.
- **Task 13.** Docs/checks/plan-b.md says the run was "at commit ecde000", but fixtures/show came with 49e8d28. Carry into the final fix wave.
- **Task 13.** The 640x800 presenter window looks cramped, large black letterbox areas round its current slide, next step and notes. The layout may want a look on a real screen.
- **Final.** Icons and SVG paths aren't in the picture budget, so many huge icons on one slide can still take a lot of memory. Unchanged by this plan.
- **Final.** A PDF export of a talk over the budget lays let-go slides out again when it collects notices. Only time, on the background thread.
- **Final.** A list's text starts 6 pt further left in PowerPoint than on the slide, because of the hanging indent. Recorded in docs/checks/plan-b.md.
- **Final.** Plan C: the click-gap rule, ExportInput and Deck::open taking a component registry, and show_on as the seam for a click step.
- **Final.** Plan D: a click=each item's source excludes its marker, and an empty item falls back to the whole list's range, so an edit by byte range must refuse that case.
