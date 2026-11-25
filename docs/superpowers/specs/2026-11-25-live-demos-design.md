# Live demos: clickable slides, a talk's own container, and maps

The owner's design conversation of 2026-11-25. It builds on the native design (`2026-11-09-native-rust-slides-design.md`), its components (stage C), the 3D models (G2) and the live terminal (G5).

## Why

Stagepack Studio is for tech talks, and a tech talk often has a demo. Today a demo means leaving the slides for another window, or showing a recording. The owner wants the demo on the slide: things the presenter clicks with the mouse during the show, commands that run live, and maps of the talk's own data that can be panned, turned and clicked. The demo must behave the same on a conference laptop as on the computer it was made on.

## Who and what

- **The presenter clicks.** Nobody else does: there is no audience interaction, and nothing is shared over a network.
- **A click on an interactive thing is for that thing.** It does not move the show on. A click anywhere else, the keyboard and a clicker move the show as they do today. Normal slides behave exactly as now.
- **The demo travels with the talk.** Its tools come from a container the talk describes, and its map data from files in the talk.

## Goals

1. **Clickable slides.** Buttons, and the talk's own components, react to clicks, drags and the wheel, and keep state of their own while the slide is shown.
2. **Programmable buttons.** A button, set up by its tag's attributes with no code, can type a command into a live terminal on the same slide, tell another interactive thing on the slide to do something (show a map layer, fly to a place, highlight a feature), or go to a click step.
3. **The talk's container.** Every live terminal and every button command of a talk with a container recipe runs inside that container, never on the host.
4. **Maps.** A map of the talk's own data, on a background map: shapes, points and routes in 2D; in 3D, tilted and turned, with buildings raised to their height. A click on a feature highlights it and shows its fields.

## Non-goals

- Audience interaction, voting, phones, or anything over the network to other people.
- A container engine of our own. The app drives Docker or Podman.
- A general GIS: no editing of map data, no analysis, no projections chosen by the talk (Web Mercator only), no raster data in the first version (GeoTIFF is a later addition).
- A web view, for anything. The project removed it deliberately.
- Map data fetched from the internet during the show, other than background tiles the talk allows.

## Anti-goals: what would make this a failure even if it works

- A click meant for a map or a button moves to the next slide, or a click meant to move on gets swallowed. The presenter must never get lost in front of an audience.
- The demo works at home and fails at the conference because of the network, a missing tool or a slow start.
- A talk opened from someone else can run a command on the presenter's own machine without a yes.
- A map looks like a debugging view: no labels, wrong colours, or jerky panning.

## Constraints

- **Native drawing only**, through the engine's display list and, for 3D, the wgpu path the models use (`crates/model`, `draw` feature). PDF and PowerPoint get a still picture of a map's starting view, and a button's look.
- **The component interface is fixed at v1** (`crates/toolkit/src/interface.rs`, held by `tests/interface_v1.rs`). Interaction is a v2 added beside it. Components built against v1 keep working unchanged.
- **The network rule changes, narrowly.** Today the app goes online only on *Get* (F2). This adds three cases:
  - building a container recipe, on *Prepare demo*;
  - fetching background tiles, only in a talk whose map allows online tiles, and only while a map is on screen;
  - whatever the talk's container does itself, which is the container's business.

  Nothing else goes online.
- **The yes gate stays.** A talk can't start a container, or run a command anywhere, without the presenter's yes. G5's per-command question covers commands on the host. A talk with a container gets one question, for the container as a whole, described below.
- **Windows and Linux both.** On Windows the container needs Docker Desktop, which runs on WSL2.
- Files a talk carries are data, checked against hard limits like every other kind (models, recordings).

## Approaches considered

