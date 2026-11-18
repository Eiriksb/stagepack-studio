//! Packed `.sptx` talks: a zip holding `slides.md`, a `talk.json` manifest and the files the
//! talk uses. A packed file is untrusted input, so it is checked when it opens (names, sizes,
//! count) and read in bounded pieces afterwards.

use crate::files::{FolderFiles, MAX_FILE_BYTES, MAX_LARGE_FILE, ReadSeek, TalkFiles, is_large_kind};
use crate::MAX_INDEX_BYTES;
use slides_components::{BuildIndex, INDEX_FILE};
use slides_format::{Block, Talk, parse_talk};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

pub const MANIFEST_NAME: &str = "talk.json";
pub const STAGEPACK_MANIFEST: &str = "stagepack.json";
pub const FORMAT_VERSION: u32 = 1;

const SLIDES_NAME: &str = "slides.md";
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const PACKED_FOLDERS: [&str; 3] = ["components", "assets", "build"];
/// The component that shows a picture from a file.
const PICTURE_TAG: &str = "Picture";
const PICTURE_SOURCE: &str = "src";
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
            entry_bytes: MAX_FILE_BYTES,
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
    /// A talk was to be written in or under its own folder.
    InsideTheTalk,
    /// A file was to be added to a talk that has a different one by that name.
    NameTaken(String),
    /// A write was asked to stop before it was done, and what it had written was taken away.
    Stopped,
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
            PackError::InsideTheTalk => write!(f, "A talk can't be saved inside its own folder. Choose a place outside it."),
            PackError::NameTaken(_) => write!(f, "This talk already has a different file with that name."),
            PackError::Stopped => write!(f, "It was stopped before it was done, and nothing was changed."),
            PackError::Io(e) => write!(f, "The file couldn't be read or written: {e}."),
        }
    }
}

impl std::error::Error for PackError {}

impl From<std::io::Error> for PackError {
    fn from(e: std::io::Error) -> PackError {
        if is_stop(&e) {
            return PackError::Stopped;
        }
        PackError::Io(e)
    }
}

/// An error from reading the pack: bad data inside the file is damage, anything else is the disk.
fn read_error(e: std::io::Error) -> PackError {
    use std::io::ErrorKind::{InvalidData, InvalidInput, UnexpectedEof};
    if is_stop(&e) {
        return PackError::Stopped;
    }
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

/// Whether `c` is a format character that draws nothing, which could make one name pass for
/// another: a zero-width space in front of `.git` isn't a hidden name to the dot check below.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{AD}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// Whether one part of a path can be used as a file name on every platform.
fn safe_part(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !part.is_empty()
        // Hidden names are how tools leave configuration that can run commands later, and
        // this covers `.` and `..` as well.
        && !part.starts_with('.')
        && !part.ends_with(['.', ' '])
        && !part.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && !part.chars().any(|c| c.is_control() || is_invisible(c))
        && !RESERVED.contains(&stem.as_str())
}

/// Whether `name` (a `/`-separated path, optionally ending in `/` for a folder) stays inside
/// the talk and can be a file on every platform.
pub(crate) fn safe_name(name: &str) -> bool {
    let name = name.strip_suffix('/').unwrap_or(name);
    !name.is_empty() && name.split('/').all(safe_part)
}

/// `path` with empty and `.` parts dropped and `..` resolved, or `None` if it starts at the root
/// or climbs out, as a folder talk would refuse it too. This is the path a pack and the files
/// added to a talk read `path` as.
pub fn normalise(path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
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

/// A file to add to a talk as it's written: its path in the talk, and its bytes.
pub type NewFile = (String, Vec<u8>);

/// Where the bytes of a file to add to a talk come from: held, or a file on disk, which is copied
/// a piece at a time as the talk is written and never read whole.
#[derive(Clone, Debug)]
pub enum FileSource {
    Bytes(Vec<u8>),
    At(PathBuf),
}

/// A file to add to a talk as it's written: its path in the talk, and where its bytes come from.
pub type NewFileFrom = (String, FileSource);

/// A file to add as it's given, before it's been looked at.
#[derive(Clone, Copy)]
enum Given<'a> {
    Bytes(&'a [u8]),
    At(&'a Path),
}

fn given_from(files: &[NewFileFrom]) -> Vec<(&str, Given<'_>)> {
    files
        .iter()
        .map(|(name, source)| match source {
            FileSource::Bytes(bytes) => (name.as_str(), Given::Bytes(bytes)),
            FileSource::At(path) => (name.as_str(), Given::At(path)),
        })
        .collect()
}

fn given_bytes(files: &[NewFile]) -> Vec<(&str, Given<'_>)> {
    files.iter().map(|(name, bytes)| (name.as_str(), Given::Bytes(bytes))).collect()
}

/// Where the bytes of an entry come from as it's written.
#[derive(Clone, Copy)]
enum Source<'a> {
    Bytes(&'a [u8]),
    /// A file on disk, with its length and modified time when it was looked at. If either has
    /// changed by the time it's been copied, or it's gone, it isn't the file that was meant, and
    /// the copy fails.
    At(&'a Path, u64, Option<SystemTime>),
}

impl Source<'_> {
    fn len(&self) -> u64 {
        match self {
            Source::Bytes(bytes) => bytes.len() as u64,
            Source::At(_, len, _) => *len,
        }
    }

    /// Its bytes, read from the start.
    fn reader(&self) -> std::io::Result<Box<dyn Read + '_>> {
        Ok(match *self {
            Source::Bytes(bytes) => Box::new(bytes),
            Source::At(path, _, _) => Box::new(File::open(path)?),
        })
    }
}

/// The regular file at `path`, looked at to be copied.
fn on_disk(path: &Path) -> Result<Source<'_>, PackError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(PackError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} isn't a regular file", path.display()),
        )));
    }
    Ok(Source::At(path, metadata.len(), metadata.modified().ok()))
}

fn look(given: Given<'_>) -> Result<Source<'_>, PackError> {
    match given {
        Given::Bytes(bytes) => Ok(Source::Bytes(bytes)),
        Given::At(path) => on_disk(path),
    }
}

/// A file to add, looked at and checked.
struct Added<'a> {
    name: &'a str,
    source: Source<'a>,
}

/// The most a file called `name` can hold, where other files can hold `entry_bytes`.
fn limit_for(name: &str, entry_bytes: u64) -> u64 {
    if is_large_kind(name) { MAX_LARGE_FILE } else { entry_bytes }
}

