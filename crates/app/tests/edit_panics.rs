//! An edit that panics inside the format crate is refused, and leaves the talk as it was. In
//! a test binary of its own, as it sets the process's panic hook to quiet the panics it makes.

use slides_engine::Fonts;
use stagepack_studio::document::{Document, EditRefused, LAYOUT_FAILED, panic_in_next_change, panic_in_next_layout};

/// A command that edits the talk.
type Command = fn(&mut Document) -> Result<Vec<usize>, EditRefused>;

const TALK: &str = "---\ntitle: T\n---\n# One\n---\n# Two\n";

/// Keeps the panics these tests make from being reported, and lets any other through.
fn quiet_the_panics_made() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let made = info.payload().downcast_ref::<&str>().copied();
        if !matches!(made, Some("a change that panics, for a test" | "a layout that panics, for a test")) {
            report(info);
        }
    }));
}

#[test]
fn an_edit_that_panics_is_refused_and_changes_nothing() {
    quiet_the_panics_made();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), TALK).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    doc.set_notes(0, "First").unwrap();
    let before = doc.source().to_string();

    let edits: [Command; 6] = [
        |doc| doc.add_slide("content"),
        |doc| doc.duplicate_current(),
        |doc| doc.delete_current(),
        |doc| doc.move_slide(0, 1),
        |doc| doc.set_current_hidden(true),
        |doc| doc.set_notes(0, "Second"),
    ];
    for edit in edits {
        panic_in_next_change();
        let refused = edit(&mut doc).unwrap_err();
        assert_eq!(refused, EditRefused::CouldNotBeMade);
        assert_eq!(
            refused.to_string(),
            "That change couldn't be made, so nothing was changed."
        );
        assert_eq!(doc.source(), before);
        assert_eq!(doc.slide_count(), 2);
        assert_eq!(doc.current(), 0);
        assert_eq!(doc.undo_label(), Some("Edit notes"));
        assert!(!doc.can_redo());
    }

    // Typing goes on in its own step, and the talk still edits and undoes as before.
    doc.set_notes(0, "First and more").unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.source(), before);
    doc.undo().unwrap();
    assert_eq!(doc.source(), TALK);
}

#[test]
fn a_talk_that_panics_as_it_is_read_says_so_instead_of_opening() {
    quiet_the_panics_made();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), TALK).unwrap();
    panic_in_next_change();
    let problem = Document::open(dir.path(), Fonts::bundled_only())
        .err()
        .unwrap();
    assert!(
        problem.starts_with("Could not read slides.md in "),
        "{problem}"
    );
    assert!(
        problem.ends_with(": something in it can't be read."),
        "{problem}"
    );
}

#[test]
fn a_slide_that_panics_as_it_is_laid_out_has_no_notices_or_thumbnail() {
    quiet_the_panics_made();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "# One\n\n![gone](missing.png)\n").unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    panic_in_next_layout();
    assert_eq!(doc.notices(), Vec::<String>::new());
    assert_eq!(doc.take_message().as_deref(), Some(LAYOUT_FAILED));
    panic_in_next_layout();
    assert!(doc.thumbnail(0, 320).is_none());
    // Laid out again, they're there.
    assert!(!doc.notices().is_empty());
    assert!(doc.thumbnail(0, 320).is_some());
}
