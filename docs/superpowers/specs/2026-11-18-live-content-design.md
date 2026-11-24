# Live content: design

Status: written 2026-11-18 from a conversation with the owner, who then said to go ahead. It adds to `2026-11-09-native-rust-slides-design.md` and changes nothing in it except where it says so.

"Owner's answer" marks what the owner said. "My call" marks what the author of this document decided, and the owner can overturn any of those.

## What this is

Five additions to the presentation app, for talks that show real work: a model, a tool running, footage.

- **Video** on a slide.
- **3D models** on a slide, turning on their own or by hand.
- **Terminal replay:** a recorded session that plays back on a slide.
- **A tool for an AI** to build and check a talk.
- **A live terminal**, as an optional add-on, with a real shell or container behind the slide.

## Why

**Owner's answer:** the owner showed a technical talk of their own as the reason. It walks through a pipeline that turns drone footage into maps and 3D models. Nearly every slide shows something that was live when it was made: a model orbiting, a viewer, a tool's output, things followed through footage. To get those onto slides, each was filmed as a looping clip. The result is 20 videos in a file of 456 MB, scripts whose only job is to film the clips again when the data changes, and clips that must never leave the machine.

So the point is to put the real thing on the slide in place of a film of it, and to have an AI assemble such slides from a project's outputs.

**Owner's answer:** these are optional bits a person connects if they like. The app's own rules stay as they are for everyone who doesn't.

**Owner's answer:** when a model or a tool is on a slide, a faithful replay that can still be steered is built into the app, and the really live version is the optional add-on.

**Owner's answer:** all five come before packaging (stage E), in the order below.

## Goals

G1. **Video:** a clip in the talk plays on its slide when the slide shows, loops if asked, and shows a still when it can't play. (The earlier design lists video as a non-goal for the first release; this brings it in.)

G2. **3D model:** a model file in the talk is drawn on its slide. It can turn on its own, be turned by hand while presenting, and move between saved views on clicks.

G3. **Terminal replay:** a recorded terminal session plays on its slide with its own timing, pausing at marks until the next click.

G4. **AI tool:** an AI can read what a talk may hold, write a talk, and see a picture of each slide and the app's complaints about it, without a window.

G5. **Live terminal:** with the add-on connected and a deliberate yes, a slide can hold a terminal with a real program behind it, typed into while presenting.

## Non-goals, planned later

- **Sound.** Video is silent in the first cut (my call). The owner's clips are muted loops; a talk's one long video with sound waits.
- **Editing video:** trimming, cropping, speed.
- **Gaussian splats and map tiles** as things on a slide. The owner's project makes both. A film of them still works through G1.
- **Recording a terminal session inside the app** without the add-on. A recording is made outside (see G3) or by the add-on.
- **3D shapes drawn by a component's code.** A component still returns flat elements; a model comes from a file.
- **Anything live other than a terminal:** a web page, a notebook, a running program's window.

## Anti-goals: these make it a failure even if it works

The earlier design's anti-goals all stand. These are the ones this work could break:

- **A talk reaching your computer.** Opening, editing and presenting a talk run nothing outside the sandbox. A video, a model and a recording are data: drawing them must not be able to run a program, open a link, read a file outside the talk, or touch the network. The live terminal is the one exception, and it never starts without a yes given for that talk in that session (see G5).
- **Needing anything else.** Video, models and replay need nothing installed. Only the live terminal may need what it runs (a shell, Docker), and a talk that uses it still opens and presents everywhere, with a still in the terminal's place.
- **Lock-in.** A video is an ordinary video file, a model an ordinary model file, a recording an ordinary text file, each readable by other programs.
- **Losing work, and rewriting the user's text.** Each of these is a tag on a slide, edited by byte-range changes like every other.
- **A talk too big to work with.** A talk holding a 400 MB video must still open at once, type at the same speed, and save without holding the video in memory.

## Constraints

- All our code is Rust, as before. A video decoder is third-party native code where it can't be avoided (see G1).
- Offline. Nothing here fetches anything.
- Windows 10/11 x64 and Linux x86_64, as before.
- The engine, the exporters and the format crate don't depend on GPUI.
- Interface v1 for components is frozen.
- Public repos: no real decks, clients or organisations named. The owner's talk is described here only in general terms and isn't copied into the repo.

## Approaches

Three ways to put live things on a slide were weighed.