/// The files of `extra` that a talk holding the files `names` doesn't have yet, each checked
/// to be a file a talk can hold, in a folder that's packed. One the talk has with the same bytes,
/// as `same` tells, is left out. One it has with other bytes, or under a name only the case
/// of its letters tells apart from one it has, is refused rather than written over, and so is
/// one named twice in `extra` with different bytes. A file of a large kind can hold up to
/// [`MAX_LARGE_FILE`], any other `entry_bytes`.
///
/// The one exception is the talk's build index, [`INDEX_FILE`]: the app keeps it, merging the
/// builds a paste brings into the talk's own, so `extra` may hold it once, in place of the
/// talk's. It must read as a build index within [`MAX_INDEX_BYTES`]. It's given back with the
/// others, and [`replaces_index`] says whether the talk's own is to be left out.
fn new_files<'a>(
    names: &[String],
    same: impl Fn(&str, Source<'_>) -> Result<bool, PackError>,
    extra: &[(&'a str, Given<'a>)],
    entry_bytes: u64,
) -> Result<Vec<Added<'a>>, PackError> {
    let mut new: Vec<Added> = Vec::new();
    let mut indexes = extra.iter().filter(|(name, _)| *name == INDEX_FILE);
    if let Some(&(name, given)) = indexes.next() {
        let source = look(given)?;
        let readable = source.len() <= MAX_INDEX_BYTES
            && match given {
                Given::Bytes(bytes) => serde_json::from_slice::<BuildIndex>(bytes).is_ok(),
                Given::At(path) => read_limited(path, MAX_INDEX_BYTES).is_ok_and(|bytes| serde_json::from_slice::<BuildIndex>(&bytes).is_ok()),
            };
        if indexes.next().is_some() || !readable {
            return Err(PackError::UnsafeName(name.to_string()));
        }
        // Only the index itself: a file whose name differs from it but for case is another.
        if names.iter().any(|other| other != INDEX_FILE && other.eq_ignore_ascii_case(INDEX_FILE)) {
            return Err(PackError::NameTaken(name.to_string()));
        }
        new.push(Added { name, source });
    }
    for &(name, given) in extra.iter().filter(|(name, _)| *name != INDEX_FILE) {
        let in_a_folder = name.split_once('/').is_some_and(|(folder, _)| PACKED_FOLDERS.contains(&folder));
        if !(in_a_folder && safe_name(name) && !name.ends_with('/') && normalise(name).as_deref() == Some(name)) {
            return Err(PackError::UnsafeName(name.to_string()));
        }
        let source = look(given)?;
        let limit = limit_for(name, entry_bytes);
        if source.len() > limit {
            return Err(PackError::TooLarge(format!("{name} is bigger than {limit} bytes")));
        }
        let alike = |other: &str| other.eq_ignore_ascii_case(name);
        if let Some(there) = names.iter().find(|other| alike(other)) {
            if there == name && same(there, source)? {
                continue;
            }
            return Err(PackError::NameTaken(name.to_string()));
        }
        if let Some(earlier) = new.iter().find(|earlier| alike(earlier.name)) {
            if earlier.name == name && same_content(&mut *earlier.source.reader()?, earlier.source.len(), source)? {
                continue;
            }
            return Err(PackError::NameTaken(name.to_string()));
        }
        new.push(Added { name, source });
    }
    Ok(new)
}

/// Whether `held`, which says it holds `len` bytes, holds what `source` does, compared a piece at
/// a time. What can't be read of `held` is damage, as it is in a pack.
fn same_content(held: &mut dyn Read, len: u64, source: Source<'_>) -> Result<bool, PackError> {
    if len != source.len() {
        return Ok(false);
    }
    let mut other = source.reader()?;
    let (mut a, mut b) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    let mut left = len;
    while left > 0 {
        let n = left.min(a.len() as u64) as usize;
        held.read_exact(&mut a[..n]).map_err(read_error)?;
        if other.read_exact(&mut b[..n]).is_err() || a[..n] != b[..n] {
            return Ok(false);
        }
        left -= n as u64;
    }
    // Neither holds more.
    Ok(held.read(&mut [0]).map_err(read_error)? == 0 && other.read(&mut [0])? == 0)
}

/// The bytes of the file at `path`, if it holds no more than `limit`.
fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, PackError> {
    read_sized(File::open(path)?, limit, &path.display().to_string())
}

/// From this size on an entry is written in the zip's large-file form, which an entry of 4 GB or
/// more needs, so that nothing near the edge is written in the other.
const LARGE_FORM_FROM: u64 = 0xFFFF_FFFF - 64 * 1024 * 1024;

/// How an entry called `name`, of `len` bytes, is written: pictures, fonts and the large kinds as
/// they are, anything else compressed.
fn entry_options(name: &str, len: u64) -> SimpleFileOptions {
    let extension = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let method = if STORED_EXTENSIONS.contains(&extension.as_str()) || is_large_kind(name) {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };
    SimpleFileOptions::default()
        .compression_method(method)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
        .large_file(len >= LARGE_FORM_FROM)
}

/// `buf.len()` bytes of `file` from `offset` on, or as many as there are, read without moving
/// anything another reader of the same file depends on.
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_at(file, buf, offset)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::FileExt::seek_read(file, buf, offset)
    }
}

/// A part of a pack's file, read by place, so that any number of these can read the one file at
/// once, each from where it is. The whole file is one too, for reading the pack's own index.
struct PackReader {
    file: Arc<File>,
    start: u64,
    len: u64,
    at: u64,
}

impl PackReader {
    fn new(file: Arc<File>, start: u64, len: u64) -> PackReader {
        PackReader { file, start, len, at: 0 }
    }
}

impl Read for PackReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.at >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let want = (self.len - self.at).min(buf.len() as u64) as usize;
        // A file cut short under it reads as ending early, which whatever reads it then finds.
        let n = read_at(&self.file, &mut buf[..want], self.start + self.at)?;
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for PackReader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        self.at = at.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "a place before the start"))?;
        Ok(self.at)
    }
}

