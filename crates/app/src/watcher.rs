//! Tells the app when the talk's file, or one of its components, has changed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use slides_engine::COMPONENTS_FOLDER;

use crate::document::SLIDES_FILE;

/// What changed in a talk's folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Its `slides.md`.
    Slides,
    /// A component's source: a `.rs` file directly in its `components` folder.
    Component,
}

/// Calls `on_change`, on the watcher's own thread, whenever `slides.md` in `folder` or a
/// component's source changes, until the returned watcher is dropped. Nothing else counts, so
/// that the app's own writes to `build` never look like a change.
///
/// It watches the folder, not the files: many editors save by writing a temporary file and
/// renaming it over the original, and a watch on the file itself would be lost.
pub fn watch(
    folder: &Path,
    on_change: impl Fn(Change) + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    // Events can name the folder as it was given, or as the system resolves it.
    let roots: Vec<PathBuf> = [Some(folder.to_path_buf()), folder.canonicalize().ok()]
        .into_iter()
        .flatten()
        .collect();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else {
            return;
        };
        for change in [Change::Slides, Change::Component] {
            if changes(&event, &roots, change) {
                on_change(change);
            }
        }
    })?;
    watcher.watch(folder, RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn changes(event: &Event, roots: &[PathBuf], change: Change) -> bool {
    // Reading a file raises events too, and the app reads these whenever they change.
    let just_read = matches!(
        event.kind,
        EventKind::Access(kind) if kind != AccessKind::Close(AccessMode::Write)
    );
    !just_read
        && event.paths.iter().any(|path| {
            roots.iter().any(|root| {
                path.strip_prefix(root)
                    .is_ok_and(|within| kind_of(within) == Some(change))
            })
        })
}

/// What a path within the talk's folder is, if it's something that counts.
fn kind_of(within: &Path) -> Option<Change> {
    let parts: Vec<&OsStr> = within.iter().collect();
    match parts.as_slice() {
        [name] if *name == OsStr::new(SLIDES_FILE) => Some(Change::Slides),
        [folder, name]
            if *folder == OsStr::new(COMPONENTS_FOLDER)
                && Path::new(name).extension() == Some(OsStr::new("rs")) =>
        {
            Some(Change::Component)
        }
        _ => None,
    }
}
