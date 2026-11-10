# Native Rust slides: design

Status: approved by the owner, 2026-11-09. It replaces `2026-11-09-native-slidev-editor-design.md`. The product has no name yet, and the name is the owner's call.

"Owner's answer" and "owner accepted" mark what the owner decided or agreed to. "My call" marks what the author of this document decided, and the owner can overturn any of those.

## What this is

A desktop presentation app written in Rust.

- **Make a talk two ways:** click and drag on the slide as in PowerPoint, or write the talk as Markdown and watch the slide update. Both change the same file.
- **Files:** a talk is saved as Markdown.
- **Custom components:** written in real Rust, and used as tags on a slide.
- **One install:** the app carries everything it needs, including the Rust compiler. Nobody needs Rust, Node, a browser or the internet.
- **Slidev (owner's answer):** an optional add-on, fetched from inside the app, exports a talk to Slidev and imports a `.sptx` made with Slidev.

It takes Slidev's ideas (a talk as text, components as tags, a presenter view) but it is a new product with its own format. It does not run Slidev.

## Why

**Owner's answer:** a Rust alternative to PowerPoint would be cool, and it should be as easy to make custom components in code as it is in Slidev.

**Why not port Slidev:** a Slidev slide is a Vue component drawn by a browser engine, and talks can carry their own Vue components and build-time code. Keeping real Slidev compatibility means keeping Vue and a web engine, so a fully native app has to be its own thing.

**Who it's for (owner's answer):** coders first, but it must also be usable with no coding knowledge.

**Success for the first public release.** Both tests must pass.

*Someone with no coding knowledge (owner accepted):*
- They download one installer and run it.
- They choose *New presentation* or double-click a talk.
- They build a ten-slide talk with text, pictures, shapes, a table and click animations.
- They present it, save it, and export it to PowerPoint for a colleague.
- They never see Markdown, Rust, a terminal or a missing-dependency error.

*A coder (my call):*
- They open a folder, write `slides.md` in their own editor, and the slide updates when they save.
- They write a Rust component from the bundled guide, drop its tag on a slide and see it within a few seconds. Compile errors appear in the app with the file and line.
- They hand the talk to someone without Rust, and it opens and presents with no prompt.
- That person can place the same component on a slide and fill in a form to set it up.

## Goals for the first release

The first twelve come from the earlier design's first-version list (owner accepted).

1. **Slides:** a strip of live thumbnails. New slide from a layout, duplicate, delete, reorder by dragging, hide and unhide.
2. **Text:** typing on the slide; bold, italic, underline; size, colour, alignment; bullets and numbering. Size and colour apply to the selected words, not only to a whole text box.
3. **Pictures:** insert from a file, drag in from the desktop, paste from the clipboard; move, resize, rotate.
4. **Shapes, arrows and icons:** ready-made shapes with fill, outline and text inside, plus an icon picker.
5. **Arrange:** align, distribute, bring forward and send back, and snapping.
6. **Tables:** a grid with add or remove row and column, and typing in cells.
7. **Animations and transitions:** click steps, entrances, staggered lists and slide transitions, from a native panel.
8. **Notes and presenting:** a notes area under the slide, full-screen presenting and a presenter view.
9. **Undo, copy and paste:** undo and redo across the whole editing session, including reordering and adding slides. Copy and paste of elements and slides, also between two open talks.
10. **New presentation:** a few starter templates.
11. **Save:** Save, Save As, an unsaved-changes prompt on close, and recovery after a crash.
12. **Export:** to editable PowerPoint with animations, and to PDF. The app's shapes arrive in PowerPoint as real, editable shapes.

Added for this product:

