use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_engine::{Fonts, Layer, pack_folder};
use stagepack_studio::document::{Document, EditRefused, OpenProblem, Reload};
use stagepack_studio::export::{ExportKind, run_export};
use stagepack_studio::saving::SaveTarget;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

fn talk(source: &str) -> (tempfile::TempDir, Document) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    let doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    (dir, doc)
}

#[test]
fn next_and_previous_stop_at_the_ends() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    doc.previous();
    assert_eq!(doc.current(), 0);
    doc.next();
    doc.next();
    assert_eq!(doc.current(), 1);
}

#[test]
fn a_folder_with_no_slides_file_says_so() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Document::open(dir.path(), Fonts::bundled_only())
            .err()
            .unwrap()
            .contains("slides.md")
    );
}

#[test]
fn the_title_says_where_you_are() {
    let (_d, mut doc) = talk("---\ntitle: Review\n---\n\n# One\n\n---\n\n# Two\n");
    doc.next();
    assert_eq!(doc.title(), "Review · 2 / 2");
}

#[test]
fn with_no_title_setting_the_title_is_the_folders_name() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("quarterly");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), "# One\n").unwrap();
    let doc = Document::open(&folder, Fonts::bundled_only()).unwrap();
    assert_eq!(doc.title(), "quarterly · 1 / 1");
}

#[test]
fn a_path_ending_in_dots_still_names_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("quarterly");
    std::fs::create_dir_all(folder.join("inner")).unwrap();
    std::fs::write(folder.join("slides.md"), "# One\n").unwrap();
    let doc = Document::open(&folder.join("inner/.."), Fonts::bundled_only()).unwrap();
    assert_eq!(doc.title(), "quarterly · 1 / 1");
}

#[test]
fn the_source_is_the_text_of_the_file() {
    let (_d, doc) = talk("# One\n\n---\n\n# Two\n");
    assert_eq!(doc.source(), "# One\n\n---\n\n# Two\n");
}

#[test]
fn an_empty_file_is_one_empty_slide() {
    let (_d, mut doc) = talk("");
    assert_eq!(doc.slide_count(), 1);
    assert_eq!(doc.layers(1.0).len(), 1, "only the background");
}

#[test]
fn layers_are_the_current_slides() {
    let (_d, mut doc) = talk(
        "# One\n\n---\n\n# Two\n\n<Shape kind=\"ellipse\" x=\"0\" y=\"0\" w=\"10\" h=\"10\" />\n",
    );
    let first = doc.layers(1.0);
    doc.next();
    let second = doc.layers(1.0);
    assert_eq!(first.len(), 2, "background and title");
    assert_eq!(second.len(), 3, "background, title and shape");
}

#[test]
fn notices_are_the_current_slides() {
    let (_d, mut doc) = talk("# One\n\n---\nlayout: nope\n---\n\n# Two\n");
    assert!(doc.notices().is_empty());
    doc.next();
    assert_eq!(doc.notices().len(), 1);
    assert!(doc.notices()[0].contains("nope"));
}

/// A bundled monospace font under a family name that only a talk can bring.
const TALK_FAMILY: &str = "Talk Font Mono";

fn font_only_a_talk_has() -> Vec<u8> {
    let mut font = include_bytes!("../../engine/assets/fonts/JetBrainsMono-Regular.ttf").to_vec();
    let from = "JetBrains Mono";
    assert_eq!(
        from.len(),
        TALK_FAMILY.len(),
        "a same-length name leaves the font's tables in place"
    );
    for (old, new) in [
        (from.as_bytes().to_vec(), TALK_FAMILY.as_bytes().to_vec()),
        (
            from.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            TALK_FAMILY
                .encode_utf16()
                .flat_map(u16::to_be_bytes)
                .collect(),
        ),
    ] {
        let mut at = 0;
        while let Some(found) = font[at..].windows(old.len()).position(|bytes| bytes == old) {
            font[at + found..at + found + new.len()].copy_from_slice(&new);
            at += found + new.len();
        }
    }
    font
}

