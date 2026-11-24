# Stagepack Studio

A native desktop app for editing Markdown slide talks, written in Rust with GPUI. "Stagepack Studio" is a working name.

It is being rebuilt as a fully native app: it reads a talk's files directly, draws the slides itself and writes edits back to the Markdown. It does not use a web view. The design is in `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md` and the plans are under `docs/superpowers/plans/`.

Started with a talk's folder or packed file, the app shows its slides, drawn by the engine from `slides.md` and the pictures and fonts beside it. The arrow and Page keys move between slides, Ctrl+Shift+M shows the talk's Markdown, and saving `slides.md` in another editor updates the window. F5 presents the talk, and Shift+F5 rehearses it with a presenter window of notes, next slide, timer and counter. Talks can be packed into one file, and exported to PDF and PowerPoint. What has and hasn't been checked is in `docs/checks/plan-b.md`.

The window is an editor of the PowerPoint kind, with tool tabs across the top, the list of slides down the left and the notes under the slide. Slides can be added in five layouts, duplicated, hidden from the show, deleted, and dragged to a new place, and every change can be undone. Each edit changes only the bytes of what was edited in `slides.md`. Save and Save As write a talk folder or a packed talk, the app asks before saving over a change made outside it, and unsaved changes are kept in the app's data folder and offered back after a crash. What has and hasn't been checked of this is in `docs/checks/plan-d1.md`.

Text is typed straight onto the slide: a click in a title, a body or a box puts a caret there, and an empty place on a new slide offers *Click to add title* or *Click to add text*. Words can be made bold, italic, underlined, coloured and sized, paragraphs made into bulleted or numbered lists with levels, and a place's text aligned, from the Home tab, whose row of tools scrolls sideways in a narrow window. Dead keys and input methods are taken as the system gives them. Text and whole slides can be cut, copied and pasted, also between talks, and Markdown copied from another editor pastes as a slide. A run of typing is one undo step, and typing rewrites only the paragraph typed in. Code, quotes and text mixed with components or pictures can't be typed in yet, and a click there says so. What has and hasn't been checked of this is in `docs/checks/plan-d2.md`.

Things can be put on a slide from the Insert tab: pictures from a file, dropped on the slide or pasted, six kinds of shape, words typed into a shape, text boxes, icons found by searching, tables typed in cell by cell with rows and columns added and taken away, charts, and the talk's own components. A click selects a thing, Delete takes it away, and a panel always shown at the right of the window holds a form for it, with its place, its colours and, for a component, a field for each of its arguments; for the panel's room, the window is at least 1,024 pixels wide. Pictures added to a talk are kept with its unsaved changes and saved with it, as a folder or packed, and things and slides copied between talks bring the files they use. What has and hasn't been checked of this is in `docs/checks/plan-d3.md`.

Things and a layout's areas are arranged on the slide with the mouse and the keyboard. A thing is dragged to move it, sized from the handles at its corners and edges, and turned from the handle above it; the arrow keys move it by one unit, or ten with Shift. Shift or Ctrl and a click picks several, and Ctrl+A picks every thing on the slide. A thing snaps to the slide's middle and edges and to the other things, with a line to show it, and Alt switches that off. The Home tab's *Arrange* menu, and a right-click on a thing, align what's selected, space three or more out evenly, and bring one forward or send it back. A table or a picture lying in the text is dragged out into a box of its own. A layout's area, the title or the main text, is picked by its frame, moved and sized the same way, and put back from the form. The Animate tab brings things and areas in on a click, appearing, fading in or flying up, changes the order of the steps, shows a list one point at a time, and sets how a slide is arrived at. *Layout* on the Home tab changes a slide's layout. Every one of these is one undo step and changes only the values it sets. What has and hasn't been checked of this is in `docs/checks/plan-d4.md`.

