//! Packed `.sptx` talks: a zip holding `slides.md`, a `talk.json` manifest and the files the
//! talk uses. A packed file is untrusted input, so it is checked when it opens (names, sizes,
//! count) and read in bounded pieces afterwards.

use crate::files::TalkFiles;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

pub const MANIFEST_NAME: &str = "talk.json";
pub const STAGEPACK_MANIFEST: &str = "stagepack.json";
pub const FORMAT_VERSION: u32 = 1;

const SLIDES_NAME: &str = "slides.md";
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const PACKED_FOLDERS: [&str; 3] = ["components", "assets", "build"];
/// Pictures and fonts are already compressed, so they are stored as they are.
const STORED_EXTENSIONS: [&str; 9] = [
    "png", "jpg", "jpeg", "gif", "webp", "avif", "ico", "woff", "woff2",
];

#[derive(Clone, Copy)]
pub struct PackLimits {
    pub entry_bytes: u64,
    pub total_bytes: u64,
    pub entries: usize,
}

impl Default for PackLimits {
    fn default() -> PackLimits {
        PackLimits {
            entry_bytes: 256 * 1024 * 1024,
            total_bytes: 2 * 1024 * 1024 * 1024,
            entries: 10_000,
        }
    }
}

#[derive(Debug)]
pub enum PackError {
    NotATalk,
    SlidevTalk,
    TooNew { format: u32 },
    UnsafeName(String),
    TooLarge(String),
    Corrupt(String),
    Io(std::io::Error),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::NotATalk => write!(f, "This file isn't a talk made with this app."),
            PackError::SlidevTalk => write!(f, "This is a Stagepack talk, which this app can't open."),
            PackError::TooNew { format } => write!(
                f,
                "This talk was saved in a newer format (version {format}). Update the app to open it."
            ),
            PackError::UnsafeName(name) => write!(f, "This file holds a file name that isn't safe to use: {name:?}."),
            PackError::TooLarge(what) => write!(f, "This talk is too large to open: {what}."),
            PackError::Corrupt(what) => write!(f, "This file is damaged or isn't a talk: {what}."),
            PackError::Io(e) => write!(f, "The file couldn't be read or written: {e}."),
        }
    }
}

impl std::error::Error for PackError {}

impl From<std::io::Error> for PackError {
    fn from(e: std::io::Error) -> PackError {
        PackError::Io(e)
    }
}

/// An error from reading the pack: bad data inside the file is damage, anything else is the disk.
fn read_error(e: std::io::Error) -> PackError {
    use std::io::ErrorKind::{InvalidData, InvalidInput, UnexpectedEof};
    match e.kind() {
        InvalidData | InvalidInput | UnexpectedEof => PackError::Corrupt(e.to_string()),
        _ => PackError::Io(e),
    }
}

impl From<zip::result::ZipError> for PackError {
    fn from(e: zip::result::ZipError) -> PackError {
        match e {
            zip::result::ZipError::Io(e) => read_error(e),
            other => PackError::Corrupt(other.to_string()),
        }
    }
}

/// Names Windows won't create as files, whatever their extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether one part of a path can be used as a file name on every platform.
fn safe_part(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.ends_with(['.', ' '])
        && !part.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && !part.chars().any(char::is_control)
        && !RESERVED.contains(&stem.as_str())
}

/// Whether `name` (a `/`-separated path, optionally ending in `/` for a folder) stays inside
/// the talk and can be a file on every platform.
fn safe_name(name: &str) -> bool {
    let name = name.strip_suffix('/').unwrap_or(name);
    !name.is_empty() && name.split('/').all(safe_part)
}

