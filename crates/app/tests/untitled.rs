//! A new presentation made from a template: untitled, held in memory, until its first save,
//! which writes all of it, in either form, so that it opens again as the same talk.

use std::path::{Path, PathBuf};

use proptest::prelude::*;
use slides_engine::{Fonts, TalkFiles as _, open_packed};
use slides_format::Order;
use stagepack_studio::added_files::FileKind;
use stagepack_studio::clipboard::{self, AppData, PastedFiles};
use stagepack_studio::document::{Document, TalkPlace, UntitledId};
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::templates::{Template, template, templates};

const SCORE_SOURCE: &str = include_str!("../../components/tests/fixtures/v1/score.rs");
const SCORE_BUILD: &[u8] = include_bytes!("../../components/tests/fixtures/v1/score.wasm");
const SCORE: &str = "<Score team=\"Reds\" points=\"3\" x=\"300\" y=\"300\" w=\"600\" h=\"300\" />";

fn photo() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/showcase/assets/photo.png")).unwrap()
}

/// Another picture, with bytes of its own: the photo with `mark` after its end.
fn other_photo(mark: &str) -> Vec<u8> {
    let mut bytes = photo();
    bytes.extend_from_slice(mark.as_bytes());
    bytes
}

fn new_talk(template: &Template) -> Document {
    Document::new_untitled(template, UntitledId::random(), Fonts::bundled_only()).unwrap()
}

/// The form a save writes: a folder, or a packed file.
fn target(dir: &Path, packed: bool) -> SaveTarget {
    if packed {
        SaveTarget::Packed(dir.join("talk.sptx"))
    } else {
        SaveTarget::Folder(dir.join("talk"))
    }
}

fn path_of(target: &SaveTarget) -> PathBuf {
    match target {
        SaveTarget::Folder(path) | SaveTarget::Packed(path) => path.clone(),
    }
}

fn reopen(at: &Path) -> Document {
    if at.extension().is_some_and(|ext| ext == "sptx") {
        Document::open_packed(at, Fonts::bundled_only()).unwrap()
    } else {
        Document::open(at, Fonts::bundled_only()).unwrap()
    }
}

/// Saves `document` at `target`, as the window does, and says whether it worked.
fn save_as(document: &mut Document, target: SaveTarget) -> Result<(), String> {
    let saved = document.save_as_job(target).run().map_err(|error| error.to_string())?;
    document.saved(saved);
    Ok(())
}