1. **Everything through a web view.** A browser engine on the slide plays video, draws 3D with WebGL and runs a terminal. It is how Slidev does it. It brings back the web engine the product was built to do without, and a page is code that runs on open. Dropped.
2. **Everything as custom components.** Let a component decode video, draw 3D and emulate a terminal inside the sandbox. It keeps one mechanism, but a sandboxed pure function can't decode 1080p video or draw a million triangles in time, and every talk would carry its own copy of that code. Dropped.
3. **Three built-in elements drawn by the app, each from a data file; the live terminal as a separate helper.** The app owns the decoder, the 3D renderer and the terminal screen; a talk carries only data. Fast, safe by construction, and a talk stays small text plus ordinary files. The live terminal can't be data, so it sits outside as an add-on with its own permission. **Chosen (my call, after the owner chose replay-built-in and live-as-add-on).**

## How it fits

Each piece is a built-in tag placed like any other thing (`x y w h rotate click enter`), set up in the form panel, moved and resized with the handles from plan D4.

```markdown
<Video src="assets/orbit.mp4" x="120" y="200" w="960" h="540" loop />

<Model src="assets/site.glb" x="1000" y="200" w="800" h="600" turn="auto" />

<Terminal src="assets/run.cast" x="120" y="200" w="1200" h="640" />
```

The exact names are the builder's. What they mean is below.

### Large files (needed by G1, used by G2)

Today a file added to a talk is held whole in memory and is 32 MB at most. A video can be hundreds of megabytes.

- A talk's files gain a way to be **read in pieces from where they lie**: a folder talk's file from the folder, an unsaved one from its working copy, a packed talk's from the pack. A file in a pack is stored without compression when it's a video or a model, so it can be read in place (my call).
- Adding a large file **copies it on disk**, never through memory, and shows progress. The cap for a video or a model is 4 GB (my call); other kinds keep 32 MB.
- Save, Save As, the working copy and copy-and-paste between talks move large files as files. A paste of a slide with a large video asks before copying more than 256 MB (my call).

### G1. Video

