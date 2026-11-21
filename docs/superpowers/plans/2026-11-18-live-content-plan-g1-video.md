# Plan G1: Large Files and Video Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A video file added to a talk plays on its slide: in the editor when asked, and in the show when its slide appears. A talk holding hundreds of megabytes of video opens, edits, saves and recovers as quickly and safely as one holding none.

**Architecture:** Two things are built. First, a talk's files can be large: read in pieces from where they lie (a folder, a working copy, a pack) and moved between places as files, never through memory. Second, a new crate, `slides-video`, opens an MP4, says what it is, and decodes H.264 frames on a thread of its own into a short queue; it knows nothing of windows. The engine treats `<Video>` as a placed thing whose layer is a still, so thumbnails, exports and every editing tool work unchanged; the app's slide canvas lays the playing frames over that layer while a video plays.

**Tech Stack:** Rust 1.98.0; new dependencies `openh264 =0.9.10` (Cisco's decoder, built from source, BSD-2), `mp4 =0.14.0` (the container) and `yuvutils-rs =0.8.3` (colour conversion), as the spike used; gpui-kit 0.9.0 with gpui-fast 0.1.13 (`paint_image`, `drop_image`); the `zip` crate already in use; proptest.

**Spec:** `docs/superpowers/specs/2026-11-18-live-content-design.md` (Goals G1; "Large files"; "G1. Video"; the anti-goals *A talk reaching your computer*, *Needing anything else*, *Losing work* and *A talk too big to work with*). `docs/checks/live-content-spikes.md` has what the video spike measured and the conditions it set. The earlier design, `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`, still binds everything it says.

## Global Constraints

- Work on the branch `plan-g1`, cut from `plan-g`.
- No GitHub Actions. Don't push.
- On the desktop, run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- `slides-format`, `toolkit`, `slides-engine`, `slides-pdf`, `slides-pptx`, `slides-components` and the new `slides-video` don't depend on GPUI.
- Interface v1 is frozen: `crates/toolkit/tests/interface_v1.rs` and `crates/components/tests/fixtures/v1/score.wasm` don't change.
- **A large file is never held whole in memory**, by the app or by a test of the app: not when added, drawn, kept in a working copy, saved in either form, copied between talks, or exported. A test adds a 300 MB file and holds the process under 200 MB more than before.
- **Nothing is lost.** Every rule of plan D5 for an untitled talk, a first save, a failed save and a crash holds for a talk with a large file. A save that fails half-way through copying a large file leaves the old talk whole.
- **A video is data.** Reading one can't run a program, follow a link, read outside the talk or use the network. A file that isn't what it says, or is damaged at any byte, shows a still or a plain panel and one plain sentence; it never crashes, hangs or decodes on the window's thread.
- **The spike's conditions** (in `docs/checks/live-content-spikes.md`) bind the decoder: no flush after each frame and a flush at the end; one decoding thread a clip; a queue of 12 frames; a plain file, not a buffered one; every replaced frame released; the colour matrix from the file; only 8-bit 4:2:0 H.264 in an `avc1` MP4 plays, at most 1920 by 1080 and 30 frames a second smoothly.
- **A visual edit changes only the bytes of what was edited**, and Undo is exact, as before.
- An idle window does no work: with no video playing, nothing redraws and no thread decodes. A keystroke keeps its 8 ms budget and a drag frame its 6 ms on a talk holding twenty videos.
- The keyboard rule from D3 holds.
- Nobody is made to see Markdown, a tag name with angle brackets, a settings key, a file path, or a codec's name without a plain word beside it. Every label and message is plain words.
- Every interactive element has an id constant and `test_support()`, and every behaviour has a headless GPUI test.
- Tests never touch the owner's real data, cache or Documents folders, and never use the owner's own video files. Test clips are made by the tests themselves.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations in commits.

Not in this plan: sound; trimming or cropping a video; other kinds of video than H.264 in MP4; the system's own decoders; 3D models (G2), though large files are built so a model can use them.

Rulings made in the plan:
- **A video is written `<Video src="…" x y w h />`**, with `loop` to repeat and `poster` to name a still. Its `click` and `enter` work as for any placed thing.
- **A video file is 4 GB at most.** Other kinds of added file keep 32 MB.
- **In a pack, a video is stored uncompressed**, so it can be read in place. A pack whose video is compressed (made by another program) is unpacked to the app's cache folder to play, 8 GB of cache at most, oldest out first.
- **The still is the video's first frame**, unless `poster` names a picture. It's what shows in the editor until Play is pressed, in thumbnails, and in exports.
- **In the editor a video plays only when its Play button is pressed**, and stops when something else is selected or the slide changes.
- **In the show it starts when its slide appears**, or on its click if it has one, and from the start each time. Without `loop` it holds its last frame.
- **A frame that isn't ready in time is skipped**, never shown late.
- **PowerPoint and PDF get the still**, and the export says once how many videos became pictures. Putting the video itself into PowerPoint is left for later. Cost: a colleague opening the PowerPoint file sees pictures.
- **Copying a slide with a video to another talk carries the video by where it lies**, and copies it when pasted, asking first if more than 256 MB would be copied. If the file is gone by then, the paste says so and leaves a video with nothing to play.
- **A video that can't be played** shows its still if one can be made, or a plain panel, with *This kind of video can't be played here. An MP4 made with the usual settings (H.264) can.*

## Review Focus

- **A file that isn't a playable video.** An empty file; text named `.mp4`; an MP4 cut off at any byte; one whose sizes say 4 GB; one with no video track, two video tracks, a 10-bit or 4:4:4 or 8K or H.265 track; frames of zero size; a sample table pointing outside the file; timestamps that go backwards or overflow; an index at the end of the file. None crashes, hangs, allocates what the file claims, or reads past the file. Owned by Task 2's `any_bytes_open_or_say_why` and `a_damaged_video_never_hangs_the_decoder`.
- **Large files and lost work.** A 300 MB video added to an untitled talk; the app killed while it's being copied in; saved first as a folder and as a pack; Save As to the other form; a save that fails half-way through the copy; the source file deleted or changed while copying; the disk filling; Undo of the insert then a save (not written); two talks holding the same file. Nothing is lost, nothing half-written is left as if whole, and memory stays flat. Owned by Task 1's `a_large_file_never_passes_through_memory` and Task 3's `a_large_file_survives_every_way_of_saving`.
- **Playing and stopping.** Play, then select something else, change slide, delete the video, Undo, close the window, start the show, leave the slide, go back; two videos on a slide; the same file twice; a video under a turned box; presenting on two screens. Every decoding thread ends, every frame is released, and nothing plays on a slide that isn't shown. Owned by Task 4's `every_way_of_leaving_stops_the_decoding` and `frames_are_released`.
- **An idle talk full of video.** Twenty videos on twenty slides, none playing: no thread decodes, nothing redraws, a keystroke and a drag frame keep their budgets, and opening the talk reads no more of each file than its still needs. Owned by Task 4's `a_talk_of_twenty_videos_sits_idle` and Task 6's timing tests.
- **Old talks and old packs.** A pack made before this plan; a pack from another program with a compressed video; a talk naming a video that isn't there; `<Video>` written by hand with no size, with `src` pointing outside the talk, with a `poster` that isn't a picture. Each opens, and says what it can't show in plain words. Owned by Task 3's `packs_old_and_odd_still_open` and Task 5's `a_hand_written_video_reads_or_says_why`.

---

### Task 1: A talk's files, read in pieces and moved as files

**Files:**
- Modify: `crates/engine/src/{files,packed,lib}.rs`
- Test: `crates/engine/tests/{files,packed}.rs`

**Interfaces:**
- Produces, in `slides_engine`:
  - `pub trait ReadSeek: Read + Seek + Send {}` (blanket-implemented).
  - On `TalkFiles`: `fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>>` with a default that reads the whole file into a cursor, and `fn lies_at(&self, path: &str) -> Option<PathBuf>` (default `None`): the file on disk a talk's file can be read from directly, when there is one.
  - `FolderFiles`: `open` is the file itself and `lies_at` its path, with the same refusal of names that leave the folder as `read`.
  - `PackedFiles`: `open` on a stored entry reads the entry in place, by its offset and length in the pack, without unpacking; on a compressed entry it is `None`.
  - `pub fn is_large_kind(path: &str) -> bool`: true for `.mp4`, `.m4v`, `.mov`, `.glb` and `.ply` under `assets/`, whatever the letter case.
  - `pub const MAX_LARGE_FILE: u64 = 4 * 1024 * 1024 * 1024;`
  - `pub enum FileSource { Bytes(Vec<u8>), At(PathBuf) }` and `pub type NewFileFrom = (String, FileSource)`.
  - `write_new_folder`, `pack_new`, `copy_talk_folder`, `pack_folder_with_slides`, `repack_with_slides` and `unpack_with_slides` each gain a form taking `&[NewFileFrom]` (the old forms stay and call the new ones), and each **streams** a file given as `At`, and any large-kind file it carries from the source talk, in pieces of 1 MB at most. In a pack a large-kind file is written stored, with the zip's large-file form when it's 4 GB or near it. Each takes `progress: &dyn Fn(u64, u64)` (bytes done, bytes in all) and `stop: &AtomicBool`, checked between pieces; stopped, it removes what it wrote and returns `PackError::Stopped`.
  - The size checks in `packed.rs` allow a large-kind file up to `MAX_LARGE_FILE` and keep every other limit. Reading a pack never trusts a size an entry claims beyond what the pack file's own length allows.

- [ ] **Step 1: Write the failing tests**

- `files.rs`: `a_folders_file_opens_and_seeks`; `open_refuses_what_read_refuses` (`../x`, a symlink out of the folder, a folder, a FIFO); `the_default_open_reads_the_whole_file`; `large_kinds_are_told_by_their_ending`.
- `packed.rs`:
  - `a_stored_entry_is_read_in_place` (bytes equal, at any offset and length, by property), and `a_compressed_entry_gives_none`.
  - `a_video_is_stored_and_a_picture_compressed_as_before`.
  - `a_large_file_never_passes_through_memory` (Review Focus): a 300 MB file of made-up bytes, given as `At`, into `write_new_folder`, `pack_new`, then `repack_with_slides`, `unpack_with_slides`, `copy_talk_folder` and `pack_folder_with_slides` from those; the process's peak resident memory, read from `/proc/self/status` before and after, grows by under 64 MB in each; every result reads back the same bytes (compared in pieces). Ignored by default as it writes 2 GB; run in Step 4.
  - `a_stopped_copy_leaves_nothing`, for each of the six; `progress_reaches_the_whole`.
  - `a_source_that_shrinks_or_goes_while_copied_fails_whole` (the old target is as it was).
  - `a_file_over_four_gigabytes_is_refused_before_anything_is_written` (a sparse file).
  - `an_entry_that_claims_more_than_the_pack_holds_is_refused`.
  - `packs_made_before_still_open` (a pack written by the old `pack_new`, read by the new code, and the reverse).

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-engine --test files --test packed`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-engine`, then `scripts/cargo-safe.sh test --release -p slides-engine --test packed -- --ignored a_large_file`
Expected: PASS. Report the memory growth measured for each of the six.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: a talk's file can be read in pieces and moved as a file"
```

---

### Task 2: Opening and decoding a video

**Files:**
- Create: `crates/video/{Cargo.toml,src/lib.rs,src/open.rs,src/decode.rs,src/make.rs,tests/open.rs,tests/decode.rs}`
- Modify: `Cargo.toml` (the workspace's members and the three pinned dependencies)

**Interfaces:**
- Produces, in the new crate `slides-video` (no GPUI, no engine):
  - `pub struct VideoInfo { pub width: u32, pub height: u32, pub seconds: f64, pub frames: u64, pub frames_a_second: f32 }`
  - `pub enum CantPlay { NotAVideo, Damaged, NoPicture, KindNotPlayed, TooLarge, CouldNotRead }`, with `Display` in plain words, none naming a codec without *(H.264)* style help: `KindNotPlayed` says *This kind of video can't be played here. An MP4 made with the usual settings (H.264) can.*
  - `pub fn open(reader: Box<dyn ReadSeek>) -> Result<Video, CantPlay>` where `ReadSeek` is `Read + Seek + Send` (its own trait; the engine's has the same shape). It reads the file's index only, checks every size and offset against the file's length before using it, and refuses more than 4096 by 2304, more than 120 frames a second, and more than 6 hours.
  - `impl Video { pub fn info(&self) -> VideoInfo; pub fn first_frame(&mut self) -> Result<Frame, CantPlay>; pub fn play(self, from: f64, looped: bool) -> Playing }`
  - `pub struct Frame { pub width: u32, pub height: u32, pub at: f64, pub bgra: Vec<u8> }`: straight-alpha BGRA, opaque, in the file's own colours (its matrix and range, BT.601 if it says none).
  - `pub struct Playing`: owns one decoding thread and a queue of 12 frames at most. `pub fn frame_for(&mut self, seconds: f64) -> Option<Frame>`: the newest frame due at that time, skipping any older; `None` if none newer than the last given is due. `pub fn ended(&self) -> bool`; `pub fn failed(&self) -> Option<CantPlay>`. Dropping it ends the thread within 100 ms and frees its frames.
  - `pub fn make_test_video(width: u32, height: u32, frames: u32, frames_a_second: u32, picture: impl Fn(u32) -> Vec<u8>) -> Vec<u8>` behind the crate feature `make`: an H.264 MP4 made with the same library's encoder, for tests here and in the other crates. Each frame's picture is solid or simple enough that its colours survive the encoding within 8 levels.
- The decoder is set as the spike found it must be (see Global Constraints). A panic inside decoding is caught and becomes `Damaged`.

- [ ] **Step 1: Write the failing tests**

- `open.rs`: `a_made_video_says_what_it_is` (size, frames, rate, length to 1 ms); `any_bytes_open_or_say_why` (Review Focus: property over random bytes, and over a made video with any one byte changed, any range zeroed, or cut at any length: `open`, `first_frame` and a full `play` each return within 2 seconds and never panic, and no allocation exceeds 64 MB, checked by a counting allocator in the test); named cases: an empty file, text, no video track, two video tracks, sizes claiming 4 GB in a 1 KB file, a sample table pointing past the end, timestamps going backwards, an index at the end of the file, 8K, 10,000 frames a second; `each_refusal_reads_in_plain_words`.
- `decode.rs`: `every_frame_comes_out_in_order` (a made video of 90 frames, each a different solid colour: 90 frames, the right colours within 8 levels, rising times); `frames_with_reordering_come_out_in_order` (a made video with B-frames if the encoder can make them; if it can't, say so in the report and keep the test for the property that times only rise); `frame_for_skips_what_is_late`; `looped_goes_round`, and `not_looped_ends_and_holds_the_last`; `play_from_the_middle_starts_at_the_frame_before`; `dropping_it_ends_the_thread_quickly` (the thread count returns to what it was within 200 ms); `the_queue_never_holds_more_than_twelve`; `a_damaged_video_never_hangs_the_decoder` (Review Focus); `colours_follow_the_files_matrix`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p slides-video --features make`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test -p slides-video --features make`, `PROPTEST_CASES=2000` on `any_bytes_open_or_say_why`, and `scripts/cargo-safe.sh clippy --workspace --all-targets`. Then a release timing run, ignored by default: decode a made 1920 by 1080 video of 300 frames and report the median and 99th-percentile decode time a frame.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/video
git commit -m "Video: an MP4 is opened, described and decoded on a thread of its own"
```

---

### Task 3: Large files in the app: adding, keeping, saving

**Files:**
- Modify: `crates/app/src/{added_files,working_copies,recovery,saving,clipboard}.rs`, `crates/app/src/document/{files,saving,clipboard,open}.rs`, `crates/app/src/workspace_view/{saving,inserting}.rs`
- Test: `crates/app/tests/large_files.rs` (new), `crates/app/tests/{added_files,recovery,untitled,clipboard}.rs`

**Interfaces:**
- Consumes: Task 1's `open`, `lies_at`, `FileSource`, the streaming writers, `is_large_kind`, `MAX_LARGE_FILE`.
- Produces:
  - `FileKind::Video` (`.mp4`, `.m4v`, `.mov`), 4 GB at most, named and made safe as a picture is.
  - `Document::add_large_file(&mut self, kind: FileKind, suggested: &str, at: &Path, progress, stop) -> Result<String, NotAdded>`, run off the window's thread by the window: the file is copied in pieces into the window's working copy (its slot's `files/` folder, made and held as plan D5 has it), hashed as it goes, and only then becomes one of the talk's files, lying at that path. The same bytes already in the talk are found by size and hash and not copied twice. New refusals, in plain words: too large; not enough room on the disk (checked before copying, and said if it runs out); the file changed or went while it was copied.
  - A talk's file that lies on disk is held by its path, never its bytes: in the added set, in a working copy (`about.json` and the kept files as before; a large file is never rewritten when it's unchanged), in a save (`FileSource::At`), and when brought back after a crash.
  - Saving, Save As in both directions, a first save and a failed save follow plan D5's order exactly: the write, then the talk marked saved, then the working copy cleared. A large file the working copy holds is moved or copied to the saved place by the writer; the working copy's own copy goes only when its slot is cleared.
  - Copy and paste: a copied slide or thing carries a large file as where it lies (the talk's place and the file's path), not its bytes. A paste copies it in as `add_large_file` does; if more than 256 MB would be copied it asks first (*Copy 1 video, 392 MB, into this presentation?*), and if the file is gone it says so (`Brought` gains a count of large files left out).
  - While a large file is being copied in, saved or pasted, the status line shows a bar and words (*Copying video… 40%*) with a *Cancel* that stops it and leaves nothing.

- [ ] **Step 1: Write the failing tests**

`large_files.rs` (each large test makes its own 300 MB file of patterned bytes in a temp folder, and is ignored by default; smaller twins of 3 MB with the large-file threshold lowered by a test hook run always):
- `a_large_file_is_added_by_copying_not_by_reading` (resident memory grows under 64 MB; the working copy holds it; the talk reads it back in pieces).
- `a_large_file_survives_every_way_of_saving` (Review Focus): untitled, first save as a folder and as a pack; Save As each way; saved in place twice (not copied again the second time); each result reads back the same bytes and reopens.
- `a_save_that_fails_half_way_leaves_the_old_talk_whole` (the target's disk is a small tmpfs or the writer is stopped by its flag at 50%); `a_first_save_that_fails_leaves_it_untitled_with_its_copy`.
- `a_crash_while_a_large_file_is_copied_in_leaves_no_half_file` (the copy is stopped at 50% and everything dropped: the next start lists the talk without the file, and no file of the wrong length is offered).
- `a_crash_after_it_is_added_brings_it_back`.
- `undo_of_the_insert_then_a_save_does_not_write_it`, and `redo_then_a_save_does`.
- `the_same_file_added_twice_is_one_file`; `two_talks_can_hold_the_same_file`.
- `a_source_deleted_or_changed_while_copying_adds_nothing_and_says_why`; `a_full_disk_says_so_and_adds_nothing`.
- `a_slide_with_a_large_file_pastes_into_another_talk` (asked first over 256 MB; declined adds the slide without the file and says so; the source talk closed before the paste says the file is gone).
- `cancel_stops_a_copy_and_leaves_nothing`.
- `packs_old_and_odd_still_open` (Review Focus): a pack made before this plan; a pack whose video is compressed (it is unpacked to the cache folder given by a test hook, never the real one, and read from there; the cache keeps 8 GB at most, oldest out first, by a test with a small limit).
- In `recovery.rs`, `untitled.rs`, `added_files.rs`, `clipboard.rs`: every existing test passes unchanged.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test large_files`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace`, clippy, and `scripts/cargo-safe.sh test --release -p stagepack-studio --test large_files -- --ignored`
Expected: PASS. Report the memory growth measured.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "App: a large file is added, kept and saved as a file, never through memory"
```

---

### Task 4: A video on a slide: its still, and playing

**Files:**
- Create: `crates/app/src/video_player.rs`, `crates/app/tests/ui_video.rs`
- Modify: `crates/engine/Cargo.toml`, `crates/engine/src/{builtins,elements,display,deck,pictures,lib}.rs`, `crates/engine/tests/builtins.rs`, `crates/app/Cargo.toml`, `crates/app/src/{slide_canvas,present_view,lib}.rs`, `crates/app/src/slide_editor/{render,marks,selecting}.rs`

**Interfaces:**
- Consumes: Task 1's `TalkFiles::open`; Task 2's `open`, `Video`, `Playing`, `Frame`, `CantPlay`, `make_test_video`.
- Produces, in the engine:
  - A built-in `Video` tag with `src`, `poster` and `loop`, placed as any thing. Its natural size is the video's own, fitted within 960 by 540 when no size is written.
  - Its layer draws its still: the `poster` picture if one is named and reads, else the video's first frame (decoded once, cached by the file's stamp, off the caller's thread where the deck's picture cache already works so), else a plain panel in the theme's colours with a play mark.
  - `LayerSpec` gains `pub video: Option<VideoOn>` with `pub struct VideoOn { pub src: String, pub looped: bool, pub rect: Rect, pub rotate: f32 }`.
  - `Deck::video_problem(slide, source) -> Option<String>`: the plain sentence when a video can't be played, also among the slide's notices.
  - `ElementBox::keeps_shape` is true for a video.
- Produces, in the app:
  - `video_player::Player`: one for each video being played. It opens the file through the talk's files on a background thread, plays it from the start, and gives the canvas the frame due each time the window draws. It asks the window for another frame only while it plays.
  - `SlideCanvas` draws a playing video's frame over its layer, in the layer's rectangle, releasing each replaced frame with `drop_image`. A video under a turned box plays turned.
  - In the editor: a selected video shows a *Play* button at its middle (`VIDEO_PLAY_ID`), and *Stop* (`VIDEO_STOP_ID`) while it plays; Space with a video selected does the same. It stops when the selection changes, the slide changes, the talk's text changes under it, or the window is left.
  - In the show: a video starts when its slide appears, or when its click is reached, and again from the start whenever the slide is shown again; `loop` repeats; without it the last frame holds. The presenter's window shows the still, not a second playing copy.
  - A video that can't be played shows its still or panel, and its sentence on the status line when selected.

- [ ] **Step 1: Write the failing tests**

- `crates/engine/tests/builtins.rs`: `a_videos_still_is_its_first_frame` (a made video whose first frame is one colour: the layer is that colour); `a_poster_is_used_when_named`; `a_video_with_no_size_takes_its_own_shape`; `a_video_that_cannot_be_read_draws_a_panel_and_says_why`; `a_talk_of_twenty_videos_opens_reading_only_their_starts` (a counting `TalkFiles`: laying out twenty slides reads under 1 MB of each 5 MB file).
- `ui_video.rs` (each uses videos made by `make_test_video`, with frames of known colours):
  - `play_shows_the_videos_frames_in_turn` (the test clock is advanced a frame at a time; the canvas's video frame is each colour in turn); `stop_goes_back_to_the_still`; `space_plays_and_stops_a_selected_video`.
  - `every_way_of_leaving_stops_the_decoding` (Review Focus): selecting something else, another slide, deleting the video, Undo of its insert, a change to the talk from outside, closing the window, starting the show: the process's thread count is back to what it was within 300 ms and no player is left.
  - `frames_are_released` (Review Focus: after 200 frames the window's image count is what it was, by a counter on the canvas's releases and makes).
  - `in_the_show_it_starts_with_its_slide_and_again_when_shown_again`; `it_waits_for_its_click`; `loop_goes_round_and_without_it_the_last_frame_holds`; `the_presenters_window_shows_the_still`.
  - `two_videos_on_a_slide_both_play`, and `the_same_file_twice_plays_twice`.
  - `a_video_in_a_turned_box_plays_turned`.
  - `a_late_frame_is_skipped` (the clock jumps 10 frames: the frame shown is the one due, and the skipped ones were released).
  - `a_talk_of_twenty_videos_sits_idle` (Review Focus: no redraw and no extra thread over twenty seconds, with none playing; and none after one has played and stopped).
  - `a_video_that_cannot_play_says_why_when_selected`.
  - `a_video_moves_resizes_and_keeps_its_shape_like_a_picture`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_video`
Expected: FAIL to compile.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy. No golden image changes.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/engine crates/app
git commit -m "App: a video shows its still on a slide and plays when asked"
```

---

### Task 5: Putting a video on a slide, and what exports get

**Files:**
- Modify: `crates/app/src/{insert_tools,app_assets,lib}.rs`, `crates/app/src/workspace_view/inserting.rs`, `crates/app/src/document/inserting.rs`, `crates/app/src/inspector/{form,labels}.rs`, `crates/app/src/export.rs`, `crates/pptx/src/write.rs`, `crates/pdf/src/lib.rs` only where a still needs it
- Test: `crates/app/tests/ui_video.rs`, `crates/app/tests/export.rs`

**Interfaces:**
- Consumes: Task 3's `add_large_file`; Task 4's `Video` tag.
- Produces:
  - `Document::insert_video(&mut self, name: String, size: (u32, u32)) -> Result<Vec<usize>, EditRefused>`: writes `<Video src="…" x y w h />` in the middle of the slide at the video's own shape, fitted within 960 by 540, as one step (*Insert video*), selected.
  - The Insert tab gains *Video* (`INSERT_VIDEO_ID`), opening a file dialog for `.mp4`, `.m4v` and `.mov`. A video file dropped on the slide, or its path pasted, does the same, among pictures in one drop. The file is copied in with the status line's bar, then inserted; cancelled or refused, nothing is inserted.
  - The form for a video: *Loop* (a checkbox), *Still picture* (a picture from the talk, or *First frame*), its place, and one line saying its length and size (*14 seconds, 1120 by 630*). A video the app can't play shows its sentence there too.
  - Exports draw the still. An export of a talk with videos says once, in the status line, *3 videos were exported as pictures.*

- [ ] **Step 1: Write the failing tests**

- `ui_video.rs`: `the_insert_tab_has_video`; `a_chosen_video_is_copied_in_and_put_on_the_slide_selected` (one undo step; Undo takes it away; the file isn't written by a save after that); `a_dropped_video_is_inserted_among_dropped_pictures`; `a_cancelled_copy_inserts_nothing`; `a_file_that_is_not_a_video_says_so_and_inserts_nothing`; `the_form_says_how_long_and_how_big_and_sets_loop_and_the_still`; `a_hand_written_video_reads_or_says_why` (Review Focus: no size; `src` naming a file that isn't there, one outside the talk, one that isn't a video; a `poster` that isn't a picture; `loop="no"`): each is drawn or says why in words, and selecting, moving and deleting it touch only its own bytes; `nothing_about_a_video_shows_code` (no visible text holds `<`, `assets/`, `.mp4` outside a file's own name in the form, or a codec's name without its plain words).
- `export.rs`: `a_video_is_a_picture_in_the_pdf_and_in_powerpoint`, and `the_export_says_how_many`.

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/cargo-safe.sh test -p stagepack-studio --test ui_video --test export`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement it**

- [ ] **Step 4: Verify**

Run: `scripts/cargo-safe.sh test --workspace` and clippy.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates
git commit -m "App: a video is put on a slide from the Insert tab, a drop or a paste"
```

---

### Task 6: The real window, the record and the guides

**Files:**
- Create: `docs/checks/plan-g1.md`
- Modify: `README.md`, `docs/guide/components.md` where it lists what a slide can hold, `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (the G1 line gains its file names), `crates/app/tests/{typing_random,edit_speed}.rs`, `crates/app/templates/tour/slides.md` only if a line about video belongs there

**Interfaces:**
- No code interfaces. `edit_speed.rs` gains, ignored and in release: a keystroke and a drag frame on a talk of twenty slides each holding a video (the 8 ms and 6 ms budgets), opening that talk (under half a second more than the same talk without videos), and a 1920 by 1080 video played for 300 frames with the frame-to-canvas cost a frame at the median and the 99th percentile. `typing_random.rs`'s window run gains inserting a video, playing and stopping it, and the show.

- [ ] **Step 1: Extend the timing tests and the random session, and run them**

Run: `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`, and `PROPTEST_CASES=500 scripts/cargo-safe.sh test -p stagepack-studio --test typing_random`
Expected: PASS. A failure is a bug to fix in the code it points at, with a named test.

- [ ] **Step 2: Run the app in a real window**

On a scratch display with a scratch home, as plan D5's record describes, with screenshots to `target/check/g1-run/`: make a presentation; insert a short made video and a 300 MB made one (watch the bar; watch memory); play each in the editor; move, resize and turn one; set Loop; present, and step through a slide with two videos and one waiting for its click; save as a pack, close, open, play; kill the app after inserting and bring the talk back; copy a slide with a video into a second talk; export to PDF and PowerPoint and open both. Look at every screenshot. The nested display can't show how smooth playback is (see the spikes' record): say so, and measure what can be measured (frames made, skipped and released a second, processor use, memory over five minutes).

- [ ] **Step 3: Write `docs/checks/plan-g1.md`**

What ran and what was seen, the timings, what couldn't be checked, and a hand-check list for a real Linux display and for Windows: how smooth a 1080p clip plays, the owner's own talk's clips, the real file dialog, a video on a second screen in the show, building the decoder on Windows. Known issues, with the H.264 licence question from the design repeated for the owner.

- [ ] **Step 4: Update the README and the guides**

- [ ] **Step 5: Verify and commit**

Run: `scripts/cargo-safe.sh test --workspace`, clippy.
Expected: PASS.

```bash
git add docs/checks/plan-g1.md README.md docs/guide docs/superpowers/plans/2026-11-09-native-rust-roadmap.md crates/app/tests crates/app/templates
git commit -m "Docs: what plan G1 was checked with"
```