/// `path` with empty and `.` parts dropped and `..` resolved, or `None` if it climbs out.
fn normalise(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A packed talk, read on demand.
pub struct PackedFiles {
    zip: Mutex<ZipArchive<File>>,
    /// File name to its place in the zip.
    index: HashMap<String, usize>,
    directories: Vec<String>,
    entry_bytes: u64,
    total_bytes: u64,
}

/// What an entry says it holds, read without trusting it: more than `size` bytes means the
/// stated size lied.
fn read_sized(reader: impl Read, size: u64, what: &str) -> Result<Vec<u8>, PackError> {
    let mut bytes = Vec::new();
    reader
        .take(size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() as u64 > size {
        return Err(PackError::Corrupt(format!(
            "{what} holds more than it says"
        )));
    }
    Ok(bytes)
}

pub fn open_packed(path: &Path) -> Result<PackedFiles, PackError> {
    open_packed_limited(path, PackLimits::default())
}

pub fn open_packed_limited(path: &Path, limits: PackLimits) -> Result<PackedFiles, PackError> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    if zip.has_duplicate_names() {
        return Err(PackError::Corrupt("a file name appears twice".into()));
    }
    if zip.len() > limits.entries {
        return Err(PackError::TooLarge(format!(
            "it holds more than {} files",
            limits.entries
        )));
    }
    // Every name is checked before any entry is read.
    let mut index = HashMap::new();
    let mut directories = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index_data(i)?;
        let name = entry.name()?.to_string();
        if !safe_name(&name) || entry.is_symlink() {
            return Err(PackError::UnsafeName(name));
        }
        if entry.is_dir() {
            directories.push(name.trim_end_matches('/').to_string());
            continue;
        }
        if name == MANIFEST_NAME && entry.size() > MANIFEST_LIMIT {
            return Err(PackError::Corrupt("talk.json is too large".into()));
        }
        if entry.size() > limits.entry_bytes {
            return Err(PackError::TooLarge(format!(
                "{name} is bigger than {} bytes",
                limits.entry_bytes
            )));
        }
        total = total.saturating_add(entry.size());
        if total > limits.total_bytes {
            return Err(PackError::TooLarge(format!(
                "it holds more than {} bytes",
                limits.total_bytes
            )));
        }
        index.insert(name, i);
    }
    let Some(&manifest_at) = index.get(MANIFEST_NAME) else {
        return Err(if index.contains_key(STAGEPACK_MANIFEST) {
            PackError::SlidevTalk
        } else {
            PackError::NotATalk
        });
    };
    let manifest = {
        let entry = zip.by_index(manifest_at)?;
        let size = entry.size();
        read_sized(entry, size, MANIFEST_NAME)?
    };
    let manifest: serde_json::Value = serde_json::from_slice(&manifest)
        .map_err(|e| PackError::Corrupt(format!("talk.json: {e}")))?;
    let format = manifest
        .get("format")
        .and_then(|f| f.as_u64())
        .filter(|&f| f >= 1)
        .ok_or_else(|| PackError::Corrupt("talk.json has no format".into()))?;
    if format > FORMAT_VERSION as u64 {
        return Err(PackError::TooNew {
            format: u32::try_from(format).unwrap_or(u32::MAX),
        });
    }
    if manifest.get("entry").and_then(|e| e.as_str()) != Some(SLIDES_NAME) {
        return Err(PackError::Corrupt(
            "talk.json doesn't name slides.md".into(),
        ));
    }
    if !index.contains_key(SLIDES_NAME) {
        return Err(PackError::Corrupt("slides.md is missing".into()));
    }
    Ok(PackedFiles {
        zip: Mutex::new(zip),
        index,
        directories,
        entry_bytes: limits.entry_bytes,
        total_bytes: limits.total_bytes,
    })
}

impl PackedFiles {
    /// The text of `slides.md`.
    pub fn slides(&self) -> Result<String, PackError> {
        let bytes = self
            .read_checked(SLIDES_NAME)?
            .ok_or_else(|| PackError::Corrupt("slides.md is missing".into()))?;
        String::from_utf8(bytes).map_err(|_| PackError::Corrupt("slides.md isn't text".into()))
    }

    /// The names of the files in the pack, sorted. Folders aren't listed.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.index.keys().cloned().collect();
        names.sort();
        names
    }

    /// `path` as it is in the pack, read with the limits of the pack.
    fn read_checked(&self, path: &str) -> Result<Option<Vec<u8>>, PackError> {
        let Some(&i) = self.index.get(path) else {
            return Ok(None);
        };
        let mut zip = self.zip.lock().unwrap_or_else(|e| e.into_inner());
        let entry = zip.by_index(i)?;
        let size = entry.size();
        if size > self.entry_bytes {
            return Err(PackError::TooLarge(format!(
                "{path} is bigger than {} bytes",
                self.entry_bytes
            )));
        }
        read_sized(entry, size, path).map(Some)
    }
}