/// The text layer of a talk that asks for the talk's own font, drawn from `folder`.
fn drawn_title(folder: &Path) -> Arc<Layer> {
    std::fs::write(
        folder.join("slides.md"),
        format!("---\nfont: {TALK_FAMILY}\n---\n\n# Hello\n"),
    )
    .unwrap();
    let mut doc = Document::open(folder, Fonts::bundled_only()).unwrap();
    let layers = doc.layers(1.0);
    assert_eq!(layers.len(), 2, "background and title");
    layers[1].clone()
}

fn same_pixels(a: &Layer, b: &Layer) -> bool {
    a.pixmap.width() == b.pixmap.width()
        && a.pixmap.height() == b.pixmap.height()
        && a.pixmap.data() == b.pixmap.data()
}

fn talk_folder() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir_all(folder.join("assets/fonts")).unwrap();
    (dir, folder)
}

#[test]
fn fonts_in_the_talks_folder_are_used() {
    let (_plain_dir, plain) = talk_folder();
    let without = drawn_title(&plain);
    for name in ["brought.ttf", "brought.otf", "BROUGHT.TTF"] {
        let (_dir, folder) = talk_folder();
        std::fs::write(
            folder.join("assets/fonts").join(name),
            font_only_a_talk_has(),
        )
        .unwrap();
        assert!(
            !same_pixels(&drawn_title(&folder), &without),
            "{name} was not loaded"
        );
    }
}

#[test]
fn other_files_in_the_font_folder_are_not_fonts() {
    let (_plain_dir, plain) = talk_folder();
    let without = drawn_title(&plain);
    let (_dir, folder) = talk_folder();
    std::fs::write(
        folder.join("assets/fonts/brought.txt"),
        font_only_a_talk_has(),
    )
    .unwrap();
    assert!(same_pixels(&drawn_title(&folder), &without));
}

#[test]
fn a_broken_font_does_not_stop_the_talk() {
    let (_dir, folder) = talk_folder();
    std::fs::write(folder.join("assets/fonts/broken.ttf"), b"not a font").unwrap();
    drawn_title(&folder);
}

#[cfg(unix)]
#[test]
fn a_font_link_cannot_reach_outside_the_talk() {
    let (_plain_dir, plain) = talk_folder();
    let without = drawn_title(&plain);
    let (dir, folder) = talk_folder();
    let outside = dir.path().join("outside.ttf");
    std::fs::write(&outside, font_only_a_talk_has()).unwrap();
    std::os::unix::fs::symlink(&outside, folder.join("assets/fonts/link.ttf")).unwrap();
    assert!(same_pixels(&drawn_title(&folder), &without));
}

#[test]
fn saving_a_change_reports_the_slide() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    assert!(
        matches!(doc.reload(Some("# One\n\n---\n\n# Changed\n".into())), Reload::Changed(s) if s == vec![1])
    );
}

#[test]
fn saving_the_same_bytes_changes_nothing() {
    let (_d, mut doc) = talk("# One\n");
    assert!(matches!(
        doc.reload(Some("# One\n".into())),
        Reload::Unchanged
    ));
}

fn one_colour_svg(color: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><rect width=\"8\" height=\"8\" fill=\"{color}\"/></svg>"
    )
}

#[test]
fn a_replaced_picture_counts_as_a_change() {
    let source = "![](photo.svg)\n";
    let (dir, mut doc) = talk(source);
    let picture = dir.path().join("photo.svg");
    std::fs::write(&picture, one_colour_svg("red")).unwrap();
    doc.layers(1.0);

    // A different length, so that it's a different file however soon it's written.
    std::fs::write(&picture, one_colour_svg("blue")).unwrap();
    assert!(matches!(doc.reload(Some(source.into())), Reload::Changed(s) if s == vec![0]));
    assert!(
        matches!(doc.reload(Some(source.into())), Reload::Unchanged),
        "nothing has changed since"
    );
}

