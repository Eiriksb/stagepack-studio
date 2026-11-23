# Plan G5: what ran, and what to check by hand

The run was on the `plan-g5` branch, in a debug build, on a 16-core Linux laptop, with Rust 1.98.0, on 2026-11-23. The branch hasn't been pushed or merged. **Nothing here has run on Windows, and none of the Windows code has ever been compiled.**

## What was built

A slide's `<Terminal run="bash" …/>` can now run a real program while the owner presents, typed into with the keyboard, but only with the add-on present and a deliberate yes for that command.

- **The helper**, `stagepack-terminal` (`crates/terminal-helper`): runs one command in a pseudo-terminal (`portable-pty`, the only place it is used) and speaks to the app in short binary frames over its standard input and output. It ends the program's whole process group (Unix) or job (Windows), first politely and after 2 seconds by force, and also ends it when its input closes.
- **In the app**, `crates/app/src/live_terminal/`: finds the helper (`STUDIO_TERMINAL_HELPER`, else beside the app's executable); a `Session` feeds the program's output into the same bounded screen the replay uses and draws it off the window's thread (output past 4 MB a second is dropped, and the screen says so); keys are translated for the program; the screen answers only three questions, the cursor position (`CSI 6 n`), the device attributes (`CSI c`) and the status report (`CSI 5 n`), each with a fixed answer that holds nothing the program chose.
- **The yes** (`permission.rs`, `resolve.rs`, `ask.rs`): the app resolves the program itself, refuses commands with hidden or odd characters, shows the whole command, where it runs and whether the talk brought the program, and asks *Run*, *Always for this folder* (folder talks only) or *Don't run* (the default). A packed or untitled talk is never remembered; a changed command asks again; a session can start a program only with a permit that the gate alone makes.
- **Running in the show and the editor**: the show asks on reaching the slide; the editor has *Start* on a selected terminal. A click gives a running terminal the keys (a 3 px frame); Ctrl+Shift+Q gives them back, and a small hint at the terminal's foot says so. Ending the show, the talk or the window ends the programs; edits above a running tag move it along instead of ending it; at most 4 run per window.
- **Keep as recording** (this task): on a selected terminal that has run (running or ended), the button takes what the session showed, as an asciinema `.cast` of its output with times and no input, adds it to the talk as `assets/<program>.cast` through the added-files path, and sets the tag's `src` with one byte-range edit. The name is the program's file name without folders or extension, reduced to ASCII letters, digits, `-` and `_`, at most 40 characters, `terminal` if nothing is left, then made unique (`bash.cast`, `bash-2.cast`). Undo takes the `src` away, and the file is not saved because the text no longer uses it. Past 32 MB the recording stops, and the button says *This session is too long to keep.* and does nothing.
- The AI tool's guide says the tool never runs a command and an AI should give a `src` beside a `run`; it now also says the owner can make that recording with *Keep as recording*.

## How to try it

1. Build both programs: `scripts/cargo-safe.sh build -p slides-terminal-helper -p stagepack-studio`.
2. Tell the app where the helper is, or put it beside the app: `export STUDIO_TERMINAL_HELPER=$PWD/target/debug/stagepack-terminal`.
3. A scratch talk, in an empty folder, `slides.md`:

   ```
   # A live terminal

   <Terminal run="bash" cols="80" rows="24" x="100" y="150" w="1200" h="700" />
   ```

4. Open the folder in the app. Select the terminal on the slide and press *Start*; the question lists the command in full. *Run*, click the terminal, type. Ctrl+Shift+Q gives the keys back. *Keep as recording* then fills in `src`. Or press F5 and reach the slide: the same question is asked there.

## Tests on Linux

At the run's last commit (the whole suite, `scripts/cargo-safe.sh test --workspace --no-fail-fast`, run in the background on a loaded machine, load average 5 to 8):

- 128 test results: **2,723 passed, 1 failed, 56 ignored**. The ignored ones are the release timing tests and tests that need the compiler bundle.
- The one failure was `once_the_drawer_is_closing_nothing_starts_another` in `crates/app/tests/ui_model.rs`, a count of drawer threads that depends on load and failed in the same way in plan G4's run. Run alone it passed (0.59 s). A first attempt at running it alone was cut off by my own 100 second limit while the test binary was still being built, so that run says nothing. Nothing in this plan touches the 3D drawer.
- `ui_live_terminal`: 13 passed, 0 failed, including the three tests of this task: `a_session_kept_becomes_the_recording` (`printf hello`, *Keep as recording*, the tag has `src="assets/printf.cast"`, nothing is on disk until a save, Undo takes the `src` away and a save then writes no file, kept again and saved the file holds `hello` and no input events and reads with `slides_terminal::read`), `a_kept_recording_plays_like_any_other` (*Play* is offered on the tag and a picture is drawn) and `a_kept_recording_is_named_from_the_program_and_never_leaves_its_folder` (program names with `..`, `/`, `\`, spaces, non-ASCII letters, a bare `..` and a very long name all give a plain name of at most 40 characters).
- `ai_tool`: 29 passed (the guide's examples still check clean with the new sentence).
- The tests of the live terminal use the real helper and only commands they choose themselves (`sh -c`, `cat`, `printf`, `sleep`), with scratch `HOME` and `XDG_*` folders and no real display; after each, no program and no thread is left.

Not tested by a headless test: the *This session is too long to keep.* label (a session of 32 MB takes too long to make; the cast stops at its own limit, which the flood test only checks loosely, and the label and the refusal follow `Session::can_keep` and `Session::recorded`), and the still showing `hello` once `src` is set (checked only as far as *Play* drawing it).

## A real run, end to end

With no window and no display, on a scratch folder with scratch `HOME` and `XDG_*`:

1. A script (Python, outside the repository) started the real built `target/debug/stagepack-terminal`, sent a `Start` frame for `/usr/bin/printf hello` in the scratch talk's folder with a 40 by 6 screen, and read what came back: `Started`, `Output` of `hello`, `Exited` with code 0. From the output it wrote the `.cast` (`{"version": 2, "width": 40, "height": 6}` and one `[0.003822, "o", "hello"]` line) to `assets/printf.cast`, the file Keep as recording makes.
2. It wrote the scratch talk's `slides.md`: `<Terminal run="printf hello" src="assets/printf.cast" cols="40" rows="6" … />`.
3. `stagepack-studio check` on the talk: *No problems found.*, status 0. `outline` listed the slide. `picture` wrote slide 1 as a PNG with status 0. The scratch data, cache and config folders held nothing afterwards, and no program was left running.

The step that makes the file inside the app, with the button, is the headless test `a_session_kept_becomes_the_recording`.

## Not done, and for the owner

These were found in review and left, by ruling, as not blocking; none lets a talk run a command without a yes.

- **Gate, platform edges:** Windows mapped network drive letters (a drive of type remote) aren't refused; on macOS a path under `/System/Volumes/Data/net` reaches the automounter; Linux autofs `/net` isn't checked; a link inside the talk that leads to a network path stays a noted gap.
- **Gate, wording:** bare names send the unsettled PATH entry and the name; some overflowing commands get the scrollbar but not the notice about length.
- **Removing an *Always* entry** needs `terminal-permissions.json` in the data folder edited by hand. Is that enough, or does it need a button?
- **Helper, Unix and Windows:** a program that leaves its process group (`setsid`, a daemon) escapes the end on Unix; on Windows the job is assigned just after the program starts, and *End* waits 2 seconds when background children remain; there is no test of a bad `Start` JSON; `Exited` may wait on an app that has stopped reading (it is bounded).
- **Tests:** the path that drops a flood is covered only by a unit test, because this machine's pseudo-terminal is slower than 4 MB a second; keys given back the moment a program ends, a session counted while ending, and a yes used on a terminal changed meanwhile have no dedicated tests.
- **Wording:** a failure very early in the helper's start doesn't always say what the helper said; a comment above a wrong arm at `selection.rs:209` is misleading; the inspector shows no complaint about `run` for a terminal that has a good `src`, though the notice and `check` do.
- **Names:** a kept recording's name is lower case, and an underscore in the program's name becomes `-` (the same rule every added file's name follows), where the ruling said underscores stay.
- **The give-back key,** Ctrl+Shift+Q on every system (Windows keeps Ctrl+Shift+Escape for Task Manager), is a constant, a change from the plan's first choice.
- **"Always" for a program the talk brings** (`./demo`, `sh demo.sh`) binds that program's path, not its contents: after a pull or an AI edit of the file, it runs changed without asking. Should *Always* be hidden when the program comes with the talk, or bound to the file's contents too?
- **Stage E** decides whether a shipped build honours `STUDIO_TERMINAL_HELPER` (it is the owner's environment, not the talk's, but a shipped build may want to look beside the app only). A SIGKILL to the helper itself (out of memory, `kill -9`) still leaves the program running on Unix, like a program that leaves its group.
- **Packaging the helper** is stage E's: until then it is found only through the variable or beside the app.

## Hand checks for the owner

On Linux (or macOS), on a scratch display, with a scratch talk:

1. A real shell in a show: `run="bash"`, press F5, reach the slide, answer *Run*. Type commands, see the output on the slide and in the presenter's view (which shows the same picture of the current slide). With two screens, click the terminal in the presenter's window: it takes the keys there, with the frame and the hint.
2. Typing: letters, the arrows, Tab, Ctrl+C in a running `sleep 100`, Escape inside `vim`.
3. Ctrl+Shift+Q gives the keys back: the arrows then change slides.
4. A full-screen program: `htop` and `vim` (colours, cursor movement, leaving them cleanly).
5. *Keep as recording* in the editor after running a few commands: `assets/bash.cast` is made, `src` is set, *Play* plays it, Undo takes `src` away, Save writes the file only while a tag names it, and a pack made from the talk holds it.
6. *Don't run* shows the recording for the rest of the session without asking again; *Always for this folder* is remembered after closing and opening the talk, and a one-byte change to `run` asks again.
7. If Docker is there: `run="docker run --rm -it alpine sh"`, then end the show, and see with `docker ps` that nothing is left.
8. Close the window and the app with a program running, and see with `ps` that no program is left. Also start the app from a terminal and press Ctrl+C there, close that terminal, and `pkill stagepack` with a program running that ignores hang-up and terminate (`trap '' HUP TERM; sleep 1000`): no program is left after a few seconds.
9. On a Mac, type `@ | { } [ ] \ $` with Option on the Norwegian layout into a shell: each arrives as the character, not as an escape before it.
10. Run `printf hello` in the editor, let it end, press *Start* again: it runs again (the unkept recording of the first is let go of).

On Windows, from a fresh clone, with PowerShell (none of this has run):

1. **Build at all.** The Windows code in the helper and the app was written but never compiled here (the Windows target isn't installed on this machine), so `cargo build` on Windows may fail to compile first. Report the errors; they are for a fix round, not a surprise.
2. ConPTY: `run="powershell.exe"` and `run="cmd.exe"` start, show output and take typing, and a full-screen program draws.
3. The job object ends grandchildren: `run="cmd.exe /c start /b ping -t 127.0.0.1"` or a script that starts others, then *Stop* and ending the show; check Task Manager for what is left.
4. Programs that are `.exe` only: `run="node.exe …"` runs; a `.bat` or `.cmd` is refused with a sentence; `run="cmd.exe /c script.bat"` written in full runs after the yes; a double quote inside an argument is refused.
5. Network paths are refused before they are touched: a program written as `\\server\share\x.exe` and one on a mapped network drive letter (the second is a known gap, see above).
6. The give-back key, Ctrl+Shift+Q, reaches the app (Ctrl+Shift+Escape never would) and gives the keys back.
7. `run="cmd.exe /c script.bat"` finds the script in the talk's folder: the program's folder is written `C:\…`, not `\\?\C:\…`, and the question shows the same.
8. *Keep as recording* on Windows: the file name is made from `powershell.exe` as `powershell.cast`.

## The fix wave after the final review

The helper now leaves the app's process group, ignores hang-up, interrupt and quit, and ends the program's group on terminate. A terminal that fails to start, or ends, stays as a kept session in the show and is not started again on its own; *Start* in the editor runs an ended or failed one again, through the same yes. The presenter's view shows the current slide's live terminal and takes a click for the keys. Programs on Windows are given a plain `C:\…` folder where one exists (written, not compiled or run here). Option-typed characters on a Mac go as the character. A thread of a session that panics makes it failed and ends its program. New tests: the helper's three signal tests, `the_presenter_sees_the_live_terminal_and_a_click_there_gives_it_the_keys`, `a_terminal_that_fails_to_start_in_the_show_is_tried_once`, `start_runs_a_program_that_ended_again`, the plain-path test and the Option test.

## The final review

To be written by the controller.
