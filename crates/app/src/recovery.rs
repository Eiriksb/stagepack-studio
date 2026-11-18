//! Unsaved work kept safe. Edits live in memory until Save, so the window keeps a working copy
//! of a talk with unsaved changes in the app's own data folder, and offers it back if the app
//! stopped before they were saved. A working copy is someone's unsaved work, so its folder and
//! files are the user's alone.
//!
//! Each talk has a folder named for the hash of its path, and each window that keeps a copy of
//! it has a slot of its own in there: `<root>/<key>/<slot>/slides.md` and `about.json`, and the
//! files added to the talk that the text uses, at `files/<path>`. A new presentation that was
//! never saved has no path, so its folder is named for its id, and its about file says it's
//! untitled, which is how [`Recovery::untitled`] finds it.
//!
//! A window holds its slot while it's open: within the app through a set of the live slots, and
//! against any other running copy of the app through a lock on the file `held` in each of its
//! slot's folders. The system lets the lock go when the app stops, however it stops, so the
//! copies of an app that crashed are found at the next start.

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
use crate::document::{SLIDES_FILE, TalkPlace, UNTITLED_NAME, UntitledId};
use crate::permissions::BuildPermissions;

/// The working copies' folder in the app's data folder.
const FOLDER: &str = "recovery";
const ABOUT_FILE: &str = "about.json";
/// The folder in a slot that holds the files added to the talk.
const FILES_FOLDER: &str = "files";
/// The file in a slot that holds the builds entered in the talk's index with those files.
const BUILDS_FILE: &str = "builds.json";
/// The file in a slot that the window keeping it holds a lock on while it's open.
const HELD_FILE: &str = "held";
/// The largest file of builds entered that is read.
const MAX_BUILDS_BYTES: u64 = 256 * 1024;
/// How many hex characters name a talk's folder (from the hash of its path) and a slot.
const KEY_LENGTH: usize = 16;
/// The largest working copy that is read back.
const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
/// The largest about file that is read.
const MAX_ABOUT_BYTES: u64 = 64 * 1024;
/// The most of a working copy's text that is read to list it: enough for its first words and
/// its slides, for any talk a person writes.
const MAX_LISTED_TEXT_BYTES: u64 = 1024 * 1024;
/// The most new presentations listed.
const MAX_LISTED: usize = 100;
/// The most characters of a listed presentation's first words.
const FIRST_WORDS_CHARS: usize = 60;
/// The most characters of a listed presentation's name.
const NAME_CHARS: usize = 80;

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
    /// For a new presentation that was never saved, which one it is. Copies kept before there
    /// were any have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    untitled: Option<UntitledAbout>,
}

/// What a working copy of a new presentation says about it besides.
#[derive(Serialize, Deserialize)]
struct UntitledAbout {
    /// Its id, as 32 hex digits.
    id: String,
    /// What it was called when it was kept.
    name: String,
}

/// A new presentation's working copy left over from last time, as it's listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UntitledCopy {
    pub id: UntitledId,
    /// What it was called when it was kept.
    pub name: String,
    pub when: SystemTime,
    /// How many slides it has, or, if [`Self::slides_at_least`], how many are in the start of
    /// it that's read to list it.
    pub slides: usize,
    /// Whether its text is longer than is read to list it, so it may have more slides than
    /// [`Self::slides`].
    pub slides_at_least: bool,
    /// The first line of text on its first slide, as it reads, 60 characters at most, or empty
    /// if it has none.
    pub first_words: String,
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