#[test]
fn empty_read_keeps_the_last_good_talk() {
    let (_d, mut doc) = talk("# One\n");
    assert!(matches!(
        doc.reload(Some(String::new())),
        Reload::KeptLastGood
    ));
    assert!(matches!(doc.reload(None), Reload::KeptLastGood));
    assert_eq!(doc.source(), "# One\n");
}

#[test]
fn reload_keeps_your_place() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    doc.next();
    doc.reload(Some("# One\n\n---\n\n# Two!\n".into()));
    assert_eq!(doc.current(), 1);
}

#[test]
fn removing_slides_moves_you_to_the_last_one() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n\n---\n\n# Three\n");
    doc.next();
    doc.next();
    doc.reload(Some("# One\n".into()));
    assert_eq!(doc.current(), 0);
}

#[test]
fn being_moved_to_a_slide_that_did_not_change_reports_it() {
    // The first slide's text is exactly as it was, but it's now the one showing.
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    doc.next();
    assert!(matches!(doc.reload(Some("# One\n\n".into())), Reload::Changed(s) if s == vec![0]));
    assert_eq!(doc.current(), 0);
}

#[test]
fn any_slide_can_be_asked_for_by_number() {
    let (_d, mut doc) = talk(
        "# One\n\n---\n\n# Two\n\n<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"2\" />\n\n<Notes>\nSay this\n</Notes>\n",
    );
    assert_eq!(doc.layers_of(0, 1.0).len(), 2, "background and title");
    assert_eq!(
        doc.layers_of(1, 1.0).len(),
        3,
        "background, title and shape"
    );
    assert_eq!(doc.timeline_of(0).steps, 0);
    assert_eq!(doc.timeline_of(1).steps, 2);
    assert_eq!(doc.notes_of(0), None);
    assert_eq!(doc.notes_of(1).as_deref(), Some("Say this"));
    assert_eq!(doc.current(), 0, "asking doesn't move you");
}

const TWO_SLIDES: &str = "---\ntitle: Review\n---\n\n# One\n\n---\n\n# Two\n";

/// `source` packed into a file called `name`, in a folder that lasts as long as the first value.
fn packed_talk(name: &str, source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), source).unwrap();
    let file = dir.path().join(name);
    pack_folder(&folder, &file, "test").unwrap();
    (dir, file)
}

#[test]
fn a_packed_talk_opens() {
    let (_dir, file) = packed_talk("review.sptx", TWO_SLIDES);
    let doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    assert_eq!(doc.title(), "Review · 1 / 2");
    assert_eq!(doc.slide_count(), 2);
    assert!(doc.is_packed());
}

#[test]
fn a_talk_in_a_folder_is_not_packed() {
    let (_d, doc) = talk("# One\n");
    assert!(!doc.is_packed());
}

#[test]
fn with_no_title_setting_a_packed_talk_goes_by_its_files_name() {
    let (_dir, file) = packed_talk("quarterly.sptx", "# One\n");
    let doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    assert_eq!(doc.title(), "quarterly · 1 / 1");
}

#[test]
fn a_stagepack_file_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slidev.sptx");
    let mut zip = zip::ZipWriter::new(File::create(&file).unwrap());
    zip.start_file("stagepack.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.finish().unwrap();
    assert_eq!(
        Document::open_packed(&file, Fonts::bundled_only()).err(),
        Some(OpenProblem::SlidevTalk)
    );
}

#[test]
fn a_file_that_is_not_a_talk_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "just some notes").unwrap();
    assert!(matches!(
        Document::open_packed(&file, Fonts::bundled_only()).err(),
        Some(OpenProblem::Other(problem)) if !problem.is_empty()
    ));
}

#[test]
fn saving_a_packed_copy_then_opening_it_shows_the_same_talk() {
    let (_d, doc) = talk(TWO_SLIDES);
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("copy.sptx");
    let saved = doc.save_as_job(SaveTarget::Packed(out.clone())).run().unwrap();
    assert!(saved.notices().is_empty());
    let copy = Document::open_packed(&out, Fonts::bundled_only()).unwrap();
    assert_eq!(copy.source(), TWO_SLIDES);
}

