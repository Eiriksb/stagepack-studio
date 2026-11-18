//! Unsaved work kept safe. Edits live in memory until Save, so the window keeps a working copy
//! of a talk with unsaved changes in the app's own data folder, and offers it back if the app
//! stopped before they were saved. A working copy is someone's unsaved work, so its folder and
//! files are the user's alone.
//!
//! Each talk has a folder named for the hash of its path, and each window that keeps a copy of
//! it has a slot of its own in there: `<root>/<key>/<slot>/slides.md` and `about.json`, and the
//! files added to the talk that the text uses, at `files/<path>`.

use std::collections::BTreeSet;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use slides_components::BuildEntry;
use slides_engine::{open_packed, read_talk_text, write_atomically};

use crate::added_files::{FileKind, MAX_ADDED_BYTES, is_added_path};
use crate::document::{SLIDES_FILE, TalkPlace};
use crate::permissions::BuildPermissions;

/// The working copies' folder in the app's data folder.
const FOLDER: &str = "recovery";
const ABOUT_FILE: &str = "about.json";
/// The folder in a slot that holds the files added to the talk.
const FILES_FOLDER: &str = "files";
/// The file in a slot that holds the builds entered in the talk's index with those files.
const BUILDS_FILE: &str = "builds.json";
/// The largest file of builds entered that is read.
const MAX_BUILDS_BYTES: u64 = 256 * 1024;
/// How many hex characters name a talk's folder (from the hash of its path) and a slot.
const KEY_LENGTH: usize = 16;
/// The largest working copy that is read back.
const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
/// The largest about file that is read.
const MAX_ABOUT_BYTES: u64 = 64 * 1024;

/// How long a working copy is kept for a talk that isn't opened again.
pub const KEEP_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// What a working copy says about itself.
#[derive(Serialize, Deserialize)]
struct About {
    /// The talk's canonical path.
    talk: PathBuf,
    /// The SHA-256 of the text the edits started from.
    base_sha256: String,
    /// When it was kept, in seconds since 1970.
    kept_at: u64,
}

/// A working copy found for a talk.
#[derive(Debug)]
pub struct Found {
    pub text: String,
    /// The files added to the talk that were kept with the text, by their path in the talk.
    pub files: Vec<(String, Arc<[u8]>)>,
    /// The builds entered in the talk's index with those files, as a paste entered them. They
    /// were written by the app, but are read back as untrusted as anything else here.
    pub builds: Vec<BuildEntry>,
    /// Whether the talk's file still holds the text the edits started from.
    pub base_matches: bool,
    pub when: SystemTime,
    /// The folder it's kept in.
    slot: PathBuf,
}

/// Where the working copies are kept.
#[derive(Clone, Debug)]
pub struct Recovery {
    root: PathBuf,
    /// This window's own folder within each talk's.
    slot: String,
    /// Holds the slot among the live ones, while a window has it.
    live: Option<Arc<LiveSlot>>,
}

/// The slots open windows in this app are keeping their copies in. Their copies aren't left
/// over from a crash, so another window is never offered one, and never lets one go.
static LIVE_SLOTS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// A slot among the live ones, until the last copy of the [`Recovery`] holding it goes.
#[derive(Debug)]
struct LiveSlot(String);

impl LiveSlot {
    fn new(slot: &str) -> LiveSlot {
        live_slots().insert(slot.to_string());
        LiveSlot(slot.to_string())
    }
}

impl Drop for LiveSlot {
    fn drop(&mut self) {
        live_slots().remove(&self.0);
    }
}

fn live_slots() -> std::sync::MutexGuard<'static, BTreeSet<String>> {
    // A set of names is whole even if a thread panicked holding it.
    LIVE_SLOTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The SHA-256 of `text`, in lowercase hex.