impl TalkFiles for PackedFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.read_checked(&normalise(path)?).ok().flatten()
    }
}

pub struct PackReport {
    /// Things left out because they are symlinks or have names that can't be packed.
    pub skipped: Vec<String>,
}

/// Collects what goes in a pack: relative names with the path to read each from.
fn collect(
    folder: &Path,
    skipped: &mut Vec<String>,
) -> Result<Vec<(String, std::path::PathBuf)>, PackError> {
    fn walk(
        dir: &Path,
        prefix: &str,
        found: &mut Vec<(String, std::path::PathBuf)>,
        skipped: &mut Vec<String>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(file_name) = entry.file_name().to_str().map(str::to_string) else {
                skipped.push(format!("{prefix}{}", entry.file_name().to_string_lossy()));
                continue;
            };
            let name = format!("{prefix}{file_name}");
            // Never follow a link: `symlink_metadata` describes the link itself.
            let kind = std::fs::symlink_metadata(&path)?.file_type();
            if kind.is_symlink() || !safe_name(&name) || !(kind.is_dir() || kind.is_file()) {
                skipped.push(name);
            } else if kind.is_dir() {
                walk(&path, &format!("{name}/"), found, skipped)?;
            } else {
                found.push((name, path));
            }
        }
        Ok(())
    }

    let slides = folder.join(SLIDES_NAME);
    if !std::fs::symlink_metadata(&slides)?.file_type().is_file() {
        return Err(PackError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "slides.md isn't a file",
        )));
    }
    let mut found = vec![(SLIDES_NAME.to_string(), slides)];
    for name in PACKED_FOLDERS {
        let path = folder.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => skipped.push(name.to_string()),
            Ok(m) if m.is_dir() => walk(&path, &format!("{name}/"), &mut found, skipped)?,
            Ok(_) => skipped.push(name.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    found.sort();
    skipped.sort();
    Ok(found)
}

fn io_other(e: zip::result::ZipError) -> std::io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        other => std::io::Error::other(other),
    }
}

/// Packs the talk in `folder` into the file `out`, replacing it only if packing succeeds.
pub fn pack_folder(folder: &Path, out: &Path, app_version: &str) -> Result<PackReport, PackError> {
    let mut skipped = Vec::new();
    let files = collect(folder, &mut skipped)?;
    let limits = PackLimits::default();
    if files.len() + 1 > limits.entries {
        return Err(PackError::TooLarge(format!(
            "it would hold more than {} files",
            limits.entries
        )));
    }
    // Written by hand so the fields keep this order.
    let manifest = format!(
        r#"{{"format":{FORMAT_VERSION},"app":{},"entry":"{SLIDES_NAME}"}}"#,
        serde_json::Value::from(app_version)
    )
    .into_bytes();

    let mut entries: Vec<(String, Option<&Path>)> = files
        .iter()
        .map(|(name, path)| (name.clone(), Some(path.as_path())))
        .collect();
    entries.push((MANIFEST_NAME.to_string(), None));
    entries.sort();

    let mut total = 0u64;
    for (name, path) in &entries {
        let Some(path) = path else { continue };
        let size = std::fs::metadata(path)?.len();
        total = total.saturating_add(size);
        if size > limits.entry_bytes {
            return Err(PackError::TooLarge(format!(
                "{name} is bigger than {} bytes",
                limits.entry_bytes
            )));
        }
        if total > limits.total_bytes {
            return Err(PackError::TooLarge(format!(
                "it would hold more than {} bytes",
                limits.total_bytes
            )));
        }
    }

    write_atomically(out, |file| {
        let mut zip = ZipWriter::new(file);
        for (name, path) in &entries {
            let extension = name
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            let method = if STORED_EXTENSIONS.contains(&extension.as_str()) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            let options = SimpleFileOptions::default()
                .compression_method(method)
                .last_modified_time(DateTime::default())
                .unix_permissions(0o644);
            zip.start_file(name.as_str(), options).map_err(io_other)?;
            match path {
                None => zip.write_all(&manifest)?,
                Some(path) => {
                    // Bounded, in case the file grows while it is read.
                    let mut reader = File::open(path)?.take(limits.entry_bytes + 1);
                    let copied = std::io::copy(&mut reader, &mut zip)?;
                    if copied > limits.entry_bytes {
                        return Err(std::io::Error::other(format!("{name} is too large")));
                    }
                }
            }
        }
        zip.finish().map_err(io_other)?;
        Ok(())
    })?;
    Ok(PackReport { skipped })
}

