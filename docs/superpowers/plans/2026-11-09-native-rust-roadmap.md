# Native Rust slides: roadmap

The design (`docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`) is built as a series of plans. It replaces `2026-11-09-roadmap.md`, which described the Slidev-based editor. Each plan ends with software that runs and can be tested on its own.

Only the first plan is written in detail. The spikes can change the renderer, the compile setup or the text editing, so each later plan is written when the plan before it has finished.

## Plans

### 0. Spikes (go or no-go)

File: `2026-11-09-native-rust-spikes.md`.

Three throwaway builds answer three questions: can the slide be drawn fast enough, can a trimmed compiler build a component on a clean machine, and can per-word text editing write back to Markdown without touching anything else? If any answer is no, stop and go back to the owner before planning further.

### A. Engine

File: `2026-11-09-native-rust-plan-a-engine.md`.

Open a folder and see the talk. Edit its files in any editor and the slide updates on save.

This plan starts a new branch from `plan-1` and first brings `docs/` over from `spike/native-rust`, because the spike branch is never merged and it holds the design, this roadmap and the results.

- The format: slide splitting, parsing with exact byte positions, attribute scanning, source edits.
- The slide model, layouts and regions, with the five first-release layouts.
- Layout and the renderer, with bundled fonts and golden-image tests.
- Built-in components on the toolkit, drawn natively: `<Box>`, shapes, arrows, icons, pictures, tables.
- Code blocks with syntax highlighting (goal 15).
- Goal 13's folder watching and *View Markdown*.

Windows checklist: open a fixture folder and compare with the golden images.

### B. Present and export

File: `2026-11-10-native-rust-plan-b-present-export.md`.

Coders can use early builds from here.

- Full-screen presenting and the presenter view, with click steps, entrances and transitions (the presenting half of goals 7 and 8).
- The packed `.sptx`: atomic save, and telling our files from Stagepack's.
- Export to editable PowerPoint with animations, and embedded fonts.
- Export to PDF.

Windows checklist: present on two screens, and open the exported PowerPoint in desktop PowerPoint.

### C. Custom components

File: `2026-11-10-native-rust-plan-c-components.md`.

- The toolkit's public shape, the `#[component]` macro and the WebAssembly interface.
- The compiler bundle, compile on save, the problems panel and the source check.
- The sandbox: Wasmtime, memory and time caps, and the labelled box for a failed component.
- The compile permission rules: source hashes, *Build components* and *Always build this folder*.
- Forms from argument types, and `children`. Plan C delivers the schema and the fields a form will show; the form on screen is in D3.
- The optional `Cargo.toml`, the guide and examples.
- A ready-made `<Chart>` for bar, line and pie charts (goal 17, owner's decision, 2026-11-10). It's built on the toolkit, so it's also the first real test of it. PowerPoint export needs a freeform shape for its lines and slices.

Windows checklist: build and show a component with no Rust installed, and open a talk with a chart in desktop PowerPoint.

### D. Visual editor

The largest stage, and it will be split into several plans when it starts.

- **D1. Slides and safety:** the slide strip, notes panel, undo, Save As, crash recovery (goals 1, 8, 9 in part, 11). File: `2026-11-11-native-rust-plan-d1-slides-safety.md`. It also brings the tool tabs, with Insert and Animate empty until D3 and D4. What ran is in `docs/checks/plan-d1.md`.
- **D2. Text:** typing on the slide, per-word styling, bullets, copy and paste between talks (goals 2, 9). File: `2026-11-12-native-rust-plan-d2-text.md`. What ran is in `docs/checks/plan-d2.md`.
- **D3. Insert:** pictures, shapes, icons, tables and the component palette with its forms (goals 3, 4, 6). File: `2026-11-16-native-rust-plan-d3-insert.md`. What ran is in `docs/checks/plan-d3.md`.
- **D4. Arrange and animate:** moving regions and boxes, align, distribute, order, snapping, the Animate tab and the layout picker (goals 5, 7). File: `2026-11-17-native-rust-plan-d4-arrange-animate.md`. What ran is in `docs/checks/plan-d4.md`.
- **D5. New presentation:** the start screen, starter templates, and a new presentation that is untitled until its first save and brought back after a crash (goal 10, and the save part of 11). File: `2026-11-18-native-rust-plan-d5-new-presentation.md`. What ran is in `docs/checks/plan-d5.md`.

Windows checklist at the end of each.

Stage D is done with D5. How far the design's first success test holds at its end, and what plan E still owes it, is in `docs/checks/plan-d5.md`.

### G. Live content

Design: `docs/superpowers/specs/2026-11-18-live-content-design.md`. The owner asked for it to come before packaging (2026-11-18). Two spikes come first, for video and for 3D; their results go in `docs/checks/live-content-spikes.md`. Each part is its own plan, written when the one before it has finished.

- **G1. Large files and video:** a clip in the talk plays on its slide. File: `2026-11-18-live-content-plan-g1-video.md`. What ran is in `docs/checks/plan-g1.md`.
- **G2. 3D models:** a model file drawn on its slide, turned by hand or between saved views on clicks.
- **G3. Terminal replay:** a recorded session played back on its slide.
- **G4. The AI tool:** commands, and a server an AI assistant can use, to read what a talk may hold, check a talk and see a picture of a slide.
- **G5. The live terminal add-on:** a real shell or container behind a slide, off unless connected and said yes to.

### E. Packaging

After stage G. It gains what G adds: the video decoder, the add-on's helper and the AI tool's command.


- Installers for Windows and Linux, per user, with no administrator rights.
- The bundled compiler, fonts, icons and licence notices.
- `.sptx` registered as the default, with Stagepack under *Open with*.
- A local script that runs every test on Linux, and the same on the owner's Windows PC. No GitHub Actions (owner's decision, 2026-11-10).
- Both success tests from the design, run from the installers.

### F. Slidev add-on

- The *Add-ons* page, signed download, *Install from file* and updates.
- Export to Slidev.
- Import from Slidev, with its report.

It isn't part of the success tests, so it can follow the first public release if time runs short.

## Order

The spikes come first, and then A to D in order, then G, then E and F. C can start once A's slide model exists, so it can run beside B. The first public release is when both success tests pass, at the end of E.
