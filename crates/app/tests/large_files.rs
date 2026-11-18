//! Large files, such as videos, in the app: added by copying them on disk into the window's
//! working copy, held there by their path, saved in every form and copied between talks, and
//! never read whole into memory. Every rule of an untitled talk, a first save, a failed save and
//! a crash holds for a talk with one.
//!
//! Each test makes its own files of patterned bytes in a temp folder it removes. Those of 300 MB
//! are ignored by default, and run alone, in release:
//! `scripts/cargo-safe.sh test --release -p stagepack-studio --test large_files -- --ignored`
//! Their smaller twins, of 3 MB, run always, with the size over which a paste asks first lowered.

#[path = "support/typing.rs"]
mod typing;

use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext as _, ClipboardItem, TestAppContext};
use slides_engine::{Fonts, ReadSeek, TalkFiles as _, open_packed};
use stagepack_studio::added_files::{FileKind, NotAdded};
use stagepack_studio::clipboard::{self, AppData};
use stagepack_studio::document::{Document, TalkPlace, UntitledId};
use stagepack_studio::large_files::{
    self, CACHE_LIMIT, VideoCache, ask_to_copy, progress_words, set_ask_before_copying_for_test, set_free_space_for_test,
};
use stagepack_studio::recovery::Recovery;
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::slide_strip::strip_row_id;
use stagepack_studio::templates::template;
use stagepack_studio::workspace_view::{CANCEL_PROGRESS_ID, COPY_LARGE_ID, DONT_COPY_LARGE_ID, PROGRESS_ID};

/// The size of the smaller twins' files.
const TWIN: u64 = 3 * 1024 * 1024;
/// The size of the large tests' files.
const LARGE: u64 = 300 * 1024 * 1024;
/// What a paste copies before it asks, in these tests: less than a twin.
const ASK_OVER: u64 = 1024 * 1024;
/// Long enough for a working copy to be kept after the last edit.
const KEEP_PAUSE: u64 = 5_000;

fn set_up() {
    set_ask_before_copying_for_test(ASK_OVER);
}

/// The byte at `at` of a patterned file made with `seed`.
fn pattern(seed: u8, at: u64) -> u8 {
    (at.wrapping_mul(31) ^ (at >> 12) ^ u64::from(seed).wrapping_mul(97)) as u8
}

/// A file of `len` patterned bytes at `dir/name`, written a piece at a time.
fn patterned(dir: &Path, name: &str, len: u64, seed: u8) -> PathBuf {
    let path = dir.join(name);
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let mut piece = vec![0u8; 1024 * 1024];
    let mut at = 0u64;
    while at < len {
        let n = (len - at).min(piece.len() as u64) as usize;
        for (i, byte) in piece[..n].iter_mut().enumerate() {
            *byte = pattern(seed, at + i as u64);
        }
        file.write_all(&piece[..n]).unwrap();
        at += n as u64;
    }
    file.flush().unwrap();
    path
}

/// Whether `a` and `b` hold the same bytes, compared a piece at a time.
fn same_pieces(mut a: impl Read, mut b: impl Read) -> bool {
    let (mut x, mut y) = (vec![0u8; 1024 * 1024], vec![0u8; 1024 * 1024]);
    loop {
        let n = read_full(&mut a, &mut x);
        let m = read_full(&mut b, &mut y);
        if n != m || x[..n] != y[..m] {
            return false;
        }
        if n == 0 {
            return true;
        }
    }
}

fn read_full(from: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut got = 0;
    while got < buf.len() {
        match from.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(error) => panic!("read failed: {error}"),
        }
    }
    got
}

fn untitled() -> Document {
    Document::new_untitled(template("light").unwrap(), UntitledId::random(), Fonts::bundled_only()).unwrap()
}

/// Gives `document` a working copy of its own in `root`, as a window does, and gives it back.
fn keep_in(document: &mut Document, root: &Path) -> Recovery {
    let recovery = Recovery::new(root.to_path_buf()).for_another_window();
    document.keep_large_files_in(recovery.clone());
    recovery
}

fn no_progress(_: u64, _: u64) {}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// Adds the video at `from` to `document`, as the window does on its background task.
fn add(document: &mut Document, from: &Path) -> Result<String, NotAdded> {
    let suggested = from.file_name().unwrap().to_string_lossy().into_owned();
    document.add_large_file(FileKind::Video, &suggested, from, &no_progress, &never())
}

/// Puts a video showing `path` on the current slide.
fn show(document: &mut Document, path: &str) {
    document.insert_tag("Insert video", "Video", &[("src", path)], None).unwrap();
}

/// Whether the talk reads `path` back, in pieces, as the bytes of the file at `from`.
fn reads_back(document: &Document, path: &str, from: &Path) -> bool {
    let reader = document.open_large(path).expect("it can be opened").open().expect("it opens");
    same_pieces(reader, std::fs::File::open(from).unwrap())
}

/// The file at `path` in the talk saved at `at`, in either form, to be read in pieces.
fn saved_file(at: &Path, path: &str) -> Option<Box<dyn ReadSeek>> {
    if at.extension().is_some_and(|ext| ext == "sptx") {
        open_packed(at).ok()?.open(path)
    } else {
        Some(Box::new(std::fs::File::open(at.join(path)).ok()?))
    }
}

fn reopen(at: &Path) -> Document {
    if at.extension().is_some_and(|ext| ext == "sptx") {
        Document::open_packed(at, Fonts::bundled_only()).unwrap()
    } else {
        Document::open(at, Fonts::bundled_only()).unwrap()
    }
}

fn save_as(document: &mut Document, target: SaveTarget) {
    let saved = document.save_as_job(target).run().unwrap();
    document.saved(saved);
}

