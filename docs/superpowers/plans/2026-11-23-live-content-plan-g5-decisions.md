# Plan G5: decisions made while building

The calls made during plan G5 without asking the owner, and the smaller findings left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-g5.md` has what ran, what's left and the hand checks.

## Questions for the owner

- **Nothing was compiled or run on Windows.** The Windows target isn't installed here and installing it was refused by the permission check, so the helper's job object, ConPTY, the `.exe`-only rule, network paths and quotes are written and read against the libraries' sources only. The first Windows build may not compile.
- **The give-back key is Ctrl+Shift+Q** on every system, not the design's Ctrl+Shift+Escape, which Windows keeps for Task Manager. It's a constant; say if you want another.
- **Removing an *Always*** needs the app's `terminal-permissions.json` edited by hand. Is a button needed?
- **"Always" for a program the talk brings** (`./demo`) binds its path, not its contents, so an edited file runs without asking again. Hide *Always* for those, or bind the contents too?
- **On a Mac, Option-plus-a-letter is text,** so a terminal program's Alt shortcuts (Alt+B) don't reach it.

## Rulings

- **Setup.** A separate helper program owns the pseudo-terminal (`portable-pty =0.9.0`), so the app never links one; app and helper speak in short binary frames over stdio. Cost if wrong: none found.
- **Task 1.** The helper puts the program in its own group (Unix) or a kill-on-close job (Windows), ends it politely then by force after 2 s, and sets `TERM`, `COLUMNS` and `LINES` itself.
- **Task 2.** Output past 4 MB a second is dropped, and the half-read sequence cleared; on a slow machine fewer pictures are drawn rather than output dropped. `Session::end` kills the helper 5 s after being asked if it hasn't ended. The live screen answers three questions: `CSI 5 n`, `CSI 6 n`, `CSI c`.
- **Task 3.** `src` is no longer needed on a terminal; `run` alone is valid. A bad `run` with a good `src` still shows and plays the recording, and the complaint is a notice.
- **Task 4 (four security rounds).** One gate, `may_run`, refuses bad runs and gives a `Permit` that alone can start a session. The program is resolved by the app before asking: bare names only in absolute PATH entries and never inside the talk; a program written as a path is marked as the talk's. Every invisible, format, control and right-to-left character is refused or shown as its code; the question shows the full path, where it runs and the warning above the words, with a scrollbar always drawn. On Windows only `.exe`/`.com`, no network paths, no quotes in arguments. *Always* is per folder and exact resolved command; never for a pack or an untitled talk.
- **Task 5.** Terminals are followed through edits; keys go only to a terminal on the slide being looked at, are released on blur, slide change, a question and the show starting.
- **Task 6.** A kept recording's name is the program's name reduced to safe ASCII, made unique in `assets/`; *Keep* works after the program has ended.
- **Final.** The helper leaves the app's process group and ignores hang-up, interrupt and quit, so Ctrl+C in a terminal can't orphan a program; a failed or ended terminal isn't restarted on its own; the presenter's view shows the live terminal and takes the keys; Windows programs get a plain `C:\` folder.

## Smaller findings and notes

The full list is in `docs/checks/plan-g5.md`, "Not done, and for the owner". The ones most likely to matter: Windows mapped network drives aren't refused; a program that leaves its process group, or a `kill -9` of the helper, escapes the end on Unix; several behaviours (keys given back as a program ends, a panicking session, the 32 MB keep limit, Redo of a kept recording) have no test of their own.
