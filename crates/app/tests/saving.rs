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

    doc.take_theirs(theirs.clone()).unwrap();

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
    doc.undo().unwrap();
    assert!(doc.has_unsaved_changes());
    doc.redo().unwrap();
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

#[test]
fn saving_a_packed_talk_changed_outside_does_not_write_and_asks() {
    let (dir, file) = packed_talk(TWO_SLIDES);
    let mut doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    doc.add_slide("content").unwrap();
    let mine = doc.source().to_string();
    // A second window, or a sync client, puts another pack in its place.
    let theirs = format!("{TWO_SLIDES}\n---\n\n# Theirs\n");
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("slides.md"), &theirs).unwrap();
    pack_folder(&other, &file, "test").unwrap();

    let refused = doc.save_job().run().err();

    assert!(
        matches!(&refused, Some(SaveError::ChangedOutside(found)) if *found == theirs),
        "{:?}",
        refused.map(|e| e.to_string())
    );
    assert_eq!(open_packed(&file).unwrap().slides().unwrap(), theirs, "nothing was written");
    assert!(matches!(doc.reload(Some(theirs)), Reload::Conflict));
    doc.keep_mine();
    save(&mut doc);
    assert_eq!(open_packed(&file).unwrap().slides().unwrap(), mine);
}

#[test]
fn saving_a_packed_talk_that_holds_what_is_here_goes_ahead() {
    let (_dir, file) = packed_talk(TWO_SLIDES);
    let mut doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    doc.add_slide("content").unwrap();
    save(&mut doc);
    doc.add_slide("content").unwrap();
    save(&mut doc);
    assert_eq!(open_packed(&file).unwrap().slides().unwrap(), doc.source());
}

#[test]
fn save_as_a_folder_says_which_pictures_it_left_out() {
    let (dir, doc) = talk("![](pics/photo.jpg)\n\n![](assets/in.png)\n");
    std::fs::create_dir(dir.path().join("pics")).unwrap();
    std::fs::write(dir.path().join("pics/photo.jpg"), "a picture").unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/in.png"), "another").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let saved = doc
        .save_as_job(SaveTarget::Folder(elsewhere.path().join("copy")))
        .run()
        .unwrap();
    assert_eq!(saved.notices(), ["The picture pics/photo.jpg is outside assets/ and wasn't copied"]);
}

/// Saves a talk whose `slides.md` has been made `broken`, and says what came of it, with the
/// folder's name in it read as "the talk".
fn save_over(broken: impl FnOnce(&Path)) -> (Option<String>, tempfile::TempDir) {
    let (dir, mut doc) = talk(TWO_SLIDES);
    doc.add_slide("content").unwrap();
    let file = dir.path().join("slides.md");
    broken(&file);
    let name = dir.path().file_name().unwrap().to_string_lossy().into_owned();
    let problem = doc
        .save_job()
        .run()
        .err()
        .map(|problem| problem.to_string().replace(&format!("Couldn't save {name}:"), "Couldn't save the talk:"));
    (problem, dir)
}

#[test]
fn a_slides_file_that_is_there_but_cannot_be_read_is_not_written_over() {
    let not_text = b"# One\n\xff\xfe\n".to_vec();
    let (problem, dir) = save_over(|file| std::fs::write(file, &not_text).unwrap());
    assert_eq!(
        problem.as_deref(),
        Some("Couldn't save the talk: its slides.md can't be read, as it isn't UTF-8 text, so it was left as it is.")
    );
    assert_eq!(std::fs::read(dir.path().join("slides.md")).unwrap(), not_text);

    let (problem, dir) = save_over(|file| {
        std::fs::File::create(file).unwrap().set_len(slides_engine::MAX_FILE_BYTES + 1).unwrap();
    });
    assert_eq!(
        problem.as_deref(),
        Some("Couldn't save the talk: its slides.md can't be read, as it's over the limit of 256 MiB, so it was left as it is.")
    );
    assert_eq!(std::fs::metadata(dir.path().join("slides.md")).unwrap().len(), slides_engine::MAX_FILE_BYTES + 1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let (problem, dir) = save_over(|file| {
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o200)).unwrap();
        });
        let file = dir.path().join("slides.md");
        // Someone the system lets read anything sees no refusal to test.
        if std::fs::read(&file).is_err() {
            assert_eq!(
                problem.as_deref(),
                Some("Couldn't save the talk: its slides.md can't be read, as the app isn't allowed to read it, so it was left as it is.")
            );
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(read(&file), TWO_SLIDES);
        }
    }
}

