# Plan G1: what ran, and what to check by hand

The run was on the `plan-g1` branch, starting from commit 3759890, on a 16-core Linux laptop, with Rust 1.98.0, in a release build, on 2026-11-19. It found one thing to fix, with a test that failed before its fix: a video kept playing in the editor after a click in the slide's words began typing there. A second change lets a window write how many video pictures it shows, skips and releases each second, which is how the playing below was measured, and which the hand checks use. Both are under *What it found*. The branch hasn't been pushed or merged yet. Nothing here has run on Windows.

Every video in this run and in the tests was made by the app's own test maker, `slides_video::make_test_video`: pictures of plain colours, a white bar sweeping across each second, and in the large one a band of changing speckles a third of the picture high, so that the file is large. None of the owner's own videos was used. **A made clip decodes far more easily than filmed footage**, so every decoding time below flatters: the spikes' record has the times for real clips, 14.5 ms a 1080p picture at the median and 70 ms at the 99th percentile.

## Tests on Linux

At the run's last commit, `scripts/cargo-safe.sh test --workspace` passed: 2,406 tests, 0 failed, 30 ignored, no warnings. The four new ignored ones are the release timing tests below. One test of the video crate, `dropping_it_ends_the_thread_quickly`, failed two runs in three on its own before this run's changes too: it looked for the decoding thread's name the moment playing began, before the thread had named itself. It now waits up to 200 ms for the name, and passed five runs in five. `scripts/cargo-safe.sh clippy --workspace --all-targets` gave no warnings.

The random editing session, `crates/app/tests/typing_random.rs`, has its three parts as before: typing, arranging, and the window. The window's part now has three more kinds of step among its 70: a made video put on the slide, from the Video tool's dialog or dropped at a random place; a video on the slide selected and played or stopped, by its button or Space, with a few twentieths of a second let pass; and the show, started with F5 or Shift+F5, stepped through with Right, Left and Space, and ended with Escape from one of its windows at random. After every step, besides the checks it had, a video plays in the editor only while it alone is selected, and the show's windows are all gone once Escape is pressed; at the end nothing plays. The talks opened from a folder now keep their working copies in the run's scratch folder too, as a video is copied in there.

That check found the fix below at its first run, at seed 1, step 57: a click in the slide's words, as typing began there, left the video playing. With the fix in, `PROPTEST_CASES=500 scripts/cargo-safe.sh test -p stagepack-studio --test typing_random` passed in 7 minutes 11 seconds, the window part with five seeds for each of its four starts.

The release timing tests, `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`, ran three times, and the four new ones twice more while they were written. The four new ones are on a talk of twenty slides, each with a heading, words, a list and a video of 1280 by 720, none playing, and on one video of 1920 by 1080 played:

| What | Median | Slowest |
| --- | --- | --- |
| A keystroke beside a video, on a talk of twenty videos, the slide laid out and drawn again | 2.5 to 3.6 ms | 4.7 to 11.7 ms |
| A frame of a drag of a video on that talk | 0.008 to 0.017 ms | 0.027 to 0.051 ms |
| Opening that talk as the window does, every slide drawn small and the first drawn, against the same talk without its videos | 18.3 to 18.7 ms against 16.9 to 17.3 ms; once 35.6 ms against 33.4 ms | |
| Its twenty stills made after, one after another, off the window's thread | 134 to 258 ms | |
| A 1920 by 1080 video played for 300 pictures: taking each picture from its decoding into the window's image | 0.0056 to 0.0077 ms | 0.013 to 0.015 ms at the 99th percentile |
| The same: the whole frame drawn with that picture, in a test window that draws to no screen | 2.0 to 3.4 ms | 4.4 to 7.5 ms at the 99th percentile |

The budgets are 8 ms for a keystroke and 6 ms for a frame of a drag, at the median, and opening may take half a second more than without videos: all hold. A video dragged alone moves its drawn layer, so its frame is far quicker than the sixty things'. Taking a picture to the window costs so little because the picture is decoded and turned into window colours on the video's own thread, and handed over as it is; painting it on a real screen, about 1.5 to 1.9 ms a 1080p picture in the spike, isn't in these numbers. The ten timing tests from before ran within their budgets as in D5, the slowest single keystroke, in a table cell, at 19.8 ms.

## The scratch display, and the owner's data