#[cfg(unix)]
#[test]
fn saving_a_packed_copy_says_what_was_left_out() {
    let (dir, doc) = talk("# One\n");
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::os::unix::fs::symlink("/nowhere", dir.path().join("assets/link.png")).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let saved = doc
        .save_as_job(SaveTarget::Packed(elsewhere.path().join("copy.sptx")))
        .run()
        .unwrap();
    let notices = saved.notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("assets/link.png"), "{notices:?}");
}

#[test]
fn saving_a_packed_copy_names_a_picture_it_could_not_take() {
    let (dir, doc) = talk("![](pics/photo.jpg)\n");
    std::fs::create_dir(dir.path().join("pics")).unwrap();
    std::fs::write(dir.path().join("pics/photo.jpg"), "a picture").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let saved = doc.save_as_job(SaveTarget::Packed(elsewhere.path().join("copy.sptx"))).run().unwrap();
    assert_eq!(saved.notices(), ["The picture pics/photo.jpg is outside assets/ and wasn't packed"]);
}

#[test]
fn a_mistyped_path_is_named_in_what_goes_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-talk");
    let problem = Document::open_packed(&missing, Fonts::bundled_only()).err();
    assert!(
        matches!(&problem, Some(OpenProblem::Other(problem)) if problem.contains(&missing.display().to_string())),
        "{problem:?}"
    );
}

#[test]
fn an_export_shares_the_talks_fonts_instead_of_copying_them() {
    let (dir, _) = talk("# One\n");
    let fonts = dir.path().join("assets/fonts");
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("a.ttf"), font_only_a_talk_has()).unwrap();
    let doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let (first, second) = (doc.export_input(), doc.export_input());
    assert!(Arc::ptr_eq(&first.talk_fonts[0], &second.talk_fonts[0]));
}

#[test]
fn a_packed_talk_can_be_saved_as_a_folder() {
    let (dir, file) = packed_talk("talk.sptx", "# One\n");
    let doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    let folder = dir.path().join("unpacked");
    doc.save_as_job(SaveTarget::Folder(folder.clone())).run().unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join("slides.md")).unwrap(),
        "# One\n"
    );
}

