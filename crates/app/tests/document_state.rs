use std::path::Path;
use std::sync::Arc;

use slides_engine::{Fonts, Layer};
use stagepack_studio::document::Document;

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