fn save(document: &mut Document) {
    let saved = document.save_job().run().unwrap();
    document.saved(saved);
}

/// Where the talk's file at `path` lies, if anywhere.
fn lies_at(document: &Document, path: &str) -> Option<PathBuf> {
    document.export_input().files.lies_at(path)
}

/// Every file under `dir`, by its path from there.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The videos under `dir`, whole or not.
fn videos_under(dir: &Path) -> Vec<String> {
    files_under(dir).into_iter().filter(|path| path.contains(".mp4")).collect()
}

/// A peak and the resident size now, in bytes.
#[cfg(target_os = "linux")]
fn memory() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let field = |name: &str| {
        let line = status.lines().find(|line| line.starts_with(name)).unwrap();
        line.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 1024
    };
    (field("VmHWM:"), field("VmRSS:"))
}

/// How much more the process held at its peak while `step` ran than before it, in bytes.
#[cfg(target_os = "linux")]
fn growth_of(step: impl FnOnce()) -> u64 {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    let (_, before) = memory();
    step();
    let (peak, _) = memory();
    peak.saturating_sub(before)
}

/// Free space under `dir`, as `df` tells it.
fn free_disk(dir: &Path) -> Option<u64> {
    let out = std::process::Command::new("df").args(["-k", "--output=avail"]).arg(dir).output().ok()?;
    String::from_utf8(out.stdout).ok()?.lines().nth(1)?.trim().parse::<u64>().ok().map(|k| k * 1024)
}

fn enough_disk(dir: &Path, bytes: u64) {
    if let Some(free) = free_disk(dir) {
        assert!(free > bytes, "only {free} bytes free under {}", dir.display());
    }
}

// Adding.

#[cfg(target_os = "linux")]
fn added_by_copying(len: u64) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    enough_disk(work.path(), len * 4);
    let from = patterned(work.path(), "Holiday Clip.MP4", len, 1);
    let root = work.path().join("recovery");
    let mut document = untitled();
    keep_in(&mut document, &root);
    let mut path = None;
    let grew = growth_of(|| path = Some(add(&mut document, &from).unwrap()));
    let path = path.unwrap();
    eprintln!("adding {len} bytes: peak resident grew by {:.1} MB", grew as f64 / 1e6);
    // Measured for the whole process, so only where it runs alone.
    if len == LARGE {
        assert!(grew < 64 * 1024 * 1024, "grew by {grew} bytes");
    }
    assert_eq!(path, "assets/holiday-clip.mp4", "named as a picture is");
    let lies = lies_at(&document, &path).expect("it lies on disk");
    assert!(lies.starts_with(&root), "in the window's working copy: {}", lies.display());
    assert!(lies.ends_with("files/assets/holiday-clip.mp4"));
    assert_eq!(std::fs::metadata(&lies).unwrap().len(), len);
    assert!(document.export_input().files.read(&path).is_none(), "never read whole");
    assert_eq!(document.export_input().files.len(&path), Some(len));
    assert!(reads_back(&document, &path, &from));
    // Read from anywhere in it.
    let mut reader = document.open_large(&path).unwrap().open().unwrap();
    use std::io::Seek as _;
    reader.seek(std::io::SeekFrom::Start(len - 10)).unwrap();
    let mut end = Vec::new();
    reader.read_to_end(&mut end).unwrap();
    assert_eq!(end, (len - 10..len).map(|at| pattern(1, at)).collect::<Vec<_>>());
}

#[cfg(target_os = "linux")]
#[test]
fn a_large_file_is_added_by_copying_not_by_reading_twin() {
    added_by_copying(TWIN);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "writes 300 MB files: run alone, in release"]
fn a_large_file_is_added_by_copying_not_by_reading() {
    added_by_copying(LARGE);
}

#[test]
fn a_video_is_named_and_checked_as_a_picture_is() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let mut document = untitled();
    keep_in(&mut document, &work.path().join("recovery"));
    let not_a_video = patterned(work.path(), "notes.txt", 1000, 2);
    assert_eq!(
        document.add_large_file(FileKind::Video, "notes.txt", &not_a_video, &no_progress, &never()),
        Err(NotAdded::NotThatKind)
    );
    let mov = patterned(work.path(), "CON.mov", 1000, 2);
    assert_eq!(document.add_large_file(FileKind::Video, "CON.mov", &mov, &no_progress, &never()).as_deref(), Ok("assets/con-file.mov"));
    let other = patterned(work.path(), "con.mov", 1000, 3);
    assert_eq!(
        document.add_large_file(FileKind::Video, "con.mov", &other, &no_progress, &never()).as_deref(),
        Ok("assets/con-file-2.mov"),
        "a name taken is numbered"
    );
    // Over 4 GB, and refused before anything is copied: the file costs nothing on disk.
    let huge = work.path().join("huge.mp4");
    std::fs::File::create(&huge).unwrap().set_len(slides_engine::MAX_LARGE_FILE + 1).unwrap();
    let copied = std::cell::Cell::new(0);
    let counted = |done: u64, _: u64| copied.set(done);
    let refused = document.add_large_file(FileKind::Video, "huge.mp4", &huge, &counted, &never());
    assert_eq!(refused, Err(NotAdded::VideoTooLarge));
    assert_eq!(copied.get(), 0);
    assert!(NotAdded::VideoTooLarge.to_string().contains("4 GB"), "{}", NotAdded::VideoTooLarge);
    // A video's bytes aren't added as a small file's are.
    assert_eq!(document.add_file(FileKind::Video, "clip.mp4", vec![0; 100]), Err(NotAdded::NotThatKind));
    // Without a working copy there's nowhere to keep it.
    let mut alone = untitled();
    assert_eq!(add(&mut alone, &mov), Err(NotAdded::NowhereToKeep));
}

