# Plan G4: what ran, and what to check by hand

The run was on the `plan-g4` branch, in a debug build, on a 16-core Linux laptop, with Rust 1.98.0, on 2026-11-23. The branch hasn't been pushed or merged. Nothing here has run on Windows.

## What was built

`stagepack-studio` can now be used without a window, by an AI or a script, on a talk that is a folder or a `.sptx` file. The code is in `crates/app/src/ai_tool/`. A first argument that names a command runs it and never starts the window toolkit, the recovery sweep or a working copy; any other first argument opens a window as before (to open a folder called `check`, write `./check`). A command that can't go ahead says one plain sentence on standard error and gives status 2.

| Command | What it does | Example |
| --- | --- | --- |
| `describe [TALK] [--json]` | What a talk may hold: layouts and regions, talk and slide settings, placement settings, every built-in tag with each setting's kind, whether it is needed, its meaning and choices; with a talk, its own built components | `stagepack-studio describe ./talk` |
| `check TALK [--json]` | Every problem, by slide, as the window shows it; status 0 when none, 1 when some | `stagepack-studio check ./talk` |
| `outline TALK [--json]` | Each slide's number, layout, first words, clicks, hidden and notes | `stagepack-studio outline ./talk` |
| `picture TALK SLIDE [--step N] [--width W] --out FILE.png` | One slide at a click step as a PNG, 64 to 3840 wide (through `mcp`: 64 to 1920, 1280 by default, and drawn again narrower if over 3.5 MB); `--out` must end in `.png` and may not replace a file that isn't a PNG | `stagepack-studio picture ./talk 3 --step 0 --out s3.png` |
| `guide` | Prints the guide for an AI, `docs/guide/writing-talks-for-ai.md`, put in the program with `include_str!` | `stagepack-studio guide` |
| `mcp TALK` | A Model Context Protocol server for that talk over standard input and output, with the tools `describe`, `check`, `outline`, `picture` and `guide`; the talk is opened afresh for every call | `stagepack-studio mcp ./talk` |
| `help`, `--help`, `-h` | Lists the commands | |