impl Found {
    /// Whether `other` is this same copy, in the same slot.
    pub fn is_same_copy(&self, other: &Found) -> bool {
        self.slot == other.slot
    }
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

/// A slot among the live ones, until the last copy of the [`Recovery`] holding it goes, with
/// the locked `held` file of each of its folders made so far.
#[derive(Debug)]
struct LiveSlot {
    name: String,
    held: Mutex<Vec<(PathBuf, std::fs::File)>>,
    /// Whether taking a lock fails, as it can on a folder that refuses them, for tests.
    locks_fail: std::sync::atomic::AtomicBool,
}

impl LiveSlot {
    fn new(slot: &str) -> LiveSlot {
        live_slots().insert(slot.to_string());
        LiveSlot {
            name: slot.to_string(),
            held: Mutex::new(Vec::new()),
            locks_fail: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Vec<(PathBuf, std::fs::File)>> {
        self.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Holds `folder`, one of this slot's, against other running copies of the app: its `held`
    /// file is made if it isn't there and locked for as long as this is live. If the lock can't
    /// be taken, for whatever reason, the copy is kept all the same: a working copy that no
    /// other running app knows is held is far better than none.
    fn hold(&self, folder: &Path) -> io::Result<()> {
        let path = folder.join(HELD_FILE);
        let mut held = self.held();
        if held.iter().any(|(at, file)| at == folder && is_same_file(&path, file)) {
            return Ok(());
        }
        // Gone with the folder, if it was there: it's made again.
        held.retain(|(at, _)| at != folder);
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        no_follow(&mut options);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("something other than a file is where a working copy's lock would go"));
        }
        // Another copy of the app only ever holds it shared for a moment, to look.
        let locked = if self.locks_fail.load(std::sync::atomic::Ordering::Relaxed) {
            Err(io::Error::other("the lock was refused, for a test"))
        } else {
            file.lock()
        };
        if locked.is_ok() {
            held.push((folder.to_path_buf(), file));
        }
        Ok(())
    }

    /// Lets go of `folder`, as it's about to be removed: a file held open can't be removed on
    /// every system.
    fn let_go(&self, folder: &Path) {
        self.held().retain(|(at, _)| at != folder);
    }
}

impl Drop for LiveSlot {
    fn drop(&mut self) {
        live_slots().remove(&self.name);
    }
}

/// Whether the file at `path` is `file`, as far as the system can tell.
fn is_same_file(path: &Path, file: &std::fs::File) -> bool {
    let (Ok(there), Ok(open)) = (std::fs::symlink_metadata(path), file.metadata()) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        there.dev() == open.dev() && there.ino() == open.ino()
    }
    #[cfg(not(unix))]
    {
        there.is_file() && open.is_file()
    }
}

/// Whether a window in another running copy of the app holds the slot `slot`: its `held` file
/// is locked.
fn locked_elsewhere(slot: &Path) -> bool {
    let Some(file) = open_plain(&slot.join(HELD_FILE)) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(std::fs::TryLockError::WouldBlock))
}

