use std::path::{Path, PathBuf};

use slides_engine::{Fonts, TalkFiles as _, open_packed, pack_folder};
use stagepack_studio::document::{Document, Reload};
use stagepack_studio::saving::{SaveError, SaveTarget};

const TWO_SLIDES: &str = "---\ntitle: Review\n---\n\n# One\n\n---\n\n# Two\n";

fn talk(source: &str) -> (tempfile::TempDir, Document) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    let doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    (dir, doc)
}

fn packed_talk(source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::write(folder.join("slides.md"), source).unwrap();
    std::fs::write(folder.join("assets/a.png"), b"\x89PNG not really").unwrap();
    let file = dir.path().join("talk.sptx");
    pack_folder(&folder, &file, "test").unwrap();
    (dir, file)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// Every file under `root`, with what it holds, by its path from there.
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Saves `doc` where it's kept, as the window does.
fn save(doc: &mut Document) {
    let saved = doc.save_job().run().unwrap();
    doc.saved(saved);
}

#[test]
fn saving_writes_the_text_and_nothing_else() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/a.png"), "a picture").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "mine").unwrap();
    let before = snapshot(dir.path());
    doc.add_slide("content").unwrap();
    assert!(doc.has_unsaved_changes());

    save(&mut doc);

    assert!(!doc.has_unsaved_changes());
    assert_eq!(read(&dir.path().join("slides.md")), doc.source());
    let others = |files: Vec<(String, Vec<u8>)>| -> Vec<(String, Vec<u8>)> {
        files.into_iter().filter(|(name, _)| name != "slides.md").collect()
    };
    assert_eq!(others(snapshot(dir.path())), others(before));
}

#[test]
fn saving_keeps_the_files_line_endings() {
    let source = "\u{feff}---\r\ntitle: Review  \r\n---\r\n\r\n# One\r\n\r\n---\r\n\r\n# Two\r\n";
    let (dir, mut doc) = talk(source);
    doc.add_slide("content").unwrap();
    save(&mut doc);
    let written = std::fs::read(dir.path().join("slides.md")).unwrap();
    assert_eq!(written, doc.source().as_bytes());
    assert!(written.starts_with("\u{feff}---\r\ntitle: Review  \r\n".as_bytes()));
    let text = String::from_utf8(written).unwrap();
    assert_eq!(text.matches('\n').count(), text.matches("\r\n").count(), "{text:?}");
}

#[cfg(unix)]
#[test]
fn a_failed_save_leaves_the_talk_unsaved_and_the_file_as_it_was() {
    use std::os::unix::fs::PermissionsExt as _;

    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.path().join("probe");
    if std::fs::write(&probe, "x").is_ok() {
        // Running as root, where a folder can't be made read-only.
        std::fs::remove_file(&probe).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    let outcome = doc.save_job().run();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

    let problem = outcome.err().expect("the save fails").to_string();
    assert!(!problem.is_empty());
    assert!(doc.has_unsaved_changes());
    assert_eq!(read(&dir.path().join("slides.md")), TWO_SLIDES);
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["slides.md"]);
}

#[test]
fn saving_a_packed_talk_rewrites_the_pack() {
    let (_dir, file) = packed_talk(TWO_SLIDES);
    let mut doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    doc.add_slide("content").unwrap();

    save(&mut doc);

    assert!(!doc.has_unsaved_changes());
    let packed = open_packed(&file).unwrap();
    assert_eq!(packed.slides().unwrap(), doc.source());
    assert_eq!(packed.read("assets/a.png").unwrap(), b"\x89PNG not really");
    // The open talk still reads its files, and saves again.
    let export = doc.export_input();
    assert_eq!(export.files.read("assets/a.png").unwrap(), b"\x89PNG not really");
    doc.add_slide("content").unwrap();
    save(&mut doc);
    assert_eq!(open_packed(&file).unwrap().slides().unwrap(), doc.source());
}

#[test]
fn save_as_a_folder_then_edits_go_there() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/a.png"), "a picture").unwrap();
    doc.add_slide("content").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let copy = elsewhere.path().join("copy");

    let saved = doc.save_as_job(SaveTarget::Folder(copy.clone())).run().unwrap();
    doc.saved(saved);

    assert!(!doc.has_unsaved_changes());
    assert_eq!(read(&copy.join("slides.md")), doc.source());
    assert_eq!(read(&copy.join("assets/a.png")), "a picture");
    assert_eq!(read(&dir.path().join("slides.md")), TWO_SLIDES, "the old place is as it was");
    assert!(matches!(doc.place(), stagepack_studio::document::TalkPlace::Folder(folder) if *folder == copy));

    doc.add_slide("content").unwrap();
    save(&mut doc);
    assert_eq!(read(&copy.join("slides.md")), doc.source());
    assert_eq!(read(&dir.path().join("slides.md")), TWO_SLIDES);
    // Its pictures are read from there now too.
    std::fs::write(copy.join("assets/b.png"), "another").unwrap();
    assert_eq!(doc.export_input().files.read("assets/b.png").unwrap(), b"another");
}