/// Unpacks a packed talk into `folder`, which must be missing or empty. `talk.json` isn't
/// written, since packing makes a new one. A failure leaves `folder` as it was.
pub fn unpack_to_folder(packed: &Path, folder: &Path) -> Result<(), PackError> {
    let files = open_packed(packed)?;
    // Only what a talk is made of is written: not `.git/config` and the like.
    let under_a_folder =
        |name: &str| PACKED_FOLDERS.contains(&name.split('/').next().unwrap_or(""));
    let bad_file = files.index.keys().find(|n| {
        !(n.as_str() == SLIDES_NAME
            || n.as_str() == MANIFEST_NAME
            || (n.contains('/') && under_a_folder(n)))
    });
    let bad_directory = files.directories.iter().find(|d| !under_a_folder(d));
    if let Some(bad) = bad_file.or(bad_directory) {
        return Err(PackError::UnsafeName(bad.clone()));
    }
    let existed = match std::fs::read_dir(folder) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(PackError::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "the folder isn't empty",
                )));
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    let result = unpack_into(&files, folder);
    if result.is_err() {
        if existed {
            if let Ok(entries) = std::fs::read_dir(folder) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                }
            }
        } else {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
    result
}

fn unpack_into(files: &PackedFiles, folder: &Path) -> Result<(), PackError> {
    // The place a name leads to, which must be inside `folder`.
    let target_of = |name: &str| {
        let target = folder.join(name);
        let plain = Path::new(name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
        (plain && target.starts_with(folder))
            .then_some(target)
            .ok_or_else(|| PackError::UnsafeName(name.to_string()))
    };
    std::fs::create_dir_all(folder)?;
    for directory in &files.directories {
        std::fs::create_dir_all(target_of(directory)?)?;
    }
    let mut written = 0u64;
    for name in files.names() {
        if name == MANIFEST_NAME {
            continue;
        }
        let bytes = files
            .read_checked(&name)?
            .ok_or_else(|| PackError::Corrupt(format!("{name} is missing")))?;
        written = written.saturating_add(bytes.len() as u64);
        if written > files.total_bytes {
            return Err(PackError::TooLarge(format!(
                "it holds more than {} bytes",
                files.total_bytes
            )));
        }
        let target = target_of(&name)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        file.write_all(&bytes)?;
    }
    Ok(())
}

/// Writes `out` by way of a temporary file beside it, so a failure never leaves a half-written
/// file in its place.
pub fn write_atomically(
    out: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let name = out
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?
        .to_string_lossy();
    let (mut file, temp) = create_temp(out, &name)?;
    let result = (|| {
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        rename_with_retry(&temp, out)?;
        #[cfg(unix)]
        {
            let parent = match out.parent() {
                Some(p) if !p.as_os_str().is_empty() => p,
                _ => Path::new("."),
            };
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// A new temporary file beside `out`. The name has a random part and the file is created only
/// if nothing is there, so a link planted at a likely name is never followed.
fn create_temp(out: &Path, name: &str) -> std::io::Result<(File, std::path::PathBuf)> {
    use std::hash::{BuildHasher, Hasher};
    let mut last = None;
    for _ in 0..16 {
        let random = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let temp = out.with_file_name(format!(".{name}.{}.{random:016x}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => return Ok((file, temp)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no temporary file name was free")))
}

fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // Another program (a virus scanner, a sync client) can hold the file for a moment.
        for _ in 0..4 {
            if std::fs::rename(from, to).is_ok() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    std::fs::rename(from, to)
}