/// A packed talk, read on demand.
pub struct PackedFiles {
    zip: Mutex<ZipArchive<PackReader>>,
    /// The pack's file, as it was opened, for reading entries in place.
    pack: Arc<File>,
    pack_len: u64,
    /// File name to its place in the zip.
    index: HashMap<String, usize>,
    directories: Vec<String>,
    /// The files of a large kind stored as they are, which are held to the pack's own length
    /// rather than to its limits.
    in_place: HashSet<String>,
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
    let pack = Arc::new(File::open(path)?);
    let pack_len = pack.metadata()?.len();
    let mut zip = ZipArchive::new(PackReader::new(pack.clone(), 0, pack_len))?;
    // First, so that one of these gets its own message and not a complaint about what's in it.
    if zip.index_for_name(STAGEPACK_MANIFEST).is_some() && zip.index_for_name(MANIFEST_NAME).is_none() {
        return Err(PackError::SlidevTalk);
    }
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
    let mut in_place = HashSet::new();
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
        // A large file stored as it is can't hold more than the pack does, as is checked below,
        // so it isn't counted with the rest. Compressed, it could say anything, so it is.
        let stored_large = is_large_kind(&name) && entry.compression() == CompressionMethod::Stored;
        let limit = if stored_large { MAX_LARGE_FILE } else { limits.entry_bytes };
        if entry.size() > limit {
            return Err(PackError::TooLarge(format!("{name} is bigger than {limit} bytes")));
        }
        if stored_large {
            if entry.size() != entry.compressed_size() || entry.encrypted() {
                return Err(PackError::Corrupt(format!("{name} isn't stored as it says")));
            }
            in_place.insert(name.clone());
        } else {
            total = total.saturating_add(entry.size());
        }
        if total > limits.total_bytes {
            return Err(PackError::TooLarge(format!(
                "it holds more than {} bytes",
                limits.total_bytes
            )));
        }
        index.insert(name, i);
    }
    // What each entry says it holds is held to the pack's own length: no entry runs into another,
    // or into the index at the end, or past the end of the file.
    if zip.has_overlapping_entries()? {
        return Err(PackError::Corrupt("a file in it says it holds more than there is".into()));
    }
    let Some(&manifest_at) = index.get(MANIFEST_NAME) else {
        return Err(PackError::NotATalk);
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
        pack,
        pack_len,
        index,
        directories,
        in_place,
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

    fn zip(&self) -> std::sync::MutexGuard<'_, ZipArchive<PackReader>> {
        self.zip.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `path` as it is in the pack, read with the limits of the pack.
    fn read_checked(&self, path: &str) -> Result<Option<Vec<u8>>, PackError> {
        let Some(&i) = self.index.get(path) else {
            return Ok(None);
        };
        let mut zip = self.zip();
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

    /// Whether the pack's file `path` holds what `source` does, compared a piece at a time.
    fn holds(&self, path: &str, source: Source<'_>) -> Result<bool, PackError> {
        let &i = self.index.get(path).ok_or_else(|| PackError::Corrupt(format!("{path} is missing")))?;
        let mut zip = self.zip();
        let mut entry = zip.by_index(i)?;
        let size = entry.size();
        same_content(&mut entry, size, source)
    }

    /// Copies the pack's file `path` to `to` a piece at a time, unpacking it if it's compressed,
    /// telling `progress` the bytes done and the bytes in all as it goes, and stopping if `stop`
    /// is set. A file of a large kind can hold up to [`MAX_LARGE_FILE`], any other what the pack
    /// allows; one that holds more or less than it says is damage, and nothing more is written.
    pub fn copy_entry(&self, path: &str, to: &mut dyn Write, progress: &dyn Fn(u64, u64), stop: &AtomicBool) -> Result<(), PackError> {
        let path = normalise(path).ok_or_else(|| PackError::UnsafeName(path.to_string()))?;
        let &i = self.index.get(&path).ok_or_else(|| PackError::Corrupt(format!("{path} is missing")))?;
        let mut zip = self.zip();
        let mut entry = zip.by_index(i)?;
        let size = entry.size();
        let limit = if is_large_kind(&path) { MAX_LARGE_FILE } else { self.entry_bytes };
        if size > limit {
            return Err(PackError::TooLarge(format!("{path} is bigger than {limit} bytes")));
        }
        let mut moving = Moving::new(progress, stop, size);
        moving.copy(&mut entry, to, size, &path, true)
    }

    /// Whether what `name` holds counts toward the pack's total.
    fn counted(&self, name: &str) -> bool {
        !self.in_place.contains(name)
    }
}

impl TalkFiles for PackedFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.read_checked(&normalise(path)?).ok().flatten()
    }

    /// An entry stored as it is, read where it lies in the pack, by its own reader. A compressed
    /// one can't be read so, and gives `None`. The checksum isn't asked, as it's only known once
    /// the whole entry has been read.
    fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>> {
        let &i = self.index.get(&normalise(path)?)?;
        let (start, len) = {
            let mut zip = self.zip();
            let entry = zip.by_index_raw(i).ok()?;
            let stored = entry.compression() == CompressionMethod::Stored && !entry.encrypted();
            if !stored || entry.size() != entry.compressed_size() {
                return None;
            }
            (entry.data_start()?, entry.compressed_size())
        };
        // Held to the file as it is, whatever the entry says.
        let end = start.checked_add(len)?;
        (end <= self.pack_len).then(|| Box::new(PackReader::new(self.pack.clone(), start, len)) as Box<dyn ReadSeek>)
    }

    /// The size the entry states, which was held to the pack's limits when it opened.
    fn len(&self, path: &str) -> Option<u64> {
        let &i = self.index.get(&normalise(path)?)?;
        let zip = self.zip();
        let size = zip.by_index_data(i).ok()?.size();
        Some(size)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        let Some(folder) = normalise(folder) else {
            return Vec::new();
        };
        let prefix = format!("{folder}/");
        let mut names: Vec<String> = self
            .index
            .keys()
            .filter(|name| name.strip_prefix(&prefix).is_some_and(|rest| !rest.contains('/')))
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// A pack is read from the file as it was opened, and isn't written to.
    fn never_changes(&self) -> bool {
        true
    }
}

pub struct PackReport {
    /// Things left out because they are symlinks or have names that can't be packed.
    pub skipped: Vec<String>,
    /// Pictures the talk shows from outside the folders that are packed, by the path it uses.
    pub outside_pictures: Vec<String>,
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

/// Every path `talk` shows a picture from, in Markdown or through the picture component.
fn picture_paths(talk: &Talk) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let mut blocks: Vec<&Block> = talk.slides.iter().flat_map(|slide| &slide.blocks).collect();
    while let Some(block) = blocks.pop() {
        match block {
            Block::Picture { src, .. } => {
                paths.insert(src.clone());
            }
            Block::List { items, .. } => blocks.extend(items.iter().flatten()),
            Block::Tag(tag) => {
                paths.extend(tag.attr(PICTURE_SOURCE).filter(|_| tag.name == PICTURE_TAG).map(str::to_string));
                blocks.extend(&tag.children);
            }
            _ => {}
        }
    }
    paths
}

/// The pictures the talk in `folder` shows from files that exist but aren't in a packed folder,
/// sorted. Found in the talk's text, so nothing is decoded. A picture that is in a packed folder
/// and still didn't go in has been reported as skipped.
fn pictures_outside(folder: &Path, source: &str) -> Vec<String> {
    let files = FolderFiles::new(folder.to_path_buf());
    let packed = |path: &str| path == SLIDES_NAME || PACKED_FOLDERS.contains(&path.split('/').next().unwrap_or(""));
    let outside: BTreeSet<String> = picture_paths(&parse_talk(source))
        .iter()
        .filter_map(|path| normalise(path))
        .filter(|path| !packed(path) && files.stamp(path).is_some())
        .collect();
    outside.into_iter().collect()
}