#[test]
fn save_as_packed_from_a_folder() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("copy.sptx");

    let saved = doc.save_as_job(SaveTarget::Packed(out.clone())).run().unwrap();
    assert!(saved.notices().is_empty(), "{:?}", saved.notices());
    doc.saved(saved);

    assert!(doc.is_packed());
    assert!(!doc.has_unsaved_changes());
    assert_eq!(open_packed(&out).unwrap().slides().unwrap(), doc.source());
    assert_eq!(read(&dir.path().join("slides.md")), TWO_SLIDES);
    assert_eq!(doc.name(), "Review");
}

#[test]
fn save_as_packed_says_what_was_left_out() {
    let (dir, doc) = talk("![](pics/photo.jpg)\n");
    std::fs::create_dir(dir.path().join("pics")).unwrap();
    std::fs::write(dir.path().join("pics/photo.jpg"), "a picture").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let saved = doc
        .save_as_job(SaveTarget::Packed(elsewhere.path().join("copy.sptx")))
        .run()
        .unwrap();
    assert_eq!(saved.notices(), ["The picture pics/photo.jpg is outside assets/ and wasn't packed"]);
}

#[test]
fn save_as_a_folder_from_a_packed_talk() {
    let (dir, file) = packed_talk(TWO_SLIDES);
    let mut doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    doc.add_slide("content").unwrap();
    let folder = dir.path().join("unpacked");

    let saved = doc.save_as_job(SaveTarget::Folder(folder.clone())).run().unwrap();
    doc.saved(saved);

    assert!(!doc.is_packed());
    assert_eq!(read(&folder.join("slides.md")), doc.source());
    assert_eq!(std::fs::read(folder.join("assets/a.png")).unwrap(), b"\x89PNG not really");
    assert_eq!(open_packed(&file).unwrap().slides().unwrap(), TWO_SLIDES);
}

#[test]
fn save_as_into_a_folder_with_things_in_it_says_why_and_leaves_them() {
    let (dir, doc) = talk(TWO_SLIDES);
    let folder = dir.path().join("in-use");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("keep.txt"), "mine").unwrap();
    let problem = doc.save_as_job(SaveTarget::Folder(folder.clone())).run().err();
    assert!(problem.is_some_and(|problem| !problem.to_string().is_empty()));
    assert_eq!(snapshot(&folder), [("keep.txt".to_string(), b"mine".to_vec())]);
}

#[test]
fn the_apps_own_save_is_not_an_outside_change() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    save(&mut doc);
    assert_eq!(doc.stamp_on_disk(), doc.source());

    let reload = doc.reload(Some(read(&dir.path().join("slides.md"))));

    assert!(!matches!(reload, Reload::Conflict), "the app's own save");
    assert!(doc.outside_change().is_none());
    assert!(doc.can_undo(), "the history is kept");
    // Not even once more is typed after it.
    doc.add_slide("content").unwrap();
    let reload = doc.reload(Some(read(&dir.path().join("slides.md"))));
    assert!(!matches!(reload, Reload::Conflict));
    assert!(doc.has_unsaved_changes());
}

#[test]
fn an_outside_change_with_nothing_unsaved_is_taken() {
    let (_dir, mut doc) = talk(TWO_SLIDES);
    let theirs = format!("{TWO_SLIDES}\n---\n\n# Three\n");

    let reload = doc.reload(Some(theirs.clone()));

    assert!(matches!(reload, Reload::Changed(_)));
    assert_eq!(doc.source(), theirs);
    assert_eq!(doc.stamp_on_disk(), theirs);
    assert!(!doc.has_unsaved_changes());
}

