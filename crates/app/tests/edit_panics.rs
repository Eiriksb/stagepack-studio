//! An edit that panics inside the format crate is refused, and leaves the talk as it was. In
//! a test binary of its own, as it sets the process's panic hook to quiet the panics it makes.

use slides_engine::Fonts;
use stagepack_studio::document::{Document, EditRefused, panic_in_next_change};

/// A command that edits the talk.
type Command = fn(&mut Document) -> Result<Vec<usize>, EditRefused>;

const TALK: &str = "---\ntitle: T\n---\n# One\n---\n# Two\n";

#[test]
fn an_edit_that_panics_is_refused_and_changes_nothing() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<&str>() != Some(&"a change that panics, for a test") {
            report(info);
        }
    }));
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