pub fn hash_text(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Recovery {
    /// Working copies under `root`, in a slot of their own: each window that keeps them makes
    /// its own, so two windows on one talk never write over each other's.
    pub fn new(root: PathBuf) -> Recovery {
        Recovery {
            root,
            slot: random_name(),
            live: None,
        }
    }

    /// The same place, with a slot of its own for a window, which is live for as long as the
    /// window holds it: no other window finds the copies in it.
    pub fn for_another_window(&self) -> Recovery {
        let slot = random_name();
        Recovery {
            root: self.root.clone(),
            live: Some(Arc::new(LiveSlot::new(&slot))),
            slot,
        }
    }

    /// Where the app keeps working copies: in its own data folder, if the system has one.
    pub fn default_root() -> Option<PathBuf> {
        Some(BuildPermissions::app_folder()?.join(FOLDER))
    }

    /// Keeps `text` as this slot's working copy of the talk at `place`, whose edits started
    /// from the text with the SHA-256 `base_hash`. Each file is written whole or not at all.
    pub fn keep(&self, place: &TalkPlace, base_hash: &str, text: &str) -> io::Result<()> {
        self.keep_with_files(place, base_hash, text, &[])
    }

    /// Keeps `text` as [`Self::keep`] does, with `files`, files added to the talk that the text
    /// uses, by their path in the talk. They're written first, and one kept already stays as
    /// it is: a file added to a talk never changes.
    pub fn keep_with_files(
        &self,
        place: &TalkPlace,
        base_hash: &str,
        text: &str,
        files: &[(String, Arc<[u8]>)],
    ) -> io::Result<()> {
        self.keep_with_builds(place, base_hash, text, files, &[])
    }

    /// Keeps `text` and `files` as [`Self::keep_with_files`] does, with `builds`, the builds
    /// entered in the talk's index with those files, which are written before the text too.
    pub fn keep_with_builds(
        &self,
        place: &TalkPlace,
        base_hash: &str,
        text: &str,
        files: &[(String, Arc<[u8]>)],
        builds: &[BuildEntry],
    ) -> io::Result<()> {
        if let Some((odd, _)) = files.iter().find(|(path, _)| !is_added_path(path)) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{odd:?} isn't a file added to a talk")));
        }
        if text.len() as u64 > MAX_TEXT_BYTES {
            return Err(io::Error::other(
                "the talk is too large to keep a working copy of",
            ));
        }
        let talk = canonical(place);
        let folder = self.slot_folder(&talk)?;
        let about = About {
            talk,
            base_sha256: base_hash.to_string(),
            kept_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        let about = serde_json::to_vec_pretty(&about).map_err(io::Error::other)?;
        for (path, bytes) in files {
            keep_file(&folder, path, bytes)?;
        }
        let builds_file = folder.join(BUILDS_FILE);
        if builds.is_empty() {
            match std::fs::remove_file(&builds_file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        } else {
            write_private(&builds_file, &serde_json::to_vec(builds).map_err(io::Error::other)?)?;
        }
        // The text first and what it's about last: stopped between the two, the new text has
        // the base it had before, which can only make the talk look changed since when it
        // isn't. The other way round, older text would be taken as made from the file as it is.
        write_private(&folder.join(SLIDES_FILE), text.as_bytes())?;
        write_private(&folder.join(ABOUT_FILE), &about)
    }

    /// Keeps `bytes`, the file added to the talk at `place` whose path in it is `path`, in this
    /// slot, as it's added. It's found with the next text kept, which brings it back; a slot that
    /// holds no text yet offers nothing.
    pub fn keep_file(&self, place: &TalkPlace, path: &str, bytes: &[u8]) -> io::Result<()> {
        if !is_added_path(path) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{path:?} isn't a file added to a talk")));
        }
        let folder = self.slot_folder(&canonical(place))?;
        keep_file(&folder, path, bytes)
    }

    /// This slot's folder for the talk at the canonical path `talk`, made if it isn't there, and
    /// never through a link.
    fn slot_folder(&self, talk: &Path) -> io::Result<PathBuf> {
        let key_folder = self.root.join(key(talk));
        let folder = key_folder.join(&self.slot);
        refuse_link(&key_folder)?;
        refuse_link(&folder)?;
        create_private(&folder)?;
        // Made just now, but something may have put a link there meanwhile.
        refuse_link(&key_folder)?;
        refuse_link(&folder)?;
        make_private(&self.root)?;
        make_private(&key_folder)?;
        make_private(&folder)?;
        Ok(folder)
    }

    /// Removes this slot's working copy of the talk at `place`, if there is one.
    pub fn clear(&self, place: &TalkPlace) {
        let key_folder = self.root.join(key(&canonical(place)));
        remove_slot(&key_folder.join(&self.slot));
    }

    /// Removes `found`, a working copy found with [`Self::find`], whichever slot it's in.
    pub fn discard(&self, found: &Found) {
        remove_slot(&found.slot);
    }

    /// The newest working copy of the talk at `place` that can be read, in any slot. For a
    /// window's own, that's in any slot but those other open windows hold.
    pub fn find(&self, place: &TalkPlace) -> Option<Found> {
        let talk = canonical(place);
        let key_folder = self.root.join(key(&talk));
        if refuse_link(&key_folder).is_err() {
            return None;
        }
        let others_live: BTreeSet<String> = match &self.live {
            Some(_) => live_slots()
                .iter()
                .filter(|slot| **slot != self.slot)
                .cloned()
                .collect(),
            None => BTreeSet::new(),
        };
        let newest = slots(&key_folder)
            .into_iter()
            .filter(|slot| {
                slot.file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| !others_live.contains(name))
            })
            .filter_map(|slot| {
                let (about, written) = read_about(&slot)?;
                (about.talk == talk).then_some((about, written, slot))
            })
            .filter_map(|(about, written, slot)| {
                let when = kept_at(&about)?;
                let text =
                    String::from_utf8(read_small(&slot.join(SLIDES_FILE), MAX_TEXT_BYTES)?).ok()?;
                Some((when, written, about.base_sha256, text, slot))
            })
            .max_by_key(|(when, written, ..)| (*when, *written))?;
        let (when, _, base, text, slot) = newest;
        let base_matches = on_disk(place).is_some_and(|disk| hash_text(&disk) == base);
        Some(Found {
            text,
            files: kept_files(&slot),
            builds: kept_builds(&slot),
            base_matches,
            when,
            slot,
        })
    }

    /// Removes the working copies kept longer ago than `older_than`. One whose about file
    /// can't be read goes by when its folder last changed.
    pub fn sweep(&self, older_than: Duration) {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        let now = SystemTime::now();
        let is_old = |kept: SystemTime| now.duration_since(kept).is_ok_and(|age| age > older_than);
        for entry in entries.flatten() {
            let is_talk = entry.file_name().to_str().is_some_and(is_hex_name)
                && entry.file_type().is_ok_and(|kind| kind.is_dir());
            if !is_talk {
                continue;
            }
            for slot in slots(&entry.path()) {
                let kept = read_about(&slot)
                    .and_then(|(about, _)| kept_at(&about))
                    .or_else(|| {
                        std::fs::metadata(&slot)
                            .and_then(|meta| meta.modified())
                            .ok()
                    });
                if kept.is_some_and(is_old) {
                    remove_slot(&slot);
                }
            }
        }
    }
}