#[test]
fn an_outside_change_while_unsaved_asks() {
    let (_dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let mine = doc.source().to_string();
    let theirs = format!("{TWO_SLIDES}\n---\n\n# Three\n");

    let reload = doc.reload(Some(theirs.clone()));

    assert!(matches!(reload, Reload::Conflict));
    assert_eq!(doc.source(), mine, "the text is still mine");
    assert_eq!(doc.outside_change(), Some(theirs.as_str()));
    assert!(doc.has_unsaved_changes());
    assert!(doc.can_undo());
}

#[test]
fn loading_theirs_is_a_step_that_undo_takes_back() {
    let (_dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let mine = doc.source().to_string();
    let theirs = format!("{TWO_SLIDES}\n---\n\n# Three\n");
    doc.reload(Some(theirs.clone()));

    doc.take_theirs(theirs.clone());

    assert_eq!(doc.source(), theirs);
    assert!(doc.outside_change().is_none());
    assert!(!doc.has_unsaved_changes());
    assert_eq!(doc.slide_count(), 3);
    assert_eq!(doc.undo_label(), Some("Load the changed file"));

    doc.undo().unwrap();
    assert_eq!(doc.source(), mine, "the edits are back");
    assert!(doc.has_unsaved_changes());
    doc.undo().unwrap();
    assert_eq!(doc.source(), TWO_SLIDES, "and the steps before them still undo");
    doc.redo().unwrap();
    doc.redo().unwrap();
    assert_eq!(doc.source(), theirs);
    assert!(!doc.has_unsaved_changes());
}

#[test]
fn keeping_mine_then_saving_writes_mine() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let mine = doc.source().to_string();
    let theirs = format!("{TWO_SLIDES}\n---\n\n# Three\n");
    std::fs::write(dir.path().join("slides.md"), &theirs).unwrap();
    doc.reload(Some(theirs.clone()));

    doc.keep_mine();

    assert!(doc.outside_change().is_none());
    assert!(doc.has_unsaved_changes());
    // The same change again isn't asked about twice.
    assert!(!matches!(doc.reload(Some(theirs.clone())), Reload::Conflict));
    // Undoing back to what was saved before isn't what the file holds now.
    doc.undo();
    assert!(doc.has_unsaved_changes());
    doc.redo();
    // A new change is.
    let again = format!("{theirs}\n---\n\n# Four\n");
    std::fs::write(dir.path().join("slides.md"), &again).unwrap();
    assert!(matches!(doc.reload(Some(again)), Reload::Conflict));
    doc.keep_mine();

    save(&mut doc);
    assert_eq!(read(&dir.path().join("slides.md")), mine);
    assert!(!doc.has_unsaved_changes());
}

#[test]
fn edits_made_while_a_save_runs_stay_unsaved() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let job = doc.save_job();
    doc.add_slide("content").unwrap();

    let saved = job.run().unwrap();
    doc.saved(saved);

    assert!(doc.has_unsaved_changes());
    assert_ne!(read(&dir.path().join("slides.md")), doc.source());
    // And the file it wrote isn't taken for someone else's.
    let reload = doc.reload(Some(read(&dir.path().join("slides.md"))));
    assert!(!matches!(reload, Reload::Conflict));
}

/// Whether `folder` holds anything at all.
fn has_anything(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_some())
}

#[test]
fn save_as_inside_the_talks_own_folder_is_refused() {
    let (dir, doc) = talk(TWO_SLIDES);
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    let before = snapshot(dir.path());

    for target in [
        SaveTarget::Folder(dir.path().join("assets/copy")),
        SaveTarget::Folder(dir.path().join("new/deeper")),
        SaveTarget::Packed(dir.path().join("assets/copy.sptx")),
        SaveTarget::Packed(dir.path().join("slides.md")),
        SaveTarget::Packed(dir.path().join("assets/../talk.sptx")),
    ] {
        let problem = doc.save_as_job(target.clone()).run().err();
        let problem = problem.unwrap_or_else(|| panic!("{target:?} is refused")).to_string();
        assert!(problem.contains("inside"), "{problem}");
        assert_eq!(snapshot(dir.path()), before, "{target:?}");
    }
    assert!(!has_anything(&dir.path().join("assets")));
}

#[test]
fn save_as_beside_the_talks_folder_is_fine() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), TWO_SLIDES).unwrap();
    let doc = Document::open(&folder, Fonts::bundled_only()).unwrap();
    // A name that starts like the talk's folder isn't inside it.
    doc.save_as_job(SaveTarget::Folder(dir.path().join("talk-copy"))).run().unwrap();
    doc.save_as_job(SaveTarget::Packed(dir.path().join("talk.sptx"))).run().unwrap();
}

#[test]
fn a_save_over_an_outside_change_does_not_write_and_asks() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let mine = doc.source().to_string();
    let theirs = format!("{TWO_SLIDES}\n---\n\n# Theirs\n");
    std::fs::write(dir.path().join("slides.md"), &theirs).unwrap();

    let refused = doc.save_job().run().err();

    assert!(
        matches!(&refused, Some(SaveError::ChangedOutside(found)) if *found == theirs),
        "{:?}",
        refused.map(|e| e.to_string())
    );
    assert_eq!(read(&dir.path().join("slides.md")), theirs, "nothing was written");
    assert!(doc.has_unsaved_changes());

    // The window then asks, and keeping mine lets the next save go ahead.
    assert!(matches!(doc.reload(Some(theirs)), Reload::Conflict));
    doc.keep_mine();
    save(&mut doc);
    assert_eq!(read(&dir.path().join("slides.md")), mine);
}

#[test]
fn saving_over_an_emptied_file_goes_ahead() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    // An editor halfway through its own save, or a file emptied outside the app.
    std::fs::write(dir.path().join("slides.md"), "").unwrap();

    save(&mut doc);

    assert_eq!(read(&dir.path().join("slides.md")), doc.source());
    assert!(!doc.has_unsaved_changes());
}