/// The name of the slot `slot`, if it has one that can be read.
fn slot_name(slot: &Path) -> Option<&str> {
    slot.file_name().and_then(|name| name.to_str())
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

    /// Has every lock this window's slot takes from now on fail, as on a folder that refuses
    /// them. For tests.
    #[doc(hidden)]
    pub fn fail_locks_for_test(&self) {
        if let Some(live) = &self.live {
            live.locks_fail.store(true, std::sync::atomic::Ordering::Relaxed);
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
        self.keep_named(place, None, base_hash, text, files, builds)
    }

    /// Keeps `text`, `files` and `builds` as [`Self::keep_with_builds`] does, as the working
    /// copy of the new presentation `id`, which is called `name`.
    pub fn keep_untitled(
        &self,
        id: UntitledId,
        name: &str,
        text: &str,
        files: &[(String, Arc<[u8]>)],
        builds: &[BuildEntry],
    ) -> io::Result<()> {
        // No file holds anything of it, so there's no text it started from.
        self.keep_named(&TalkPlace::Untitled(id), Some(name), &hash_text(""), text, files, builds)
    }

    /// Keeps the working copy of the talk at `place`, which is called `name` if it's untitled
    /// and that's given, or else what a new presentation is called.
    fn keep_named(
        &self,
        place: &TalkPlace,
        name: Option<&str>,
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
            untitled: match place {
                TalkPlace::Untitled(id) => Some(UntitledAbout {
                    id: id.to_string(),
                    name: name.unwrap_or(UNTITLED_NAME).to_string(),
                }),
                TalkPlace::Folder(_) | TalkPlace::Packed(_) => None,
            },
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
        if let Some(live) = &self.live {
            live.hold(&folder)?;
        }
        Ok(folder)
    }

    /// Removes this slot's working copy of the talk at `place`, if there is one.
    pub fn clear(&self, place: &TalkPlace) {
        let key_folder = self.root.join(key(&canonical(place)));
        let folder = key_folder.join(&self.slot);
        if let Some(live) = &self.live {
            live.let_go(&folder);
        }
        remove_slot(&folder);
    }

    /// Removes `found`, a working copy found with [`Self::find`], whichever slot it's in, unless
    /// another window has come to hold it since.
    pub fn discard(&self, found: &Found) {
        if !self.held_by_another(&found.slot, &live_slots().clone()) {
            remove_slot(&found.slot);
        }
    }

    /// Whether a window other than this one holds `slot`, among the slots `live` in this app or
    /// in another running copy of it.
    fn held_by_another(&self, slot: &Path, live: &BTreeSet<String>) -> bool {
        let name = slot_name(slot);
        if self.live.is_some() && name == Some(self.slot.as_str()) {
            return false;
        }
        name.is_some_and(|name| live.contains(name)) || locked_elsewhere(slot)
    }

    /// The newest working copy of the talk at `place` that can be read, in any slot. For a
    /// window's own, that's in any slot but those other open windows hold.
    pub fn find(&self, place: &TalkPlace) -> Option<Found> {
        let key_folder = self.key_folder(place)?;
        let live = live_slots().clone();
        let slots = slots(&key_folder)
            .into_iter()
            .filter(|slot| self.live.is_none() || !self.held_by_another(slot, &live))
            .collect();
        newest_of(place, slots)
    }

    /// The folder the talk at `place` has its working copies in, unless it's a link.
    fn key_folder(&self, place: &TalkPlace) -> Option<PathBuf> {
        let key_folder = self.root.join(key(&canonical(place)));
        refuse_link(&key_folder).is_ok().then_some(key_folder)
    }

    /// The working copies of new presentations that were never saved, left over from last time:
    /// each one that can be read and no open window holds, this one's or another's, in this app
    /// or another running copy of it, newest first, [`MAX_LISTED`] at most. Only what's in the
    /// working copies' folder is read, never through a link, and of each copy's text no more
    /// than [`MAX_LISTED_TEXT_BYTES`]. It's quick, and can be asked from any thread while
    /// windows keep their copies.
    ///
    /// With them comes how many were skipped, as they couldn't be read: a slot no window holds
    /// whose about file can't be read, so that it can't say what it's a copy of; one whose
    /// about file says it's a new presentation's but doesn't hold together, or whose text can't
    /// be brought back; and a talk's folder that can't be looked in.
    pub fn untitled(&self) -> (Vec<UntitledCopy>, usize) {
        let live = live_slots().clone();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return (Vec::new(), 0);
        };
        let mut skipped = 0;
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let Some(key_name) = entry.file_name().to_str().filter(|name| is_hex_name(name)).map(str::to_string) else {
                continue;
            };
            // Not a link, and not a file.
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Some(slots) = slots_in(&entry.path()) else {
                skipped += 1;
                continue;
            };
            for slot in slots {
                // One holding neither was never kept in: there's nothing in it to read.
                if is_held(&slot, &live) || holds_no_copy(&slot) {
                    continue;
                }
                match untitled_about(&slot, &key_name) {
                    Some(Some(about)) => found.push(about),
                    Some(None) => {}
                    None => skipped += 1,
                }
            }
        }
        found.sort_by_key(|about| std::cmp::Reverse((about.when, about.written)));
        let mut listed: Vec<UntitledCopy> = Vec::new();
        for about in found {
            if listed.len() == MAX_LISTED {
                break;
            }
            // An older copy of one listed already, as a crash can leave while one is brought
            // back: it goes with the newer.
            if listed.iter().any(|copy| copy.id == about.id) {
                continue;
            }
            match listed_copy(about) {
                Some(copy) => listed.push(copy),
                None => skipped += 1,
            }
        }
        (listed, skipped)
    }

    /// The newest working copy of the new presentation `id`, to bring it back with
    /// [`Document::untitled_with`] and [`WorkspaceView::bring_back_untitled`], unless it's gone,
    /// an open window holds it, or it can't be read. It stays where it is until the window that
    /// brings it back has kept its own.
    ///
    /// [`Document::untitled_with`]: crate::document::Document::untitled_with
    /// [`WorkspaceView::bring_back_untitled`]: crate::workspace_view::WorkspaceView::bring_back_untitled
    pub fn take_untitled(&self, id: UntitledId) -> Result<Found, NotTaken> {
        let place = TalkPlace::Untitled(id);
        let key_folder = self.key_folder(&place).ok_or(NotTaken::Gone)?;
        let live = live_slots().clone();
        let (held, free): (Vec<PathBuf>, Vec<PathBuf>) =
            slots(&key_folder).into_iter().partition(|slot| self.held_by_another(slot, &live));
        let any_free = !free.is_empty();
        match newest_of(&place, free) {
            Some(found) => Ok(found),
            None if !held.is_empty() => Err(NotTaken::Held),
            None if any_free => Err(NotTaken::Unreadable),
            None => Err(NotTaken::Gone),
        }
    }

    /// Removes the working copies of the new presentation `id` that say they're its, but those
    /// an open window holds.
    pub fn discard_untitled(&self, id: UntitledId) {
        let place = TalkPlace::Untitled(id);
        let Some(key_folder) = self.key_folder(&place) else {
            return;
        };
        let talk = canonical(&place);
        for slot in slots(&key_folder) {
            let its = read_about(&slot).is_some_and(|(about, _)| about.talk == talk && says_it_is(&about, &place));
            // Looked at again just before it goes, as a window may have come to hold it.
            if its && !is_held(&slot, &live_slots().clone()) {
                remove_slot(&slot);
            }
        }
    }

    /// Removes the working copies kept longer ago than `older_than`, but those an open window
    /// holds. One whose about file can't be read goes by when its folder last changed. A slot
    /// that holds neither text nor an about file goes whatever its age: it holds only files
    /// kept as they were added, which nothing can bring back without the text that uses them.
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
                if is_held(&slot, &live_slots().clone()) {
                    continue;
                }
                let kept = read_about(&slot)
                    .and_then(|(about, _)| kept_at(&about, &slot))
                    .or_else(|| changed_at(&slot));
                let goes = holds_no_copy(&slot) || kept.is_some_and(is_old);
                // Looked at again just before it goes, as a window may have come to hold it.
                if goes && !is_held(&slot, &live_slots().clone()) {
                    remove_slot(&slot);
                }
            }
        }
    }
}

