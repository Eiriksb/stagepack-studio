//! Unsaved work kept safe. Edits live in memory until Save, so the window keeps a working copy
//! of a talk with unsaved changes in the app's own data folder, and offers it back if the app
//! stopped before they were saved. A working copy is someone's unsaved work, so its folder and
//! files are the user's alone.

use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use slides_engine::{open_packed, read_talk_text, write_atomically};

use crate::document::{SLIDES_FILE, TalkPlace};
use crate::permissions::BuildPermissions;

/// The working copies' folder in the app's data folder.
const FOLDER: &str = "recovery";
const ABOUT_FILE: &str = "about.json";
/// How many hex characters of the hash of a talk's path name its working copy's folder.
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
}

/// Where the working copies are kept.
#[derive(Clone, Debug)]
pub struct Recovery {
    root: PathBuf,
}

/// The SHA-256 of `text`, in lowercase hex.
pub fn hash_text(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Recovery {
    pub fn new(root: PathBuf) -> Recovery {
        Recovery { root }
    }

    /// Where the app keeps working copies: in its own data folder, if the system has one.
    pub fn default_root() -> Option<PathBuf> {
        Some(BuildPermissions::app_folder()?.join(FOLDER))
    }

    /// Keeps `text` as the working copy of the talk at `place`, whose edits started from the
    /// text with the SHA-256 `base_hash`. Each file is written whole or not at all.
    pub fn keep(&self, place: &TalkPlace, base_hash: &str, text: &str) -> io::Result<()> {
        if text.len() as u64 > MAX_TEXT_BYTES {
            return Err(io::Error::other(
                "the talk is too large to keep a working copy of",
            ));
        }
        let talk = canonical(place);
        let folder = self.root.join(key(&talk));
        create_private(&folder)?;
        make_private(&self.root)?;
        make_private(&folder)?;
        let about = About {
            talk,
            base_sha256: base_hash.to_string(),
            kept_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        let about = serde_json::to_vec_pretty(&about).map_err(io::Error::other)?;
        write_private(&folder.join(SLIDES_FILE), text.as_bytes())?;
        write_private(&folder.join(ABOUT_FILE), &about)
    }

    /// Removes the working copy of the talk at `place`, if there is one.
    pub fn clear(&self, place: &TalkPlace) {
        let folder = self.root.join(key(&canonical(place)));
        // A link here is removed, not followed.
        std::fs::remove_dir_all(folder).ok();
    }

    /// The working copy of the talk at `place`, if there's one that can be read.
    pub fn find(&self, place: &TalkPlace) -> Option<Found> {
        let talk = canonical(place);
        let folder = self.root.join(key(&talk));
        let about: About =
            serde_json::from_slice(&read_small(&folder.join(ABOUT_FILE), MAX_ABOUT_BYTES)?).ok()?;
        if about.talk != talk {
            return None;
        }
        let text =
            String::from_utf8(read_small(&folder.join(SLIDES_FILE), MAX_TEXT_BYTES)?).ok()?;
        let base_matches = on_disk(place).is_some_and(|disk| hash_text(&disk) == about.base_sha256);
        Some(Found {
            text,
            base_matches,
            when: SystemTime::UNIX_EPOCH + Duration::from_secs(about.kept_at),
        })
    }

    /// Removes the working copies kept longer ago than `older_than`. One whose about file
    /// can't be read goes by when its folder last changed.
    pub fn sweep(&self, older_than: Duration) {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_working_copy = entry.file_name().to_str().is_some_and(is_key)
                && entry.file_type().is_ok_and(|kind| kind.is_dir());
            if !is_working_copy {
                continue;
            }
            let about: Option<About> = read_small(&path.join(ABOUT_FILE), MAX_ABOUT_BYTES)
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            let kept = match about {
                Some(about) => Some(SystemTime::UNIX_EPOCH + Duration::from_secs(about.kept_at)),
                None => entry.metadata().and_then(|meta| meta.modified()).ok(),
            };
            let old =
                kept.is_some_and(|kept| now.duration_since(kept).is_ok_and(|age| age > older_than));
            if old {
                std::fs::remove_dir_all(&path).ok();
            }
        }
    }
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

fn is_key(name: &str) -> bool {
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
