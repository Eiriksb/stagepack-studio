//! Tells the app when the talk's file, or one of its components, has changed.
//!
//! Only two folders are watched, each on its own: the talk's folder, and its `components`
//! folder if that is a real folder. A talk's tree can be any size, and watching all of it could
//! use up the system's watches, so that even `slides.md` would stop being followed. A link is
//! never followed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use slides_engine::COMPONENTS_FOLDER;

use crate::document::SLIDES_FILE;

/// What changed in a talk's folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Its `slides.md`.
    Slides,
    /// A component's source: a `.rs` file directly in its `components` folder, or that folder
    /// itself.
    Component,
}

type OnChange = Arc<Mutex<dyn Fn(Change) + Send>>;

/// Keeps a talk's folder watched until it's dropped.
pub struct TalkWatcher {
    _folder: RecommendedWatcher,
    _components: Arc<Mutex<Option<RecommendedWatcher>>>,
}

/// Calls `on_change`, on a watcher's own thread, whenever `slides.md` in `folder` or a
/// component's source changes, until the returned watcher is dropped. Nothing else counts, so
/// that the app's own writes to `build` never look like a change.
///
/// It watches folders, not files: many editors save by writing a temporary file and renaming
/// it over the original, and a watch on the file itself would be lost.
pub fn watch(
    folder: &Path,
    on_change: impl Fn(Change) + Send + 'static,
) -> notify::Result<TalkWatcher> {
    let on_change: OnChange = Arc::new(Mutex::new(on_change));
    // Events can name the folder as it was given, or as the system resolves it.
    let roots = roots(folder);
    let components_folder = folder.join(COMPONENTS_FOLDER);
    let components = Arc::new(Mutex::new(watch_components(
        &components_folder,
        on_change.clone(),
    )));

    let slot = components.clone();
    let tell = on_change.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else {
            return;
        };
        if just_read(&event) {
            return;
        }
        let names = |name: &str| {
            event.paths.iter().any(|path| {
                path.file_name() == Some(OsStr::new(name))
                    && path
                        .parent()
                        .is_some_and(|parent| roots.iter().any(|root| root == parent))
            })
        };
        if names(SLIDES_FILE) {
            call(&tell, Change::Slides);
        }
        if names(COMPONENTS_FOLDER) {
            // Made, removed or replaced: watch whatever is there now, before saying so, so
            // that nothing saved after this is missed.
            let fresh = watch_components(&components_folder, tell.clone());
            if let Ok(mut slot) = slot.lock() {
                *slot = fresh;
            }
            call(&tell, Change::Component);
        }
    })?;
    watcher.watch(folder, RecursiveMode::NonRecursive)?;
    Ok(TalkWatcher {
        _folder: watcher,
        _components: components,
    })
}

fn roots(folder: &Path) -> Vec<PathBuf> {
    [Some(folder.to_path_buf()), folder.canonicalize().ok()]
        .into_iter()
        .flatten()
        .collect()
}

fn call(on_change: &OnChange, change: Change) {
    if let Ok(on_change) = on_change.lock() {
        on_change(change);
    }
}

/// Reading a file raises events too, and the app reads these files whenever they change.
fn just_read(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Access(kind) if kind != AccessKind::Close(AccessMode::Write)
    )
}

fn is_real_folder(path: &Path) -> bool {
    // `symlink_metadata` describes a link itself, rather than where it leads.
    std::fs::symlink_metadata(path).is_ok_and(|found| found.file_type().is_dir())
}

/// A watch on the sources directly in `folder`, if it's a real folder and not a link.
fn watch_components(folder: &Path, on_change: OnChange) -> Option<RecommendedWatcher> {
    if !is_real_folder(folder) {
        return None;
    }
    let roots = roots(folder);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else {
            return;
        };
        let a_source = event.paths.iter().any(|path| {
            path.extension() == Some(OsStr::new("rs"))
                && path
                    .parent()
                    .is_some_and(|parent| roots.iter().any(|root| root == parent))
        });
        if a_source && !just_read(&event) {
            call(&on_change, Change::Component);
        }
    })
    .ok()?;
    watcher.watch(folder, RecursiveMode::NonRecursive).ok()?;
    // It may have been swapped for a link while the watch was set up.
    is_real_folder(folder).then_some(watcher)
}