fn io_other(e: zip::result::ZipError) -> std::io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        other => std::io::Error::other(other),
    }
}

/// What a pack says about itself, written by hand so the fields keep this order.
fn manifest_for(app_version: &str) -> Vec<u8> {
    format!(
        r#"{{"format":{FORMAT_VERSION},"app":{},"entry":"{SLIDES_NAME}"}}"#,
        serde_json::Value::from(app_version)
    )
    .into_bytes()
}

/// Holds what goes in to the limits of a pack: how many files, and how big each is and all are.
/// `own` is the talk's own files, with `slides.md` as it will be, and `extra` what's added. A file
/// of a large kind can hold up to [`MAX_LARGE_FILE`], and as it's stored as it is, it isn't counted
/// with the rest.
fn check_sizes(own: &[(&str, Source<'_>)], extra: &[Added<'_>], limits: PackLimits) -> Result<(), PackError> {
    if own.len() + extra.len() + 1 > limits.entries {
        return Err(PackError::TooLarge(format!(
            "it would hold more than {} files",
            limits.entries
        )));
    }
    let mut total = 0u64;
    let sizes = own.iter().map(|(name, source)| (*name, source.len()));
    for (name, size) in sizes.chain(extra.iter().map(|added| (added.name, added.source.len()))) {
        let limit = limit_for(name, limits.entry_bytes);
        if size > limit {
            return Err(PackError::TooLarge(format!("{name} is bigger than {limit} bytes")));
        }
        if !is_large_kind(name) {
            total = total.saturating_add(size);
        }
        if total > limits.total_bytes {
            return Err(PackError::TooLarge(format!(
                "it would hold more than {} bytes",
                limits.total_bytes
            )));
        }
    }
    Ok(())
}

/// Whether `extra`, as [`new_files`] gives it, replaces the talk's own build index.
fn replaces_index(extra: &[Added<'_>]) -> bool {
    extra.iter().any(|added| added.name == INDEX_FILE)
}

/// The files of the talk in a folder, as `collect` found them, each looked at to be copied, with
/// `slides` as its `slides.md` if given, and without its own build index if `extra` replaces it.
fn own_files<'a>(
    found: &'a [(String, PathBuf)],
    slides: Option<&'a str>,
    extra: &[Added<'_>],
) -> Result<Vec<(&'a str, Source<'a>)>, PackError> {
    found
        .iter()
        .filter(|(name, _)| !replaces_index(extra) || name != INDEX_FILE)
        .map(|(name, path)| match slides {
            Some(slides) if name == SLIDES_NAME => Ok((name.as_str(), Source::Bytes(slides.as_bytes()))),
            _ => Ok((name.as_str(), on_disk(path)?)),
        })
        .collect()
}

/// The files of `extra` that the talk whose files are `found` doesn't have yet, as [`new_files`]
/// finds them.
fn new_folder_files<'a>(
    found: &[(String, PathBuf)],
    extra: &[(&'a str, Given<'a>)],
    limits: PackLimits,
) -> Result<Vec<Added<'a>>, PackError> {
    let names: Vec<String> = found.iter().map(|(name, _)| name.clone()).collect();
    let same = |name: &str, source: Source<'_>| {
        let (_, path) = found.iter().find(|(there, _)| there == name).expect("a name from the list");
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        same_content(&mut file, len, source)
    };
    new_files(&names, same, extra, limits.entry_bytes)
}

/// Bytes are copied this many at a time at most, so that a stop asked for is seen soon and
/// nothing large is ever held.
const PIECE: usize = 1024 * 1024;

/// Stands for a stop that was asked for, while it's passed along as an error of the disk.
#[derive(Debug)]
struct StopAsked;

impl fmt::Display for StopAsked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "it was stopped")
    }
}

impl std::error::Error for StopAsked {}

fn is_stop(e: &std::io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<StopAsked>())
}

/// The error for `what`, a file on disk, which changed or went while it was copied.
fn changed(what: &str) -> PackError {
    PackError::Io(std::io::Error::other(ChangedWhileCopied(what.to_string())))
}

/// What a write's error of the disk holds when a file it copied, named here, changed or went
/// while it was copied: whatever passes the error on can tell it from any other by this.
#[derive(Debug)]
pub struct ChangedWhileCopied(pub String);

impl fmt::Display for ChangedWhileCopied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} changed while it was copied", self.0)
    }
}

impl std::error::Error for ChangedWhileCopied {}

impl PackError {
    /// Whether this is a file that changed or went while it was copied.
    pub fn is_changed_while_copied(&self) -> bool {
        matches!(self, PackError::Io(e) if e.get_ref().is_some_and(|inner| inner.is::<ChangedWhileCopied>()))
    }
}

/// How far a write has got, told as it goes, and whether it's been asked to stop.
struct Moving<'a> {
    progress: &'a dyn Fn(u64, u64),
    stop: &'a AtomicBool,
    done: u64,
    total: u64,
    piece: Vec<u8>,
}

impl<'a> Moving<'a> {
    fn new(progress: &'a dyn Fn(u64, u64), stop: &'a AtomicBool, total: u64) -> Moving<'a> {
        Moving { progress, stop, done: 0, total, piece: Vec::new() }
    }

    fn check(&self) -> Result<(), PackError> {
        if self.stop.load(Ordering::SeqCst) { Err(PackError::Stopped) } else { Ok(()) }
    }

    fn moved(&mut self, bytes: u64) {
        self.done = self.done.saturating_add(bytes);
        (self.progress)(self.done, self.total);
    }

    /// Copies what `from` holds, which is to be `len` bytes, to `to`, a piece at a time. If it
    /// holds more or less, nothing more is written and it's an error: damage in a pack, if
    /// `from_pack`, and otherwise a file that changed while it was copied.
    fn copy(&mut self, from: &mut dyn Read, to: &mut dyn Write, len: u64, what: &str, from_pack: bool) -> Result<(), PackError> {
        if self.piece.is_empty() {
            self.piece = vec![0; PIECE];
        }
        let mut copied = 0u64;
        loop {
            self.check()?;
            // One more than is left, to find one that holds more.
            let want = (len - copied).saturating_add(1).min(PIECE as u64) as usize;
            let n = match from.read(&mut self.piece[..want]) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if from_pack => return Err(read_error(e)),
                Err(e) => return Err(e.into()),
            };
            if n == 0 {
                break;
            }
            copied += n as u64;
            if copied > len {
                break;
            }
            to.write_all(&self.piece[..n])?;
            self.moved(n as u64);
        }
        match (copied == len, from_pack) {
            (true, _) => Ok(()),
            (false, true) => Err(PackError::Corrupt(format!("{what} doesn't hold what it says"))),
            (false, false) => Err(changed(what)),
        }
    }

    /// Copies `source`, the bytes of `what`, to `to`, a piece at a time.
    fn copy_source(&mut self, source: Source<'_>, to: &mut dyn Write, what: &str) -> Result<(), PackError> {
        match source {
            Source::Bytes(mut bytes) => self.copy(&mut bytes, to, source.len(), what, false),
            Source::At(path, len, modified) => {
                let mut file = File::open(path).map_err(|_| changed(what))?;
                self.copy(&mut file, to, len, what, false)?;
                // Gone, or changed in place, since it was looked at, it isn't what was meant.
                let now = std::fs::metadata(path).map_err(|_| changed(what))?;
                if now.len() != len || now.modified().ok() != modified {
                    return Err(changed(what));
                }
                Ok(())
            }
        }
    }
}