/// Keeps `bytes`, the file added to the talk at `path`, in the working copy's `slot`.
fn keep_file(slot: &Path, path: &str, bytes: &[u8]) -> io::Result<()> {
    let files = slot.join(FILES_FOLDER);
    let (folder, _) = path.split_once('/').expect("an added file's path is in a folder");
    let folder = files.join(folder);
    for made in [&files, &folder] {
        refuse_link(made)?;
        create_private(made)?;
        refuse_link(made)?;
        make_private(made)?;
    }
    let file = slot.join(FILES_FOLDER).join(path);
    if read_small(&file, MAX_ADDED_BYTES as u64).is_some_and(|kept| kept == bytes) {
        return Ok(());
    }
    write_private(&file, bytes)
}

/// The files added to the talk kept in the working copy's `slot`: plain files only, with names a
/// file added to a talk can have, each no bigger than one can be.
fn kept_files(slot: &Path) -> Vec<(String, Arc<[u8]>)> {
    let files = slot.join(FILES_FOLDER);
    if refuse_link(&files).is_err() {
        return Vec::new();
    }
    let folders: BTreeSet<&str> =
        [FileKind::Picture, FileKind::Data, FileKind::ComponentSource, FileKind::ComponentBuild].map(FileKind::folder).into();
    let mut kept = Vec::new();
    for folder in folders {
        let folder_path = files.join(folder);
        if refuse_link(&folder_path).is_err() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(folder_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(|name| format!("{folder}/{name}")) else {
                continue;
            };
            if !is_added_path(&name) {
                continue;
            }
            // `read_small` reads only a plain file, and not through a link.
            if let Some(bytes) = read_small(&entry.path(), MAX_ADDED_BYTES as u64) {
                kept.push((name, Arc::from(bytes)));
            }
        }
    }
    kept.sort();
    kept
}