Started on its own, the app shows a start screen: five templates to begin a new presentation from (*Light*, *Dark*, *Warm* and *Ocean*, each one title slide in its colours, and *Tour*, a short talk that shows what a slide can hold), and buttons to open a talk file or folder. A new presentation is called *Untitled presentation* and can be edited at once, with nothing asked first. Its first save is a Save As, which suggests the Documents folder and a packed talk, and writes the text and every picture the talk uses. Until then its unsaved changes are kept like any talk's, and after a crash the start screen lists it under *Not saved last time*, to open or discard. Ctrl+N, or *New presentation…* in the File menu, opens a start screen in a new window, Ctrl+O opens a talk file and Ctrl+Shift+O a talk folder. What has and hasn't been checked of this, and how far the design's first success test now holds, is in `docs/checks/plan-d5.md`.

A video goes on a slide as a picture does: with *Video* on the Insert tab, by dropping its file on the slide, or by pasting its path. It's copied into the talk with a bar and *Cancel* on the status line, and kept, saved, packed and copied between talks as a file, never read whole into memory, so a talk can hold videos of up to 4 GB each. On the slide it shows its first picture, or a still picture chosen in its form. In the editor a selected video plays when its *Play* button or Space is pressed, and stops when something else is selected; in the show it plays when its slide appears, or on its click, and *Loop* in its form repeats it. MP4 files made with the usual settings (H.264) play, smoothly up to 1920 by 1080 at 30 pictures a second; any other kind shows its still and says why. PDF and PowerPoint exports get the still, and say how many videos became pictures. What has and hasn't been checked of this is in `docs/checks/plan-g1.md`.

A 3D model goes on a slide the same ways, with *Model* on the Insert tab, by dropping its file or by pasting its path, and is copied in, kept, saved and packed as a video is, up to 256 MB. Two kinds of file are read: a model saved as one file (`.glb`, binary glTF) with everything inside it, and a point cloud (`.ply`). It's put on the slide framed whole, and shows a still of itself from its starting view. In the editor a selected model is turned by its *Turn* button or Space: dragging it turns it round and the wheel brings it nearer, *Set view* makes what's shown its starting view, and *Add this view* in its form adds a view the show flies to on a click. In the show a model set to *Turns on its own* goes slowly round, and any model can be turned by dragging it and zoomed with the wheel; leaving its slide puts it back. A computer that can't draw 3D shows the still, or a plain panel saying why. PDF and PowerPoint exports get the still, and say how many models became pictures. A model is only data: one that names other files, or isn't what it says, is refused in plain words, and a model of another kind, such as `.gltf`, `.obj`, `.stl` or `.fbx`, says why it can't be drawn. What has and hasn't been checked of this is in `docs/checks/plan-g2.md`.

A terminal recording goes on a slide the same ways, with *Terminal recording* on the Insert tab, by dropping its file or by pasting its path. It's a recording made by asciinema in its second format (a `.cast` file) of up to 32 MB, 24 hours and a screen of 500 by 200, kept in the talk as a small file like a chart's data. Before it's added the whole recording is read and played through off the window's thread, one at a time, the status line saying so with a Cancel; one that isn't a recording, or is damaged, is refused in one plain sentence. A slide pasted from another talk brings its recording with it, which is checked only by its header then: one that's damaged shows a plain panel saying so. On the slide it shows its screen as the recording ends, drawn as real text in the bundled monospace font, and its form says its size, how long it plays and how many stops it has. In the editor a selected recording plays from its start when its *Play* button or Space is pressed, through its stops, and stops when something else is selected. In the show it plays when its slide appears, at its own pace with any pause longer than a second shortened to a second, and waits at each of the recording's marks for the next click; going back a step shows the screen at the stop before, and coming back from a later slide shows its end. A recording is only data: what it asks of a terminal beyond putting characters and colours on its own screen (a title, the clipboard, links, questions, the bell) does nothing. PDF exports get the last screen as text that can be selected, and PowerPoint gets it as a picture; both say how many recordings were exported as their last screen. What has and hasn't been checked of this is in `docs/checks/plan-g3.md`.

A talk can have components of its own: small pieces of Rust in its `components` folder, used on a slide by their tag, like the ready-made `<Chart>`. The app builds them to WebAssembly with the compiler bundle that ships with it, only when asked, and runs them in a sandbox. How to write one is in `docs/guide/components.md`, with examples in `fixtures/components`. What has and hasn't been checked of this is in `docs/checks/plan-c.md`.