As in plan D5: a nested X display, `Xephyr :86` at 1,600 by 900 with no window manager; every launch through one script that refuses to start without that display and a scratch message bus, with `DISPLAY=:86`, `WAYLAND_DISPLAY` and `STUDIO_BUNDLE` taken away, `HOME` set to a scratch home under `target/check/g1-run/home`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `XDG_DOCUMENTS_DIR` set to folders in it, and a `Documents` folder there. The running app's environment was read back from `/proc` to be sure. The system's file dialogs were answered by D5's stand-in for the desktop portal on that bus, which writes down what each Open and Save asked and answers with a path. Keys, clicks and drags went in through the X test extension, and files were dropped on the window with D3's small program speaking the drag-and-drop protocol. Screenshots are under `target/check/g1-run/shots/`, which is not committed; every one was looked at.

The real data folders, `~/.local/share/stagepack-studio` and `~/.cache/stagepack-studio`, were listed with every file's time and size before the first launch and after the last: the two lists, of 25,134 entries each, are the same, and nothing in them is newer than 2026-11-09. The modification times of the two folders and of the real `~/Documents` are the same before and after, and so is the list of what's in `~/Documents`; only the home folder's own time, which other programs on the desktop change, moved. The three files of about 300 MB the run made, the video, the saved pack and a working copy's copy, were deleted after it.

The nested display draws in software and **shows about 8 pictures a second, whatever the app asks**, as the spikes found for an empty window. So how smooth a video looks can't be judged here: that is the first hand check. What was measured instead, with the counts the app now writes when asked (below): pictures shown, skipped and released each second; processor use and memory, read from `/proc` each second.

## Real-window check on Linux

### What it did

In three sessions of the app, with a crash before the third:

