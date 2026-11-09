# Native Slidev editor: design

Status: approved by the owner, 2026-11-09. The working name is **Stagepack Studio**. The final name is the owner's call.

## What this is

A desktop app that lets someone who uses PowerPoint today make, edit, present and share Slidev talks, as a single install.

- **Native window:** built with Longbridge's [gpui-kit](https://github.com/longbridge/gpui-kit).
- **The slide itself:** a live Slidev page inside an embedded web view.
- **Editing:** a fork of [slidev-addon-studio](https://github.com/Eiriksb/slidev-addon-studio), driven from native panels.
- **Underneath:** Stagepack's bundled runtime (Node, a pinned Slidev and [slidev-pptx-export](https://github.com/Eiriksb/slidev-pptx-export)).
- **Files:** a talk is a `.sptx` file, the way a PowerPoint talk is a `.pptx`.

## Why

**Who it's for (owner's answer):** people moving from PowerPoint to Slidev and `.sptx`.

**What pulls them (owner's answer):** Slidev can do more than PowerPoint. Today, getting those features means a terminal, Node and editing Markdown. This app removes that barrier and keeps the features.

**Success:**
- A PowerPoint user downloads one installer and runs it.
- They choose *New presentation* or double-click a `.sptx`.
- They build a ten-slide talk with text, pictures, shapes, a table and click animations.
- They present it, save it, and export it to PowerPoint for a colleague.
- They never see Markdown, a terminal or a missing-dependency error.

## Goals for the first version

The owner accepted this split on 2026-11-09.

1. **Slides:** a strip of live thumbnails. New slide from a layout, duplicate, delete, reorder by dragging, hide and unhide.
2. **Text:** typing in place; bold, italic, underline; size, colour, alignment; bullets and numbering.
   - **Size and colour apply to the selected words** (owner's answer), as in PowerPoint, not only to a whole text box.
   - So the fork's in-place editor must read and write styled inline spans, for example `<span class="text-red-600 text-2xl">`, and round-trip them without loss. Today it refuses styled spans. This is the largest single piece of fork work.
3. **Pictures:** insert from a file, drag in from the desktop, paste from the clipboard; move, resize, rotate.
4. **Shapes, arrows and icons:** a set of ready-made shapes, with fill, outline and text inside, plus an icon picker.
5. **Arrange:** align, distribute, bring forward and send back, and snapping. Studio already snaps.
6. **Tables:** a grid with add or remove row and column, and typing in cells.
7. **Animations and transitions:** Studio's click steps, entrances, staggered lists and slide transitions, from a native panel.
8. **Notes and presenting:** a notes area under the slide. *Present* opens Slidev's presenter view, as Stagepack does today.
9. **Undo, copy and paste:**
   - undo and redo across the whole editing session, including slide reordering and adding;
   - copy and paste of elements and slides, including between two open talks.
10. **New presentation:** a few starter templates. None of them carries its own code, so none triggers the trust prompt.
11. **Save:** writes back into the `.sptx`. Also *Save as*, an unsaved-changes prompt on close, and recovery after a crash.
12. **Export:** to PowerPoint (editable, and with animations) through slidev-pptx-export, and to PDF.
    - **In the editable PowerPoint, the app's shapes become real, editable PowerPoint shapes** (owner's answer). A rectangle arrives as a rectangle and an arrow as an arrow, with the same fill, outline and text.
    - That means work in slidev-pptx-export too: it learns the shapes addon's components and maps each one to a native shape. Without this, its render mode turns inline SVG into pictures.

## Non-goals for the first version, planned later

Later, in roughly this order:
- charts;
- video and audio;
- cropping and grouping;
- switching themes;
- find and replace;
- importing `.pptx`;
- macOS.

## Out

- Comments.
- Editing the same talk with others at the same time.
- An online or web version.

## Anti-goals: these make it a failure even if it works

- **Lock-in:** a talk saved by the app must stay a plain Slidev project. Unpacked, it runs with `slidev` and renders the same.
  - The app writes Markdown, frontmatter, UnoCSS utility classes, Slidev's own components and directives, and components from published addons.
  - It writes no format of its own and no hidden state that only the app understands.
- **Studio in the deck:** opening a talk to edit it must not add Studio, or anything else that exists only for editing, to the talk's files. A talk edited in the app and opened in plain Stagepack shows no trace of the editor.
- **Markdown in the way:** in a talk made in the app, any first-version task that forces a user to see or type Markdown counts as a bug. A *View Markdown* panel may exist for those who want it.
  - **The guarantee covers talks made in the app** (owner's answer): those started from its templates and built with its tools.
  - Hand-written Slidev talks open and mostly edit visually. Parts Studio can't edit visually, such as custom components or unusual HTML, may still fall back to showing their Markdown. That is acceptable there.
- **Editing the wrong lines:** Studio's rule stays. When it can't be sure which lines a click maps to, it refuses and says so.
- **Losing work:** a crash or kill during editing or saving never leaves a damaged `.sptx`, and unsaved edits can be recovered.
- **Needing the internet, Node or npm** to make, edit, present or export a talk. The one exception is installing WebKitGTK on Linux; see Constraints.
- **Bypassing trust:** a talk that carries its own code gets Stagepack's existing "contains code" question before anything of it runs, in the editor too.

## Constraints

- **Platforms in the first version:** Windows 10/11 x64 and Linux x86_64, the same as Stagepack today.
- **Linux display:** `gpui-webview` needs X11. The app runs on X11, or on XWayland in a Wayland session such as Ubuntu's default.
- **gpui-kit:** depend on the published crates (0.9 at the time of writing), not a fork. `gpui-webview` is experimental, and the owner accepts that. Where it lacks something, work around it in the app or send a fix upstream.
  - **GPUI Fast:** build with gpui-kit's `gpui-fast` feature, and the same feature on `gpui-webview` (my call). It uses Longbridge's own published GPUI Fast crates, which are still a dependency rather than a fork of ours. It is what lets GPUI draw popups over the page on Linux.
- **Studio:** work in the fork. Keep its MIT notice, and credit Daniel Christensen in the app's licences screen. Changes worth having upstream are offered back as pull requests.
- **The runtime:** Stagepack's pinned runtime is today Slidev 53.0.0, `@slidev/theme-default`, Inter and JetBrains Mono, `playwright-core` and slidev-pptx-export 0.5.0, run by Stagepack's bundled Node. The editor uses that runtime, plus whatever the editor itself needs (Studio, the shapes components).
- **Browsers:** exporting to PowerPoint or PDF needs Chrome, Edge or Chromium on the computer, as Stagepack does today. The web view itself is the system engine: WebView2 on Windows, WebKitGTK 4.1 on Linux. If WebKitGTK is missing, the installer says so in one sentence, with the command to install it.
- **Install:** per user, with no administrator rights, on both platforms, like Stagepack. It registers `.sptx` files.
- **Licences:**
  - the new repo: MIT, like Stagepack and the exporter (my call);
  - gpui-kit: Apache-2.0, with its notices kept;
  - the Studio fork: MIT.
- **Public repos:** don't name real decks, clients or organisations in commits, issues or PRs. Use fixtures.

## How it fits together (decided)

### One window, three layers

```
┌ title bar: file name • unsaved dot • Save • Undo/Redo • Present • Export ───────┐
│ ribbon tabs: Home  Insert  Arrange  Animate  Slide  View                        │
├──────────────┬──────────────────────────────────────────────┬───────────────────┤
│ slide strip  │                                              │ inspector         │
│ (live        │          slide canvas (web view:             │ (the selection's  │
│ thumbnails)  │          Slidev + Studio's canvas layer)     │ properties)       │
│              │                                              │                   │
│              ├──────────────────────────────────────────────┤                   │
│              │ notes                                        │                   │
└──────────────┴──────────────────────────────────────────────┴───────────────────┘
```

This layout copies PowerPoint's on purpose, so a switcher's hands already know where things are. Exact panels and ribbon contents are the builder's call within the goals above.

1. **The native shell (Rust, gpui-kit):** the window, menus, ribbon, inspector, notes, dialogs, file handling, undo history and recent files. It is native GPUI throughout.
2. **The canvas (web view):** Slidev's dev server serves the talk, as Stagepack runs it today (`slidev ./<entry> --port N --open false`, on localhost only). The canvas shows one slide at a time, with the Studio fork's canvas layer on top: selection, handles, snapping, dragging, in-place text editing, and tracing a click back to its Markdown.
3. **The bridge:** a small, versioned JSON message protocol between the shell and the Studio fork. It runs over Wry's IPC: page to shell through `window.ipc.postMessage`, shell to page through `evaluate_script`.
   - **The page reports:** what's selected; the selected element's props schema, classes, position and animation; the slide list; the click steps; errors and refusals.
   - **The shell sends commands:** "set this prop", "apply these classes", "align these", "add a slide with this layout", "insert this component", and so on.
   - **Writing Markdown stays in the Studio fork.** It already writes through Slidev's own endpoints and refuses unsafe edits, so the shell never patches Markdown text itself, apart from undo (below).

### What happens to the Studio fork

- **It splits in two:**
  - the canvas layer, which stays and is driven by the bridge;
  - Studio's own web panels (Element, Components, Animate, Layout, Slides, Assets), which are replaced by native ones.
- **It keeps working as a normal Slidev addon** in a browser for people who don't use the app. The web panels stay available when the bridge isn't there, so the fork's own users lose nothing.
- **It's published under its own npm name** (my call: `@eiriksb/slidev-addon-studio`), so the runtime can pin it.
- **The editor loads it without touching the talk's files** (see the anti-goals). How it does that (a wrapper entry, a Vite config in the work folder, or a runtime flag) is the builder's call.
- **Hidden slides:** Slidev drops them from its slide list, so the page can't report them. That's why Studio says hiding a slide can't be undone from the editor. For the strip to show hidden slides and unhide them, the fork's Vite plugin needs a slide list read from the Markdown itself.
- **Slidev 53:** Studio was built against Slidev 52.19, and nobody has checked it on the runtime's 53.0.0. The first piece of fork work is running its tests and playground on 53.

### Shapes and icons

- **Shapes:** a new published addon of Vue components, for example rectangle, rounded rectangle, ellipse, line, arrow, callout and text box, with fill, outline and text props. Studio's props editor already turns component props into controls.
- **In a saved talk:** the talk lists the addon in its headmatter, so it renders in plain Slidev once the addon is installed. The editor's runtime and Stagepack's next release both bundle it, so a `.sptx` that uses shapes also opens in an up-to-date plain Stagepack.
- **Slidev's own `<Arrow>`** is used where it's enough.
- **Icons** come from Iconify sets bundled in the runtime, so the picker works offline. Which sets is the builder's call; keep the size reasonable.

### Files, saving and recovery

- **On open:** a `.sptx` unpacks to a work folder, as Stagepack does today.
- **While editing:** Studio writes to the work folder, and the `.sptx` stays untouched until **Save**. This is PowerPoint's model: edits are unsaved until you save.
- **On Save:** the `.sptx` is rewritten from the work folder atomically, using Stagepack's journalled, crash-safe write approach, extended from one entry to the whole folder.
- **The work folder doubles as AutoRecover:** if the app dies, the next open of that talk offers to recover the unsaved edits.
- **Undo history** is snapshots of the talk's files, taken by the shell after each completed edit. Undo writes a snapshot back, and Slidev's watcher picks it up. This replaces Studio's in-page undo, which is lost on reload and on slide renumbering.
- **New presentation** writes a starter template into a new work folder. The first Save asks where to put the `.sptx`.

### Stagepack underneath

- **Stagepack's Go code stays the engine** for unpacking, packing, saving, trust, export, the runtime and install. The shell calls it as a bundled sidecar binary, either by commands or through a small local protocol. This is my call: it's 10,000 tested lines that already work on both platforms, and rewriting them in Rust buys nothing for the user. The builder may move pieces into Rust where calling the sidecar is awkward. Unpacking a talk to show its first slide is one example.
- **A separate app beside Stagepack** (owner's answer).
  - It has its own per-user install folder and its own copy of the Stagepack binary and runtime.
  - It updates on its own schedule. A plain Stagepack install, if there is one, stays as it is.
- **One install, with nothing else needed.** The installer bundles:
  - the shell;
  - its own Stagepack binary;
  - the runtime (Node, Slidev, the exporter, ffmpeg, the Studio fork, the shapes addon, the icon sets);
  - licence notices.

  It needs no Stagepack install, and it works whether or not one is there.
- **Work folders** live under the editor's own folder, never under Stagepack's runtime folder, so plain Stagepack's stale-folder clean-up can't reach them. The editor's own sidecar must keep folders with unsaved edits or recovery data too (see "Stagepack changes" above).
- **The file type:** the editor takes over double-clicking `.sptx`, so a double-click opens the talk for editing. Plain Stagepack stays under *Open with*, and its right-click entries (*Present* and the two PowerPoint exports) stay.
  - If Stagepack is installed or upgraded later and takes the default back, the editor notices at its next start and offers to take it back (my call).
  - Uninstalling the editor hands the default back to Stagepack if Stagepack is installed.
- **Talks with shapes opened in plain Stagepack:** someone without the editor may open such a talk in plain Stagepack. So Stagepack's next release also bundles the shapes addon and adds it to the trust allowlist.
  - Older Stagepack versions show the talk without its shapes. The plan adds a clear "needs a newer Stagepack" message there, if that can be done without changing old installs; otherwise this is documented.
- **PDF export:** Slidev's own `slidev export`, pointed at the computer's Chrome, Edge or Chromium with `--executable-path`.
  - Slidev 53's export only loads `playwright-chromium`, so the runtime adds that package with its browser download skipped. `playwright-core` alone isn't enough.
  - PowerPoint export works as it does today.
- **Presenting from the editor (my call):** *Present* opens presenter view on the editor's own Slidev server, so it shows the current, unsaved state.
  - Notes edited in presenter view land in the work folder, unsaved like any other edit.
  - Right-clicking *Present* on a `.sptx` that's already open in the editor brings up that editor's presenter view. It does not start a second Stagepack that would write into the same file.
- **Stagepack changes the editor needs:**
  - Stagepack's trust check lets only `slidev-addon-stagepack` through (`bundledAddons`, internal/deck/code.go). The shapes addon must join that list, or every talk with a shape gets the "contains code" question.
  - Stagepack deletes work folders that no running instance owns once they are 10 minutes old (`CleanStale`, internal/app/app.go). The editor's own copy of Stagepack must leave alone the work folders that hold unsaved edits or recovery data.

## Repos this touches

- **The new repo** (name to be decided): the shell, the installers, the starter templates, and the shapes addon, published to npm from there (my call).
- **The Studio fork** (Eiriksb/slidev-addon-studio):
  - the bridge;
  - the panels' split, with the web panels kept for browser users;
  - styled inline spans;
  - the source-level slide list;
  - Slidev 53.
- **slidev-pptx-export:** turning the shapes addon's components into native PowerPoint shapes.
- **Stagepack:** bundling the shapes addon and allowing it in the trust check, keeping folders that hold unsaved edits during clean-up, and the default-handler behaviour above.

## Known risks (accepted)

- **Windows: the web view covers GPUI popups.** GPUI Fast lets popups draw over the page on Linux, but not on Windows. There, any ribbon menu, colour picker or dialog that overlaps the canvas would be hidden behind it. The shell must handle this. It can keep popups clear of the canvas, swap the canvas for a still image of itself while a popup is open, or hide the web view under modal dialogs. The builder picks.
  - On Windows, a web view that's only partly scrolled out of its container also spills past the container's edge. Each web view should be sized to its panel and scroll inside the page. A thumbnail strip should not be a web view inside a GPUI scroll area.
  - gpui-kit's Windows example sets `GPUI_DISABLE_DIRECT_COMPOSITION=true` before the web view renders, and the app needs the same.
- **Linux: Wayland isn't supported natively,** so the app runs through XWayland.
- **Stagepack on Windows has never run on a real Windows machine.** Its README's "Open items" says Windows is checked only by cross-compiling and unit tests. The animated PowerPoint export also hasn't been checked in desktop PowerPoint. The plan checks the sidecar on real Windows early, not at the end.
- **Studio is young:** version 0.3.1, from one author. Its source tracing is a "hint, not a contract", and the fork inherits its limitations, such as classes on multi-line blocks and fragment-root components.
- **The bridge is the largest new piece,** and the native panels depend on it. Its protocol should be the first thing built and tested.

## Left to the builder

- The crate and module layout of the shell, and the bridge's message names.
- Exact ribbon tabs and which commands sit where, within the goals.
- How the slide strip renders live thumbnails, for example a second web view or rendered images. They must match the slide and update as it's edited.
- The set of starter templates, shapes and icon sets.
- How the sidecar is called, and anything moved from Go to Rust.
- How the editor loads Studio without writing it into the talk.
- Test strategy. The shell's UI should use gpui-kit's headless UI test harness where it can.