/// Why a new presentation's working copy wasn't taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTaken {
    /// It isn't there any more.
    Gone,
    /// An open window has it, in this app or another running copy of it.
    Held,
    /// It's there, but can't be read.
    Unreadable,
}

impl std::fmt::Display for NotTaken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NotTaken::Gone => "This presentation isn't kept any more.",
            NotTaken::Held => "This presentation is open in another window.",
            NotTaken::Unreadable => "This presentation's kept copy couldn't be read.",
        })
    }
}

/// Whether an open window holds `slot`, among the slots `live` in this app or in another
/// running copy of it.
fn is_held(slot: &Path, live: &BTreeSet<String>) -> bool {
    slot_name(slot).is_some_and(|name| live.contains(name)) || locked_elsewhere(slot)
}

/// Whether nothing is at the names of a working copy's text and about file in `slot`: no copy
/// was ever kept in it, only files added to the talk as they were added. Anything at either
/// name, a link or a folder too, is a copy, if one that can't be read.
fn holds_no_copy(slot: &Path) -> bool {
    [SLIDES_FILE, ABOUT_FILE].iter().all(|name| {
        std::fs::symlink_metadata(slot.join(name)).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    })
}

/// The newest working copy of the talk at `place` that can be read, among `slots`.
fn newest_of(place: &TalkPlace, slots: Vec<PathBuf>) -> Option<Found> {
    let talk = canonical(place);
    let newest = slots
        .into_iter()
        .filter_map(|slot| {
            let (about, written) = read_about(&slot)?;
            (about.talk == talk && says_it_is(&about, place)).then_some((about, written, slot))
        })
        .filter_map(|(about, written, slot)| {
            let when = kept_at(&about, &slot)?;
            let text = String::from_utf8(read_small(&slot.join(SLIDES_FILE), MAX_TEXT_BYTES)?).ok()?;
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
    slots_in(key_folder).unwrap_or_default()
}

/// The working copies' folders in the talk's folder `key_folder`, or none if it can't be read.
fn slots_in(key_folder: &Path) -> Option<Vec<PathBuf>> {
    let entries = std::fs::read_dir(key_folder).ok()?;
    let slots = entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(is_hex_name))
        // Not a link, and not a file.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    Some(slots)
}

/// A new presentation's copy, as its about file has it, before its text is read.
struct UntitledFound {
    id: UntitledId,
    name: String,
    when: SystemTime,
    written: SystemTime,
    slot: PathBuf,
}

/// What the working copy in `slot`, in the talk's folder named `key_name`, says about the new
/// presentation it's a copy of: none if it's a copy of a talk with a place, and nothing if it
/// can't be read or doesn't hold together.
fn untitled_about(slot: &Path, key_name: &str) -> Option<Option<UntitledFound>> {
    let (about, written) = read_about(slot)?;
    let Some(untitled) = &about.untitled else {
        return Some(None);
    };
    let id = untitled_id(&untitled.id)?;
    // Kept where its id says, and saying the same itself.
    let talk = canonical(&TalkPlace::Untitled(id));
    if about.talk != talk || key(&talk) != key_name {
        return None;
    }
    let name: String = untitled.name.chars().filter(|c| !c.is_control() && !is_invisible(*c)).take(NAME_CHARS).collect();
    let name = match name.trim() {
        "" => UNTITLED_NAME.to_string(),
        name => name.to_string(),
    };
    Some(Some(UntitledFound {
        id,
        name,
        when: kept_at(&about, slot)?,
        written,
        slot: slot.to_path_buf(),
    }))
}

/// Whether `c` is drawn as nothing but changes how the text around it reads: a mark that only
/// orders or joins the letters around it, or one that breaks the line.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{85}' | '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}'
    )
}