#[test]
fn a_missing_slides_file_is_written_without_asking() {
    let (problem, dir) = save_over(|file| std::fs::remove_file(file).unwrap());
    assert_eq!(problem, None);
    let saved = read(&dir.path().join("slides.md"));
    assert_eq!(saved.lines().filter(|line| *line == "---").count(), 4, "the edit was saved: {saved}");
}

// Files added to a talk.

fn red_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// Puts a picture from `path` on the end of the last slide.
fn show(doc: &mut Document, path: &str) {
    let end = doc.source().len();
    let change = slides_format::Change::single(slides_format::SourceEdit { range: end..end, text: format!("\n![]({path})\n") });
    doc.edit("Insert picture", change, 0).unwrap();
}

/// Whether the last slide draws with nothing to say and something red on it.
fn last_draws_red(doc: &mut Document) -> bool {
    let last = doc.slide_count() - 1;
    doc.go_to(last);
    let notices = doc.notices();
    assert!(notices.is_empty(), "{notices:?}");
    let thumbnail = doc.thumbnail(last, 480).expect("the slide draws");
    thumbnail.data().chunks(4).any(|pixel| pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40)
}

/// Opens what's at `place` afresh, a folder or a pack.
fn open_fresh(place: &Path) -> Document {
    if place.is_dir() {
        Document::open(place, Fonts::bundled_only()).unwrap()
    } else {
        Document::open_packed(place, Fonts::bundled_only()).unwrap()
    }
}

/// The bytes of `path` in the talk at `place`, a folder or a pack.
fn file_in(place: &Path, path: &str) -> Option<Vec<u8>> {
    if place.is_dir() {
        slides_engine::FolderFiles::new(place.to_path_buf()).read(path)
    } else {
        open_packed(place).unwrap().read(path)
    }
}

#[test]
fn an_added_file_survives_each_way_of_saving() {
    enum Way {
        Save,
        SaveAs(fn(&Path) -> SaveTarget),
    }
    let to_packed: fn(&Path) -> SaveTarget = |dir| SaveTarget::Packed(dir.join("copy.sptx"));
    let to_folder: fn(&Path) -> SaveTarget = |dir| SaveTarget::Folder(dir.join("copy"));
    let cases: [(&str, bool, Way); 5] = [
        ("folder Save", false, Way::Save),
        ("packed Save", true, Way::Save),
        ("Save As folder to packed", false, Way::SaveAs(to_packed)),
        ("Save As packed to folder", true, Way::SaveAs(to_folder)),
        ("Save As folder to folder", false, Way::SaveAs(to_folder)),
    ];
    for (what, packed, way) in cases {
        let (dir, folder_doc) = talk(TWO_SLIDES);
        let (_packed_dir, file) = packed_talk(TWO_SLIDES);
        let (mut doc, from) = if packed {
            (Document::open_packed(&file, Fonts::bundled_only()).unwrap(), file.clone())
        } else {
            (folder_doc, dir.path().to_path_buf())
        };
        let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "Photo", red_png()).unwrap();
        assert_eq!(path, "assets/photo.png", "{what}");
        show(&mut doc, &path);
        let elsewhere = tempfile::tempdir().unwrap();

        let (job, saved_at) = match way {
            Way::Save => (doc.save_job(), from.clone()),
            Way::SaveAs(target) => {
                let target = target(elsewhere.path());
                let (SaveTarget::Folder(at) | SaveTarget::Packed(at)) = target.clone();
                (doc.save_as_job(target), at)
            }
        };
        let saved = job.run().unwrap();
        doc.saved(saved);

        assert!(!doc.has_unsaved_changes(), "{what}");
        assert!(doc.added_files().is_empty(), "{what}: no longer pending");
        assert_eq!(file_in(&saved_at, &path), Some(red_png()), "{what}: the bytes are exact");
        let mut fresh = open_fresh(&saved_at);
        assert_eq!(fresh.source(), doc.source(), "{what}");
        assert!(last_draws_red(&mut fresh), "{what}");
        // The open talk still draws it, and saves again.
        assert!(last_draws_red(&mut doc), "{what}");
        doc.add_slide("content").unwrap();
        save(&mut doc);
        assert_eq!(file_in(&saved_at, &path), Some(red_png()), "{what}: saved again");
    }
}