// Saving.

#[cfg(target_os = "linux")]
/// Each way of saving, one after another, from a new presentation holding a video of `len`
/// bytes: each result holds the same bytes, and opens again holding them.
fn survives_every_way_of_saving(len: u64) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    enough_disk(work.path(), len * 12);
    let from = patterned(work.path(), "clip.mp4", len, 4);
    let root = work.path().join("recovery");
    let checked = |at: &Path, path: &str| {
        assert!(same_pieces(saved_file(at, path).expect("saved"), std::fs::File::open(&from).unwrap()), "{}", at.display());
        let again = reopen(at);
        assert!(reads_back(&again, path, &from), "{} opens again", at.display());
    };
    for first_as_pack in [false, true] {
        let dir = work.path().join(if first_as_pack { "pack-first" } else { "folder-first" });
        std::fs::create_dir(&dir).unwrap();
        let mut document = untitled();
        keep_in(&mut document, &root);
        let path = add(&mut document, &from).unwrap();
        show(&mut document, &path);
        let in_working_copy = lies_at(&document, &path).unwrap();
        let mut grew = 0;
        let mut step = |document: &mut Document, target: SaveTarget| {
            grew = grew.max(growth_of(|| save_as(document, target)));
        };
        let (a, b, c, d) = if first_as_pack {
            (dir.join("a.sptx"), dir.join("b"), dir.join("c.sptx"), dir.join("d.sptx"))
        } else {
            (dir.join("a"), dir.join("b.sptx"), dir.join("c"), dir.join("d.sptx"))
        };
        let target = |at: &Path| if at.extension().is_some() { SaveTarget::Packed(at.to_path_buf()) } else { SaveTarget::Folder(at.to_path_buf()) };
        step(&mut document, target(&a));
        assert!(!document.is_untitled());
        assert!(!document.has_unsaved_changes());
        checked(&a, &path);
        // The talk's own now, read from where it was saved.
        assert!(document.files_to_write().is_empty());
        assert!(lies_at(&document, &path).is_none_or(|lies| !lies.starts_with(&root)));
        // The working copy's own copy stays until its slot is cleared, which the window does.
        assert!(in_working_copy.exists());
        for at in [&b, &c, &d] {
            step(&mut document, target(at));
            checked(at, &path);
        }
        // Saved in place twice: nothing is copied from the working copy again.
        for _ in 0..2 {
            document.insert_tag("Insert text", "Box", &[], None).unwrap();
            assert!(document.files_to_write().is_empty(), "the video is the pack's now");
            let job = document.save_job();
            grew = grew.max(growth_of(|| document.saved(job.run().unwrap())));
            checked(&d, &path);
        }
        eprintln!("saving {len} bytes every way: peak resident grew by {:.1} MB at most", grew as f64 / 1e6);
        if len == LARGE {
            assert!(grew < 64 * 1024 * 1024, "grew by {grew} bytes");
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_large_file_survives_every_way_of_saving_twin() {
    survives_every_way_of_saving(TWIN);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "writes 300 MB files: run alone, in release"]
fn a_large_file_survives_every_way_of_saving() {
    survives_every_way_of_saving(LARGE);
}

/// A stop asked for once half of the bytes are written.
fn stop_at_half(stop: &AtomicBool) -> impl Fn(u64, u64) + '_ {
    move |done, total| {
        if done * 2 >= total {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

fn hash_of(path: &Path) -> Vec<u8> {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut std::fs::File::open(path).unwrap(), &mut hasher).unwrap();
    hasher.finalize().to_vec()
}

#[test]
fn a_save_that_fails_half_way_leaves_the_old_talk_whole() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 5);
    let mut document = untitled();
    keep_in(&mut document, &work.path().join("recovery"));
    let pack = work.path().join("talk.sptx");
    save_as(&mut document, SaveTarget::Packed(pack.clone()));
    let path = add(&mut document, &from).unwrap();
    show(&mut document, &path);
    let before = hash_of(&pack);
    let edited = document.source().to_string();

    // In place, stopped half-way through: the pack is as it was, and the talk still has its
    // changes and its video, in the working copy.
    let stop = never();
    let failed = document.save_job().run_with(&stop_at_half(&stop), &stop);
    assert!(failed.is_err());
    assert_eq!(hash_of(&pack), before, "the old talk is whole");
    assert!(document.has_unsaved_changes());
    assert_eq!(document.source(), edited);
    assert!(reads_back(&document, &path, &from));
    assert!(files_under(work.path()).iter().all(|name| !name.ends_with(".tmp")), "nothing half-written is left");

    // Save As to a folder, stopped: nothing is there.
    let stop = never();
    let folder = work.path().join("copy");
    assert!(document.save_as_job(SaveTarget::Folder(folder.clone())).run_with(&stop_at_half(&stop), &stop).is_err());
    assert!(!folder.exists());
    assert!(matches!(document.place(), TalkPlace::Packed(at) if *at == pack));

    // And the next save works.
    save(&mut document);
    assert!(same_pieces(saved_file(&pack, &path).unwrap(), std::fs::File::open(&from).unwrap()));
}

#[test]
fn a_first_save_that_fails_leaves_it_untitled_with_its_copy() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 6);
    let mut document = untitled();
    keep_in(&mut document, &work.path().join("recovery"));
    let path = add(&mut document, &from).unwrap();
    show(&mut document, &path);
    for target in [SaveTarget::Packed(work.path().join("first.sptx")), SaveTarget::Folder(work.path().join("first"))] {
        let stop = never();
        assert!(document.save_as_job(target.clone()).run_with(&stop_at_half(&stop), &stop).is_err());
        assert!(document.is_untitled());
        let (SaveTarget::Packed(at) | SaveTarget::Folder(at)) = &target;
        assert!(!at.exists(), "nothing at {}", at.display());
        assert!(reads_back(&document, &path, &from), "its copy is still there");
    }
    save_as(&mut document, SaveTarget::Folder(work.path().join("first")));
    assert!(same_pieces(saved_file(&work.path().join("first"), &path).unwrap(), std::fs::File::open(&from).unwrap()));
}