/// Score as a copy of it from the fixture talk brings it: its source, and its build with the
/// hash of the source it was built from.
fn copied_score() -> (String, PastedFiles) {
    let folder = tempfile::tempdir().unwrap();
    for sub in ["components", "build"] {
        std::fs::create_dir(folder.path().join(sub)).unwrap();
    }
    std::fs::write(folder.path().join("slides.md"), format!("# From\n\n{SCORE}\n")).unwrap();
    std::fs::write(folder.path().join("components/score.rs"), SCORE_SOURCE).unwrap();
    std::fs::write(folder.path().join("build/score.wasm"), SCORE_BUILD).unwrap();
    let hash = slides_components::source_hash(SCORE_SOURCE.as_bytes());
    let index = format!(r#"{{"format":1,"components":[{{"tag":"Score","file":"score","source_hash":"{hash}"}}]}}"#);
    std::fs::write(folder.path().join("build/index.json"), index).unwrap();
    let mut from = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let at = from.source().find("<Score").unwrap();
    assert!(from.select(0, at..at + SCORE.len()));
    let (text, files) = from.copy_selected().unwrap();
    let entry = clipboard::element_entry(&text, &files).unwrap();
    match clipboard::read_app_data(entry.metadata.as_deref()) {
        AppData::Element(carried) => (carried.markdown, carried.files),
        _ => panic!("element data"),
    }
}

/// Every slide's layers, as pixels and where they go, drawn small.
fn drawing(document: &mut Document) -> Vec<Vec<(i32, i32, Vec<u8>)>> {
    (0..document.slide_count())
        .map(|slide| {
            document
                .layers_of(slide, 0.25)
                .iter()
                .map(|layer| (layer.x, layer.y, layer.pixmap.data().to_vec()))
                .collect()
        })
        .collect()
}

/// Every file under `root`, by its path from there.
fn files_under(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// The bytes of the file at `path` in the talk saved at `at`, in either form.
fn saved_file(at: &Path, path: &str) -> Option<Vec<u8>> {
    if at.extension().is_some_and(|ext| ext == "sptx") {
        open_packed(at).unwrap().read(path)
    } else {
        std::fs::read(at.join(path)).ok()
    }
}

#[test]
fn a_new_talk_from_each_template_is_not_edited_yet() {
    for template in templates() {
        let document = new_talk(template);
        assert_eq!(document.source(), template.slides, "{}", template.id);
        assert!(!document.has_unsaved_changes(), "{}", template.id);
        assert!(document.is_untitled(), "{}", template.id);
        assert!(matches!(document.place(), TalkPlace::Untitled(_)));
        assert!(!document.is_packed());
        assert!(!document.can_undo());
        for (path, bytes) in template.files {
            assert!(document.has_file(path), "{}: {path}", template.id);
            assert_eq!(document.export_input().files.read(path).as_deref(), Some(*bytes));
        }
    }
}

#[test]
fn an_untitled_talks_name_and_number() {
    let mut document = new_talk(template("light").unwrap());
    assert_eq!(document.name(), "Untitled presentation");
    assert_eq!(document.title(), "Untitled presentation · 1 / 1");
    document.set_untitled_number(2);
    assert_eq!(document.name(), "Untitled presentation 2");
    document.set_untitled_number(1);
    assert_eq!(document.name(), "Untitled presentation");
    assert_eq!(TalkPlace::Untitled(UntitledId(7)).name(), "Untitled presentation");

    // Its id is random, and written as 32 hex digits.
    let (a, b) = (UntitledId::random(), UntitledId::random());
    assert_ne!(a, b);
    assert_eq!(UntitledId(0xab).to_string(), format!("{:032x}", 0xab));
    assert_eq!(a.to_string().len(), 32);

    // Saved, it's called what its place is.
    let dir = tempfile::tempdir().unwrap();
    document.set_untitled_number(3);
    save_as(&mut document, SaveTarget::Folder(dir.path().join("My talk"))).unwrap();
    assert_eq!(document.name(), "My talk");
    assert!(!document.is_untitled());
}

#[test]
fn an_untitled_talk_with_no_place_is_never_saved_where_it_is() {
    let mut document = new_talk(template("light").unwrap());
    document.add_slide("content").unwrap();
    assert!(document.save_job().run().is_err());
    assert!(document.is_untitled() && document.has_unsaved_changes());
}

#[test]
fn an_untitled_talk_brought_back_is_unsaved_with_its_files() {
    let text = "# Back\n\n![](assets/photo.png)\n".to_string();
    let files = vec![("assets/photo.png".to_string(), std::sync::Arc::from(photo()))];
    let id = UntitledId::random();
    let document = Document::untitled_with(id, text.clone(), files, Vec::new(), Fonts::bundled_only()).unwrap();
    assert_eq!(document.source(), text);
    assert!(document.has_unsaved_changes());
    assert!(document.is_untitled());
    assert!(matches!(document.place(), TalkPlace::Untitled(back) if *back == id));
    assert!(document.has_file("assets/photo.png"));
}

#[test]
fn the_first_save_writes_all_the_talk_uses() {
    let (markdown, files) = copied_score();
    for template in templates() {
        for packed in [false, true] {
            let what = format!("{}, packed {packed}", template.id);
            let mut document = new_talk(template);
            let added = document.add_file(FileKind::Picture, "added.png", other_photo("added")).unwrap();
            document.insert_tag("Insert picture", "Picture", &[("src", &added)], None).unwrap();
            let removed = document.add_file(FileKind::Picture, "removed.png", other_photo("removed")).unwrap();
            document.insert_tag("Insert picture", "Picture", &[("src", &removed)], None).unwrap();
            document.undo().unwrap();
            assert!(!document.source().contains(&removed), "{what}");
            document.paste_element(&markdown, files.clone()).unwrap();
            assert!(document.notices().is_empty(), "{what}: {:?}", document.notices());

            let dir = tempfile::tempdir().unwrap();
            let target = target(dir.path(), packed);
            let at = path_of(&target);
            let text = document.source().to_string();
            let drawn = drawing(&mut document);
            save_as(&mut document, target).unwrap_or_else(|problem| panic!("{what}: {problem}"));
            assert!(!document.is_untitled(), "{what}");
            assert!(!document.has_unsaved_changes(), "{what}");
            assert_eq!(document.is_packed(), packed, "{what}");

            let mut reopened = reopen(&at);
            assert_eq!(reopened.source().as_bytes(), text.as_bytes(), "{what}");
            assert!(reopened.notices().is_empty(), "{what}: {:?}", reopened.notices());
            assert!(drawing(&mut reopened) == drawn, "{what}: the slides draw alike");
            assert!(saved_file(&at, &removed).is_none(), "{what}: the picture taken away isn't written");
            assert_eq!(saved_file(&at, &added), Some(other_photo("added")), "{what}");
            for (path, bytes) in template.files {
                assert_eq!(saved_file(&at, path).as_deref(), Some(*bytes), "{what}: {path}");
            }
            assert_eq!(saved_file(&at, "components/score.rs"), Some(SCORE_SOURCE.as_bytes().to_vec()), "{what}");
            assert_eq!(saved_file(&at, "build/score.wasm"), Some(SCORE_BUILD.to_vec()), "{what}");
            let index: serde_json::Value = serde_json::from_slice(&saved_file(&at, "build/index.json").expect("the index")).unwrap();
            assert_eq!(index["components"][0]["tag"], "Score", "{what}");
            if !packed {
                let mut expected = vec!["slides.md".to_string(), added.clone(), "build/index.json".into(), "build/score.wasm".into(), "components/score.rs".into()];
                expected.extend(template.files.iter().map(|(path, _)| path.to_string()));
                expected.sort();
                assert_eq!(files_under(&at), expected, "{what}: nothing else is written");
            }
            // Saved again where it is now, it's an ordinary talk.
            document.add_slide("content").unwrap();
            let saved = document.save_job().run().unwrap();
            document.saved(saved);
            assert_eq!(reopen(&at).source(), document.source(), "{what}");
        }
    }
}

#[test]
fn a_template_picture_the_text_no_longer_uses_is_not_written() {
    let tour = template("tour").unwrap();
    let mut document = new_talk(tour);
    let text = document.source().replace("assets/photo.png", "assets/missing.png");
    document.recover(text).unwrap();
    for packed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let target = target(dir.path(), packed);
        let at = path_of(&target);
        let mut copy = Document::untitled_with(UntitledId::random(), document.source().to_string(), Vec::new(), Vec::new(), Fonts::bundled_only()).unwrap();
        save_as(&mut copy, target).unwrap();
        assert!(saved_file(&at, "assets/photo.png").is_none());
    }
    let dir = tempfile::tempdir().unwrap();
    save_as(&mut document, SaveTarget::Folder(dir.path().join("talk"))).unwrap();
    assert!(!dir.path().join("talk/assets/photo.png").exists());
}

/// The ways a first save can fail: the target, and how it's made unwritable.
fn failing_targets(dir: &Path, packed: bool) -> Vec<(&'static str, SaveTarget)> {
    let not_empty = dir.join("not-empty");
    std::fs::create_dir_all(&not_empty).unwrap();
    std::fs::write(not_empty.join("someone's.txt"), "theirs").unwrap();
    let a_file = dir.join("a-file");
    std::fs::write(&a_file, "a file").unwrap();
    let read_only = dir.join("read-only");
    std::fs::create_dir_all(&read_only).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let name = if packed { "talk.sptx" } else { "talk" };
    let mut targets = vec![
        ("under a file", if packed { SaveTarget::Packed(a_file.join(name)) } else { SaveTarget::Folder(a_file.join(name)) }),
        ("in a read-only folder", if packed { SaveTarget::Packed(read_only.join(name)) } else { SaveTarget::Folder(read_only.join(name)) }),
    ];
    if !packed {
        targets.push(("a folder that isn't empty", SaveTarget::Folder(not_empty)));
    }
    targets
}

/// Whether the system lets this test's own files be written whatever their permissions, as it
/// does for an administrator.
fn permissions_are_ignored(dir: &Path) -> bool {
    let probe = dir.join("read-only/probe");
    let written = std::fs::write(&probe, "x").is_ok();
    std::fs::remove_file(&probe).ok();
    written
}

#[test]
fn a_save_that_fails_leaves_it_untitled_and_whole() {
    for packed in [false, true] {
        let mut document = new_talk(template("tour").unwrap());
        document.insert_picture("added.png", photo()).unwrap();
        let text = document.source().to_string();
        let dir = tempfile::tempdir().unwrap();
        for (how, target) in failing_targets(dir.path(), packed) {
            if how == "in a read-only folder" && permissions_are_ignored(dir.path()) {
                continue;
            }
            let what = format!("{how}, packed {packed}");
            let before = files_under(dir.path());
            assert!(save_as(&mut document, target).is_err(), "{what}");
            assert!(document.is_untitled(), "{what}");
            assert!(document.has_unsaved_changes(), "{what}");
            assert_eq!(document.source(), text, "{what}");
            assert_eq!(files_under(dir.path()), before, "{what}: nothing is left behind");
        }
        // Somewhere it can be written, it is.
        let elsewhere = tempfile::tempdir().unwrap();
        let target = target(elsewhere.path(), packed);
        let at = path_of(&target);
        save_as(&mut document, target).unwrap();
        assert_eq!(reopen(&at).source(), text);
        assert!(saved_file(&at, "assets/photo.png").is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(dir.path().join("read-only"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

#[test]
fn saving_a_thirty_megabyte_picture_works() {
    let mut big = photo();
    big.resize(30 * 1024 * 1024, 0);
    for packed in [false, true] {
        let mut document = new_talk(template("light").unwrap());
        let path = document.add_file(FileKind::Picture, "big.png", big.clone()).unwrap();
        document.insert_tag("Insert picture", "Picture", &[("src", &path)], None).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let target = target(dir.path(), packed);
        let at = path_of(&target);
        save_as(&mut document, target).unwrap();
        assert!(saved_file(&at, &path) == Some(big.clone()), "packed {packed}");
        assert_eq!(reopen(&at).source(), document.source());
    }
}

/// One edit a person might make on a new presentation.
#[derive(Clone, Debug)]
enum Edit {
    Type(String),
    TextBox,
    Shape,
    AddSlide,
    ToBack,
    Picture,
    Undo,
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        "[a-zA-Z ]{1,12}".prop_map(Edit::Type),
        Just(Edit::TextBox),
        Just(Edit::Shape),
        Just(Edit::AddSlide),
        Just(Edit::ToBack),
        Just(Edit::Picture),
        Just(Edit::Undo),
    ]
}

fn make(document: &mut Document, edit: &Edit) {
    // Whatever's refused leaves the talk as it was, which the save then writes.
    match edit {
        Edit::Type(words) => {
            let slide = document.current();
            if document.begin_text_in_region(slide, "title") {
                document.apply_text(|text| text.insert(words)).ok();
            }
        }
        Edit::TextBox => {
            document.insert_text_box().ok();
        }
        Edit::Shape => {
            document.insert_shape("rect").ok();
        }
        Edit::AddSlide => {
            document.add_slide("content").ok();
        }
        Edit::ToBack => {
            document.reorder_selected(Order::Back).ok();
        }
        Edit::Picture => {
            document.insert_picture("photo.png", photo()).ok();
        }
        Edit::Undo => {
            document.undo().ok();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn any_edits_then_a_first_save_reopen_alike(
        which in 0..5usize,
        edits in prop::collection::vec(edit(), 0..12),
        packed in any::<bool>(),
    ) {
        let mut document = new_talk(&templates()[which]);
        for edit in &edits {
            make(&mut document, edit);
        }
        let text = document.source().to_string();
        let dir = tempfile::tempdir().unwrap();
        let target = target(dir.path(), packed);
        let at = path_of(&target);
        save_as(&mut document, target).unwrap();
        let mut reopened = reopen(&at);
        prop_assert_eq!(reopened.source().as_bytes(), text.as_bytes());
        prop_assert!(reopened.notices().is_empty(), "{:?}", reopened.notices());
    }
}
