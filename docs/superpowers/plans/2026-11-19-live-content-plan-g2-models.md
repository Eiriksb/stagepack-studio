# Plan G2: 3D Models Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A 3D model file added to a talk is drawn on its slide. It turns slowly on its own if asked, can be turned and zoomed by hand while presenting, and flies between saved views on clicks. A talk holding one opens, edits, saves and exports as one without does, and shows a still where 3D can't be drawn.

**Architecture:** A new crate, `slides-model`, reads a model file (`.glb`, `.ply`) into a checked, bounded mesh or point cloud, and draws it with wgpu off-screen into a picture, on a thread of its own; it knows nothing of windows. The engine treats `<Model>` as a placed thing whose layer is a still, made off the window's thread by the same job mechanism video stills use, so thumbnails, exports and every editing tool work unchanged. The app lays the live picture over that layer while the model is shown in the show or turned in the editor, exactly as video frames are laid. Large files from plan G1 carry the model file; the save's reader gate covers its readers.

**Tech Stack:** Rust 1.98.0; `wgpu =30.0.1` (already in Cargo.lock through the window toolkit), `gltf =1.4.1`, `bytemuck` and `pollster` (in Cargo.lock), as the spike used; gpui-kit 0.9.0 with gpui-fast 0.1.13 (`paint_image`, `drop_image`); proptest.

**Spec:** `docs/superpowers/specs/2026-11-18-live-content-design.md` (Goal G2; "G2. 3D model"; the anti-goals *A talk reaching your computer*, *Needing anything else*, *Losing work*, *A talk too big to work with*). `docs/checks/live-content-spikes.md`, section 2, has what the 3D spike measured and the conditions it set. `docs/superpowers/plans/2026-11-18-live-content-plan-g1-decisions.md` has what plan G1 left for this plan.

## Global Constraints