// A crash.

#[test]
fn a_crash_while_a_large_file_is_copied_in_leaves_no_half_file() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 7);
    let root = work.path().join("recovery");
    let id = UntitledId::random();
    for crash in [false, true] {
        {
            let mut document =
                Document::new_untitled(template("light").unwrap(), id, Fonts::bundled_only()).unwrap();
            let recovery = keep_in(&mut document, &root);
            // Edited and kept, as the window keeps it.
            recovery.keep_untitled(id, "Untitled presentation", "# Kept\n", &[], &[]).unwrap();
            let stop = never();
            let copy = std::panic::AssertUnwindSafe(&mut document);
            let from = from.clone();
            let outcome = std::panic::catch_unwind(move || {
                let copy = copy;
                let half = |done: u64, total: u64| {
                    if done * 2 >= total {
                        if crash {
                            // Stops the app where it is, as a crash does, with nothing tidied.
                            panic!("the app stops");
                        }
                        stop.store(true, Ordering::SeqCst);
                    }
                };
                copy.0.add_large_file(FileKind::Video, "clip.mp4", &from, &half, &stop)
            });
            match outcome {
                Ok(added) => assert_eq!(added, Err(NotAdded::Cancelled)),
                Err(_) => assert!(crash),
            }
            // Everything goes.
        }
        let next_start = Recovery::new(root.clone());
        let (listed, skipped) = next_start.untitled();
        assert_eq!(skipped, 0);
        assert!(listed.iter().any(|copy| copy.id == id), "the talk is listed");
        let found = next_start.take_untitled(id).unwrap();
        assert_eq!(found.text, "# Kept\n");
        assert!(found.large.is_empty(), "without the file: {:?}", found.large);
        assert!(found.files.iter().all(|(path, _)| !path.contains(".mp4")));
        // Nothing under a video's name is anywhere: what a crash leaves is under a name of its own.
        assert!(
            files_under(&root).iter().all(|name| !name.ends_with(".mp4")),
            "crash {crash}: {:?}",
            files_under(&root)
        );
    }
}

/// Opens a new presentation in a window keeping its working copies under `root`, adds the video
/// at `from` to it from the window, and puts it on the slide.
fn window_with_video(cx: &mut TestAppContext, root: &Path, from: &Path) -> (Typing, String) {
    let talk = untitled();
    let typing = open_untitled_talk(cx, talk, root);
    let path = copy_in(&typing, cx, from);
    (typing, path)
}

/// Copies the video at `from` into the talk in `typing`'s window, as Insert does, and puts it
/// on the slide once it's there.
fn copy_in(typing: &Typing, cx: &mut TestAppContext, from: &Path) -> String {
    let from = from.to_path_buf();
    let added = std::rc::Rc::new(std::cell::RefCell::new(None));
    let given = added.clone();
    cx.update(|cx| {
        typing.view.update(cx, |view, cx| {
            view.copy_in_large_file(
                FileKind::Video,
                from,
                move |view, path, cx| {
                    *given.borrow_mut() = Some(path.clone());
                    let document = view.document().unwrap().clone();
                    document.update(cx, |document, cx| {
                        document.insert_tag("Insert video", "Video", &[("src", &path)], None).unwrap();
                        cx.notify();
                    });
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    typing.settle(cx);
    added.borrow().clone().expect("it was copied in")
}

#[gpui_kit::test]
fn a_crash_after_it_is_added_brings_it_back(cx: &mut TestAppContext) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 8);
    let root = work.path().join("root");
    let (typing, path) = window_with_video(cx, &root, &from);
    wait(cx, KEEP_PAUSE);
    let place = typing.document(cx).read_with(cx, |document, _| document.place().clone());
    let TalkPlace::Untitled(id) = place else { panic!("untitled") };
    // The app stops: what's kept is all there is.
    let found = Recovery::new(root.join("recovery")).find(&place).expect("a working copy");
    assert!(found.text.contains(&path));
    assert_eq!(found.large.len(), 1, "the video, by where it lies");
    assert_eq!(found.large[0].0, path);
    assert!(same_pieces(std::fs::File::open(&found.large[0].1).unwrap(), std::fs::File::open(&from).unwrap()));
    let back = open_brought_back(cx, id, found, 2, &root);
    back.settle(cx);
    let document = back.document(cx);
    document.read_with(cx, |document, _| {
        assert!(document.source().contains(&path));
        assert!(reads_back(document, &path, &from));
        let lies = lies_at(document, &path).unwrap();
        assert!(lies.starts_with(root.join("recovery")), "in the new window's working copy");
    });
    // Kept as its own, and saved from there.
    wait(cx, KEEP_PAUSE);
    let out = root.join("Documents").join("saved");
    let saved = document.update(cx, |document, _| document.save_as_job(SaveTarget::Folder(out.clone())).run());
    assert!(saved.is_ok());
    assert!(same_pieces(saved_file(&out, &path).unwrap(), std::fs::File::open(&from).unwrap()));
}

// Undo and Redo.

#[gpui_kit::test]
fn undo_of_the_insert_then_a_save_does_not_write_it(cx: &mut TestAppContext) {
    undo_redo_and_save(cx, false);
}

#[gpui_kit::test]
fn redo_then_a_save_does(cx: &mut TestAppContext) {
    undo_redo_and_save(cx, true);
}

fn undo_redo_and_save(cx: &mut TestAppContext, redo: bool) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 9);
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), "# Talk\n").unwrap();
    let talk_folder = folder.path().to_path_buf();
    let root = work.path().join("recovery");
    let typing = open_folder(cx, folder, Some(Recovery::new(root.clone())));
    // Another edit, for the save to write.
    typing.press(cx, "ctrl-m");
    let path = copy_in(&typing, cx, &from);
    let lies = typing.document(cx).read_with(cx, |document, _| lies_at(document, &path)).unwrap();
    wait(cx, KEEP_PAUSE);
    typing.press(cx, "ctrl-z");
    assert!(!typing.source(cx).contains(&path));
    // A save writes nothing of it, and the working copy keeps it while the talk may use it again.
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    assert!(!talk_folder.join(&path).exists(), "not written");
    assert!(lies.exists(), "kept in the working copy");
    if !redo {
        return;
    }
    typing.press(cx, "ctrl-shift-z");
    assert!(typing.source(cx).contains(&path));
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    assert!(same_pieces(std::fs::File::open(talk_folder.join(&path)).unwrap(), std::fs::File::open(&from).unwrap()));
    // Saved: the working copy's goes with its slot.
    assert!(!lies.exists(), "cleared with the slot");
}

