# Plan G4: decisions made while building

The calls made during plan G4 without asking the owner, and the smaller findings left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-g4.md` has what ran.

## Questions for the owner

- **Nothing was run on Windows,** where `picture` and `mcp` must work from PowerShell, and an assistant must be connected by hand (`docs/checks/plan-g4.md`).
- **No real assistant has been connected yet.** The server was driven by the tests and a scripted session; connecting Claude Code to a scratch talk and asking it to build one is the hand check.
- **One program, two uses.** `stagepack-studio check …` is a command; anything else opens a window. A folder literally named like a command opens as `./check`.

## Rulings

- **Setup.** Commands live in the app crate (`ai_tool`), so they reuse its headless pieces; no new dependency, MCP written by hand over standard input and output. Cost if wrong: an assistant wanting another method or transport gets it added.
- **Task 1.** Arguments are read by hand. Font loading is shared with the export.
- **Task 2.** A setting is *needed* only when nothing can be drawn without it (a file's `src`); every other setting says what it is when left out, as the engine fills it. Meanings live beside the form panel's labels, and a test fails when a built-in setting has none.
- **Task 3.** `check` builds first only for a folder set to *Always build this folder* with a compiler present, never a pack; problems are worded as the window words them, the two building sentences shared from `builds.rs`. Cost if wrong: none found.
- **Task 4.** A picture is the slide as the PDF draws it at that step: layers whose click is still to come are left out, videos are their stills, models drawn as stills, recordings their last screen. `compose` takes layers held alone or shared.
- **Task 5.** The server starts on any path that exists, so an assistant can start it on an empty folder and then write the talk; every call opens the talk afresh. A schema violation is `-32602`; a value the talk lacks (slide 13 of 12) is a failed call in words.
- **Task 6.** The guide is in the program and every example in it is checked to open with no complaints. Its review found it claiming that clicks work on any tag and that `each` goes on a tag; both were wrong and are fixed, with a test.
- **Final.** Pictures over MCP are kept under 3.5 MB of PNG (about 4.7 MB sent), drawn narrower if need be; `--out` writes only a `.png` and replaces only a PNG.

## Smaller findings and notes

- **Not fixed:** a caught panic still prints Rust's own message on standard error; the no-window test checks the data folder only; a picture redrawn narrower is drawn from scratch each time; the refresh-panic path in `check` has no test; two tests spell out the building sentences rather than reading them; no test draws a picture in the middle of a region built up by `each`.