- **What plays (my call, to be settled by a spike):** MP4 files with H.264 video, which is what cameras, screen recorders and `ffmpeg` produce by default. The decoder is the open-source OpenH264 library, built in. A file the app can't decode shows its still and says in plain words that this kind of video can't be played.
- **The patent question (for the owner):** H.264 is patented. Cisco pays the licence for its own prebuilt OpenH264 binary, not for copies built from source. Before a public release the owner should choose between downloading Cisco's binary at first use (the one thing besides the Slidev add-on that would use the network), shipping without H.264 and playing only free formats, or taking legal advice. Until then it's built from source.
- **A still:** each video has a still, its first frame unless a `poster` picture is named. The still is what the editor shows when the video isn't selected, what thumbnails show, and what exports use.
- **In the editor:** a selected video has a play button; it plays in place. It never plays on its own while editing.
- **Presenting:** it starts when its slide shows, or on its click if it has one, and starts again each time the slide is shown. `loop` repeats it. Without `loop` it holds its last frame. Frames are shown at the video's own rate; a frame the computer can't decode in time is dropped, never queued.
- **Export:** PDF gets the still. PowerPoint gets the video itself, set to play as it does here, if that proves simple; otherwise the still, with a notice (builder's call).
- **Limits:** 4096 by 2304 at most (my call). More than four videos playing at once on one slide play, and the app says the slide may stutter.

### G2. 3D model

- **Files (my call):** glTF binary (`.glb`) first, since it carries a mesh with its textures in one file and is what the owner's viewer already writes. Then point clouds as `.ply`. Other kinds later.
- **Drawing:** on the graphics card, through wgpu, which is already built as part of the window toolkit, into a picture the slide shows as a layer. Where there's no usable graphics card, a software path draws a still.
- **Views:** a model has a starting view. A person turns and zooms it on the slide in the editor and presses *Set view*; the view is written on the tag. More views can be saved as a list, and each click while presenting flies to the next (my call: this is the "programmed animation" the owner asked for, in its simplest form that needs no code).
- **Turning:** `turn="auto"` orbits slowly on its own. While presenting, a drag on the model turns it and the wheel zooms; the keys still run the show. A model the presenter has turned goes back to its view when the slide is left.
- **A still:** as for video, for thumbnails, exports and computers that can't draw it.
- **Untrusted files:** a model is parsed in Rust with size limits (triangles, texture size, file size). It can name no outside file: everything it needs is in the one file.
- **Tests:** drawing on a graphics card can't be compared pixel for pixel across machines, so tests check the parsed model, the view's numbers and that a known model's still is close to a stored one, on the software path.

### G3. Terminal replay

- **Files (my call):** asciinema's recording format, version 2 (`.cast`): a line of settings, then one line per burst of output with its time. It's plain text, open, and `asciinema rec` writes it on any computer. The live add-on writes it too.
- **Drawing:** the app keeps a terminal screen in Rust and feeds it the recording. The screen is drawn with the bundled monospace font through the same display list as everything else, so the PDF gets real text and thumbnails work.
- **Playing:** it starts when the slide shows, with the recording's own timing, so typing looks typed. A pause longer than a second is shortened to a second (my call). A mark in the recording stops it until the next click, so a presenter talks between commands. With no marks it plays through.
- **What a recording can't do:** it can only draw characters and colours. Sequences that would set the window's title, write to the clipboard, open a link or ask the terminal questions are ignored.
- **Export:** the screen as it ends.

### G4. A tool for an AI

A talk is already plain text, which is the part an AI is good at. What it lacks is the rules and eyes.

- **The same program, with no window (my call):** `stagepack-studio` gains commands that read and check a talk without opening a window, and one that serves them to an AI assistant over the Model Context Protocol, the standard such assistants use for tools.
  - **What can a talk hold:** the layouts, every tag with its settings and what each means, generated from the same descriptions the form panel uses, so it can't go stale.
  - **Check:** every complaint the app has about a talk, by slide, as the window would show them.
  - **Picture:** a slide at a click step as a PNG, so the AI can look at what it made.
  - **Outline:** the slides, their layouts, their first words.
- **Writing** is done by the AI in the talk's own files. The tool never needs to edit for it.
- **A guide for the AI**, shipped with the app, saying how a talk is written, in the terms above.
- **Rules:** the tool reads and writes only inside the talk it's pointed at. It compiles nothing unless the folder is already one the owner said to always build.

### G5. The live terminal add-on

- **What it is:** a small separate program that runs a command in a terminal of its own and passes the screen and the keys to and from the app. The slide's tag names the command: a shell, or `docker run …` for a container, or anything else. The app doesn't manage Docker or virtual machines; it runs what the tag says (my call).
- **Off unless connected (owner's answer).** Without the add-on, or without a yes, the slide shows the terminal's still, or its recording if it has one.
- **The yes:** the first time a talk would start a command in a session, the app shows the command in full and asks. The answer is per talk and per session, never remembered for a packed talk, and remembered for a folder only if the owner says *Always for this folder*. Nothing starts when a talk opens; a terminal starts when the presenter reaches its slide or presses *Start*.
- **Typing while presenting:** a click on the terminal gives it the keys, shown by a frame. A key chosen for the purpose gives them back to the show (my call: Ctrl+Shift+Q, changeable; Windows keeps Ctrl+Shift+Escape for Task Manager). Until then the arrow keys go to the terminal, not the show.
- **Recording:** a session can be saved as a recording for G3, which is how a risky demo becomes a safe one.
- **Ending:** leaving the talk or closing the window ends what was started, and says so if it won't end.

## Order of work

**Two throwaway spikes first (my call).** Each answers one question. If an answer is no, the piece is redesigned before it's planned.

1. **Video:** can OpenH264, built in, decode 1080p H.264 at its own frame rate and get the frames onto a slide through the window toolkit without stutter, with a 400 MB file read from disk and memory staying flat?
2. **3D:** can wgpu draw a textured model of a million triangles into a picture the slide shows, at a steady rate while it turns, and draw a still with no graphics card?

Then, each as its own plan, written when the one before it has finished:

- **G1. Large files and video.**
- **G2. 3D models.**
- **G3. Terminal replay.**
- **G4. The AI tool.**
- **G5. The live terminal add-on.**

Then stage E, packaging, which gains what these add: the decoder, the add-on's helper, and the AI tool's command.

## Known risks

- **The H.264 licence** (above).
- **A second renderer.** 3D on the graphics card is new ground beside the CPU renderer that makes every machine draw alike. Models won't be pixel-identical across machines.
- **Video in the window toolkit.** Its pictures are meant to change rarely. The spike decides whether frames can be swapped fast enough.
- **Large files touch saving, recovery and the pack format,** the parts where work could be lost. They get the same care as plan D5's first save.
- **The live terminal is the first thing in the app that runs a talk's command.** Its yes must be impossible to skip.

## Left to the builder

- The tags' and settings' exact names.
- The crates for the MP4 container, the model parser and the terminal screen.
- Whether PowerPoint gets the video or a still.
- How the add-on's helper and the app talk.