13. **Text-first working (owner accepted):** open a folder, edit its files in any editor, and the slide updates on save. A *View Markdown* panel is available in the app.
14. **Custom components in Rust (owner's answer):** see "Custom components".
15. **Code blocks with syntax highlighting (my call):** coders come first, and they will show code.
16. **Slidev add-on (owner's answer):** get it from the app's interface, then export a talk to Slidev or import a `.sptx` made with Slidev. See "The Slidev add-on". It isn't part of the success tests, so it can follow the first public release if time runs short (my call).
17. **Charts (owner's answer, 2026-11-10):** a ready-made chart component, so that a graph needs no code. See "Charts".

## Non-goals for the first release, planned later

Carried over from the earlier design:
- video and audio;
- cropping and grouping;
- switching themes (the first release has one theme, with colours and fonts set in the talk);
- find and replace;
- importing `.pptx`;
- macOS.

Added:
- outside Rust libraries from crates.io in components (owner accepted);
- components that respond to the mouse or keyboard while presenting (my call);
- maths and diagrams (my call);
- PowerPoint's own chart objects, and charts that follow a spreadsheet (my call);
- add-ons from anyone but us (my call);
- right-to-left text, and bundled Chinese, Japanese and Korean fonts (owner's answer).

## Out

- Comments.
- Editing the same talk with others at the same time.
- An online or web version.
- Opening and editing a Slidev talk directly. The Slidev add-on imports a copy instead (owner's answer).
- Running Vue, JavaScript or npm packages.

## Anti-goals: these make it a failure even if it works

- **Lock-in:** a talk stays plain text. Its Markdown and component source are readable in any editor, and compiled components can be rebuilt from that source.
- **Code in the way:** any first-release task that forces a non-coder to see Markdown, Rust or a terminal is a bug.
- **Rewriting the user's text:** a visual edit changes only the bytes of what was edited. The app never reformats a file, so a coder's hand-written layout survives.
- **Editing the wrong thing:** if the app can't be sure which text a click maps to, it refuses and says so.
- **Losing work:** a crash or kill during editing or saving never leaves a damaged talk, and unsaved edits can be recovered.
- **Needing anything else:** making, editing, compiling, presenting or exporting must not need the internet, an installed Rust, Node, a browser or any other program. The one exception is fetching the optional Slidev add-on, and the app makes no network requests until someone asks for that.
- **A talk reaching your computer:** opening a talk must never compile anything or run anything outside the sandbox.
- **A DeckCraft copy (owner's answer):** DeckCraft is an existing alpha Rust PowerPoint clone built around `.pptx`. It may be read for reference, but no code is taken from it, and this product is built around Markdown and code components instead of `.pptx` parity.

## Constraints

- **Platforms:** Windows 10/11 x64 and Linux x86_64 (my call, unchanged from the earlier design). Linux runs natively on both X11 and Wayland, because the web view that forced X11 is gone.
- **All our code is Rust (owner's answer).** There is no web view, JavaScript engine or Node. Third-party native code is still present where it can't be avoided: the bundled compiler contains LLVM, which is C++, and GPUI uses the platform's graphics libraries.
- **gpui-kit:** the published crates, pinned exactly (`=0.9.0` with `gpui-fast`), not a fork. This is what the repo uses today. Releases are weekly and the API moves, so upgrades are deliberate.
- **The compiler:** one pinned Rust release per app release.
- **Size:** measured on Linux with Rust 1.98, the compiler and linker are 346 MB on disk (104 MB compressed with zstd), and the WebAssembly standard library adds 96 MB. Budget for the whole install: 600 MB on disk (my call), against about 820 MB for the Node and Slidev bundle it replaces.
- **Install:** per user, with no administrator rights, on both platforms. It registers the talk file type.
- **File type (owner's answer):** `.sptx`, the extension Stagepack uses for Slidev talks. "The talk on disk" says how the two kinds are told apart.
- **Licences:** MIT for the app (my call, as in this repo). Notices are kept for gpui-kit (Apache-2.0), the bundled Rust toolchain and LLVM, and the bundled fonts and icons.
- **Public repos:** don't name real decks, clients or organisations in commits, issues or PRs. Use fixtures.

## How it fits together

### One source of truth: the Markdown

```
slides.md + components/ + assets/
        │  parse, keeping exact byte positions
        ▼
   slide model ◄──────────────── source edit (replace these bytes)
        │  layout                        ▲
        ▼                                │
   display list ──► screen, thumbnails   │
        │       ──► PDF                  │
        │       ──► PowerPoint           │
        ▼                                │
   click, drag, type ────────────────────┘
```

The data flows one way (my call). There is no second copy of the talk that can drift from the file.

- A gesture becomes a **source edit**: replace this byte range with this text. The affected slide is then parsed again and redrawn.
- During a drag, the slide shows a temporary override. Releasing the mouse produces one source edit.
- **Undo** is the inverse of those edits, across the whole session. Assets stay available until the session ends, so undoing a deleted picture works.

The parser, engine and exporters don't depend on GPUI, so they can be tested headlessly (my call).

### The talk on disk

A talk exists in two forms with the same contents (my call). Coders use the folder and non-coders use the packed file. The app opens both.

```
my-talk/
  slides.md
  components/score.rs
  assets/photo.jpg
  build/score.wasm      (generated)
```

- **The packed form** is a `.sptx` file (owner's answer): a zip with a small manifest (format version, app version), saved with a temp file, sync and rename so a crash can't damage it.
- **Two kinds of `.sptx` (my call on how they're told apart):** Stagepack's Slidev talks use the same extension, and this app can't open them.
  - This app's manifest has its own file name, not Stagepack's (`brand.ManifestName`). Stagepack then reports "not a Stagepack file", which is true. Reusing its name with a higher format number would tell people to get a newer Stagepack, which wouldn't help.
  - When this app opens a file holding Stagepack's manifest, it imports a copy if the Slidev add-on is installed. If it isn't, the app says in one sentence that this is a Slidev talk and offers *Get the add-on*, plus *Open in Stagepack* if Stagepack is installed.
  - This app registers as the default for `.sptx`, and Stagepack stays available under *Open with*.
- **Saving (my call):** as in PowerPoint, visual edits are unsaved until *Save*, in both forms. They live in a working copy under the app's own data folder, which doubles as crash recovery.
- **Changes made outside the app:** a folder is watched. If nothing visual is unsaved, the app reloads. If something is, it asks.
- **WebAssembly is the same on every platform,** so a component built on Linux runs on Windows.
- **Converting:** *Save as folder* and *Save as packed talk* move a talk between the two forms.

### The Markdown

```markdown
---
title: Quarterly review
format: 1
---

# Quarterly review

---
layout: two-columns
transition: fade
---

# Results

<Box x="120" y="240" w="800" h="300" rotate="5" click="1">
Revenue is **up** <Text color="red" size="64">12%</Text> this year.
</Box>

<Score team="Oslo" points="3" colour="green" click="2" />

<Notes>
Mention the new office.
</Notes>
```

The exact tag and attribute names are the builder's. The shape of it is my call:

- **Slides** are separated by a `---` line, and each may start with a settings block. The first block holds the talk's settings.
- **Ordinary Markdown** (headings, paragraphs, lists, tables, pictures, code fences) flows inside the regions of the slide's layout. See "Layouts and moving things".
- **Anything PowerPoint-like that Markdown can't say** goes in tags: position and rotation in `<Box>`, per-word styling in `<Text>`, click order and entrances as attributes. Slide units are 1920×1080 for a 16:9 talk.
- **`<Box>` is a built-in component.** `<Text>`, `<Notes>` and `<Region>` are part of the format and handled by the parser.
- **Layouts** are named in the settings block. The first release has a handful: title, content, two columns, section, blank.
- **Unknown tags** show as a labelled placeholder and are kept in the file.
- **A custom component can't reuse a built-in's name.** A clash is reported as an error.

**Parsing:** the `markdown` crate (1.0.0) is already in `Cargo.lock` through gpui-component. It supports tables and JSX-style tags, and gives every node a byte position. Three things the builder should know, checked in its source:
- Enable `mdx_jsx_flow` and `mdx_jsx_text` individually, with `html_flow`, `html_text`, ESM and `{}` expressions off. HTML comments therefore aren't available, which is why notes use a `<Notes>` tag.
- Slides need a small splitting pass first, keeping byte offsets, because a `---` line otherwise means a horizontal rule. It has to respect code fences.
- Tag attributes have no positions in its tree, so a small scanner inside the tag's byte range has to find them.

### Layouts and moving things (my call)

The owner's test is "like in PowerPoint", so this follows how PowerPoint's placeholders behave.

- **A layout defines named regions,** each with its own place on the slide, such as `title`, `body`, or `left` and `right`.
- **Which content goes where:** the slide's first heading goes in `title`, and the rest goes in the layout's main region. A `<Region name="right" />` line sends what follows it to that region.
- **New slide from a layout** writes only the settings block. Empty regions show "Click to add text" in the editor, and show nothing when presenting or exporting.
- **You select and drag a region,** as you do a PowerPoint placeholder. Moving, resizing, rotating or animating one region leaves the others where they are, and text reflows only inside its own region.
- **Where the change is written:** a region's moved place, rotation and click order are a line in the slide's settings block. Its content isn't touched.
- **Click order for a region** can be the whole region, or each list item in turn.
- **Things inserted from the ribbon** (pictures, shapes, tables, text boxes, components) are positioned tags, so each one moves on its own.
- **Flowing content a coder wrote:** dragging a single block out of a region wraps just that block in a `<Box>` at its current place, and the rest of the region closes up.

### Drawing the slide

GPUI draws the window and panels. It can't draw the slide: it only rotates SVGs (`Transformation` in `elements/svg.rs`), `paint_image` can't rotate, and its text system differs per platform.

So the slide has its own renderer (my call):

- **Layout:** Taffy for flows and cosmic-text for text, both already in `Cargo.lock`.
- **Fonts and languages (owner's answer):** the first release covers Latin, Greek and Cyrillic with bundled Inter and JetBrains Mono, plus a bundled colour emoji font. Layout uses those and any fonts carried in the talk, so such a talk lays out the same on every machine. Characters they don't cover fall back to fonts on the computer, with a notice, so other scripts appear but may lay out differently between machines. Colour emoji must work in the renderer and in PDF. PowerPoint receives them as ordinary text.
- **Drawing:** the engine builds a display list of shapes, text, pictures, transforms, opacity and gradients. tiny-skia, also already in `Cargo.lock` through resvg, draws it to a bitmap. Being CPU-based, it produces the same pixels on every machine, which makes golden-image tests possible.
- **Layers:** each element is cached as a layer, so a drag only recomposes.
- **On screen (changed after the drawing spike):** each cached layer is its own image in GPUI's sprite atlas, placed with `paint_image` inside `canvas`, so moving a layer or fading a slide costs the CPU nothing. Composing one bitmap per frame was too slow. Images hold straight-alpha BGRA, because GPUI's shader multiplies by alpha itself. A layer that changes must release its old image with `drop_image`. GPUI draws selection handles and snap guides on top.
- **Thumbnails** use the same path at a lower resolution, cached per slide.

The drawing spike passed this way (`docs/checks/native-rust-spikes.md`), though only with 4K images in a smaller window. If a real 4K screen disappoints, the fallback is a small GPU compositor for the layers, using wgpu, which is also already in `Cargo.lock`.

### Custom components

```rust
use toolkit::prelude::*;

#[component]
fn Score(team: String, points: u32, colour: Color) -> Element {
    row![text(team).size(48), badge(points).fill(colour)]
}
```

- **Where they live (owner accepted):** one Rust file in the talk's `components` folder, and the function's name becomes the tag. One `#[component]` per file (my call).
- **What a component is (my call):** a pure function from its arguments and a context (current click step, time since the slide appeared, slide size, theme) to a tree of elements: text, rows, columns, shapes, pictures, icons and paths. It can read files inside the talk's `assets` folder and nothing else.
- **Wrapping content (my call):** a component can take a `children` argument and place it in its tree, like a Slidev slot, as in `<Callout>Some **Markdown**</Callout>`. `children` is an opaque placeholder and the host lays out the real content, so source positions never cross into WebAssembly and typing inside it still maps to the file. It gets no form field, because its content is edited on the slide.
- **Blocks only (my call):** custom components are placed as blocks, not in the middle of a sentence.
- **Built-ins use the same toolkit (owner accepted):** they are ordinary `#[component]` functions, compiled into the app.
- **Only our toolkit at first (owner accepted):** no outside crates. Because a component is one file and has no Cargo project, no download step exists.
- **Forms for non-coders (owner accepted):** the macro records each argument's type, and the inspector turns it into a field. `String` is a text box, numbers are number boxes, `bool` is a checkbox, `Color` is a colour picker, an enum is a dropdown, and a picture type is a file picker.

**Compiling** (my call on the method):
- The app runs its bundled `rustc` directly, with no Cargo, for `wasm32-unknown-unknown`. The bundle also holds the linker, the standard library for that target, the toolkit already compiled, and the `#[component]` macro already compiled for that platform.
- It compiles on save, in a folder where building is allowed (see the rules below). Goal: under two seconds for a 100-line component on a mid-range laptop. The owner accepted "probably within a few seconds", and the speed is unmeasured.
- Errors appear in a problems panel with the file and line, using `rustc`'s JSON output. The last good build stays on the slide.
- On request, the app can write a `Cargo.toml` that points at the toolkit's bundled source, so a coder who has Rust gets completions in their editor. It never does this unasked.
- A short guide with examples ships with the app.

**Running** (my call on the method):
- The compiled component travels in the talk with its source (owner accepted), so someone who only opens or presents the talk compiles nothing.
- Wasmtime runs it. Zed runs its Rust extensions the same way, on the same UI toolkit. It offers epoch interruption and memory limits, which the sandbox needs.
- A component gets no WASI and no imports other than the toolkit's own host functions, so it has no access to files or the network (owner accepted). Opening a talk needs no "this contains code" warning.
- Each render has a memory cap and a time cap. A component that hangs or crashes shows a labelled box on its slide and can't take the app down.
- The interface is a few plain exports: `abi_version`, `alloc`, `schema` and `render`, passing bytes in and out. This avoids the WebAssembly component-model tooling.

**Two rules that can't be relaxed later:**
- **The interface is forever.** Talks carry compiled code, so every later release must run every earlier interface version and read its element-tree format.
- **Opening a talk never compiles, and nothing compiles without a deliberate yes (my call on the details).** A file watcher can't tell a save from a `git pull`, so permission is per folder, not per change:
  - Each build records a hash of its source, and the app compares them per component on open. An up-to-date component runs. An out-of-date one runs its last build and is marked out of date. One with no build shows a "needs building" placeholder.
  - If any component is out of date or unbuilt, a bar offers *Build components* (once) or *Always build this folder*.
  - With *Always build this folder*, any change to a component there compiles on save, including changes from `git pull`. The app remembers this itself, never in the talk. Talks and components created in the app start with it on.
  - A packed talk never compiles. *Save as folder* unpacks it for editing, and the same rules then apply.
  - A component that currently fails to compile keeps its last good build. Saving a packed talk includes that build and says so in a one-line notice.
  - Before compiling, the app rejects `include!`, `include_str!`, `include_bytes!`, `env!`, `option_env!`, `#[path]`, `mod name;` and `#![feature]`. Otherwise a talk's source could copy a private file into its compiled output. `rustc` runs with a clean environment.

### Charts

The owner decided that a ready-made chart ships with custom components. The details below are my call, and none of them has been tested yet.

```markdown
<Chart kind="bar" labels="Q1, Q2, Q3" values="10, 24, 17" title="Sales" />
<Chart kind="line" data="assets/sales.csv" />
```

- **What it is:** a built-in, written with the same toolkit and `#[component]` macro as anyone's own component. It's compiled into the app, so it works in any talk with no build step, and the guide's worked example is a small bar chart built the same way.
- **Kinds:** bar, line and pie in the first release.
- **Data:** one series written in the tag, or one or more series from a CSV file in the talk's `assets` folder. A bad file or a mismatched list shows the labelled box, not a crash.
- **For people who don't code:** the usual form from its argument types, with a dropdown for the kind, a file picker for the data, and text boxes for labels and values. Colours come from the theme.
- **Animation:** a chart appears whole by default, and can build up one series, or for a single series one bar or point, per click.
- **Export:** the PDF gets vectors. In PowerPoint it follows the rule for plain elements, so it becomes native shapes. Bars are rectangles and labels are text boxes. Lines and pie slices are paths, and the PowerPoint writer has no freeform shape yet, so this stage adds one. A chart that builds up by clicks arrives in PowerPoint whole, with a notice.

### The editor

- **The window** follows the earlier design's PowerPoint-style layout: title bar, ribbon tabs, slide strip, slide canvas, notes and inspector. All of it is GPUI. A problems panel and the *View Markdown* panel are added.
- **Carrying over from this repo:** the window shell and key bindings in `workspace_view.rs`, the inspector's form approach, the pure state-machine style of `document.rs`, and the GPUI headless UI tests.
- **Dropped:** `canvas.rs`, `crates/protocol`, `crates/sidecar`, and the forced X11 mode in `main.rs`.
- **Typing on the slide (my call):** start from cosmic-text's `Editor` and `Buffer::set_rich_text`, with GPUI's input handling for IME. A rotated box is shown upright while typing.
- **Writing text back:** only the edited paragraph or box is written back, from its styled spans to Markdown. Bytes outside it are untouched. Writing and re-parsing must give back the same spans, which should be property-tested.
- **Copy and paste between talks:** the clipboard carries the Markdown, the assets it uses, and any custom component's source and compiled file.

### Presenting

Native windows, with no server (my call). The audience window goes full screen on the second display. The presenter window shows the current slide, next slide, notes and a timer. The engine drives click steps and transitions.

### Export

- **PowerPoint:** port the owner's MIT-licensed `slidev-pptx-export` OOXML writer (`src/ooxml/`, about 1,600 lines of JavaScript) and its animation timing to Rust, using quick-xml and a zip crate (my call). The input is now the engine's element tree, so its browser-driving and page-scraping code isn't needed.
- **What maps to what:** text to text boxes, shapes to preset shapes, tables to native tables, pictures to pictures, click steps and transitions to PowerPoint's own.
- **Custom components:** a component made of plain elements becomes native shapes. Anything else becomes a picture of its current state, with a notice.
- **Fonts:** embedding is ported too (`fonts.mjs`, `ooxml/eot.mjs`), because a typical PowerPoint machine has no Inter.
- **PDF:** krilla, which Typst's PDF export uses, from the display list (my call). Text stays selectable and fonts are subset.

### The Slidev add-on

The owner decided that Slidev compatibility is an add-on you get from the app's interface, for exporting to Slidev and importing a `.sptx` made in Slidev. The core app knows nothing about Slidev beyond recognising its files. The details below are my call, and none of them has been tested yet.

**Getting it:**
- An *Add-ons* page lists it with a *Get* button. That is the only thing in the app that uses the network.
- It comes over HTTPS from this project's releases, and its signature is checked against a key built into the app.
- *Install from file* covers computers with no internet.
- It updates from the same page, never in the background, so it can follow a new Slidev major version without a new app release.
- Removing it changes nothing about anyone's talks, because talks never depend on it.

**What it is:** a Rust translator compiled to WebAssembly, run in the same kind of sandbox as components. It takes a talk's files as bytes and returns files as bytes, and the app does any writing.

**Export to Slidev** writes a plain Slidev project, either as a `.sptx` that Stagepack can open or as a folder.
- It bakes our tags into what Slidev already understands, so no Vue version of each built-in is needed. `<Box>` becomes a positioned `<div>`, `<Text>` a styled `<span>`, shapes and icons inline SVG, clicks `v-click`, and notes a trailing comment, with `canvasWidth: 1920`.
- Regions that haven't been moved map to Slidev's own layouts (`cover`, `default`, `two-cols`, `section`, `none`). Moved ones become positioned `<div>`s.
- Custom components are called at export time and their output is inlined as SVG. A component that changes with click steps gets one version per step, switched with `$clicks`. One that animates over time is frozen, with a notice.
- The result holds no `.vue`, `.js` or `.wasm` and names no add-ons. By Stagepack's rules it opens with no "contains code" question and no change to Stagepack, as long as its manifest's `slidev` version has the major Stagepack runs.
- It looks close to the original, not identical, because a browser wraps some text differently.
- An exported `.sptx` is a Slidev talk, so double-clicking it here would import it. The save dialog suggests a name that says Slidev, and Stagepack is under *Open with*.

**Import from Slidev** makes a new, unsaved talk and never overwrites the original.
- **Converted:** Markdown, settings, notes, slots (`::right::`), `v-click` and `<v-clicks>`, transitions, code fences, `public/` files, and slides imported with `src:`. Layouts map to the closest of ours.
- **Not converted:** Vue components, unusual HTML, custom themes and scripts. Each becomes a labelled placeholder that keeps the original text in a code block, so nothing is silently dropped. Nothing from the Slidev talk is run.
- A short report lists what didn't come through, slide by slide.
- Export followed by import isn't lossless, because baked components come back as placeholders.

### Packaging

One installer per platform, containing the app, the compiler bundle, the compiled toolkit, fonts, icons and licence notices. Updates are manual in the first release (my call). The trimmed compiler must be tested on clean machines with no Rust installed, on both platforms.

### The repository (my call)

Build in this repo, on a new branch from `plan-1`, because the shell is there. Plan 1's branch stays as it is. Stagepack, the Studio fork and slidev-pptx-export don't change. The repo is renamed along with the product.

## Order of work

**Three throwaway spikes come first (my call).** Each answers one question. If any answer is no, come back to the owner before planning further.

1. **Drawing:** does a tiny-skia bitmap through GPUI handle rotated, styled text, dragging, and a fade transition smoothly at 1080p and 4K?
2. **Compile loop:** does a trimmed compiler with a pre-built macro and toolkit work on clean Linux and Windows machines, and how many seconds does a save take?
3. **Typing on the slide:** can per-word styling, accented input through the system's input method, and emoji work on cosmic-text and write back to Markdown without changing anything else? Right-to-left text and Chinese, Japanese or Korean input are outside this spike.

**Then, in this order (owner accepted that the engine comes before the editor):**

- **A. Engine:** open a folder, parse, draw, reload on save, built-in components and layouts.
- **B. Present and export:** coders can use early builds from here.
- **C. Custom components:** compiling, sandbox, forms and the ready-made chart.
- **D. Visual editor:** goals 1 to 11.
- **E. Packaging.**
- **F. Slidev add-on.**

**Windows checks (owner's answer):** the owner runs these by hand on a Windows PC with desktop PowerPoint. Spike 2 and each stage end with a short checklist for that machine, and the next stage waits for it. Spike 2's check must run with no Rust installed, for example under a fresh user account.

The first public release is when both success tests pass.

## Known risks

- **The editor is most of the project.** The browser used to provide text layout, typing and selection, and now we build them. Typing and styling words on the slide is the hardest part.
- **The first release is large:** an engine, a compile system and a full visual editor.
- **Text can wrap differently in PowerPoint,** because PowerPoint does its own layout. Embedded fonts reduce this but don't remove it.
- **Animated PowerPoint export has only been checked in PowerPoint for the web,** so it needs checking in desktop PowerPoint on the owner's Windows PC.
- **Reload speed is unmeasured.** If it is slow, the levers are optimisation level, code generation settings and keeping the toolkit pre-built.
- **Slidev import is best-effort.** Talks that lean on Vue components or custom themes will come through with many placeholders.
- **gpui-kit is young and moves weekly.**
- **Nothing here has run on real Windows yet.**

## Left to the builder

- The crate layout, and the exact names of tags, attributes and toolkit functions.
- The settings-block parser.
- The set of built-in layouts, starter templates, shapes and icons.
- The exact ribbon tabs and which commands sit where.
- The sandbox's memory and time limits.
- Test strategy, beyond the golden images and round-trip property tests named above.
