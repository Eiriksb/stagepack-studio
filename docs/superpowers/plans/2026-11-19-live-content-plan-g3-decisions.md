# Plan G3: decisions made while building

The calls made during plan G3 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-g3.md` has what ran.

## Questions for the owner

- **Nothing was run on Windows,** and how smoothly a recording plays is a hand check on a real screen (`docs/checks/plan-g3.md`).
- **A recording made on your own computer** with `asciinema rec` hasn't been played: every recording tested was made by the tests.
- **Right-to-left text and letters a font draws wider than a cell** are kept on their cells, narrowed if need be; whether that reads well is for a look in a window.
- **PowerPoint gets a recording's last screen as a picture,** and PDF as real text.

## Rulings

- **Setup.** The plan wasn't held for the owner's review (the owner said to go ahead). The terminal screen is the app's own, not a library's: none was in the local cache, and every feature of a general one would need checking against the rule that a recording reaches nothing outside its screen. Cost if wrong: a rare sequence shows wrong until it's added.
- **Task 1.** The million-event limit counts every line, typed input included, and the 24-hour limit is on recorded time; both bound the work. Only a recording's text counts against what it may keep (twice the largest file); its events and marks are bounded by their number, so one can hold about 136 MB at worst while read.
- **Task 2.** Up to two combining marks are joined to a cell, a third dropped. Line drawing is drawn. The screen charges its work and refuses a recording past about a second and a half of it as too large; a slower computer takes longer to refuse.
- **Task 3.** A recording's header is read on the layout thread for its size; its marks come with its still. Busy screens are shaped and drawn aside, an earlier picture standing in meanwhile, stretched while resized or zoomed.
- **Task 4.** Coming back to a slide from a later one shows a recording's end, as a model is shown at its last view; going forward starts it from the beginning. Before a replay's first picture its box shows a plain panel, never its last screen. The next slide's recordings are readied ahead.
- **Final.** Recordings stay within their boxes and letters on their cells; at most four play or are readied at once per window; recordings dropped together are checked one at a time. Cancel on a check drops those waiting behind it.

## Smaller findings and notes

- **Fixed by the reviews, before the end:** a crafted recording could hold gigabytes after reading, crash the player with a few hundred marks, or take minutes going back; a character could crash layout or draw a line outside the screen; recordings in packs couldn't be read; laying out and drawing busy screens held up the window; going back replayed a whole recording; its last screen showed before it started; letters ran past the box with the system's fonts; a packed talk unpacked every recording whole on opening; many recordings had no memory bound.
- **Not fixed:** zooming in past full size on a crafted busy screen takes 7 to 13 ms a frame; a long zoom's stand-in gets blockier until the new picture lands; a back step within a slide drops the next slide's readied recording, read again later; the show has no jump to a slide; a recording changed during a show isn't tested for leaving; the long recording draws about 27 pictures a second in its timing test, not 60, and why isn't known; a slide pasted from another talk checks a recording's header only; more marks than a slide's most clicks are never reached; the seam test checks only exact background pixels.
