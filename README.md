# Stagepack Studio

A native desktop editor for Markdown slide talks, for people who are used to PowerPoint. It's written in Rust with [GPUI](https://www.gpui.rs/). It reads a talk's `slides.md` and the files beside it, draws the slides itself, with no web view, and writes every edit back to the Markdown, changing only the bytes you edited.

> **Early days.** This is version 0.1. It is tested on Linux. The Windows build has not been run yet. The packages are not code-signed, and there is no automatic update. "Stagepack Studio" is a working name.

## What it does

- **An editor of the PowerPoint kind.** It has tool tabs across the top, the slides down the left and the notes under the slide. Slides come in five layouts and can be added, duplicated, hidden, reordered and deleted. Every change can be undone.
- **Typing on the slide.** You can make text bold, italic, underlined, coloured and sized, and make lists with levels. Cut, copy and paste work between talks, and Markdown pasted from another editor becomes a slide.
- **Things on a slide.** Pictures, shapes, text boxes, icons, tables, charts, videos (H.264 MP4), 3D models (`.glb` and `.ply` point clouds) and terminal recordings (asciinema `.cast`). You arrange them with the mouse or the keyboard, with snapping, alignment and layering.
- **Animation.** Things appear, fade in or fly up on a click. Lists can show one point at a time, and you can set how each slide arrives.
- **Presenting.** F5 starts the show. Shift+F5 rehearses it, with a presenter window showing your notes, the next slide and a timer.
- **Live terminals.** A slide can run a real program during the show. It runs only after you say yes to that exact command.
- **Clickable slides.** A `<Button>` can go to a click step, type a line into a live terminal after a yes of its own, or send an action to a component of your own, and that component can take clicks and keep a state.
- **Your own components.** A talk can bring small pieces of Rust that draw on its slides. The app builds them to WebAssembly with its own bundled compiler and runs them in a sandbox. See [the components guide](docs/guide/components.md).
- **Files.** A talk can be a folder or a single packed `.sptx` file. It exports to PDF and PowerPoint. With the free Slidev add-on, it also exports to Slidev and opens Slidev talks. The app keeps unsaved work and offers it back after a crash.
- **For AI assistants and scripts.** Commands work without a window: `describe`, `check` and `picture`, plus an MCP server (`stagepack-studio mcp ./talk`). See [writing talks for an AI](docs/guide/writing-talks-for-ai.md).

[What Stagepack Studio does](docs/guide/features.md) has the full description, area by area.

## Install

Download a package for your system from [Releases](https://github.com/Eiriksb/stagepack-studio/releases). Both packages install for the current user only. They need no administrator rights and no internet. Everything the app needs is included, the components compiler too, so it takes about 560 MB. To upgrade, install the newer package over the old one. Installing, upgrading or uninstalling never touches your talks or the app's saved work.

**Linux (x86_64).** It runs on Ubuntu 22.04, Debian 12 or newer: anything with glibc 2.35 or later.

```bash
tar --use-compress-program=zstd -xf stagepack-studio-0.1.0-linux-x86_64.tar.zst
cd stagepack-studio-0.1.0
sh install.sh
```

The app goes in `~/.local/lib/stagepack-studio`, with a link in `~/.local/bin`, a menu entry, and `.sptx` files set to open in it. To remove it, run `~/.local/lib/stagepack-studio/uninstall.sh`, which also gives `.sptx` back to the program that opened it before. The app needs a Vulkan driver and a few libraries that a desktop system nearly always has. On Debian and Ubuntu they are `libxkbcommon0 libxkbcommon-x11-0 libxcb1 libstdc++6 libvulkan1 libwayland-client0`. `install.sh` checks that the app starts, and if it can't, it says why and changes nothing.

**Windows (x86_64).** Run `stagepack-studio-0.1.0-windows-x86_64-setup.exe`. It installs under `%LOCALAPPDATA%\Programs\Stagepack Studio`, adds a Start menu entry, opens `.sptx` files in the app, and adds the app to your user `PATH`, so a new terminal can run `stagepack-studio`. Uninstall it from *Installed apps*, which also takes the app off your `PATH` and puts back the earlier `.sptx` default.

**The Slidev add-on.** Get it in the app, under *File ▸ Add-ons… ▸ Get*. It is downloaded only when you click, and it is checked against a signature built into the app. It comes from [stagepack-studio-addons](https://github.com/Eiriksb/stagepack-studio-addons/releases).

## Build from source

You need Rust 1.98 or newer, on Linux or Windows, with the `wasm32-unknown-unknown` target for components. On a Linux desktop you are working on, use `scripts/cargo-safe.sh` instead of `cargo`. A GPUI build can use more memory than the desktop can spare, so the script runs cargo in a memory-limited scope. That way only the build is stopped, not your session.

```bash
scripts/cargo-safe.sh run                         # the start screen
scripts/cargo-safe.sh run -- fixtures/showcase    # open a talk straight away
scripts/test-all.sh                               # every test (--quick skips the slow ones)
```

How the install packages are built is in [Building the packages](docs/guide/packaging.md). How the Slidev add-on is released is in [Publishing the add-on](docs/guide/publishing-the-addon.md).

## How it's made

The design is in [`docs/superpowers/specs/`](docs/superpowers/specs/) and the plans it was built from are in [`docs/superpowers/plans/`](docs/superpowers/plans/). Each stage left a record of what was tested and what still needs checking by hand, in [`docs/checks/`](docs/checks/).

## Licence

[MIT](LICENSE). The licences of the bundled fonts, libraries and compiler ship with the app, in its `NOTICES.txt` file.