// The same file.

#[test]
fn the_same_file_added_twice_is_one_file() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 10);
    let root = work.path().join("recovery");
    let mut document = untitled();
    keep_in(&mut document, &root);
    let first = add(&mut document, &from).unwrap();
    let again = patterned(work.path(), "another name.mp4", TWIN, 10);
    assert_eq!(add(&mut document, &again).unwrap(), first, "found by its size and hash");
    assert_eq!(videos_under(&root).len(), 1, "{:?}", videos_under(&root));
    // A talk that has it in its folder already: nothing is copied.
    let folder = work.path().join("talk");
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::write(folder.join("slides.md"), "# Talk\n").unwrap();
    std::fs::copy(&from, folder.join("assets/kept.mp4")).unwrap();
    let mut saved = Document::open(&folder, Fonts::bundled_only()).unwrap();
    let other_root = work.path().join("other");
    keep_in(&mut saved, &other_root);
    assert_eq!(add(&mut saved, &from).unwrap(), "assets/kept.mp4");
    assert!(videos_under(&other_root).is_empty());
    // Other bytes of the same length are another file.
    let other = patterned(work.path(), "kept.mp4", TWIN, 11);
    assert_eq!(add(&mut saved, &other).unwrap(), "assets/kept-2.mp4");
}

#[test]
fn two_talks_can_hold_the_same_file() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 12);
    let root = work.path().join("recovery");
    let (mut one, mut two) = (untitled(), untitled());
    keep_in(&mut one, &root);
    keep_in(&mut two, &root);
    let (a, b) = (add(&mut one, &from).unwrap(), add(&mut two, &from).unwrap());
    show(&mut one, &a);
    show(&mut two, &b);
    assert_ne!(lies_at(&one, &a), lies_at(&two, &b), "each its own copy");
    save_as(&mut one, SaveTarget::Packed(work.path().join("one.sptx")));
    save_as(&mut two, SaveTarget::Folder(work.path().join("two")));
    assert!(same_pieces(saved_file(&work.path().join("one.sptx"), &a).unwrap(), std::fs::File::open(&from).unwrap()));
    assert!(same_pieces(saved_file(&work.path().join("two"), &b).unwrap(), std::fs::File::open(&from).unwrap()));
}

// What goes wrong.

#[test]
fn a_source_deleted_or_changed_while_copying_adds_nothing_and_says_why() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("recovery");
    type Change = fn(&Path);
    let changes: [(&str, Change); 3] = [
        ("deleted", |at| std::fs::remove_file(at).unwrap()),
        ("grown", |at| std::fs::OpenOptions::new().append(true).open(at).unwrap().write_all(b"more").unwrap()),
        ("cut short", |at| std::fs::OpenOptions::new().write(true).open(at).unwrap().set_len(1000).unwrap()),
    ];
    for (what, change) in changes {
        let from = patterned(work.path(), "clip.mp4", TWIN, 13);
        let mut document = untitled();
        keep_in(&mut document, &root);
        let changed = std::cell::Cell::new(false);
        let half = |done: u64, total: u64| {
            if done * 2 >= total && !changed.replace(true) {
                change(&from);
            }
        };
        let added = document.add_large_file(FileKind::Video, "clip.mp4", &from, &half, &never());
        assert_eq!(added, Err(NotAdded::Changed), "{what}");
        assert!(NotAdded::Changed.to_string().contains("changed"), "{}", NotAdded::Changed);
        assert!(document.export_input().files.len("assets/clip.mp4").is_none(), "{what}");
        assert!(videos_under(&root).is_empty(), "{what}: {:?}", files_under(&root));
    }
}

