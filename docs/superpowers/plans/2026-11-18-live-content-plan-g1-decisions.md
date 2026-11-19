# Plan G1: decisions made while building

The calls made during plan G1 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-g1.md` has what ran, and `docs/checks/live-content-spikes.md` what the two spikes before it found.

## Questions for the owner

- **The H.264 licence.** Video plays through Cisco's OpenH264, built from the source. H.264 is patented, and Cisco pays the licence only for its own prebuilt library. Before a public release: download Cisco's library at first use (the network, once), play only free formats (no H.264, which is most video), or take legal advice.
- **A portrait phone video plays sideways.** The turn a phone writes in the file isn't read yet. It is the first thing to fix in video.
- **A video is silent.** Sound wasn't in this plan.
- **PowerPoint and PDF get a video's still**, with one sentence saying how many videos became pictures. Putting the video itself into PowerPoint wasn't in this plan.
- **Playback smoothness hasn't been seen.** The nested display used for every run shows about eight pictures a second, so how smooth a 1080p clip plays is a hand check on a real screen.

## For the start of plan G2 (3D models)

- **Only `FileKind::Video` exists in the app.** `.glb` and `.ply` are already large kinds in the engine, but several app paths are written for video alone (`kept_large` reads only the video folder; `large_for_copy`, `pasted_kind` and `may_bring` assume a video). Generalise them first, or a model brought back after a crash or pasted between talks is silently left out.
- **Reuse the save's reader gate** (`Readers` in `crates/app/src/large_files.rs`) for every reader of a model's file, and keep the window's thread to metadata only, with opens that can't block.
- **Parse a model's file the way `crates/video/src/open.rs` does:** every size and offset checked against its parent and the file's real length before anything is allocated or read.

## Rulings

- **Setup.** The design and this plan weren't held for the owner's review. The owner agreed the shape (replay built in, live as an add-on, five pieces in order) and said to go ahead. Cost if wrong: rework of what the owner would change.
- **Setup.** No fresh reader checked the design; two spikes checked its risky parts instead.
- **Task 1.** The streaming writers are named `…_from` beside the old ones. Every file copied from disk fails if its length or modified time changes while it's copied. A pack is refused whole when its entries overlap or run past the file.
- **Task 2.** The decoder's assembly build is used where `nasm` is installed, and its C build where it isn't, so builds can differ between machines. A file cut short is called damaged rather than played in part. Interlaced video isn't played. A last picture with no time of its own is given one frame; a time further past the end is refused.
- **Task 3.** A paste of a large file checks its length and modified time, not a hash. Every copy running at once is kept, with the newest one's Cancel shown. *Don't save* while a save runs waits for the save.
- **Task 4.** A video's still is made off the window's thread from the moment a talk is opened. A video's size comes from its file's headers, 64 KB at most. Turned frames are made off the window's thread.
- **Task 5.** A video goes on the slide that was shown when it was asked for, and the window shows that slide when it lands. Copies run one at a time, in drop order. A file without a modified time isn't hashed to tell versions apart.
- **Final.** One fix wave took both Important findings and the seven small ones.

## Smaller findings and notes

- **Fixed by the reviews, before the end:** a working-copy clear during a copy could delete the video being copied, or its whole folder; a video brought back after a crash could point at a file then removed; *Don't save* during a save could delete the video the save was reading; closing could clear the working copy before a save was taken in; a damaged video looped for ever, mistaken for a save; opening a talk decoded stills on the window's thread; a save could replace a file a video was still reading; crafted video files could make decoding grind for hours or freeze a loop; a crafted size or hidden start code could get past the decoder's checks.
- **Fixed by the final review:** a video whose last picture has no duration was refused; Save waited on the window's thread for a read in progress; a closing window could still start a save or a copy; Cancel in the close dialog didn't cancel a close waiting on a save; a pack saved in place stayed open, holding its old disk space.
- **Not fixed:** a pack with one damaged entry no longer opens at all; sample data shared between pictures is refused; a video in a pack whose videos are compressed shows a panel until played; a size from the file's track header isn't checked against the stream's, so a few files jump when their still arrives; *Click to add text* shows over a video placed mid-slide; the button still says *Stop* after a video ends; a width or height typed in the form stretches the video; the Turn field is empty when a video isn't turned; the status bar nudges the layout while a copy or save runs.
- **Disk:** the build cache passed 190 GB twice during the plan. Stale test programs and the incremental cache were deleted from `target/debug`.