#[test]
fn saving_over_a_folder_that_holds_things_says_why_and_leaves_them() {
    let (dir, file) = packed_talk("talk.sptx", "# One\n");
    let doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    let folder = dir.path().join("in-use");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("keep.txt"), "mine").unwrap();
    assert!(doc.save_as_job(SaveTarget::Folder(folder.clone())).run().is_err());
    assert_eq!(
        std::fs::read_to_string(folder.join("keep.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn a_talk_can_be_saved_as_a_copy_in_the_form_it_already_has() {
    let elsewhere = tempfile::tempdir().unwrap();
    let (_d, in_a_folder) = talk("# One\n");
    let copy = elsewhere.path().join("a");
    in_a_folder.save_as_job(SaveTarget::Folder(copy.clone())).run().unwrap();
    assert_eq!(std::fs::read_to_string(copy.join("slides.md")).unwrap(), "# One\n");
    let (_dir, file) = packed_talk("talk.sptx", "# One\n");
    let packed = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    let out = elsewhere.path().join("b.sptx");
    packed.save_as_job(SaveTarget::Packed(out.clone())).run().unwrap();
    assert_eq!(Document::open_packed(&out, Fonts::bundled_only()).unwrap().source(), "# One\n");
}

/// The text layer of a talk that asks for the talk's own font, packed from `folder` and drawn.
fn drawn_title_when_packed(folder: &Path) -> Arc<Layer> {
    drawn_title(folder);
    let file = folder.with_extension("sptx");
    pack_folder(folder, &file, "test").unwrap();
    let mut doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    let layers = doc.layers(1.0);
    assert_eq!(layers.len(), 2, "background and title");
    layers[1].clone()
}

#[test]
fn fonts_in_a_packed_talk_are_used() {
    let (_plain_dir, plain) = talk_folder();
    let without = drawn_title_when_packed(&plain);
    let (_dir, folder) = talk_folder();
    std::fs::write(
        folder.join("assets/fonts/brought.ttf"),
        font_only_a_talk_has(),
    )
    .unwrap();
    assert!(!same_pixels(&drawn_title_when_packed(&folder), &without));
}

#[test]
fn only_font_files_right_in_the_font_folder_of_a_pack_are_fonts() {
    let (_plain_dir, plain) = talk_folder();
    let without = drawn_title_when_packed(&plain);
    let (_dir, folder) = talk_folder();
    std::fs::create_dir(folder.join("assets/fonts/deeper")).unwrap();
    for name in ["deeper/brought.ttf", "brought.txt"] {
        std::fs::write(
            folder.join("assets/fonts").join(name),
            font_only_a_talk_has(),
        )
        .unwrap();
    }
    assert!(same_pixels(&drawn_title_when_packed(&folder), &without));
}

#[test]
fn fonts_past_what_a_talk_can_bring_are_left_out() {
    let (_dir, folder) = talk_folder();
    std::fs::write(folder.join("assets/fonts/a.ttf"), font_only_a_talk_has()).unwrap();
    // Sparse, so that this costs the test some memory for a moment and no disk. Each is small
    // enough to read, and together they're more than a talk can bring.
    for name in ["b.ttf", "c.ttf"] {
        let big = File::create(folder.join("assets/fonts").join(name)).unwrap();
        big.set_len(129 * 1024 * 1024).unwrap();
    }
    drawn_title(&folder);
    let doc = Document::open(&folder, Fonts::bundled_only()).unwrap();
    let talk_fonts = doc.export_input().talk_fonts;
    assert_eq!(talk_fonts.len(), 2);
    assert_eq!(talk_fonts[0], Arc::new(font_only_a_talk_has()));
}

#[test]
fn a_slides_file_over_the_limit_is_refused_and_the_folder_named() {
    let dir = tempfile::tempdir().unwrap();
    let huge = File::create(dir.path().join("slides.md")).unwrap();
    // Sparse, so the test costs no disk.
    huge.set_len(slides_engine::MAX_FILE_BYTES + 1).unwrap();
    let Err(problem) = Document::open(dir.path(), Fonts::bundled_only()) else {
        panic!("a file that size should not open");
    };
    assert!(problem.contains(&dir.path().display().to_string()), "{problem}");
    assert!(problem.contains("limit"), "{problem}");
}

#[test]
fn an_export_takes_the_talks_own_fonts_along() {
    let (dir, folder) = talk_folder();
    std::fs::write(
        folder.join("assets/fonts/brought.ttf"),
        font_only_a_talk_has(),
    )
    .unwrap();
    drawn_title(&folder);
    let doc = Document::open(&folder, Fonts::bundled_only()).unwrap();

    let out = dir.path().join("out.pptx");
    run_export(doc.export_input(), ExportKind::PowerPoint, &out).unwrap();

    let archive = ZipArchive::new(File::open(&out).unwrap()).unwrap();
    assert!(
        archive
            .file_names()
            .filter_map(Result::ok)
            .any(|name| name.starts_with("ppt/fonts/")),
        "the talk's font was embedded"
    );
}

#[test]
fn an_export_of_a_packed_talk_reads_the_pack() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::write(folder.join("assets/photo.svg"), one_colour_svg("red")).unwrap();
    std::fs::write(folder.join("slides.md"), "![](assets/photo.svg)\n").unwrap();
    let file = dir.path().join("talk.sptx");
    pack_folder(&folder, &file, "test").unwrap();
    let doc = Document::open_packed(&file, Fonts::bundled_only()).unwrap();

    let out = dir.path().join("out.pptx");
    let notices = run_export(doc.export_input(), ExportKind::PowerPoint, &out).unwrap();

    assert!(notices.is_empty(), "the picture was found: {notices:?}");
    let archive = ZipArchive::new(File::open(&out).unwrap()).unwrap();
    assert!(
        archive
            .file_names()
            .filter_map(Result::ok)
            .any(|name| name.starts_with("ppt/media/")),
        "the picture is in the file"
    );
}

/// An edit to make, and what Undo calls it.
type NamedEdit = (&'static str, fn(&mut Document));

const FOUR_SLIDES: &str = "---\r\ntitle: Review\r\n---\r\n\r\n# One\r\n\r\n---\r\n\r\n# Two  \r\n\r\n---\r\n\r\n# Three\r\n\r\n---\r\n\r\n# Four\r\n";

#[test]
fn undo_goes_back_through_every_kind_of_change() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    let mut texts = vec![doc.source().to_string()];
    let mut labels = Vec::new();
    let steps: [NamedEdit; 6] = [
        ("Add slide", |doc| drop(doc.add_slide("title").unwrap())),
        ("Duplicate slide", |doc| drop(doc.duplicate_current().unwrap())),
        ("Move slide", |doc| drop(doc.move_slide(0, 3).unwrap())),
        ("Hide slide", |doc| drop(doc.set_current_hidden(true).unwrap())),
        ("Edit notes", |doc| drop(doc.set_notes(1, "Say this").unwrap())),
        ("Delete slide", |doc| drop(doc.delete_current().unwrap())),
    ];
    for (label, step) in steps {
        step(&mut doc);
        assert_ne!(doc.source(), texts.last().unwrap(), "{label} changed nothing");
        assert_eq!(doc.undo_label(), Some(label));
        texts.push(doc.source().to_string());
        labels.push(label);
    }
    assert!(doc.has_unsaved_changes());
    for (text, label) in texts.iter().rev().skip(1).zip(labels.iter().rev()) {
        assert_eq!(doc.undo_label(), Some(*label));
        assert!(doc.undo().is_some());
        assert_eq!(doc.source(), text, "after undoing {label}");
        assert_eq!(doc.redo_label(), Some(*label));
    }
    assert!(!doc.can_undo());
    assert!(!doc.has_unsaved_changes(), "back to the text as it was opened");
    for (text, label) in texts.iter().skip(1).zip(&labels) {
        assert!(doc.redo().is_some());
        assert_eq!(doc.source(), text, "after redoing {label}");
    }
    assert!(!doc.can_redo());
    assert!(doc.undo().is_some());
    assert_eq!(doc.source(), texts[texts.len() - 2]);
}

#[test]
fn the_current_slide_follows_what_changed() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    doc.select(2);
    doc.delete_current().unwrap();
    doc.select(0);
    doc.undo().unwrap();
    assert_eq!(doc.current(), 2);
    assert!(doc.notes_of(2).is_none());

    doc.move_slide(3, 1).unwrap();
    assert_eq!(doc.current(), 1, "where the slide went");
    doc.select(0);
    doc.undo().unwrap();
    assert_eq!(doc.current(), 3, "where the slide came back to");
    doc.redo().unwrap();
    assert_eq!(doc.current(), 1);
}