#[test]
fn a_full_disk_says_so_and_adds_nothing() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("recovery");
    let from = patterned(work.path(), "clip.mp4", TWIN, 14);
    let mut document = untitled();
    keep_in(&mut document, &root);
    set_free_space_for_test(&root, TWIN / 2);
    let copied = std::cell::Cell::new(0);
    let counted = |done: u64, _: u64| copied.set(done);
    assert_eq!(document.add_large_file(FileKind::Video, "clip.mp4", &from, &counted, &never()), Err(NotAdded::NoRoom));
    assert_eq!(copied.get(), 0, "checked before copying");
    assert!(NotAdded::NoRoom.to_string().contains("room"), "{}", NotAdded::NoRoom);
    assert!(NotAdded::RanOutOfRoom.to_string().contains("filled"), "{}", NotAdded::RanOutOfRoom);
    assert!(videos_under(&root).is_empty());
    set_free_space_for_test(&root, u64::MAX);
    assert!(add(&mut document, &from).is_ok());
}

#[test]
fn cancel_stops_a_copy_and_leaves_nothing() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("recovery");
    let from = patterned(work.path(), "clip.mp4", TWIN, 15);
    let mut document = untitled();
    keep_in(&mut document, &root);
    let stop = never();
    let added = document.add_large_file(FileKind::Video, "clip.mp4", &from, &stop_at_half(&stop), &stop);
    assert_eq!(added, Err(NotAdded::Cancelled));
    assert!(videos_under(&root).is_empty(), "{:?}", files_under(&root));
    assert!(document.export_input().files.len("assets/clip.mp4").is_none());
    assert!(!document.has_unsaved_changes());
}

#[gpui_kit::test]
fn cancel_in_the_window_stops_a_copy_and_leaves_nothing(cx: &mut TestAppContext) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 16);
    let root = work.path().join("root");
    let typing = open_untitled_talk(cx, untitled(), &root);
    let given = std::rc::Rc::new(std::cell::Cell::new(false));
    let called = given.clone();
    cx.update(|cx| {
        typing.view.update(cx, |view, cx| {
            view.copy_in_large_file(FileKind::Video, from.clone(), move |_, _, _| called.set(true), cx);
        })
    });
    // The bar shows while it copies, with its words and a Cancel.
    let label = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let label = window.find(PROGRESS_ID).label().map(str::to_string);
        window.click(CANCEL_PROGRESS_ID, cx);
        label
    });
    assert_eq!(label.as_deref(), Some("Copying video… 0%"));
    cx.run_until_parked();
    typing.settle(cx);
    assert!(!given.get(), "nothing was added");
    assert!(typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(PROGRESS_ID).is_none()
    }));
    assert!(videos_under(&root).is_empty(), "{:?}", files_under(&root));
    assert!(typing.status(cx).is_some_and(|said| said == NotAdded::Cancelled.to_string()));
}

#[test]
fn the_status_line_says_how_far_it_has_got() {
    assert_eq!(progress_words("Copying video", 40, 100), "Copying video… 40%");
    assert_eq!(progress_words("Saving talk.sptx", 0, 0), "Saving talk.sptx… 0%");
    // At the end, the file is still being made whole and put in place.
    assert_eq!(progress_words("Copying video", 100, 100), "Finishing…");
    assert_eq!(ask_to_copy(1, 392 * 1000 * 1000), "Copy 1 video, 392 MB, into this presentation?");
    assert_eq!(ask_to_copy(2, 1_500_000), "Copy 2 videos, 2 MB, into this presentation?");
}

// Copy and paste.

/// A folder talk whose one slide shows the video at `from`, held in its assets.
fn talk_with_video(from: &Path) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::copy(from, folder.path().join("assets/clip.mp4")).unwrap();
    std::fs::write(folder.path().join("slides.md"), format!("# From\n\n{VIDEO}\n")).unwrap();
    folder
}

const VIDEO: &str = "<Video src=\"assets/clip.mp4\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />";

/// What copying the video from the talk in `folder` puts on the clipboard.
fn copied_from(folder: &Path) -> ClipboardItem {
    let mut from = Document::open(folder, Fonts::bundled_only()).unwrap();
    let at = from.source().find("<Video").unwrap();
    assert!(from.select(0, at..at + VIDEO.len()));
    let (text, files) = from.copy_selected().unwrap();
    let large = from.large_for_copy(&text);
    assert_eq!(large.len(), 1, "carried by where it lies");
    let entry = clipboard::element_entry_with_large(&text, &files, &large).unwrap();
    let metadata = entry.metadata.unwrap();
    assert!(metadata.len() < 4096, "not its bytes: {} bytes", metadata.len());
    ClipboardItem::new_string_with_metadata(entry.text, metadata)
}

fn paste_in(typing: &Typing, cx: &mut TestAppContext, item: ClipboardItem) {
    cx.write_to_clipboard(item);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(strip_row_id(0), cx);
    });
    typing.settle(cx);
    typing.press(cx, "ctrl-v");
    cx.run_until_parked();
    typing.settle(cx);
}