The tool reads only inside the talk, writes only the file `--out` names (and the talk's `build` folder when it is allowed to build), opens no socket, and builds a component only in a folder the owner set to *Always build this folder*, with the app's own compiler; a packed talk is never built.

## Connecting an assistant

For Claude Code, in a shell:

```
claude mcp add stagepack -- stagepack-studio mcp /path/to/talk
```

For a client that reads a JSON file of servers (the file's name and place depend on the client):

```json
{
  "mcpServers": {
    "stagepack": {
      "command": "stagepack-studio",
      "args": ["mcp", "/path/to/talk"]
    }
  }
}
```

The path may be an empty folder: the server starts, the assistant reads the guide and writes `slides.md` there, and the next call finds it. The assistant should call the `guide` tool first.

## Tests on Linux

At the run's last commit:

- `scripts/cargo-safe.sh test --workspace` (stops at the first failing test binary) was run first and stopped at `crates/app/tests/export.rs`, where `stagepack_is_asked_to_open_the_file` failed with *Text file busy* (a script written and run at once, while another test in the same run forked). Run alone three times after, it passed each time.
- `scripts/cargo-safe.sh test --workspace --no-fail-fast` then ran every test binary: 121 test results, **2,626 passed, 1 failed, 56 ignored**. The one failure was `once_the_drawer_is_closing_nothing_starts_another` in `crates/app/tests/ui_model.rs` (a count of drawer threads, `left: 1, right: 0`), which is known to depend on load. Run alone twice after, it passed both times (0.47 s and 0.42 s). Neither failure is in code this plan touched. The ignored ones are the release timing tests and tests that need the compiler bundle.
- `STUDIO_BUNDLE=$PWD/target/bundle scripts/cargo-safe.sh test -p stagepack-studio --test ai_tool` (the compiler bundle present, so the permitted-build path runs): 27 passed after the final fix wave (24 after fix round 1), 0 failed, including `builds_only_where_always_built`.
- `scripts/cargo-safe.sh test -p stagepack-studio --test ai_tool_mcp`: 7 passed.

The three tests of this task: `guide_prints_the_guide` (the command prints exactly the guide), `the_guide_names_every_built_in` (every built-in tag appears as `<Tag`), and `every_example_in_the_guide_checks_clean` (each of the 18 fenced `markdown` examples in the guide is written as the `slides.md` of a scratch talk, with a made PNG, recording, CSV, model and video in `assets`, and `check` finds nothing; none needed an excused complaint).

## A real run, end to end

On a scratch talk of three slides (a title, a chart from `assets/sales.csv`, a two-column slide with a box that fades in on click 1 and notes), with a scratch `HOME` and `XDG_*` folders and no `DISPLAY`:

- `describe .` printed the layouts, settings and tags; `describe . --json` was 12,621 bytes.
- `check .` said *No problems found.*, status 0.
- `outline .` gave `1. Title — "Scratch talk Three slides"`, `2. Content — "Sales"`, `3. Two-columns — "Two sides Left words. Right words." (1 click, notes)`.
- `guide` printed 11,192 bytes (after fix round 1; 10,430 before).
- `picture . 3 --step 0 --out s3a.png` said *Slide 3, click 0 of 1, written to s3a.png.* and made a 1,920 by 1,080 PNG; `picture . 3 --out s3b.png --width 800` made an 800 by 450 PNG with the blue box in it (looked at). The scratch data, cache and config folders held nothing afterwards.
- A scripted session piped into `stagepack-studio mcp .`: `initialize` answered with protocol version 2025-06-18 and `tools` capability; `tools/list` named `describe`, `check`, `outline`, `picture`, `guide`; `tools/call picture` with `slide` 2 and `width` 640 returned an `image/png` content of 17,267 bytes (a PNG) and a text content, *Slide 2.*; `tools/call guide` returned a text of 11,176 characters (10,414 before fix round 1). The server ended at the end of its input.

## Hand checks for the owner

1. On Linux: `claude mcp add stagepack -- stagepack-studio mcp /path/to/an/empty/scratch/folder`, then ask Claude Code to make a three-slide talk with a chart and to look at each slide. It should read the guide, write `slides.md`, call `check` and look at a `picture` of each slide. Open the same folder in the window and see that the pictures match what the window shows.
2. The same on Windows, from a fresh clone: from PowerShell in the installed folder, `stagepack-studio picture` (which runs `stagepack-studio.com`, the console twin of the windowed `stagepack-studio.exe`: the prompt must wait for it and `$LASTEXITCODE` must hold its exit status) on a talk and `stagepack-studio mcp` connected to an assistant must work, with no window opening. Standard input and output there must carry the server's lines without a change to them.
3. Ask the assistant to use a component the talk has that isn't built, and see that `check` says so and the assistant tells you to build it in the app, not that it builds it.

## The final review

A whole-branch review on the most capable model read the design, the plan, the rulings and the diff from `plan-g`, and ran the built program on scratch talks. Its verdict was *ready after fixes*: three important findings and four minor ones, all fixed in one wave (b72750e) and checked by a scoped re-review, which found each fixed.

- **Important:** `describe` over MCP failed on a folder with no talk yet, which is the guide's first step; it now lists the built-ins and says why the talk couldn't be opened (976dc01). A slide holding a photo made an MCP picture of 11 MB at the default width and 40 MB at the widest, over the 5 MB an assistant takes; over MCP a picture is now 1,280 wide by default and 1,920 at most, and is drawn narrower, down to 640, until its PNG is under 3.5 MB. A command could end while the 3D drawer still held the graphics device; every command now closes the drawer first.
- **Minor:** `picture --out` could overwrite any file, the talk's `slides.md` included; it now writes only a `.png` name and replaces only a PNG. `check` didn't mark hidden slides in its text. The guide said both that the tool never builds and that it builds where *Always build this folder* is set, and described `picture` only by its command-line options.
- **Checked and found sound:** a link, `..` or an absolute path in a talk never reads outside it; building follows the window's rule on the folder's canonical path; nothing but replies reaches the server's standard output; every first argument that isn't a command name opens a window as before; slides and steps are numbered alike in every tool.

After the fixes, the whole suite: **2,633 passed, 0 failed, 56 ignored** (`scripts/cargo-safe.sh test --workspace --no-fail-fast`, without a compiler bundle). With `STUDIO_BUNDLE` set to the bundle in `target/bundle`, `--test ai_tool` 27 passed and `--test ai_tool_mcp` 9 passed.

Still by hand: that a command drawing a 3D model ends cleanly on a computer with a graphics card (this one has none, so the drawer's device was never held here), and everything on Windows.