#[test]
fn adding_a_slide_makes_it_the_current_one() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    doc.select(1);
    let shown = doc.add_slide("section").unwrap();
    assert_eq!(doc.current(), 2);
    assert_eq!(doc.slide_count(), 5);
    assert!(shown.contains(&2), "{shown:?}");
    assert!(doc.source().contains("# Two  \r\n\r\n---\r\nlayout: section\r\n---\r\n"), "{:?}", doc.source());
    assert_eq!(doc.talk().slides[2].settings.as_ref().unwrap().get("layout"), Some("section"));
    doc.undo().unwrap();
    assert_eq!(doc.source(), FOUR_SLIDES);
    assert_eq!(doc.current(), 1, "the slide it was added after");
}

#[test]
fn a_content_slide_needs_no_settings() {
    let (_d, mut doc) = talk("# One\n");
    doc.add_slide("content").unwrap();
    assert_eq!(doc.source(), "# One\n---\n# \n");
    assert!(doc.talk().slides[1].settings.is_none());
}

#[test]
fn deleting_the_last_slide_moves_to_the_one_before() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    doc.select(3);
    doc.delete_current().unwrap();
    assert_eq!(doc.current(), 2);
    assert_eq!(doc.slide_count(), 3);
}

#[test]
fn the_only_slide_cannot_be_deleted() {
    let (_d, mut doc) = talk("# One\n");
    assert_eq!(doc.delete_current().unwrap(), Vec::<usize>::new());
    assert_eq!(doc.source(), "# One\n");
    assert!(!doc.can_undo());
    assert!(!doc.has_unsaved_changes());
}