/// The builds entered kept in the working copy's `slot`, or none if there are none that can be
/// read.
fn kept_builds(slot: &Path) -> Vec<BuildEntry> {
    read_small(&slot.join(BUILDS_FILE), MAX_BUILDS_BYTES)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The working copies' folders in the talk's folder `key_folder`.
fn slots(key_folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(key_folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(is_hex_name))
        // Not a link, and not a file.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// What the working copy in `slot` says about itself, and when that was written.
fn read_about(slot: &Path) -> Option<(About, SystemTime)> {
    let file = slot.join(ABOUT_FILE);
    let about = serde_json::from_slice(&read_small(&file, MAX_ABOUT_BYTES)?).ok()?;
    let written = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Some((about, written))
}

/// When the copy was kept, unless it says a time no clock can hold.
fn kept_at(about: &About) -> Option<SystemTime> {
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(about.kept_at))
}

/// Removes the working copy in `slot`, and its talk's folder once that's empty. A link is
/// removed, not followed.
fn remove_slot(slot: &Path) {
    std::fs::remove_dir_all(slot).ok();
    if let Some(key_folder) = slot.parent() {
        // Only when nothing is left in it.
        std::fs::remove_dir(key_folder).ok();
    }
}

fn refuse_link(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(io::Error::other("a link is where a working copy would go"))
        }
        _ => Ok(()),
    }
}

/// A name no other window's slot has.
fn random_name() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    if let Ok(since) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        hasher.write_u128(since.as_nanos());
    }
    format!("{:016x}", hasher.finish())
}

/// What the talk's working copies are kept under: its path, as one path however it's written,
/// if it's there to be found. An untitled talk has no path, so it's its id, under a name no
/// talk's path can be, as those all start at the root.
fn canonical(place: &TalkPlace) -> PathBuf {
    match place {
        TalkPlace::Folder(path) | TalkPlace::Packed(path) => path
            .canonicalize()
            .unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.clone())),
        TalkPlace::Untitled(id) => PathBuf::from(format!("untitled/{id}")),
    }
}

/// The name of the folder that holds the working copy of the talk at `talk`.
fn key(talk: &Path) -> String {
    let mut key = hex(&Sha256::digest(talk.as_os_str().as_encoded_bytes()));
    key.truncate(KEY_LENGTH);
    key
}

fn is_hex_name(name: &str) -> bool {
    name.len() == KEY_LENGTH
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// What the talk's file holds now.
fn on_disk(place: &TalkPlace) -> Option<String> {
    match place {
        TalkPlace::Folder(folder) => read_talk_text(&folder.join(SLIDES_FILE)).ok(),
        TalkPlace::Packed(file) => open_packed(file).ok()?.slides().ok(),
        // Held in memory: no file holds anything of it.
        TalkPlace::Untitled(_) => None,
    }
}

/// The bytes of the plain file at `path`, unless it holds more than `most`.
fn read_small(path: &Path, most: u64) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > most {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(most + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= most).then_some(bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomically(path, |file| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)
    })
}

fn create_private(folder: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(folder)
}

#[cfg(unix)]
fn make_private(folder: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn make_private(_folder: &Path) -> io::Result<()> {
    Ok(())
}