/// Writes `entries` into the file `out` as a pack, in their order, each as [`entry_options`] has it.
fn write_entries(out: &mut File, entries: &[(&str, Source<'_>)], moving: &mut Moving<'_>) -> Result<(), PackError> {
    let mut zip = ZipWriter::new(out);
    for &(name, source) in entries {
        moving.check()?;
        zip.start_file(name, entry_options(name, source.len())).map_err(io_other)?;
        moving.copy_source(source, &mut zip, name)?;
    }
    zip.finish().map_err(io_other)?;
    Ok(())
}

fn total_of(entries: &[(&str, Source<'_>)]) -> u64 {
    entries.iter().map(|(_, source)| source.len()).sum()
}

/// Packs the talk in `folder` into the file `out`, replacing it only if packing succeeds.
pub fn pack_folder(folder: &Path, out: &Path, app_version: &str) -> Result<PackReport, PackError> {
    pack(folder, out, None, app_version, &[], &|_, _| {}, &AtomicBool::new(false))
}

/// Packs the talk in `folder` into the file `out` as [`pack_folder`] does, with `slides` as its
/// `slides.md` in place of the file in the folder, which isn't touched, and the files of `extra`
/// as well, as [`repack_with_slides`] adds them.
pub fn pack_folder_with_slides(
    folder: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
    extra: &[NewFile],
) -> Result<PackReport, PackError> {
    pack(folder, out, Some(slides), app_version, &given_bytes(extra), &|_, _| {}, &AtomicBool::new(false))
}

/// Packs the talk in `folder` as [`pack_folder_with_slides`] does, with the files of `extra` taken
/// from where they're given. Every file is copied a piece at a time, telling `progress` the bytes
/// done and the bytes in all as it goes; if `stop` is set, it stops, and `out` is as it was.
pub fn pack_folder_with_slides_from(
    folder: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
    extra: &[NewFileFrom],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<PackReport, PackError> {
    pack(folder, out, Some(slides), app_version, &given_from(extra), progress, stop)
}

fn pack(
    folder: &Path,
    out: &Path,
    slides: Option<&str>,
    app_version: &str,
    extra: &[(&str, Given<'_>)],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<PackReport, PackError> {
    let mut skipped = Vec::new();
    let found = collect(folder, &mut skipped)?;
    let limits = PackLimits::default();
    let extra = new_folder_files(&found, extra, limits)?;
    let own = own_files(&found, slides, &extra)?;
    check_sizes(&own, &extra, limits)?;
    let manifest = manifest_for(app_version);
    // Read once, so that what's packed and what's reported come from the same text.
    let source = match slides {
        Some(slides) => slides.to_string(),
        None => std::fs::read_to_string(folder.join(SLIDES_NAME)).unwrap_or_default(),
    };

    let mut entries: Vec<(&str, Source)> = own;
    entries.extend(extra.iter().map(|added| (added.name, added.source)));
    entries.push((MANIFEST_NAME, Source::Bytes(&manifest)));
    entries.sort_by_key(|(name, _)| *name);

    let mut moving = Moving::new(progress, stop, total_of(&entries));
    atomically(out, |file| write_entries(file, &entries, &mut moving))?;
    Ok(PackReport { skipped, outside_pictures: pictures_outside(folder, &source) })
}

/// The files of `files`, which a talk made whole from them holds besides `slides.md`, checked as
/// a pack and a copy of a folder check theirs: each a file a talk can hold, none twice, and all
/// within the limits.
fn whole_talk_files<'a>(files: &[(&'a str, Given<'a>)], limits: PackLimits) -> Result<Vec<Added<'a>>, PackError> {
    let files = new_files(&[], |name, _| Err(PackError::Corrupt(format!("{name} is missing"))), files, limits.entry_bytes)?;
    check_sizes(&[], &files, limits)?;
    Ok(files)
}

/// Writes a talk, made of `slides` and `files`, as a pack in the file `out`, with the manifest
/// [`pack_folder_with_slides`] writes. The entries are the same ones that would make, in the same
/// order, within the same limits, and the file is written beside and put in place as that one is:
/// `out` is replaced only by a whole pack. Each of `files` must be one a pack can hold, as
/// [`repack_with_slides`] asks of the files it adds.
pub fn pack_new(out: &Path, slides: &str, app_version: &str, files: &[NewFile]) -> Result<(), PackError> {
    pack_new_given(out, slides, app_version, &given_bytes(files), &|_, _| {}, &AtomicBool::new(false))
}

/// Writes a talk as a pack as [`pack_new`] does, with `files` taken from where they're given,
/// a piece at a time, telling `progress` the bytes done and the bytes in all as it goes. If `stop`
/// is set, it stops, and `out` is as it was.
pub fn pack_new_from(
    out: &Path,
    slides: &str,
    app_version: &str,
    files: &[NewFileFrom],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    pack_new_given(out, slides, app_version, &given_from(files), progress, stop)
}

fn pack_new_given(
    out: &Path,
    slides: &str,
    app_version: &str,
    files: &[(&str, Given<'_>)],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    let files = whole_talk_files(files, PackLimits::default())?;
    let manifest = manifest_for(app_version);
    let mut entries: Vec<(&str, Source)> = files.iter().map(|added| (added.name, added.source)).collect();
    entries.push((SLIDES_NAME, Source::Bytes(slides.as_bytes())));
    entries.push((MANIFEST_NAME, Source::Bytes(&manifest)));
    entries.sort_by_key(|(name, _)| *name);
    let mut moving = Moving::new(progress, stop, total_of(&entries));
    atomically(out, |file| write_entries(file, &entries, &mut moving))
}

/// Writes a talk, made of `slides` and `files`, as the folder `to`, which must be missing or
/// empty. Each file is written whole or not at all, `slides.md` last, so that a folder cut short
/// has no talk in it. Each of `files` must be one a pack can hold, as for [`pack_new`]. A
/// failure leaves `to` as it was found.
pub fn write_new_folder(to: &Path, slides: &str, files: &[NewFile]) -> Result<(), PackError> {
    write_new_folder_given(to, slides, &given_bytes(files), &|_, _| {}, &AtomicBool::new(false))
}

/// Writes a talk as the folder `to` as [`write_new_folder`] does, with `files` taken from where
/// they're given, a piece at a time, telling `progress` the bytes done and the bytes in all as it
/// goes. If `stop` is set, it stops, and `to` is as it was found.
pub fn write_new_folder_from(
    to: &Path,
    slides: &str,
    files: &[NewFileFrom],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    write_new_folder_given(to, slides, &given_from(files), progress, stop)
}

fn write_new_folder_given(
    to: &Path,
    slides: &str,
    files: &[(&str, Given<'_>)],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    let files = whole_talk_files(files, PackLimits::default())?;
    let existed = missing_or_empty(to)?;
    let total = files.iter().map(|added| added.source.len()).sum::<u64>() + slides.len() as u64;
    let mut moving = Moving::new(progress, stop, total);
    let result = (|| {
        std::fs::create_dir_all(to)?;
        for added in &files {
            let target = target_in(to, added.name)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            new_atomically(&target, |file| moving.copy_source(added.source, file, added.name))?;
        }
        new_atomically(&target_in(to, SLIDES_NAME)?, |file| moving.copy_source(Source::Bytes(slides.as_bytes()), file, SLIDES_NAME))
    })();
    if result.is_err() {
        clear_away(to, existed);
    }
    result
}

/// Unpacks a packed talk into `folder`, which must be missing or empty. `talk.json` isn't
/// written, since packing makes a new one. A failure leaves `folder` as it was.
pub fn unpack_to_folder(packed: &Path, folder: &Path) -> Result<(), PackError> {
    unpack(packed, folder, None, &[], &|_, _| {}, &AtomicBool::new(false))
}

/// Unpacks a packed talk into `folder` as [`unpack_to_folder`] does, with `slides` as its
/// `slides.md` in place of the one in the pack, and the files of `extra` as well, as
/// [`repack_with_slides`] adds them.
pub fn unpack_with_slides(packed: &Path, folder: &Path, slides: &str, extra: &[NewFile]) -> Result<(), PackError> {
    unpack(packed, folder, Some(slides), &given_bytes(extra), &|_, _| {}, &AtomicBool::new(false))
}

/// Unpacks a packed talk as [`unpack_with_slides`] does, with the files of `extra` taken from where
/// they're given. Every file is copied a piece at a time, telling `progress` the bytes done and
/// the bytes in all as it goes; if `stop` is set, it stops, and `folder` is as it was.
pub fn unpack_with_slides_from(
    packed: &Path,
    folder: &Path,
    slides: &str,
    extra: &[NewFileFrom],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    unpack(packed, folder, Some(slides), &given_from(extra), progress, stop)
}

fn unpack(
    packed: &Path,
    folder: &Path,
    slides: Option<&str>,
    extra: &[(&str, Given<'_>)],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
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
    if slides.is_some_and(|slides| slides.len() as u64 > files.entry_bytes) {
        return Err(PackError::TooLarge(format!(
            "{SLIDES_NAME} is bigger than {} bytes",
            files.entry_bytes
        )));
    }
    let extra = new_files(&files.names(), |name, source| files.holds(name, source), extra, files.entry_bytes)?;
    let existed = missing_or_empty(folder)?;
    let mut moving = Moving::new(progress, stop, 0);
    let result = unpack_into(&files, folder, slides, &extra, &mut moving);
    if result.is_err() {
        clear_away(folder, existed);
    }
    result
}

/// Whether `folder` is there, when it's empty: it must be missing or empty to be written into.
fn missing_or_empty(folder: &Path) -> Result<bool, PackError> {
    match std::fs::read_dir(folder) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(PackError::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "the folder isn't empty",
                )));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Takes away what a failed write put in `folder`, which was missing or empty before it:
/// all of it, or the folder itself if it wasn't there.
fn clear_away(folder: &Path, existed: bool) {
    if !existed {
        let _ = std::fs::remove_dir_all(folder);
        return;
    }
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
}

/// The place `name`, a path within a talk, leads to in `folder`, which must be inside it.
fn target_in(folder: &Path, name: &str) -> Result<std::path::PathBuf, PackError> {
    let target = folder.join(name);
    let plain = Path::new(name)
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (plain && target.starts_with(folder))
        .then_some(target)
        .ok_or_else(|| PackError::UnsafeName(name.to_string()))
}

/// The new file `target`, made with the folders it's in, to be written.
fn create_new(target: &Path) -> std::io::Result<File> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
}

/// `slides.md` is written last, so that a folder cut short has no talk in it rather than a
/// talk missing some of its files. Every file is copied from the pack a piece at a time.
fn unpack_into(
    files: &PackedFiles,
    folder: &Path,
    slides: Option<&str>,
    extra: &[Added<'_>],
    moving: &mut Moving<'_>,
) -> Result<(), PackError> {
    let own_index = !replaces_index(extra);
    let mut names = files.names();
    names.retain(|name| name != MANIFEST_NAME && name != SLIDES_NAME && (own_index || name != INDEX_FILE));
    let slides_len = match slides {
        Some(slides) => slides.len() as u64,
        None => files.len(SLIDES_NAME).unwrap_or(0),
    };
    // What's written is held to the pack's total, all but the large files stored as they are.
    let held: u64 = names.iter().filter(|name| files.counted(name)).filter_map(|name| files.len(name)).sum();
    let added: u64 = extra.iter().filter(|added| !is_large_kind(added.name)).map(|added| added.source.len()).sum();
    if held.saturating_add(added).saturating_add(slides_len) > files.total_bytes {
        return Err(PackError::TooLarge(format!(
            "it holds more than {} bytes",
            files.total_bytes
        )));
    }
    let every: u64 = names.iter().filter_map(|name| files.len(name)).sum();
    moving.total = every + extra.iter().map(|added| added.source.len()).sum::<u64>() + slides_len;

    std::fs::create_dir_all(folder)?;
    for directory in &files.directories {
        std::fs::create_dir_all(target_in(folder, directory)?)?;
    }
    for name in &names {
        unpack_entry(files, folder, name, moving)?;
    }
    for added in extra {
        let mut file = create_new(&target_in(folder, added.name)?)?;
        moving.copy_source(added.source, &mut file, added.name)?;
    }
    match slides {
        Some(slides) => {
            let mut file = create_new(&target_in(folder, SLIDES_NAME)?)?;
            moving.copy_source(Source::Bytes(slides.as_bytes()), &mut file, SLIDES_NAME)
        }
        None => unpack_entry(files, folder, SLIDES_NAME, moving),
    }
}

/// Writes the pack's file `name` as the new file of that name in `folder`, a piece at a time.
fn unpack_entry(files: &PackedFiles, folder: &Path, name: &str, moving: &mut Moving<'_>) -> Result<(), PackError> {
    let &i = files.index.get(name).ok_or_else(|| PackError::Corrupt(format!("{name} is missing")))?;
    let mut zip = files.zip();
    let mut entry = zip.by_index(i)?;
    let size = entry.size();
    let limit = if files.counted(name) { files.entry_bytes } else { MAX_LARGE_FILE };
    if size > limit {
        return Err(PackError::TooLarge(format!("{name} is bigger than {limit} bytes")));
    }
    let mut file = create_new(&target_in(folder, name)?)?;
    moving.copy(&mut entry, &mut file, size, name, true)
}

/// Copies the packed talk `packed` into the file `out`, with `slides` as its `slides.md` and a
/// `talk.json` naming `app_version`. Every other entry is copied as it is, byte for byte, without
/// being unpacked. `out` may be `packed` itself: the copy is written beside it and takes its
/// place only once it's whole.
///
/// The files of `extra` are written as new entries after them. One the pack has already with
/// the same bytes is the entry that's there. One it has with other bytes is refused, and
/// nothing is written: an entry is never replaced.
pub fn repack_with_slides(
    packed: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
    extra: &[NewFile],
) -> Result<(), PackError> {
    repack(packed, out, slides, app_version, &given_bytes(extra), &|_, _| {}, &AtomicBool::new(false))
}

/// Copies a packed talk as [`repack_with_slides`] does, with the files of `extra` taken from where
/// they're given. Every entry is copied a piece at a time, telling `progress` the bytes done and
/// the bytes in all as it goes; if `stop` is set, it stops, and `out` is as it was.
pub fn repack_with_slides_from(
    packed: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
    extra: &[NewFileFrom],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    repack(packed, out, slides, app_version, &given_from(extra), progress, stop)
}

/// The pack being copied from, read through this so that while `counting`, the copy can be
/// stopped and tell how far it's got.
struct Watched<'a, 'm> {
    inner: PackReader,
    counting: &'a Cell<bool>,
    moving: &'a RefCell<Moving<'m>>,
}

impl Read for Watched<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.counting.get() {
            return self.inner.read(buf);
        }
        if self.moving.borrow().check().is_err() {
            return Err(std::io::Error::other(StopAsked));
        }
        let want = buf.len().min(PIECE);
        let n = self.inner.read(&mut buf[..want])?;
        self.moving.borrow_mut().moved(n as u64);
        Ok(n)
    }
}

impl Seek for Watched<'_, '_> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

fn repack(
    packed: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
    extra: &[(&str, Given<'_>)],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    let files = open_packed(packed)?;
    if slides.len() as u64 > files.entry_bytes {
        return Err(PackError::TooLarge(format!(
            "{SLIDES_NAME} is bigger than {} bytes",
            files.entry_bytes
        )));
    }
    let names = files.names();
    let extra = new_files(&names, |name, source| files.holds(name, source), extra, files.entry_bytes)?;
    if !extra.is_empty() {
        let limits = PackLimits::default();
        // The index that's replaced isn't kept, so it isn't counted.
        let names: Vec<String> = names.iter().filter(|name| !replaces_index(&extra) || *name != INDEX_FILE).cloned().collect();
        if names.len() + files.directories.len() + extra.len() > limits.entries {
            return Err(PackError::TooLarge(format!("it would hold more than {} files", limits.entries)));
        }
        let held: u64 = names.iter().filter(|name| *name != SLIDES_NAME && files.counted(name)).filter_map(|name| files.len(name)).sum();
        let added: u64 = extra.iter().filter(|added| !is_large_kind(added.name)).map(|added| added.source.len()).sum();
        if held.saturating_add(added).saturating_add(slides.len() as u64) > files.total_bytes {
            return Err(PackError::TooLarge(format!("it would hold more than {} bytes", files.total_bytes)));
        }
    }
    let manifest = manifest_for(app_version);
    let own_index = !replaces_index(&extra);
    let copied = |name: &str| name != SLIDES_NAME && name != MANIFEST_NAME && (own_index || name != INDEX_FILE);
    let total = {
        let zip = files.zip();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let entry = zip.by_index_data(i)?;
            if copied(&entry.name()?) {
                total = total.saturating_add(entry.compressed_size());
            }
        }
        total + slides.len() as u64 + manifest.len() as u64 + extra.iter().map(|added| added.source.len()).sum::<u64>()
    };
    atomically(out, move |file| {
        let PackedFiles { zip, pack, pack_len, .. } = files;
        // Read again through a reader of its own, which can be stopped.
        drop(zip);
        let moving = RefCell::new(Moving::new(progress, stop, total));
        let counting = Cell::new(false);
        let mut zip = ZipArchive::new(Watched { inner: PackReader::new(pack, 0, pack_len), counting: &counting, moving: &moving })?;
        let mut writer = ZipWriter::new(file);
        for i in 0..zip.len() {
            moving.borrow().check()?;
            let entry = zip.by_index_raw(i)?;
            let name = entry.name()?.to_string();
            if !own_index && name == INDEX_FILE {
                continue;
            }
            let replacement = match name.as_str() {
                SLIDES_NAME => Some(slides.as_bytes()),
                MANIFEST_NAME => Some(manifest.as_slice()),
                _ => None,
            };
            match replacement {
                Some(bytes) => {
                    drop(entry);
                    let options = SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Deflated)
                        .last_modified_time(DateTime::default())
                        .unix_permissions(0o644);
                    writer.start_file(name.as_str(), options).map_err(io_other)?;
                    moving.borrow_mut().copy_source(Source::Bytes(bytes), &mut writer, &name)?;
                }
                None => {
                    let len = entry.compressed_size();
                    let before = moving.borrow().done;
                    counting.set(true);
                    let raw = writer.raw_copy_file(entry);
                    counting.set(false);
                    raw?;
                    // An entry cut short by the end of the file isn't copied as if it were whole.
                    if moving.borrow().done - before != len {
                        return Err(PackError::Corrupt(format!("{name} doesn't hold what it says")));
                    }
                }
            }
        }
        for added in &extra {
            moving.borrow().check()?;
            writer.start_file(added.name, entry_options(added.name, added.source.len())).map_err(io_other)?;
            moving.borrow_mut().copy_source(added.source, &mut writer, added.name)?;
        }
        writer.finish().map_err(io_other)?;
        // The pack is let go of here, before the copy takes its place: Windows won't replace
        // a file that's open.
        drop(zip);
        Ok(())
    })
}

/// Copies the talk in `from` into the folder `to`, which must be missing or empty, with
/// `slides` as its `slides.md`. What's copied is what [`pack_folder`] would pack, with the same
/// refusals and limits, and what it left out is reported as a pack's is. The files of `extra` are
/// written as well, as [`repack_with_slides`] adds them. `from` isn't touched, and a failure
/// leaves `to` as it was.
pub fn copy_talk_folder(from: &Path, to: &Path, slides: &str, extra: &[NewFile]) -> Result<PackReport, PackError> {
    copy_talk(from, to, slides, &given_bytes(extra), &|_, _| {}, &AtomicBool::new(false))
}

/// Copies the talk in `from` as [`copy_talk_folder`] does, with the files of `extra` taken from
/// where they're given. Every file is copied a piece at a time, telling `progress` the bytes done
/// and the bytes in all as it goes; if `stop` is set, it stops, and `to` is as it was.
pub fn copy_talk_folder_from(
    from: &Path,
    to: &Path,
    slides: &str,
    extra: &[NewFileFrom],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<PackReport, PackError> {
    copy_talk(from, to, slides, &given_from(extra), progress, stop)
}

fn copy_talk(
    from: &Path,
    to: &Path,
    slides: &str,
    extra: &[(&str, Given<'_>)],
    progress: &dyn Fn(u64, u64),
    stop: &AtomicBool,
) -> Result<PackReport, PackError> {
    if lies_within(to, from) {
        // A copy in the talk's own folders would travel in every pack made from it.
        return Err(PackError::InsideTheTalk);
    }
    let mut skipped = Vec::new();
    let found = collect(from, &mut skipped)?;
    let limits = PackLimits::default();
    let extra = new_folder_files(&found, extra, limits)?;
    let own = own_files(&found, Some(slides), &extra)?;
    check_sizes(&own, &extra, limits)?;
    let existed = missing_or_empty(to)?;
    let total = total_of(&own) + extra.iter().map(|added| added.source.len()).sum::<u64>();
    let mut moving = Moving::new(progress, stop, total);
    let result = copy_into(&own, to, slides, &extra, &mut moving);
    if result.is_err() {
        clear_away(to, existed);
    }
    result.map(|()| PackReport { skipped, outside_pictures: pictures_outside(from, slides) })
}

/// Whether `path`, which needn't exist yet, is `folder` or lies anywhere under it. Both are
/// compared as the system resolves them, links and all: a path that doesn't exist is resolved
/// from the nearest folder above it that does.
pub fn lies_within(path: &Path, folder: &Path) -> bool {
    let Ok(folder) = folder.canonicalize() else {
        return false;
    };
    let Ok(path) = std::path::absolute(path) else {
        return false;
    };
    // The part of `path` that doesn't exist yet, last part first.
    let mut rest = Vec::new();
    let mut existing = path.as_path();
    let resolved = loop {
        if let Ok(resolved) = existing.canonicalize() {
            break resolved;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            // `..` or the root: resolve what's left by its parts below.
            _ => match existing.parent() {
                Some(parent) => {
                    rest.push(std::ffi::OsString::from(".."));
                    existing = parent;
                }
                None => return false,
            },
        }
    };
    let mut full = resolved;
    for part in rest.iter().rev() {
        if part == ".." {
            full.pop();
        } else if part != "." {
            full.push(part);
        }
    }
    full.starts_with(&folder)
}

/// `slides.md` is written last, as in [`unpack_into`]. `own` holds it as it's to be.
fn copy_into(
    own: &[(&str, Source<'_>)],
    to: &Path,
    slides: &str,
    extra: &[Added<'_>],
    moving: &mut Moving<'_>,
) -> Result<(), PackError> {
    std::fs::create_dir_all(to)?;
    let files = own.iter().copied().filter(|(name, _)| *name != SLIDES_NAME);
    for (name, source) in files.chain(extra.iter().map(|added| (added.name, added.source))) {
        let mut file = create_new(&target_in(to, name)?)?;
        moving.copy_source(source, &mut file, name)?;
    }
    let mut file = create_new(&target_in(to, SLIDES_NAME)?)?;
    moving.copy_source(Source::Bytes(slides.as_bytes()), &mut file, SLIDES_NAME)
}

/// Writes `out` by way of a temporary file beside it, so a failure never leaves a half-written
/// file in its place.
pub fn write_atomically(
    out: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    atomically(out, write)
}

/// [`write_atomically`], for a `write` whose errors are of any kind the disk's can be made into.
fn atomically<E: From<std::io::Error>>(out: &Path, write: impl FnOnce(&mut File) -> Result<(), E>) -> Result<(), E> {
    let name = out
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?
        .to_string_lossy();
    let (mut file, temp) = create_temp(out, &name)?;
    let result = (|| -> Result<(), E> {
        write(&mut file)?;
        // A file being replaced keeps its permissions. Where they can't be set, the new content
        // still matters more.
        if let Ok(existing) = std::fs::metadata(out) {
            file.set_permissions(existing.permissions()).ok();
        }
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

/// Writes `bytes` as the new file `out` by way of a temporary file beside it, so a failure never
/// leaves a half-written file there. Whatever is at `out` already is never replaced: that's an
/// error of the kind `AlreadyExists`.
pub fn write_new_atomically(out: &Path, bytes: &[u8]) -> std::io::Result<()> {
    new_atomically(out, |file| file.write_all(bytes))
}

/// Writes the new file `out` by way of a temporary file beside it, as [`write_new_atomically`]
/// does, with what's in it written by `write`, a piece at a time if it likes. A failure, or a
/// stop, leaves nothing at `out`, and whatever is there already is never replaced.
pub fn write_new_atomically_with(out: &Path, write: impl FnOnce(&mut File) -> Result<(), PackError>) -> Result<(), PackError> {
    new_atomically(out, write)
}

/// [`write_new_atomically`], with what's written written by `write`.
fn new_atomically<E: From<std::io::Error>>(out: &Path, write: impl FnOnce(&mut File) -> Result<(), E>) -> Result<(), E> {
    let name = out
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?
        .to_string_lossy();
    let (mut file, temp) = create_temp(out, &name)?;
    let result = (|| -> Result<(), E> {
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        place_without_replacing(&temp, out)?;
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
    // Linked into place, or never placed: either way it's not needed. Once renamed, it's gone.
    let _ = std::fs::remove_file(&temp);
    result
}

/// Gives the file `temp` the name `out` too, unless something has it. A link can't replace
/// anything, so no other program can slip a file in between looking and placing.
fn place_without_replacing(temp: &Path, out: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(temp, out) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        // A disk that can't link, such as one formatted FAT: the next best is to look first.
        Err(_) => {
            if std::fs::symlink_metadata(out).is_ok() {
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "a file is there already"));
            }
            rename_with_retry(temp, out)
        }
    }
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