/// Whether `about` says it's a copy of the talk at `place` as its kind of place has it: a new
/// presentation's says which one it is.
fn says_it_is(about: &About, place: &TalkPlace) -> bool {
    match place {
        TalkPlace::Untitled(id) => about.untitled.as_ref().and_then(|untitled| untitled_id(&untitled.id)) == Some(*id),
        TalkPlace::Folder(_) | TalkPlace::Packed(_) => about.untitled.is_none(),
    }
}

/// The id written as `hex`, if it's 32 lowercase hex digits, as ids are written.
fn untitled_id(hex: &str) -> Option<UntitledId> {
    let digits = hex.len() == 32 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    digits.then(|| u128::from_str_radix(hex, 16).ok()).flatten().map(UntitledId)
}

/// `found` as it's listed, with its slides and first words read from its text, unless the text
/// can't be brought back.
fn listed_copy(found: UntitledFound) -> Option<UntitledCopy> {
    let file = found.slot.join(SLIDES_FILE);
    let meta = std::fs::symlink_metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > MAX_TEXT_BYTES {
        return None;
    }
    let mut bytes = read_start(&file, MAX_LISTED_TEXT_BYTES)?;
    let longer = meta.len() > MAX_LISTED_TEXT_BYTES;
    if longer {
        // Only its start is read: up to the last whole line in that.
        let end = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |at| at + 1);
        bytes.truncate(end);
    }
    let text = String::from_utf8(bytes).ok()?;
    let (slides, first_words) = std::panic::catch_unwind(|| {
        let talk = slides_format::parse_talk(&text);
        (talk.slides.len(), first_words(&talk))
    })
    .ok()?;
    Some(UntitledCopy {
        id: found.id,
        name: found.name,
        when: found.when,
        slides,
        slides_at_least: longer,
        first_words,
    })
}

/// The first line of text on the talk's first slide, as it reads, [`FIRST_WORDS_CHARS`]
/// characters at most, or empty if there's none.
fn first_words(talk: &slides_format::Talk) -> String {
    let line = slides_format::text_flows(talk, 0)
        .into_iter()
        .flat_map(|flow| flow.paras)
        .flat_map(|para| {
            let text: String = para.spans.iter().map(|span| span.text.as_str()).collect();
            text.split('\n').map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>()
        })
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let words: String = line.chars().take(FIRST_WORDS_CHARS).collect();
    words.trim_end().to_string()
}

/// What the working copy in `slot` says about itself, and when that was written.
fn read_about(slot: &Path) -> Option<(About, SystemTime)> {
    let file = slot.join(ABOUT_FILE);
    let about = serde_json::from_slice(&read_small(&file, MAX_ABOUT_BYTES)?).ok()?;
    let written = std::fs::symlink_metadata(&file)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Some((about, written))
}

/// How far ahead of the clock a copy's time can be and still be taken as when it was kept.
const CLOCK_SLIP: Duration = Duration::from_secs(24 * 60 * 60);

/// When the copy in `slot` was kept, unless it says a time no clock can hold. A time further
/// ahead than a clock that was set wrong explains is when its folder last changed instead, so
/// that it neither stays first in the list nor is kept for ever.
fn kept_at(about: &About, slot: &Path) -> Option<SystemTime> {
    let kept = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(about.kept_at))?;
    let latest = SystemTime::now().checked_add(CLOCK_SLIP)?;
    if kept > latest {
        return changed_at(slot);
    }
    Some(kept)
}

/// When the folder `slot` last changed.
fn changed_at(slot: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(slot).and_then(|meta| meta.modified()).ok()
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
    let file = open_plain(path)?;
    if file.metadata().ok()?.len() > most {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(most + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= most).then_some(bytes)
}

/// The first `most` bytes of the plain file at `path`, or all of it if it holds fewer.
fn read_start(path: &Path, most: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    open_plain(path)?.take(most).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The plain file at `path`, opened to read, unless it's a link or anything but a file. Where
/// the system allows, a link put there after it's looked at isn't followed either, and nothing
/// waits on what's at the other end of a pipe.
fn open_plain(path: &Path) -> Option<std::fs::File> {
    if !std::fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    no_follow(&mut options);
    let file = options.open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// Opens a file with `options` without following a link, where the system allows.
fn no_follow(options: &mut std::fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    let _ = options;
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
