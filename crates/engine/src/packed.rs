//! Packed `.sptx` talks: a zip holding `slides.md`, a `talk.json` manifest and the files the
//! talk uses. A packed file is untrusted input, so it is checked when it opens (names, sizes,
//! count) and read in bounded pieces afterwards.

use crate::files::{FolderFiles, MAX_FILE_BYTES, TalkFiles};
use slides_format::{Block, Talk, parse_talk};
use std::collections::{BTreeSet, HashMap};
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
fn safe_name(name: &str) -> bool {
    let name = name.strip_suffix('/').unwrap_or(name);
    !name.is_empty() && name.split('/').all(safe_part)
}

/// `path` with empty and `.` parts dropped and `..` resolved, or `None` if it starts at the root
/// or climbs out, as a folder talk would refuse it too.
fn normalise(path: &str) -> Option<String> {
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

    /// The size the entry states, which was held to the pack's limits when it opened.
    fn len(&self, path: &str) -> Option<u64> {
        let &i = self.index.get(&normalise(path)?)?;
        let zip = self.zip.lock().unwrap_or_else(|e| e.into_inner());
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

/// Holds what goes in from a folder to the limits of a pack: how many files, and how big each
/// is and all are. `slides`, if given, is what `slides.md` will hold in place of the file.
fn check_sizes(
    files: &[(String, std::path::PathBuf)],
    slides: Option<&str>,
    limits: PackLimits,
) -> Result<(), PackError> {
    if files.len() + 1 > limits.entries {
        return Err(PackError::TooLarge(format!(
            "it would hold more than {} files",
            limits.entries
        )));
    }
    let mut total = 0u64;
    for (name, path) in files {
        let size = match slides {
            Some(slides) if name == SLIDES_NAME => slides.len() as u64,
            _ => std::fs::metadata(path)?.len(),
        };
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
    Ok(())
}

/// Packs the talk in `folder` into the file `out`, replacing it only if packing succeeds.
pub fn pack_folder(folder: &Path, out: &Path, app_version: &str) -> Result<PackReport, PackError> {
    pack(folder, out, None, app_version)
}

/// Packs the talk in `folder` into the file `out` as [`pack_folder`] does, with `slides` as its
/// `slides.md` in place of the file in the folder, which isn't touched.
pub fn pack_folder_with_slides(
    folder: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
) -> Result<PackReport, PackError> {
    pack(folder, out, Some(slides), app_version)
}

fn pack(
    folder: &Path,
    out: &Path,
    slides: Option<&str>,
    app_version: &str,
) -> Result<PackReport, PackError> {
    let mut skipped = Vec::new();
    let files = collect(folder, &mut skipped)?;
    let limits = PackLimits::default();
    check_sizes(&files, slides, limits)?;
    let manifest = manifest_for(app_version);
    // Read once, so that what's packed and what's reported come from the same text.
    let source = match slides {
        Some(slides) => slides.to_string(),
        None => std::fs::read_to_string(folder.join(SLIDES_NAME)).unwrap_or_default(),
    };

    let mut entries: Vec<(String, Option<&Path>)> = files
        .iter()
        .map(|(name, path)| (name.clone(), Some(path.as_path())))
        .collect();
    entries.push((MANIFEST_NAME.to_string(), None));
    entries.sort();

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
            match (path, slides) {
                (None, _) => zip.write_all(&manifest)?,
                (Some(_), Some(slides)) if name == SLIDES_NAME => {
                    zip.write_all(slides.as_bytes())?
                }
                (Some(path), _) => {
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
    Ok(PackReport { skipped, outside_pictures: pictures_outside(folder, &source) })
}

/// Unpacks a packed talk into `folder`, which must be missing or empty. `talk.json` isn't
/// written, since packing makes a new one. A failure leaves `folder` as it was.
pub fn unpack_to_folder(packed: &Path, folder: &Path) -> Result<(), PackError> {
    unpack(packed, folder, None)
}

/// Unpacks a packed talk into `folder` as [`unpack_to_folder`] does, with `slides` as its
/// `slides.md` in place of the one in the pack.
pub fn unpack_with_slides(packed: &Path, folder: &Path, slides: &str) -> Result<(), PackError> {
    unpack(packed, folder, Some(slides))
}

fn unpack(packed: &Path, folder: &Path, slides: Option<&str>) -> Result<(), PackError> {
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
    let existed = missing_or_empty(folder)?;
    let result = unpack_into(&files, folder, slides);
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

/// Writes `bytes` as the new file `target`, making the folders it's in.
fn write_new(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    file.write_all(bytes)
}

/// `slides.md` is written last, so that a folder cut short has no talk in it rather than a
/// talk missing some of its files.
fn unpack_into(files: &PackedFiles, folder: &Path, slides: Option<&str>) -> Result<(), PackError> {
    std::fs::create_dir_all(folder)?;
    for directory in &files.directories {
        std::fs::create_dir_all(target_in(folder, directory)?)?;
    }
    let mut written = 0u64;
    let mut names = files.names();
    names.retain(|name| name != MANIFEST_NAME && name != SLIDES_NAME);
    names.push(SLIDES_NAME.to_string());
    for name in names {
        let bytes = match slides {
            Some(slides) if name == SLIDES_NAME => slides.as_bytes().to_vec(),
            _ => files
                .read_checked(&name)?
                .ok_or_else(|| PackError::Corrupt(format!("{name} is missing")))?,
        };
        written = written.saturating_add(bytes.len() as u64);
        if written > files.total_bytes {
            return Err(PackError::TooLarge(format!(
                "it holds more than {} bytes",
                files.total_bytes
            )));
        }
        write_new(&target_in(folder, &name)?, &bytes)?;
    }
    Ok(())
}

/// Copies the packed talk `packed` into the file `out`, with `slides` as its `slides.md` and a
/// `talk.json` naming `app_version`. Every other entry is copied as it is, byte for byte, without
/// being unpacked. `out` may be `packed` itself: the copy is written beside it and takes its
/// place only once it's whole.
pub fn repack_with_slides(
    packed: &Path,
    out: &Path,
    slides: &str,
    app_version: &str,
) -> Result<(), PackError> {
    let files = open_packed(packed)?;
    if slides.len() as u64 > files.entry_bytes {
        return Err(PackError::TooLarge(format!(
            "{SLIDES_NAME} is bigger than {} bytes",
            files.entry_bytes
        )));
    }
    let manifest = manifest_for(app_version);
    write_atomically(out, move |file| {
        let mut zip = files.zip.into_inner().unwrap_or_else(|e| e.into_inner());
        let mut writer = ZipWriter::new(file);
        for i in 0..zip.len() {
            let entry = zip.by_index_raw(i).map_err(io_other)?;
            let name = entry.name().map_err(io_other)?.to_string();
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
                    writer.write_all(bytes)?;
                }
                None => writer.raw_copy_file(entry).map_err(io_other)?,
            }
        }
        writer.finish().map_err(io_other)?;
        // The pack is let go of here, before the copy takes its place: Windows won't replace
        // a file that's open.
        drop(zip);
        Ok(())
    })?;
    Ok(())
}

/// Copies the talk in `from` into the folder `to`, which must be missing or empty, with
/// `slides` as its `slides.md`. What's copied is what [`pack_folder`] would pack, with the same
/// refusals and limits, and the names it left out are returned. `from` isn't touched, and a
/// failure leaves `to` as it was.
pub fn copy_talk_folder(from: &Path, to: &Path, slides: &str) -> Result<Vec<String>, PackError> {
    let mut skipped = Vec::new();
    let files = collect(from, &mut skipped)?;
    let limits = PackLimits::default();
    check_sizes(&files, Some(slides), limits)?;
    let existed = missing_or_empty(to)?;
    let result = copy_into(&files, to, slides, limits);
    if result.is_err() {
        clear_away(to, existed);
    }
    result.map(|()| skipped)
}

/// `slides.md` is written last, as in [`unpack_into`].
fn copy_into(
    files: &[(String, std::path::PathBuf)],
    to: &Path,
    slides: &str,
    limits: PackLimits,
) -> Result<(), PackError> {
    std::fs::create_dir_all(to)?;
    for (name, path) in files {
        if name == SLIDES_NAME {
            continue;
        }
        let target = target_in(to, name)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        // Bounded, in case the file grows while it is read.
        let mut reader = File::open(path)?.take(limits.entry_bytes + 1);
        if std::io::copy(&mut reader, &mut file)? > limits.entry_bytes {
            return Err(PackError::TooLarge(format!(
                "{name} is bigger than {} bytes",
                limits.entry_bytes
            )));
        }
    }
    write_new(&target_in(to, SLIDES_NAME)?, slides.as_bytes())?;
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