- **A: all native (chosen).** Interaction goes through the existing WebAssembly sandbox as interface v2. Buttons and maps are built-in tags. Maps are drawn by our engine from vector tiles and the talk's data files, and 3D maps by the models' wgpu path. It matches the app's whole design (one way of drawing, the same pixels, still pictures in exports), and costs the most work, mostly in making maps look good.
- **B: a web view for interactive things.** MapLibre would give 3D buildings and popups at once. It was rejected because it brings back the web view the project removed, and a second drawing system that behaves differently from the slide, in exports and in the yes gate.
- **C: RustGIS's map view.** Rejected because it draws with egui, which would have to run off-screen and be copied into GPUI. Its pure-Rust readers (Apache-2.0) may be borrowed inside A.

For the container, the owner chose **both**: the talk holds a recipe, which is built on each computer ahead of the talk, and *Pack for travel* puts the built image into the talk for offline use. The alternatives were a recipe only, which fails if the network is blocked at the venue, and an image only, which is heavy for every talk.

For background maps, the owner chose **both**: online tiles while the talk is being made, and *Pack for travel* saving the area the talk shows into it. The alternatives were online only, which needs the network during the show, and offline only, which is clumsy while making the talk.

## The three projects, in order

The owner agreed this order: 1 → 2 → 3. Buttons need 1's clicks, and their commands need 2's container. Maps build on both.

### 1. Clickable slides and buttons

**Where clicks go in the show** (owner: a click on an interactive thing is for that thing):
- A click, drag or wheel turn that lands on an *interactive* thing goes to it, and the show does not advance. Today's mouse-click-advances behaviour stays for everything else on the slide.
- Interactive things are: a button, a map, a live terminal (which already takes keys after a click), and a component that declares it handles input.
- My call: the mouse pointer is shown on an interactive slide and hidden after 2 s without movement on other slides, as today.
- My call: an interactive thing shows a faint outline on hover in the show, so the presenter can see where a click will go. It never appears in exports.

**`<Button>`, a built-in tag.**
- `label`, plus the usual place attributes.
- My call: a small fixed look set (primary, secondary, quiet), coloured from the theme.
- One action, given by attributes:
  - `run="…"` with `in="<id>"`: types the command, followed by Enter, into the live terminal with that `id` on the same slide, starting it first if needed, behind the usual gate.
    - In a talk with a container, the container's yes covers it.
    - My call (builder check): in a talk without one, on the host, each button's command text is its own yes, asked on the first press with the whole text shown, matched exactly, and asked again if the text changes. A yes for the terminal's shell does not cover text a button types into it. *Check* lists a talk's button commands.
  - `do="<action>"` with `on="<id>"` and an optional `value="…"`: sends a named action to another interactive thing on the slide. Maps define `show-layer`, `hide-layer`, `toggle-layer`, `fly-to` and `highlight`, and a component defines its own.
  - `step="N"`: goes to click step N of the slide.
- My call: a button is pressed with a click only, and Space or Enter keep moving the show. This keeps the keyboard predictable.
- The editor's form for a button lists the slide's `id`s to pick from, and *Check* (the AI `check` command too) reports an `in` or `on` that names nothing.

**Interface v2 for the talk's own components.**
- A component opts in by taking an event. Its `render` gets the component's own state, as a JSON value of at most 64 KiB, and the events since the last call. An event is a pointer down, move, up or wheel in the component's own coordinates, or an action from a button. It returns its element and its new state.
- Elements may carry a `hit` name, so an event says which part was clicked.
- State lives while the slide is shown. Leaving the slide resets it, as a model's view is reset today. It is never written to the talk.
- My call: the existing sandbox limits apply per call, and events are coalesced to at most 60 calls a second.
- v1 components are run exactly as now. The guide gets a chapter, with an example talk in `fixtures/components`: a counter, and a toggle that a button drives.

**With the presenter window open** (my call, builder check): the presenter's current slide is clickable, as the audience window's is, and both show one state, kept with the show rather than in either window, so a click on one is seen on both. The next-slide preview stays a still picture.

