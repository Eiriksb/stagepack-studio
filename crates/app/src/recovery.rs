//! Unsaved work kept safe. Edits live in memory until Save, so the window keeps a working copy
//! of a talk with unsaved changes in the app's own data folder, and offers it back if the app
//! stopped before they were saved. A working copy is someone's unsaved work, so its folder and
//! files are the user's alone.
//!
//! Each talk has a folder named for the hash of its path, and each window that keeps a copy of
//! it has a slot of its own in there: `<root>/<key>/<slot>/slides.md` and `about.json`.

use std::collections::BTreeSet;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use slides_engine::{open_packed, read_talk_text, write_atomically};

use crate::document::{SLIDES_FILE, TalkPlace};
use crate::permissions::BuildPermissions;

/// The working copies' folder in the app's data folder.
const FOLDER: &str = "recovery";
const ABOUT_FILE: &str = "about.json";
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
        if text.len() as u64 > MAX_TEXT_BYTES {
            return Err(io::Error::other(
                "the talk is too large to keep a working copy of",
            ));
        }
        let talk = canonical(place);
        let key_folder = self.root.join(key(&talk));
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
        let about = About {
            talk,
            base_sha256: base_hash.to_string(),
            kept_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        let about = serde_json::to_vec_pretty(&about).map_err(io::Error::other)?;
        // The text first and what it's about last: stopped between the two, the new text has
        // the base it had before, which can only make the talk look changed since when it
        // isn't. The other way round, older text would be taken as made from the file as it is.
        write_private(&folder.join(SLIDES_FILE), text.as_bytes())?;
        write_private(&folder.join(ABOUT_FILE), &about)
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

/// The talk's path, as one path however it's written, if it's there to be found.
fn canonical(place: &TalkPlace) -> PathBuf {
    let (TalkPlace::Folder(path) | TalkPlace::Packed(path)) = place;
    path.canonicalize()
        .unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.clone()))
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