#[gpui_kit::test]
fn a_slide_with_a_large_file_pastes_into_another_talk(cx: &mut TestAppContext) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 17);
    let source = talk_with_video(&from);
    let root = work.path().join("root");

    // Over the size a paste copies without asking: it asks, and Copy copies it in.
    let typing = open_untitled_talk(cx, untitled(), &root);
    paste_in(&typing, cx, copied_from(source.path()));
    let asked = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(COPY_LARGE_ID).is_some()
    });
    assert!(asked, "it asks first");
    click_on(&typing, cx, COPY_LARGE_ID);
    cx.run_until_parked();
    typing.settle(cx);
    let document = typing.document(cx);
    document.read_with(cx, |document, _| {
        assert!(document.source().contains("<Video src=\"assets/clip.mp4\""), "{}", document.source());
        assert!(reads_back(document, "assets/clip.mp4", &from));
        assert!(lies_at(document, "assets/clip.mp4").unwrap().starts_with(root.join("recovery")));
    });

    // Declined: the thing is pasted without it, and that's said.
    let typing = open_untitled_talk(cx, untitled(), &root);
    paste_in(&typing, cx, copied_from(source.path()));
    click_on(&typing, cx, DONT_COPY_LARGE_ID);
    cx.run_until_parked();
    typing.settle(cx);
    let document = typing.document(cx);
    document.read_with(cx, |document, _| {
        assert!(document.source().contains("<Video"));
        assert!(document.export_input().files.len("assets/clip.mp4").is_none());
    });
    let said = typing.status(cx).expect("it says so");
    assert!(said.contains("wasn't copied"), "{said}");

    // The talk it came from is gone by the time it's pasted: that's said too.
    let item = copied_from(source.path());
    drop(source);
    let typing = open_untitled_talk(cx, untitled(), &root);
    paste_in(&typing, cx, item);
    let asked = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(COPY_LARGE_ID).is_some()
    });
    assert!(!asked, "there's nothing to copy");
    assert!(typing.source(cx).contains("<Video"));
    let said = typing.status(cx).expect("it says so");
    assert!(said.contains("gone"), "{said}");
}

#[test]
fn a_copy_carries_where_a_large_file_lies_and_a_paste_checks_it() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 18);
    let source = talk_with_video(&from);
    let item = copied_from(source.path());
    let AppData::Element(carried) = clipboard::read_app_data(item.metadata().map(String::as_str)) else {
        panic!("element data")
    };
    assert_eq!(carried.files.large.len(), 1);
    let large = &carried.files.large[0];
    assert_eq!(large.path, "assets/clip.mp4");
    assert_eq!(large.len, TWIN);
    // Brought into another talk in pieces, as a paste brings it.
    let mut into = untitled();
    keep_in(&mut into, &work.path().join("recovery"));
    let path = into.bring_large(large, &no_progress, &never()).unwrap();
    assert_eq!(path, "assets/clip.mp4");
    assert!(reads_back(&into, &path, &from));
    // Changed since it was copied, it isn't what was copied.
    std::fs::OpenOptions::new().append(true).open(source.path().join("assets/clip.mp4")).unwrap().write_all(b"x").unwrap();
    let mut other = untitled();
    keep_in(&mut other, &work.path().join("recovery"));
    assert_eq!(other.bring_large(large, &no_progress, &never()), Err(NotAdded::Changed));
    // A clipboard naming a file that isn't in a talk, or a working copy, is never followed.
    let outside = patterned(work.path(), "secret.mp4", 100, 19);
    let forged = item
        .metadata()
        .unwrap()
        .replace(&source.path().join("assets/clip.mp4").display().to_string(), &outside.display().to_string())
        .replace(&format!("\"len\":{TWIN}"), "\"len\":100");
    let AppData::Element(carried) = clipboard::read_app_data(Some(&forged)) else { panic!("element data") };
    if let Some(large) = carried.files.large.first() {
        assert_eq!(other.bring_large(large, &no_progress, &never()), Err(NotAdded::Unreadable));
    }
}

// Old and odd packs.

/// A pack holding `slides` and a video at `assets/<name>` of `len` patterned bytes, compressed,
/// as another program might make it.
fn compressed_pack(dir: &Path, file: &str, name: &str, len: u64, seed: u8) -> PathBuf {
    let at = dir.join(file);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&at).unwrap());
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("talk.json", deflated).unwrap();
    zip.write_all(br#"{"format":1,"app":"another","entry":"slides.md"}"#).unwrap();
    zip.start_file("slides.md", deflated).unwrap();
    zip.write_all(format!("# Odd\n\n<Video src=\"assets/{name}\" />\n").as_bytes()).unwrap();
    zip.start_file(format!("assets/{name}"), deflated).unwrap();
    let mut piece = vec![0u8; 64 * 1024];
    let mut done = 0;
    while done < len {
        let n = (len - done).min(piece.len() as u64) as usize;
        for (i, byte) in piece[..n].iter_mut().enumerate() {
            *byte = pattern(seed, done + i as u64);
        }
        zip.write_all(&piece[..n]).unwrap();
        done += n as u64;
    }
    zip.finish().unwrap();
    at
}

#[test]
fn packs_old_and_odd_still_open() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let cache_folder = work.path().join("cache");

    // A pack made before this plan, its video compressed: it opens, and its video is unpacked to
    // the cache to be read, never into memory.
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/fixtures");
    for name in ["made-before.sptx", "made-before-with-a-video.sptx"] {
        let pack = Path::new(fixtures).join(name);
        let mut document = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
        document.use_video_cache(VideoCache::new(cache_folder.clone(), CACHE_LIMIT));
        assert_eq!(document.slide_count(), 2, "{name}");
        if name.contains("video") {
            let held = open_packed(&pack).unwrap().read("assets/clip.mp4").unwrap();
            let reader = document.open_large("assets/clip.mp4").unwrap().open().expect("unpacked to the cache");
            assert!(same_pieces(reader, held.as_slice()));
            assert_eq!(files_under(&cache_folder).len(), 1, "{:?}", files_under(&cache_folder));
            // Asked again, the cache has it.
            assert!(document.open_large("assets/clip.mp4").unwrap().open().is_some());
            assert_eq!(files_under(&cache_folder).len(), 1);
        }
    }

    // The cache keeps no more than it's given, oldest out first.
    let small = work.path().join("small-cache");
    let cache = VideoCache::new(small.clone(), 2 * 100_000 + 50_000);
    let mut opened = Vec::new();
    for (index, seed) in [21u8, 22, 23].into_iter().enumerate() {
        let pack = compressed_pack(work.path(), &format!("odd-{index}.sptx"), "clip.mp4", 100_000, seed);
        let mut document = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
        document.use_video_cache(cache.clone());
        let reader = document.open_large("assets/clip.mp4").unwrap().open().unwrap();
        let expected: Vec<u8> = (0..100_000).map(|at| pattern(seed, at)).collect();
        assert!(same_pieces(reader, expected.as_slice()));
        opened.push(document);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(files_under(&small).len(), 2, "the oldest went: {:?}", files_under(&small));
    // The one that went is unpacked again when it's asked for.
    assert!(opened[0].open_large("assets/clip.mp4").unwrap().open().is_some());
    assert_eq!(files_under(&small).len(), 2);

    // A talk naming a video that isn't there opens, and has nothing to open.
    let folder = work.path().join("missing");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), format!("# Gone\n\n{VIDEO}\n")).unwrap();
    let document = Document::open(&folder, Fonts::bundled_only()).unwrap();
    assert!(document.open_large("assets/clip.mp4").is_none_or(|open| open.open().is_none()));
    let _ = large_files::CACHE_LIMIT;
}

