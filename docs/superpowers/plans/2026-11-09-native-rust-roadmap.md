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
- **D2. Text:** typing on the slide, per-word styling, bullets, copy and paste between talks (goals 2, 9).
- **D3. Insert:** pictures, shapes, icons, tables and the component palette with its forms (goals 3, 4, 6).
- **D4. Arrange and animate:** moving regions and boxes, align, distribute, order, snapping, the Animate tab and the layout picker (goals 5, 7).
- **D5. New presentation:** starter templates (goal 10).

Windows checklist at the end of each.

### E. Packaging

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

The spikes come first, and then A to F in order. C can start once A's slide model exists, so it can run beside B. The first public release is when both success tests pass, at the end of E.