#[test]
fn a_file_added_and_undone_is_not_saved() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png()).unwrap();
    show(&mut doc, &path);
    doc.add_slide("content").unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.add_slide("content").unwrap();

    assert!(doc.added_files().is_empty());
    save(&mut doc);
    assert!(!dir.path().join("assets").exists());
}

#[test]
fn redo_after_undo_still_has_the_file() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png()).unwrap();
    show(&mut doc, &path);
    doc.undo().unwrap();
    doc.redo().unwrap();

    assert_eq!(doc.added_files().len(), 1);
    assert!(last_draws_red(&mut doc));
    save(&mut doc);
    assert_eq!(std::fs::read(dir.path().join(&path)).unwrap(), red_png());
    assert!(last_draws_red(&mut open_fresh(dir.path())));
}

#[test]
fn a_failed_save_keeps_the_file_pending_and_the_old_talk_whole() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png()).unwrap();
    show(&mut doc, &path);
    // Something that isn't a folder is where the pictures go.
    std::fs::write(dir.path().join("assets"), "not a folder").unwrap();
    let before = snapshot(dir.path());

    assert!(doc.save_job().run().is_err());

    assert_eq!(snapshot(dir.path()), before);
    assert!(doc.has_unsaved_changes());
    assert_eq!(doc.added_files().len(), 1);
    assert!(last_draws_red(&mut doc));

    std::fs::remove_file(dir.path().join("assets")).unwrap();
    save(&mut doc);
    assert_eq!(std::fs::read(dir.path().join(&path)).unwrap(), red_png());
    assert_eq!(read(&dir.path().join("slides.md")), doc.source());
}

#[test]
fn a_folder_save_never_writes_over_a_different_file() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png()).unwrap();
    show(&mut doc, &path);
    // Put there from outside the app since.
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join(&path), "someone else's").unwrap();
    let before = snapshot(dir.path());

    let problem = doc.save_job().run().err().expect("the save fails").to_string();

    assert!(problem.ends_with("This talk already has a different file with that name."), "{problem}");
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(doc.added_files().len(), 1);

    // The same bytes there already are the file.
    std::fs::write(dir.path().join(&path), red_png()).unwrap();
    save(&mut doc);
    assert_eq!(read(&dir.path().join("slides.md")), doc.source());
}

#[cfg(unix)]
#[test]
fn a_folder_save_never_writes_through_a_link() {
    let (dir, mut doc) = talk(TWO_SLIDES);
    let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png()).unwrap();
    show(&mut doc, &path);
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("assets")).unwrap();

    assert!(doc.save_job().run().is_err());

    assert!(std::fs::read_dir(elsewhere.path()).unwrap().next().is_none());
    assert_eq!(read(&dir.path().join("slides.md")), TWO_SLIDES);
}

#[test]
fn a_packed_save_never_replaces_a_different_entry() {
    let (_dir, file) = packed_talk(TWO_SLIDES);
    let mut doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png()).unwrap();
    show(&mut doc, &path);
    // The pack gets a different file of that name from outside the app.
    let theirs = vec![(path.clone(), b"someone else's".to_vec())];
    slides_engine::repack_with_slides(&file, &file, TWO_SLIDES, "test", &theirs).unwrap();
    let before = std::fs::read(&file).unwrap();

    let problem = doc.save_job().run().err().expect("the save fails").to_string();

    assert!(problem.ends_with("This talk already has a different file with that name."), "{problem}");
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert_eq!(open_packed(&file).unwrap().read(&path).unwrap(), b"someone else's");
    assert!(doc.has_unsaved_changes());
    assert_eq!(doc.added_files().len(), 1);
}