## Building

Use `scripts/cargo-safe.sh` in place of `cargo` on a desktop you are working on. A GPUI build can use more memory than the desktop can spare, and the script runs cargo in its own memory-limited systemd scope so only the build is killed, not your session.

```bash
scripts/cargo-safe.sh test --workspace
scripts/cargo-safe.sh run
scripts/cargo-safe.sh run -- fixtures/showcase
```

The first `run` opens the start screen; the second opens a talk straight away.

## Install

Two packages are made, one per system. Both install for the current user only: no administrator rights, no password, no internet. The program holds everything it needs, including the compiler for a talk's own components and the helper for live terminals (which stays off until a talk asks for a command and you say yes). It's about 560 MB installed. Neither package has been signed, and there is no automatic update; to upgrade, install the newer package over the old one. A talk's data folder (permissions, saved-but-unsaved work) is never touched by installing, upgrading or uninstalling.

**Linux** (x86_64): unpack `stagepack-studio-<version>-linux-x86_64.tar.zst` and run its `install.sh`. It needs glibc 2.39 or later (Ubuntu 24.04, Debian 13, Fedora 40 or newer) and the libraries `libxkbcommon`, `libxkbcommon-x11`, `libxcb` and `libstdc++`, which a desktop system nearly always has; `install.sh` checks that the program starts and, if it can't, says why and changes nothing.

```bash
tar --use-compress-program=zstd -xf stagepack-studio-0.1.0-linux-x86_64.tar.zst
cd stagepack-studio-0.1.0
sh install.sh
```

The program goes to `~/.local/lib/stagepack-studio`, a link to it in `~/.local/bin`, and the menu entry, the `.sptx` file type and the icons in their usual places under `~/.local/share`. `.sptx` files open in Stagepack Studio, also where Stagepack is installed (whose own type for them, `application/x-stagepack`, is taken over as well), and another program that lists them stays under *Open with*; uninstalling gives each type its earlier default back. `~/.local/lib/stagepack-studio/uninstall.sh` takes it all away again. If `~/.local/bin` isn't on your `PATH`, add it to run `stagepack-studio` from a terminal.

**Windows** (x86_64): run `stagepack-studio-<version>-windows-x86_64-setup.exe`. It installs under `%LOCALAPPDATA%\Programs\Stagepack Studio`, adds a Start menu entry, makes `.sptx` open in Stagepack Studio (the program that opened them before stays under *Open with*, also after uninstalling, but uninstalling doesn't make it the default again) and registers an uninstaller under *Installed apps*. In a terminal, run commands as `.\stagepack-studio` from the app's folder; the folder is not on the `PATH`. The program has two files, `stagepack-studio.exe` (what a double-click starts) and `stagepack-studio.com` (what a terminal runs, which waits for a command to finish); an AI assistant's MCP command should name the `.com` in full. The Windows build has not been run yet; see `docs/checks/plan-e.md`.

## Build the packages

Run the tests first with `scripts/test-all.sh` (or `scripts\test-all.ps1` on Windows; `--quick` / `-Quick` skips the slow steps). Each package is built on its own system, since nothing for Windows can be built on Linux.

Linux, with Rust and `zstd`:

```bash
scripts/make-bundle.sh target/bundle                 # the compiler bundle, once
scripts/cargo-safe.sh run --release -p xtask --bin package -- linux --bundle target/bundle
```

This makes `target/package/stagepack-studio-<version>-linux-x86_64.tar.zst`. The folder it packs is made by `package stage` from release builds, and fails if it is over 600 MB.

Windows, in PowerShell, with Rust (the `x86_64-pc-windows-msvc` and `wasm32-unknown-unknown` targets) and [Inno Setup](https://jrsoftware.org/isdl.php) 6.3 or newer:

```powershell
scripts\package-windows.ps1
```

This runs `scripts\test-all.ps1 -Quick` (skip with `-SkipTests`), stages the folder, and makes `target\package\stagepack-studio-<version>-windows-x86_64-setup.exe`. Pass `-Bundle <dir>` to reuse a bundle made by `scripts\make-bundle.ps1`.