**The editor:** interactive things work in the editor only when selected and switched on with *Try* (as a video's *Play*), so editing clicks still select and move them.

### 2. The talk's container

**The recipe and the image:**
- A talk may hold `demo/Containerfile` (also read as `Dockerfile`) and whatever files it copies in. My call: the folder `demo/`, beside `assets/` and `components/`.
- With a recipe present, the talk *has a container*. Every `<Terminal run>` and every button `run` of that talk runs inside it.
- A talk without a recipe keeps G5's behaviour on the host, gate and all.

**Engines:** the app looks for `docker`, then `podman`, on the `PATH`, and drives its command line. There is no Docker API library. With neither, the slide shows the terminal's recording if it has a `src`, and otherwise a plain panel: *This demo needs Docker or Podman*.

**Getting ready (the owner's "both"):**
- **Prepare demo**, a button in the File menu and on the start of the show when needed, builds the recipe into an image tagged by a hash of the `demo/` folder. It shows progress and has *Cancel*. This step uses the network, and only on that click.
- **Pack for travel** saves the built image (`docker save`, compressed) into the talk as `demo/image.tar.zst`, and the map areas (project 3).
  - A packed image is loaded (`docker load`) instead of building, whenever its hash matches the recipe.
  - A recipe changed after packing is said to be out of date in the editor, and *Check* says so.
- My call on the limit: a packed image may be at most 4 GB, as a video.

**Running:**
- **Fresh each show** (owner, 2026-11-25): starting the show starts a clean container from the image, so a rehearsal leaves nothing behind, and ending the show removes it. It's started as the show starts, so the first command doesn't wait.
- In the editor, a container starts on the first *Try* or *Start* and is kept while the talk is open, with *Reset demo* to start it clean. Closing the talk or window removes it.
- Files written to `/talk` are not part of the container and stay, as the folder's own files.
- One container per open talk. Each terminal or button command is an `exec` into it, through the existing helper, with its pseudo-terminal.
- The talk's folder is mounted at `/talk` and is the working folder. My call: it is mounted read-write for a folder talk, and a copy for a packed or untitled talk, so a demo can write files without changing a talk that isn't a folder.
- Network access inside the container is on. The owner's demos need it.
- My call: there are no host mounts besides `/talk`, no privileged mode, and the engine's default user. A recipe can't ask for more.

**The yes:**
- My call (builder check): the question comes *before anything of the talk's reaches the engine*, on the first *Prepare demo*, on loading a packed image, or on the first start, whichever comes first. A build runs the recipe's `RUN` lines, and a load writes the talk's image into the presenter's engine, so neither may happen on a click that didn't say so. Before a build, the question shows the recipe itself (scrollable) and its `FROM` line; before a load, the packed image's size and the recipe it was built from. The remembered key is the hash of the `demo/` folder.
- One question, the first time a talk's container is used: what the image is built from (its recipe's `FROM` line and size), that it can use the network, and that it sees the talk's folder. The answers are *Run*, *Always for this folder* (folder talks only) and *Don't run*, which is the default.
- Commands inside an allowed container aren't asked about one by one, since they can't reach the host beyond `/talk`.
- A changed recipe asks again.

**Keep as recording** works as in G5, so a talk can still carry a fallback.

### 3. Maps

**`<Map>`, a built-in interactive tag.**
- `src` names one or more data files in `assets/`.
- The view is set by `center`, `zoom`, `tilt` and `turn`, as a model's view is. *Set view* in the editor writes them, and *Add this view* adds a view the show flies to on a click, as models do.
- `basemap` takes `streets`, `light`, `dark` or `none`, and `online` takes `true` or `false` (owner's "both", below).

**Data, from files in the talk only:**
- GeoJSON for points, lines and polygons;
- GPX for routes and tracks;
- CSV with `lat` and `lon` columns, for points.
- My call on limits: at most 64 MB a file and 1 million vertices a map. A file outside the talk, or one that isn't what it says, is refused in one sentence, like every other kind.

**Layers and style:**
- Each file is a layer, named after the file.
- My call: a small style set in the tag or in a `.style.json` beside the file, covering fill, line, width, point size, a label field and a height field. Defaults come from the theme. No MapLibre style language.
- Labels are drawn with the bundled fonts.

**Clicking:** a click on a feature highlights it and opens a small panel by it, listing its fields. A click elsewhere closes the panel. Buttons can `highlight` a feature by a field value, `fly-to` a feature or view, and show, hide or toggle a layer.

**2D first, then 3D:**
- In 2D the map is drawn by the engine's display list, so it pans and zooms smoothly at 60 frames a second at 1080p.
- In 3D (`tilt` above 0), polygons with a height field become raised blocks, and routes are drawn on the ground. The drawing uses the models' wgpu path, and dragging and the wheel turn and tilt the view the same way. A highlighted building is lit in the theme's accent colour.
- Under the 3D scene lies the background map, drawn as in 2D and laid flat on the ground as a texture, labels included (owner, 2026-11-25). Background tiles drawn in perspective with upright labels were considered and left out: a second map renderer, for weeks more work.
- A computer that can't draw 3D shows the 2D map and says so in the editor.

**The background map (owner's "both"):**
- The background is vector tiles in the OpenMapTiles layer scheme, drawn by our own style for each `basemap`.
- **Online** while making the talk, from a public vector-tile source that allows this use. The project picks one whose terms allow an app and writes its attribution on the map.
- My call (builder check), keeping the rule that nothing goes online until the person asks: `online` is off unless the tag says `online="true"`, and even then the first online map in a talk asks once (*Load background maps from <source>?*, *Allow* / *Not now*), remembered per folder talk like *Always*. A stranger's talk never fetches tiles on opening. A map added in the editor writes `online="true"` and asks then, since that's the person's own click.
- **Pack for travel** cuts the tiles covering the map's views, plus a margin, at the zoom levels used, into `assets/<map>.pmtiles`.
- In the show, the packed copy is used whenever there is one. Online tiles are used only for missing areas, and only if the talk allows it. Otherwise, missing areas are blank, with a quiet note in the editor.
- My call: a packed `.pmtiles` may be at most 512 MB, and *Pack for travel* says the size before writing and refuses beyond it, suggesting fewer zoom levels. Tiles from OpenStreetMap data are under the ODbL, which allows handing them on inside a talk with attribution; the chosen source's own terms must allow the same, which narrows the choice of source.
- My call: tiles fetched online are cached in the app's cache folder, never in the talk unless packed.
- Attribution, as the tile licence requires, is always drawn in a corner and kept in exports.

**Exports:** the still picture of the starting view, with its attribution. PDF and PowerPoint say how many maps became pictures, as with models.

## The parts the owner decided

- The presenter clicks; there is no audience interaction.
- A click on an interactive thing doesn't advance the show.
- The demo environment comes with the talk, like a container: recipe plus optional packed image.
- Map data comes from files in the talk. A click shows details. There are polygons, routes, and 3D buildings that light up.
- Background maps: online while making the talk, and packed into it for the show.
- Native, approach A.
- The order: clicks and buttons, then the container, then maps.
- A 3D map has the 2D background map laid flat under it.
- A fresh container each show; kept in the editor, with *Reset demo*.

## What's left to the builder

- The exact v2 event and state JSON, and how a v2 component says it's interactive (alongside `ComponentSchema`).
- The choice of crates for GeoJSON, GPX, MVT decoding and PMTiles reading and writing, preferring pure Rust and checking their licences against the NOTICES walk. RustGIS's crates may be used if published and suitable.
- The public vector-tile source, checked for terms that allow it, and the attribution text.
- How the app finds and reports the container engine's version, and the minimum versions it accepts.
- How a stuck `docker build` or `docker load` is cancelled, and what is left behind.
- The look of buttons, the feature panel and the three basemap styles, to be shown to the owner as pictures before they are fixed.
- Performance targets beyond the 2D 60 frames a second, such as 3D with 100,000 buildings, measured on the owner's laptop.
