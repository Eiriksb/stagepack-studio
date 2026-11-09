//! Tells the app when the talk's file has changed.

use std::ffi::OsStr;
use std::path::Path;

use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::document::SLIDES_FILE;

/// Calls `on_change`, on the watcher's own thread, whenever `slides.md` in `folder` changes,
/// until the returned watcher is dropped.
///
/// It watches the folder, not the file: many editors save by writing a temporary file and
/// renaming it over the original, and a watch on the file itself would be lost.
pub fn watch(
    folder: &Path,
    on_change: impl Fn() + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if event.is_ok_and(|event| changes_the_slides_file(&event)) {
            on_change();
        }
    })?;
    watcher.watch(folder, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn changes_the_slides_file(event: &Event) -> bool {
    // Reading a file raises events too, and the app reads this one whenever it changes.
    let just_read = matches!(
        event.kind,
        EventKind::Access(kind) if kind != AccessKind::Close(AccessMode::Write)
    );
    !just_read
        && event
            .paths
            .iter()
            .any(|path| path.file_name() == Some(OsStr::new(SLIDES_FILE)))
}