#[test]
fn a_reload_from_disk_empties_the_history() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    doc.set_current_hidden(true).unwrap();
    assert!(matches!(doc.reload(Some(doc.source().to_string())), Reload::Unchanged));
    assert!(doc.can_undo(), "the same text keeps the history");

    doc.reload(Some("# Other\n".into()));
    assert!(!doc.can_undo());
    assert!(!doc.can_redo());
    assert!(!doc.has_unsaved_changes());
    assert_eq!(doc.source(), "# Other\n");
}

#[test]
fn typing_notes_in_a_row_is_one_step() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    for typed in ["S", "Sa", "Say", "Say t", "Say this"] {
        doc.set_notes(1, typed).unwrap();
    }
    assert_eq!(doc.notes_of(1).as_deref(), Some("Say this"));
    doc.undo().unwrap();
    assert_eq!(doc.source(), FOUR_SLIDES);
    assert!(!doc.can_undo());
    doc.redo().unwrap();
    assert_eq!(doc.notes_of(1).as_deref(), Some("Say this"));
}

#[test]
fn notes_typed_after_something_else_are_a_step_of_their_own() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    doc.set_notes(1, "One").unwrap();
    doc.set_notes(2, "Two").unwrap();
    doc.set_notes(1, "One more").unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.notes_of(1).as_deref(), Some("One"));
    assert_eq!(doc.notes_of(2).as_deref(), Some("Two"));
}

#[test]
fn hiding_and_showing_say_so() {
    let (_d, mut doc) = talk(FOUR_SLIDES);
    doc.select(1);
    doc.set_current_hidden(true).unwrap();
    assert_eq!(doc.undo_label(), Some("Hide slide"));
    assert_eq!(doc.shown_count(), 3);
    doc.set_current_hidden(false).unwrap();
    assert_eq!(doc.undo_label(), Some("Show slide"));
    assert_eq!(doc.source(), FOUR_SLIDES);
}

#[test]
fn an_edit_that_would_break_a_slide_is_refused() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    // A slide too large to read.
    let change = slides_format::Change::single(slides_format::SourceEdit {
        range: 5..5,
        text: "x".repeat(40 * 1024),
    });
    let refused = doc.edit("Edit", change, 0).unwrap_err();
    assert_eq!(refused, EditRefused::WouldBreakASlide);
    assert_eq!(refused.to_string(), "That change would have broken another slide, so it wasn't made.");
    assert_eq!(doc.source(), "# One\n\n---\n\n# Two\n");
    assert!(!doc.can_undo());
    assert!(doc.talk().slides.iter().all(|slide| slide.error.is_none()));
}

#[test]
fn an_edit_that_would_merge_slides_is_refused() {
    let (_d, mut doc) = talk("# One\n\n---\n\n# Two\n");
    let change = slides_format::Change::single(slides_format::SourceEdit { range: 7..11, text: String::new() });
    let refused = doc.edit("Edit", change, 0).unwrap_err();
    assert_eq!(refused, EditRefused::WouldChangeTheSlides);
    assert_eq!(doc.source(), "# One\n\n---\n\n# Two\n");
    assert_eq!(doc.slide_count(), 2);
    assert!(!doc.can_undo());
}

#[test]
fn a_slide_that_was_already_broken_can_still_be_moved() {
    let big = "x".repeat(40 * 1024);
    let (_d, mut doc) = talk(&format!("# One\n\n---\n\n{big}\n"));
    assert!(doc.talk().slides[1].error.is_some());
    doc.move_slide(1, 0).unwrap();
    assert!(doc.talk().slides[0].error.is_some());
    doc.duplicate_current().unwrap();
    assert_eq!(doc.slide_count(), 3);
}