- Work on the branch `plan-g2`, cut from `plan-g`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx`, `slides-components`, `slides-video` and the new `slides-model` don't depend on GPUI.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **A model is data.** Reading one can't run a program, follow a link, read a file outside its own, or use the network. Everything it needs is inside the one file; a reference to another file is refused. A file that isn't what it says, or is damaged at any byte, shows a still or a plain panel and one plain sentence; it never crashes, hangs, or allocates what the file claims.
- **The spike's conditions bind** (`docs/checks/live-content-spikes.md`, section 2): the 3D device is made on a worker thread the first time a model is shown, and never holds up drawing the window; limits checked before anything is decoded (256 MB, 4 million triangles or vertices counting instances, a texture 8,192 pixels a side at most, 64 million texture pixels in all, 20 million points); on an integrated card the read-back is copied on several threads or the picture drawn smaller.
- **Nothing waits on the window's thread** for a model: no parsing, no drawing, no device creation, no read beyond what plan G1 allows for a video (a header of 64 KB at most).
- **Nothing is lost**, and **a large file is never held whole by the app beyond the parse itself**, which happens off the window's thread with the limits above. Every rule of plans D5 and G1 for working copies, saving and the reader gate holds for a model.
- **A visual edit changes only the bytes of what was edited**, and Undo is exact.
- An idle window does no work: with no model turning, nothing redraws and no thread draws. A keystroke keeps its 8 ms budget and a drag frame its 6 ms on a talk holding ten models.
- The keyboard rule from D3 holds; in the show, a model being turned by the mouse doesn't take the show's keys.
- Nobody is made to see Markdown, a tag name with angle brackets, a settings key, a file path, or a format's name without a plain word beside it.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless test. Drawing on a graphics card isn't compared pixel for pixel across machines: tests check the parsed model, the view's numbers, and that a known model's still is close to a stored one on the software path, and skip with a plain message where no adapter exists.
- Tests never touch the owner's real data, cache or Documents folders, never use the owner's own files, and never run on a real display. Test models are made by the tests.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: Gaussian splats; map tiles; models made of several files; animation inside a model (skins, morphs); lights and materials beyond a model's own colours and one texture lit simply; 3D shapes drawn by a component's code; models in PowerPoint as 3D objects.

Rulings made in the plan:
- **A model is written `<Model src="…" x y w h />`**, with `turn` (`auto` to orbit slowly, absent to stay still), `view` (the starting view) and `views` (more views, one per click), and its `click` and `enter` as for any placed thing.
- **A view is six numbers** written as `yaw pitch distance x y z` (degrees, degrees, a distance in the model's own units, and the point looked at), so a person can read and change it in the text.
- **The still is the model drawn at its starting view**, made off the window's thread. Where no graphics adapter can be had (not even a software one), the still is a plain panel saying *This computer can't draw 3D models here. The slide shows a still when it's presented somewhere that can.* — a still kept in the talk is left for later.
- **In the editor a model shows its still** until it's selected and *Turn* is pressed; then it can be turned and zoomed by dragging and the wheel, and *Set view* writes the view shown into the text as one step.
- **In the show**, a model with `turn="auto"` orbits; a drag on it turns it and the wheel zooms; each click with `views` flies to the next view over one second; leaving the slide puts it back.
- **PowerPoint and PDF get the still**, and the export says once how many models became pictures.
- **A model file is 256 MB at most.** It's a large file, copied in on disk as videos are.
- **Two kinds of file are read:** a binary glTF (`.glb`) with everything inside it, and a point cloud (`.ply`, binary or text, with positions and optional colours).

## Review Focus

- **A file that isn't a usable model.** An empty file; text named `.glb`; a `.glb` cut at any byte; one whose JSON names a buffer outside the file, a data URI, an image by URL, or a required extension the app doesn't know; accessors whose counts, strides or offsets point past their buffers; indices past the vertices; a texture whose header says 60,000 by 60,000; NaN and infinite positions; a `.ply` whose header says a billion vertices in a 1 KB file, or properties of odd types. None crashes, hangs, allocates what the file claims, or reads past the file or outside it. Owned by Task 2's `any_bytes_read_or_say_why` and `nothing_outside_the_file_is_read`.
- **No graphics card, or a poor one.** No adapter at all; only a software adapter; only an integrated card; the device lost while drawing; two windows drawing models at once. The window never waits, a still or a panel shows, and nothing crashes. Owned by Task 3's `with_no_adapter_a_model_is_a_panel_and_says_why` and `a_lost_device_is_made_again`.
- **Turning and leaving.** Turn in the editor, then select something else, change slide, delete the model, Undo, close the window, start the show; in the show, drag, wheel, click through the views, leave the slide, come back. Every drawing thread ends, every picture is released, and nothing draws for a slide that isn't shown. Owned by Task 4's `every_way_of_leaving_stops_the_drawing`.
- **An idle talk full of models.** Ten models on ten slides, none turning: no thread draws, nothing redraws, a keystroke and a drag frame keep their budgets, and opening the talk reads no more of each file than its header. Owned by Task 4's `a_talk_of_ten_models_sits_idle` and Task 6's timing tests.
- **A model and the rest of the talk.** Copy and paste of a slide with a model into another talk (the file comes too); a crash after inserting one, and the next start; save in each form; Save As; a model whose file is gone. Owned by Task 1's `a_model_is_kept_saved_and_brought_back_as_a_video_is` and Task 5's `a_hand_written_model_reads_or_says_why`.

---

### Task 1: Large files that aren't videos

**Files:**
- Modify: `crates/app/src/{added_files,large_files,clipboard,working_copies,recovery}.rs`, `crates/app/src/document/{large,files,clipboard}.rs`, `crates/app/src/workspace_view/inserting.rs`
- Test: `crates/app/tests/large_files.rs`

**Interfaces:**
- Produces: `FileKind::Model` (`.glb`, `.ply`, 256 MB at most), a large kind like `FileKind::Video`. Every app path that was written for video alone takes any large kind: keeping in the working copy (`kept_large` reads every large kind's folder), bringing back after a crash, copying and pasting between talks (`large_for_copy`, `pasted_kind`, `may_bring`), the facts cache, the copy queue and the status bar's words (*Copying model… 40%*).
- Nothing else changes for videos.

- [ ] **Step 1: Write the failing tests**

`large_files.rs`: `a_model_is_kept_saved_and_brought_back_as_a_video_is` (Review Focus: a made 3 MB `.glb` added to an untitled talk; kept in the working copy; saved as a folder and a pack; brought back after a crash; pasted into another talk); `a_model_over_the_limit_is_refused_in_words`; `the_status_bar_says_model_while_a_model_is_copied`; `a_file_named_glb_that_is_text_is_refused_before_copying`. Every existing large-file and video test passes unchanged.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test large_files`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a model file is added, kept and saved as a video file is"
```

---

### Task 2: Reading a model

**Files:**
- Create: `crates/model/{Cargo.toml,src/lib.rs,src/glb.rs,src/ply.rs,src/limits.rs,src/make.rs,tests/read.rs}`
- Modify: `Cargo.toml` (members and the pinned `gltf`)

**Interfaces:**
- Produces, in the new crate `slides-model` (no GPUI, no engine):
  - `pub trait ReadSeek: Read + Seek + Send` (the same shape as the engine's and the video crate's).
  - `pub enum CantDraw { NotAModel, Damaged, NeedsOtherFiles, KindNotDrawn, TooLarge, CouldNotRead, NoAdapter }` with `Display` in plain words.
  - `pub struct ModelFacts { pub kind: ModelKind, pub triangles: u64, pub points: u64, pub bounds: ([f32; 3], [f32; 3]) }`, `pub enum ModelKind { Mesh, Points }`.
  - `pub fn facts(reader: Box<dyn ReadSeek>) -> Result<ModelFacts, CantDraw>`: from the headers only (the glTF JSON chunk, 8 MB at most; the `.ply` header), with every count checked against the limits and the file's length.
  - `pub fn read(reader: Box<dyn ReadSeek>) -> Result<Model, CantDraw>`: the whole model, checked: every accessor's offset, stride and count against its buffer view and the buffer against the file; indices against vertices; positions finite; one base colour texture at most (PNG or JPEG, its size from its header checked before decoding); no outside file, no data URI, no URL; required extensions refused unless known. Under `catch_unwind`.
  - `pub struct Model { pub meshes: Vec<Mesh>, pub points: Option<Points>, pub texture: Option<Texture>, pub facts: ModelFacts }` with plain vertex arrays (positions, normals made when absent, texture coordinates, colours).
  - `pub fn make_test_glb(…) -> Vec<u8>` and `pub fn make_test_ply(…) -> Vec<u8>` behind the feature `make`: a textured grid and a box of several materials; a coloured cloud.

- [ ] **Step 1: Write the failing tests**

`read.rs`: `a_made_model_says_what_it_is`; `a_made_cloud_says_what_it_is`; `any_bytes_read_or_say_why` (Review Focus: property over random bytes and over made files with any byte changed, any range zeroed, or cut at any length: `facts` and `read` return within 2 seconds, never panic, and no allocation exceeds 300 MB, by a counting allocator); `nothing_outside_the_file_is_read` (a buffer naming a file, a data URI, an image URL, `../x.bin`: refused as `NeedsOtherFiles` and no file is opened, by a reader that counts); named cases: a texture header of 60,000 by 60,000; a billion-vertex `.ply` header in 1 KB; indices past the vertices; NaN positions; an unknown required extension; `each_refusal_reads_in_plain_words`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-model --features make`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: the crate's tests, `PROPTEST_CASES=2000` on `any_bytes_read_or_say_why`, and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/model
git commit -m "Model: a model file is read, checked and described"
```

---

### Task 3: Drawing a model

**Files:**
- Create: `crates/model/src/{draw,view,device}.rs`, `crates/model/src/shaders/*.wgsl`, `crates/model/tests/draw.rs`
- Modify: `crates/model/Cargo.toml` (wgpu, pinned to the lock's version)

**Interfaces:**
- Produces, in `slides-model`:
  - `pub struct View { pub yaw: f32, pub pitch: f32, pub distance: f32, pub target: [f32; 3] }` with `View::framing(&ModelFacts) -> View` (the whole model in sight, from the front and a little above), `parse(&str) -> Option<View>` and `Display` (the six numbers of the plan's ruling), and `between(a, b, t) -> View` (a smooth flight).
  - `pub struct Drawer`: one per app, made on a thread of its own the first time it's asked for. `Drawer::start() -> Drawer` returns at once; the device is made on its thread (any adapter, preferring a discrete one, then an integrated one, then a software one). `pub fn ready(&self) -> Option<Result<AdapterKind, CantDraw>>`.
  - `pub struct Scene`: a model uploaded to the drawer. `Drawer::upload(&self, model: Model) -> Scene` (on the drawer's thread).
  - `pub fn draw(&self, scene: &Scene, view: View, width: u32, height: u32) -> Receiver<Result<Picture, CantDraw>>`: draws off-screen with depth and a simple light, reads back, and gives straight-alpha BGRA, opaque where the model is and clear elsewhere, with the read-back copied on several threads when the adapter is integrated or software. Never blocks the caller.
  - A lost device is made again on the next draw.
- The drawing is wgpu on Linux and Windows; nothing here touches the window toolkit's own device.

- [ ] **Step 1: Write the failing tests**

`draw.rs` (each skips with a plain message where `Drawer` reports no adapter; on this machine a software adapter exists): `a_box_drawn_from_the_front_is_where_it_should_be` (the picture's covered area against the box's projected corners, within 2 pixels); `a_view_reads_back_as_written`; `framing_shows_the_whole_model`; `between_starts_and_ends_at_its_views`; `a_textured_grid_shows_its_texture` (sample a few pixels against the texture's colours, within a tolerance); `a_cloud_is_drawn_as_points`; `a_still_is_close_to_the_stored_one_on_the_software_adapter` (a golden picture compared with a tolerance, written with `UPDATE_GOLDEN=1`); `with_no_adapter_a_model_is_a_panel_and_says_why` (Review Focus: a test hook makes adapter creation fail); `a_lost_device_is_made_again` (a hook drops the device); `draw_never_blocks_the_caller` (the call returns within 1 ms while a draw runs); `two_drawers_do_not_fight`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-model --features make --test draw`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: the crate's tests and clippy. Then a release timing run, ignored by default: draw a made million-triangle model at 1600 by 900 on each adapter this machine has, and report the median and 99th-percentile time for drawing and for reading back.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/model
git commit -m "Model: a model is drawn off-screen on a thread of its own"
```

---

### Task 4: A model on a slide: its still, turning, and the show

**Files:**
- Create: `crates/app/src/model_view.rs`, `crates/app/tests/ui_model.rs`
- Modify: `crates/engine/{Cargo.toml,src/builtins.rs,src/elements.rs,src/pictures.rs,src/display.rs,src/deck.rs}`, `crates/engine/tests/builtins.rs`, `crates/app/src/{slide_canvas,present_view,lib}.rs`, `crates/app/src/slide_editor/{render,selecting,keys,mod}.rs`, `crates/app/src/document/large.rs`

**Interfaces:**
- Consumes: Task 1's `FileKind::Model`; Tasks 2-3's `facts`, `read`, `Drawer`, `Scene`, `View`, `draw`.
- Produces, in the engine: a built-in `Model` tag with `src`, `turn`, `view`, `views`, placed as any thing; its natural size 960 by 540; its layer the still (made off the window's thread through the still jobs plan G1 built, by an app-supplied maker, the engine not depending on wgpu), else a panel with its sentence; `LayerSpec` gains `model: Option<ModelOn>` with `src`, `turn`, `view`, `views`, `rect`, `rotate`; `Deck::model_problem`; notices in plain words.
- Produces, in the app:
  - `model_view::Turner`: one for each model being turned or shown live. It reads the file (through the save's reader gate) and uploads it on a background task, asks the drawer for a picture whenever its view changes, and gives the canvas the newest picture; it asks the window for frames only while its view is moving.
  - In the editor: a selected model shows *Turn* (`MODEL_TURN_ID`); while turning, a drag turns it, the wheel zooms, *Set view* (`MODEL_SET_VIEW_ID`) writes the view into the text as one step, and *Done* (`MODEL_DONE_ID`) goes back to the still. It stops turning when the selection changes, the slide changes, the talk's text changes under it, or the window is left.
  - In the show: `turn="auto"` orbits one turn a minute; a drag on the model turns it and the wheel zooms (the show's keys are left alone); each click with `views` flies to the next over one second; leaving the slide puts it back.
  - The presenter's window shows the still.

- [ ] **Step 1: Write the failing tests**

- `builtins.rs`: `a_models_still_is_drawn_at_its_view` (with the software adapter; skip where none); `a_model_that_cannot_be_read_draws_a_panel_and_says_why`; `a_talk_of_ten_models_opens_reading_only_their_headers` (a counting reader).
- `ui_model.rs`: `turn_shows_live_pictures_and_done_goes_back_to_the_still`; `a_drag_turns_it_and_the_wheel_zooms`; `set_view_writes_six_numbers_as_one_step`; `every_way_of_leaving_stops_the_drawing` (Review Focus: thread counts and released pictures, as `ui_video` does); `in_the_show_auto_turns_and_a_drag_turns_it`; `clicks_fly_through_the_views`; `leaving_the_slide_puts_it_back`; `a_talk_of_ten_models_sits_idle` (Review Focus); `a_save_in_place_lets_go_of_a_model_being_read` (the gate); `the_window_never_waits_for_the_drawer` (a hook delays the device by 2 seconds: the window keeps drawing).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_model`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy. No golden image changes.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates
git commit -m "App: a model is drawn on its slide, turned by hand and in the show"
```

---

### Task 5: Putting a model on a slide, and what exports get

**Files:**
- Modify: `crates/app/src/{insert_tools,app_assets,export}.rs`, `crates/app/src/workspace_view/inserting.rs`, `crates/app/src/document/inserting.rs`, `crates/app/src/inspector/{form,labels}.rs`
- Test: `crates/app/tests/ui_model.rs`, `crates/app/tests/export.rs`

**Interfaces:**
- Produces: *Model* on the Insert tab (`INSERT_MODEL_ID`), a file dialog for `.glb` and `.ply`; a dropped or pasted model file does the same, among pictures and videos; `Document::insert_model` writes `<Model src x y w h view="…"/>` with the framing view, as one step, selected. The form: *Turns on its own* (a checkbox for `turn`), the place fields, and one line of facts (*42,000 triangles* or *1.2 million points*); *Views* shows how many views it has, with *Add this view* (while turning) and *Clear views*. Exports draw the still and say once *2 models were exported as pictures.*

- [ ] **Step 1: Write the failing tests**

`ui_model.rs`: `the_insert_tab_has_model`; `a_chosen_model_is_copied_in_and_put_on_the_slide_framed`; `a_dropped_model_is_inserted_among_dropped_pictures`; `the_form_says_how_big_and_sets_turning`; `add_this_view_and_clear_views_each_one_step`; `a_hand_written_model_reads_or_says_why` (Review Focus: no size, `src` missing, outside the talk, not a model, a `view` of garbage or NaN, `views` of 1,000 entries: each is drawn or says why, and selecting, moving and deleting it touch only its own bytes); `nothing_about_a_model_shows_code`. `export.rs`: `a_model_is_a_picture_in_the_pdf_and_in_powerpoint`, and `the_export_says_how_many_models`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_model --test export`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates
git commit -m "App: a model is put on a slide from the Insert tab, a drop or a paste"
```

---

### Task 6: The real window, the record and the guides

**Files:**
- Create: `docs/checks/plan-g2.md`
- Modify: `README.md`, `docs/guide/components.md` where it lists what a slide can hold, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (the G2 line gains its file names), `crates/app/tests/{typing_random,edit_speed}.rs`

**Interfaces:**
- No code interfaces. `edit_speed.rs` gains, ignored and in release: a keystroke and a drag frame on a talk of ten slides each holding a model (the 8 ms and 6 ms budgets); opening that talk; and a model turned for 300 frames with the window thread's share a frame at the median and 99th percentile. `typing_random.rs`'s window run gains inserting a model, turning it, setting a view, and the show.

- [ ] **Step 1: Extend the timing tests and the random session, and run them**

- [ ] **Step 2: Run the app in a real window**, on a scratch display with a scratch home as plan G1's record describes, with made models (a textured million-triangle terrain, a box of materials, a five-million-point cloud): insert each, turn each, set views, present with clicks through the views and a drag, save as a pack, reopen, kill and bring back, copy a slide with a model into another talk, export to PDF and PowerPoint. Measure what the nested display allows (pictures drawn and released a second, processor and memory over five minutes while turning), and say plainly that smoothness is a hand check.

- [ ] **Step 3: Write `docs/checks/plan-g2.md`**, with a hand-check list for a real Linux display (a discrete card, an integrated card, Mesa's software renderer removed) and for Windows (DirectX in the window, wgpu's own device for the model).

- [ ] **Step 4: Update the README and the guides**

- [ ] **Step 5: Verify and commit**

```bash
git add docs/checks/plan-g2.md README.md docs/guide docs/superpowers/plans/2026-11-09-native-rust-roadmap.md crates/app/tests
git commit -m "Docs: what plan G2 was checked with"
```
