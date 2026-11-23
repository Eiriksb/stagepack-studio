# Plan G4: what ran, and what to check by hand

The run was on the `plan-g4` branch, in a debug build, on a 16-core Linux laptop, with Rust 1.98.0, on 2026-11-23. The branch hasn't been pushed or merged. Nothing here has run on Windows.

## What was built

`stagepack-studio` can now be used without a window, by an AI or a script, on a talk that is a folder or a `.sptx` file. The code is in `crates/app/src/ai_tool/`. A first argument that names a command runs it and never starts the window toolkit, the recovery sweep or a working copy; any other first argument opens a window as before (to open a folder called `check`, write `./check`). A command that can't go ahead says one plain sentence on standard error and gives status 2.

| Command | What it does | Example |
| --- | --- | --- |
| `describe [TALK] [--json]` | What a talk may hold: layouts and regions, talk and slide settings, placement settings, every built-in tag with each setting's kind, whether it is needed, its meaning and choices; with a talk, its own built components | `stagepack-studio describe ./talk` |
| `check TALK [--json]` | Every problem, by slide, as the window shows it; status 0 when none, 1 when some | `stagepack-studio check ./talk` |
| `outline TALK [--json]` | Each slide's number, layout, first words, clicks, hidden and notes | `stagepack-studio outline ./talk` |
| `picture TALK SLIDE [--step N] [--width W] --out FILE.png` | One slide at a click step as a PNG, 64 to 3840 wide | `stagepack-studio picture ./talk 3 --step 0 --out s3.png` |
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
- `STUDIO_BUNDLE=$PWD/target/bundle scripts/cargo-safe.sh test -p stagepack-studio --test ai_tool` (the compiler bundle present, so the permitted-build path runs): 23 passed, 0 failed, 0 ignored, including `builds_only_where_always_built`.
- `scripts/cargo-safe.sh test -p stagepack-studio --test ai_tool_mcp`: 7 passed.

The three tests of this task: `guide_prints_the_guide` (the command prints exactly the guide), `the_guide_names_every_built_in` (every built-in tag appears as `<Tag`), and `every_example_in_the_guide_checks_clean` (each of the 17 fenced `markdown` examples in the guide is written as the `slides.md` of a scratch talk, with a made PNG, recording, CSV, model and video in `assets`, and `check` finds nothing; none needed an excused complaint).

## A real run, end to end

On a scratch talk of three slides (a title, a chart from `assets/sales.csv`, a two-column slide with a box that fades in on click 1 and notes), with a scratch `HOME` and `XDG_*` folders and no `DISPLAY`:

- `describe .` printed the layouts, settings and tags; `describe . --json` was 12,621 bytes.
- `check .` said *No problems found.*, status 0.
- `outline .` gave `1. Title — "Scratch talk Three slides"`, `2. Content — "Sales"`, `3. Two-columns — "Two sides Left words. Right words." (1 click, notes)`.
- `guide` printed 10,430 bytes.
- `picture . 3 --step 0 --out s3a.png` said *Slide 3, click 0 of 1, written to s3a.png.* and made a 1,920 by 1,080 PNG; `picture . 3 --out s3b.png --width 800` made an 800 by 450 PNG with the blue box in it (looked at). The scratch data, cache and config folders held nothing afterwards.
- A scripted session piped into `stagepack-studio mcp .`: `initialize` answered with protocol version 2025-06-18 and `tools` capability; `tools/list` named `describe`, `check`, `outline`, `picture`, `guide`; `tools/call picture` with `slide` 2 and `width` 640 returned an `image/png` content of 17,267 bytes (a PNG) and a text content, *Slide 2.*; `tools/call guide` returned a text of 10,414 characters. The server ended at the end of its input.

## Hand checks for the owner

1. On Linux: `claude mcp add stagepack -- stagepack-studio mcp /path/to/an/empty/scratch/folder`, then ask Claude Code to make a three-slide talk with a chart and to look at each slide. It should read the guide, write `slides.md`, call `check` and look at a `picture` of each slide. Open the same folder in the window and see that the pictures match what the window shows.
2. The same on Windows, from a fresh clone: from PowerShell, `stagepack-studio.exe picture` on a talk and `stagepack-studio.exe mcp` connected to an assistant must work, with no window opening. Standard input and output there must carry the server's lines without a change to them.
3. Ask the assistant to use a component the talk has that isn't built, and see that `check` says so and the assistant tells you to build it in the app, not that it builds it.

## The final review

To be written by the controller.