// A save that replaces a pack lets go of what reads it first.

#[test]
fn a_save_in_place_lets_go_of_readers_first() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 24);
    let mut document = untitled();
    keep_in(&mut document, &work.path().join("recovery"));
    let path = add(&mut document, &from).unwrap();
    show(&mut document, &path);
    let pack = work.path().join("talk.sptx");
    save_as(&mut document, SaveTarget::Packed(pack.clone()));
    let mut reader = document.open_large(&path).unwrap().open().unwrap();
    let mut start = [0u8; 16];
    reader.read_exact(&mut start).unwrap();
    assert_eq!(document.readers_held(), 1);
    document.insert_tag("Insert text", "Box", &[], None).unwrap();
    // As the window does before a save that replaces the pack.
    document.let_go_of_readers();
    assert_eq!(document.readers_held(), 0);
    assert!(reader.read(&mut start).is_err(), "a reader let go of reads no more");
    save(&mut document);
    // Nothing holds the pack that was replaced.
    #[cfg(target_os = "linux")]
    {
        let held_deleted = std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .any(|target| target.to_string_lossy().starts_with(&*pack.to_string_lossy()) && target.to_string_lossy().ends_with("(deleted)"));
        assert!(!held_deleted, "the replaced pack is still held open");
    }
    assert!(reads_back(&document, &path, &from));
}

#[allow(dead_code)]
fn clipboard_item_has_no_bytes(item: &ClipboardItem) -> bool {
    item.metadata().is_some_and(|metadata| !metadata.contains("\"bytes\":\"AAA"))
}

#[gpui_kit::test]
fn a_folder_talks_video_is_brought_back_after_a_crash(cx: &mut TestAppContext) {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 25);
    let root = work.path().join("recovery");
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), "# Talk\n").unwrap();
    let talk_folder = folder.path().to_path_buf();
    let typing = open_folder(cx, folder, Some(Recovery::new(root.clone())));
    let path = copy_in(&typing, cx, &from);
    wait(cx, KEEP_PAUSE);
    let edited = typing.source(cx);
    // The app stops, the talk's folder as it was.
    let Typing { _folder: folder, handle, view, .. } = typing;
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    assert!(!talk_folder.join(&path).exists(), "nothing on disk until Save");

    let typing = open_folder(cx, folder, Some(Recovery::new(root.clone())));
    wait(cx, 500);
    click_on(&typing, cx, stagepack_studio::workspace_view::RECOVER_ID);
    assert_eq!(typing.source(cx), edited);
    typing.document(cx).read_with(cx, |document, _| {
        assert!(reads_back(document, &path, &from));
        assert!(lies_at(document, &path).unwrap().starts_with(&root), "in this window's working copy");
    });
    // Last time's copy goes once this window has kept its own, and the video stays.
    wait(cx, KEEP_PAUSE);
    typing.document(cx).read_with(cx, |document, _| assert!(reads_back(document, &path, &from)));
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    assert!(same_pieces(std::fs::File::open(talk_folder.join(&path)).unwrap(), std::fs::File::open(&from).unwrap()));
}

#[test]
fn a_slot_cleared_while_a_file_is_copied_in_keeps_it() {
    set_up();
    let work = tempfile::tempdir().unwrap();
    let from = patterned(work.path(), "clip.mp4", TWIN, 26);
    let root = work.path().join("recovery");
    let mut document = untitled();
    let recovery = keep_in(&mut document, &root);
    recovery.keep_untitled(UntitledId(1), "Untitled presentation", "# Kept\n", &[], &[]).unwrap();
    let place = document.place().clone();
    let keep = document.large_keep();
    let cleared = std::cell::Cell::new(false);
    // A save made while it copies clears the working copy, as the window does.
    let half = |done: u64, total: u64| {
        if done * 2 >= total && !cleared.replace(true) {
            recovery.clear_keeping(&place, &keep.keeper());
        }
    };
    let path = document.add_large_file(FileKind::Video, "clip.mp4", &from, &half, &never()).unwrap();
    assert!(cleared.get());
    assert!(reads_back(&document, &path, &from));
    // Cleared again now that the talk holds it: it stays, and nothing else does.
    recovery.clear_keeping(&place, &document.large_keep().keeper());
    let lies = lies_at(&document, &path).unwrap();
    assert!(lies.exists());
    let slot = lies.parent().unwrap().parent().unwrap().parent().unwrap();
    let left: Vec<String> = files_under(slot);
    assert_eq!(left, ["files/assets/clip.mp4", "held"], "only the file and the slot's lock");
}