- **A presentation, and a short video:** *Light* on the start screen gave *Untitled presentation*; a title was typed. *Video* on the Insert tab asked the dialog for a file (*Insert*, several allowed), and the stand-in answered a made 8-second video of 1280 by 720, 135 KB. It was on the slide at once, in the middle at 960 by 540, selected, with *Play* at its middle and *Video selected* on the status line. Its form says *Video*, its name *short-clip.mp4*, *8 seconds, 1280 by 720*, *Loop* unticked, *Still picture* *First frame*, and its place. The list of slides shows its first picture.
- **Played in the editor:** *Play* became *Stop*, the colours changed each second and the bar swept across, and at its end it held its last picture. The decoding thread was gone after it, the process back to its 66 threads. The status line said nothing.
- **The 300 MB video:** a new slide, and a made video of 1920 by 1080, 23 seconds, 310 MB, dropped on it. The status line showed a bar and *Copying video… 32%* with *Cancel*; it was in the working copy in under a second (the file was in the system's cache), and on the slide with *23 seconds, 1920 by 1080*. **Resident memory went from 314 MB to 328 MB**, and the peak didn't move. Played, it went on to its end, taking 1.3 to 1.7 cores, at 464 to 472 MB, 8 pictures shown a second, 22 to 26 skipped, and every one shown released.
- **Moved, resized, turned, looped:** dragged, it moved; dragged by a corner it kept its shape, 870 by 490; dragged by the handle above it, it turned 40 degrees; *Loop* ticked. Played, the turned video's pictures turned with it, inside its outline.
- **Five minutes looping:** that turned 1080p video looped for 300 seconds. **Memory stayed between 445 and 493 MB**, the last 270 seconds at 493 MB, flat; 1.46 cores on average; 7.7 pictures shown a second, 23.9 skipped and 7.7 released, so each picture shown was let go of.
- **Two videos and a click:** a second made video, 4 seconds of 960 by 540, put on the first slide with the Video tool, the two set side by side from the form, and the second given *Appear* on the Animate tab, *Click 1 of 1*. F5 presented: the first slide came with the first video playing and the second not shown; Right showed the second, and both played, each its own colours; Right went to the second slide, where the turned video played from its start, looping; Left came back, both playing from the start; Escape ended the show. The process was back to 66 threads.
- **Saved as a pack, closed, opened, played:** Ctrl+S asked *Keep the talk as a folder of files, or pack it into one file to send.*; *Packed talk (.sptx)* asked the dialog for *Untitled presentation.sptx* in Documents, the stand-in answered `Videos.sptx`, and the status line showed a bar and *Saving Videos.sptx… 3%* with *Cancel*, then *Saved Videos.sptx.*, the title *Videos · 1 / 2*. Resident memory while saving was 410 to 429 MB. The pack holds the three videos stored, not compressed, as the plan has it. The working copy was gone. The window closed without asking, and the app ended. Started again, *Open a talk file…* opened `Videos.sptx` with both slides' stills, and the turned video played from the pack.
- **A crash, and the talk brought back:** Ctrl+N, *Dark*, a title, and the short video dropped on it; five seconds later the app was ended with `kill -9`. Started again, the start screen listed it under *Not saved last time*, *Untitled presentation*, *Brought back*, *1 slide*; *Open* showed it with its video, which played.
- **A slide copied to another talk:** with `Videos.sptx` open in a second window, its second slide was copied from the list of slides and pasted into the brought-back talk. The paste asked *Copy the video?* *Copy 1 video, 310 MB, into this presentation?* with *Don't copy* and *Copy*; *Copy* showed the bar, then *Finishing…*, and the slide came with its turned video, the 310 MB file in that talk's working copy.
- **Exported:** *PDF…* and *PowerPoint…* from the Export menu of `Videos.sptx`, answered with files in a scratch folder: each said *Exported Videos.pdf. 3 videos were exported as pictures.* (and *.pptx*). The PDF has 2 pages of 960 by 540 points, each video its first picture, the turned one turned. The PowerPoint file, turned into a PDF by LibreOffice with a scratch profile, shows the same, the three stills as pictures in it.
- **Idle:** with two windows open and nothing playing, the app used 0.01 of a core and 361 MB.

### What it found

- **A video kept playing after a click in the slide's words.** With a video playing and selected, a click in a title or body began typing there, which lets go of the selection, but the video went on. Typing begun changes nothing of the talk, so the editor didn't look again at what was playing. It now does after every edit it makes, changed or not. Test: the case *a click in the slide's words, to type there* in `every_way_of_leaving_stops_the_decoding`, `crates/app/tests/ui_video.rs`. Found by the random session's new check, which fails without it.
- **Nothing counted the pictures skipped**, so how a video plays couldn't be measured outside a test. The decoding now counts the pictures it passes over, the canvas adds them up beside those it shows and releases, and with `STUDIO_VIDEO_COUNTS` set the window writes, each second a video plays, a line such as *video pictures in 1.04 s: 8 shown, 23 skipped, 8 released* to the standard error. Tests: `frame_for_skips_what_is_late` in `crates/video/tests/decode.rs`, and `a_late_frame_is_skipped` in `crates/app/tests/ui_video.rs`.

### What looks wrong and wasn't changed

- **On a new slide, *Click to add text* shows over a video** put in the middle of the slide, as over a picture there: the empty body area lies under it. On the title layout the title typed lay under the video entirely.
- **Played to its end in the editor, the button still says *Stop***, with the last picture held. *Stop*, then *Play*, plays it again.
- **Width or height typed in the form stretches the video**; dragging a corner keeps its shape.
- **The form's *Turn* field is empty for a video not turned**, rather than *0*.
- **The status line's bar is a few pixels taller than its words**, so the slide moves down a little while a video is copied or saved.
- **The form names the file**, *short-clip.mp4*, as a picture's form does.
- **In one screenshot of the show a picture was torn**, half one picture and half the next, as the nested display drew it. Whether that happens on a real display is in the hand checks.
- **A turned 1080p video showed 4 to 5 pictures a second on this display when played from a pack**, against 8 for the same unpacked; the display's own limit makes this hard to read. The hand check asks.

## Not exercised

- **How smooth any video looks**, on any real display. The nested display shows about 8 pictures a second.
- **Anything at all on Windows**, including building the decoder there.
- **The owner's own clips**, filmed, with B-frames, and their decoding times in the app (the spike timed them outside it).
- **The system's real file dialogs**, for *Video*, Save As, Open and Export.
- **A second screen**, and the show's presenter window with a video (covered by a headless test: it shows the still).
- **A pack from another program holding a compressed video**, played from the cache folder: covered by headless tests only.
- **Cancel** pressed on a copy or a save: the copies here were too quick to catch; covered by headless tests.
- **Desktop PowerPoint**: the PowerPoint file was drawn by LibreOffice.
- **A video on a network share, or a disk filling while it's copied**: headless tests only.

## Hand check on a real Linux display

Under an X11 session and a Wayland session, from a release build started with `STUDIO_VIDEO_COUNTS=1` in a terminal, so that it writes its counts.

1. Make a video of 1920 by 1080 at 30 pictures a second with a screen recorder or a phone, a minute long, as an MP4. Put it on a slide with *Video* and play it. Say whether it plays smoothly, and what the counts say: about 30 shown and few skipped a second is smooth.
2. The same in the show, F5, with the video filling the slide.
3. Three of your own talk's clips, among them the 392 MB one: put each on a slide, play it in the editor and in the show, and say whether any stutters, and what the counts say.
4. A phone video filmed upright: say how it shows (it's known to lie sideways).
5. *Video* on the Insert tab: say whether the system's Open dialog shows, and whether it offers MP4 files.
6. Present a talk with a video on a second screen, with Shift+F5's presenter window on the first: the video should play on the audience's screen and show its still on the presenter's.
7. Watch a video playing for a minute: say whether any picture tears or flickers.
8. Loop a video for ten minutes, and say what the system monitor shows of the app's memory: it should stay flat.

## Windows check, by hand

The branch hasn't been pushed or merged yet. Once it's in `main`, this check starts from a clone of `main`, on a Windows PC as in `docs/checks/plan-d5.md`.

1. `cargo test --workspace`. The video crate builds Cisco's decoder from its C++ source: say whether it builds with the MSVC tools, and if not, send back the first error.
2. `cargo build --release -p stagepack-studio`, and start it from a terminal after `set STUDIO_VIDEO_COUNTS=1`.
3. Steps 1 to 3 and 6 to 8 of the Linux check above.
4. *Video* on the Insert tab: say whether the Open dialog shows MP4 files, and put a video on a slide by dragging it from Explorer.
5. Save the talk packed, close it, and open it again while the video plays in another window of the same talk: say whether the save says anything about the file being in use.
6. Export it to PowerPoint and open it in desktop PowerPoint: each video should be its still picture.
7. D5's Windows check, where it's still open.

## Known after the run

- **A portrait phone video lays out and plays sideways.** The turn a phone writes in the file isn't read.
- **A video in a pack whose videos are compressed** (made by another program) shows a plain panel and no length until it has been played once, when it is unpacked to the cache folder.
- **The window's thread opens a video's file to read its size**, 64 KB at most, when its slide is laid out. On a network share that has gone away, the open itself can block. (Save no longer waits on the window's thread for a read under way: fixed after the final review, in 4a1d19a.)
- **A pack with one damaged entry no longer opens at all**, where before the rest of it did.
- **The H.264 licence, for the owner.** H.264 is patented. Cisco pays the licence for its own prebuilt OpenH264 library, not for copies built from source, as this one is. Before a public release the owner should choose between downloading Cisco's library at first use (which would use the network), shipping without H.264 and playing only free formats, or taking legal advice. Until then it's built from source.
- **Not in this plan:** sound; trimming or cropping a video; other kinds of video than H.264 in MP4; the system's own decoders; putting the video itself into PowerPoint.

## The final review

A reviewer read the whole branch at adb3891 in a copy of its own, ran the engine's and the video crate's tests, eight of the app's suites (the video, large-file, recovery, untitled, clipboard, export and added-file tests among them), the video crate's random-bytes properties at 3,000 cases and the random editing session at 150, and wrote a throwaway test to prove what it suspected. It followed lost work with large files across the tasks: the write, the talk marked saved and the working copy cleared stay in that order on every path it traced, a save that fails or is cancelled clears nothing, and a copy stopped or dropped takes only its own file away. It found nothing that loses work, crashes, or reads or writes outside the talk. It asked for two fixes that matter and seven small ones, and the one fix wave made them:

- **A video whose last picture has no time of its own was refused** as damaged, so it showed only a panel. Some programs write files that way. Such a last picture is now given one frame; a time further past the end is still refused, which keeps the earlier fix of a looped video freezing. (dd96f43)
- **Save waited on the window's thread for a video's read in progress**, so a read from a network share that had gone away froze the window. The window's thread no longer waits; the save's own background work does, and its Cancel works while it waits. (4a1d19a)
- **Smaller:** a window closing no longer starts a save from an open Save As dialog or another queued copy; Cancel in the close dialog now cancels a close that was waiting for a save; a refused video file is closed before the save goes on; a packed talk holding a video is opened again after each save in place, so the replaced pack's disk space is freed; a file is opened in a way a named pipe swapped in can't hang; and a file changed while it's copied says to try again. (3b9af08)

At 3b9af08, `scripts/cargo-safe.sh test --workspace` passed: 2,419 tests, 0 failed, 30 ignored, and clippy gave no warnings. The video crate's properties at 2,000 cases, the video tests three runs in a row, the random session and the release timing tests passed: a keystroke on the talk of twenty videos takes 3.0 ms at the median, and a drag frame 0.19 ms. A second reviewer checked the wave, re-traced the whole of the save's reader gate, and found every finding met and nothing new broken. The window wasn't run again after the wave, so its fixes have been seen in tests only.

The smaller findings, the notes for plan G2 and the questions for the owner are in `docs/superpowers/plans/2026-11-18-live-content-plan-g1-decisions.md`.

