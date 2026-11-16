//! Files added to a talk: pictures, data and components brought in from outside it. What may be
//! added, under what name, and which of them the talk's text uses, so that a save writes those.
//! The bytes are kept in memory, in the talk's [`AddedSet`], until a save writes them.
//!
//! [`AddedSet`]: slides_engine::AddedSet

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use slides_engine::{PictureFormat, TalkFiles, picture_as_png, picture_format};
use slides_format::{Block, Talk};

/// The largest file that can be added to a talk.
pub const MAX_ADDED_BYTES: usize = 32 * 1024 * 1024;

/// The longest a file's name can be, without its extension.
const MAX_STEM: usize = 40;
/// The most a taken name is numbered up to.
const MAX_NUMBER: u32 = 99;
/// Names Windows won't create as files, whatever their extension.
const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];
/// What a WebAssembly module starts with.
const WASM_MAGIC: &[u8] = b"\0asm";

/// What a file added to a talk is for, which says where it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Picture,
    /// Data a component reads, such as a chart's figures.
    Data,
    /// The source of one of the talk's own components.
    ComponentSource,
    /// A component built, ready to run.
    ComponentBuild,
}

impl FileKind {
    /// The talk's folder the file goes in.
    pub fn folder(self) -> &'static str {
        match self {
            FileKind::Picture | FileKind::Data => "assets",
            FileKind::ComponentSource => "components",
            FileKind::ComponentBuild => "build",
        }
    }

    /// The extensions a file of this kind can have, the one it's given first.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileKind::Picture => &["png", "jpg", "jpeg", "webp", "svg"],
            FileKind::Data => &["csv"],
            FileKind::ComponentSource => &["rs"],
            FileKind::ComponentBuild => &["wasm"],
        }
    }

    fn is_component(self) -> bool {
        matches!(self, FileKind::ComponentSource | FileKind::ComponentBuild)
    }
}

/// Why a file wasn't added. Its words are the sentence the person adding it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotAdded {
    TooLarge,
    NotAPicture,
    NotThatKind,
    Unreadable,
    NameTaken,
}

impl std::fmt::Display for NotAdded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NotAdded::TooLarge => "That file is larger than 32 MB, so it wasn't added.",
            NotAdded::NotAPicture => "That file isn't a picture this app can show.",
            NotAdded::NotThatKind => "That kind of file can't be used here.",
            NotAdded::Unreadable => "That file couldn't be read.",
            NotAdded::NameTaken => "This talk already has a different file with that name.",
        })
    }
}

impl std::error::Error for NotAdded {}

/// `name`'s extension, if it ends in a dot and up to ten letters and digits, and what's before it.
fn split_extension(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if (1..=10).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) => (stem, Some(ext)),
        _ => (name, None),
    }
}

/// `suggested` made a name that's safe on every system: lower case, each run of anything but
/// `a`–`z` and `0`–`9` one `-`, none at either end, at most 40 characters, `file` if nothing is
/// left, and never a name Windows keeps for a device. An extension `suggested` ends in is left out.
fn safe_stem(suggested: &str) -> String {
    let (stem, _) = split_extension(suggested);
    let mut safe = String::new();
    for c in stem.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            safe.push(c);
        } else if !safe.is_empty() && !safe.ends_with('-') {
            safe.push('-');
        }
    }
    safe.truncate(MAX_STEM);
    let mut safe = safe.trim_end_matches('-').to_string();
    if safe.is_empty() {
        safe = "file".to_string();
    }
    if DEVICE_NAMES.contains(&safe.as_str()) {
        safe.push_str("-file");
    }
    safe
}

/// A file name for `suggested` with the extension `ext`, safe on every system: see the rules of
/// the stem in [`safe_stem`]. The extension is kept to lower-case letters.
pub fn safe_name(suggested: &str, ext: &str) -> String {
    let ext: String = ext.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_lowercase()).collect();
    format!("{}.{ext}", safe_stem(suggested))
}

/// `bytes` checked to be a picture the engine shows, with the extension its content calls for,
/// whatever it was called: a BMP, TIFF or GIF, or another kind the engine reads but that isn't
/// kept as it is, is written again as a PNG. An SVG is kept as it is: the engine draws it
/// without its scripts or anything it links to. `hint_ext` is the name's extension, which only
/// picks between `jpg` and `jpeg`.
pub fn picture_bytes(bytes: Vec<u8>, hint_ext: Option<&str>) -> Result<(Vec<u8>, &'static str), NotAdded> {
    if bytes.len() > MAX_ADDED_BYTES {
        return Err(NotAdded::TooLarge);
    }
    let ext = match picture_format(&bytes).ok_or(NotAdded::NotAPicture)? {
        PictureFormat::Png => "png",
        PictureFormat::Jpeg if hint_ext.is_some_and(|hint| hint.eq_ignore_ascii_case("jpeg")) => "jpeg",
        PictureFormat::Jpeg => "jpg",
        PictureFormat::Webp => "webp",
        PictureFormat::Svg => "svg",
        PictureFormat::Gif | PictureFormat::Bmp | PictureFormat::Tiff | PictureFormat::Other => {
            let png = picture_as_png(&bytes).ok_or(NotAdded::NotAPicture)?;
            if png.len() > MAX_ADDED_BYTES {
                return Err(NotAdded::TooLarge);
            }
            return Ok((png, "png"));
        }
    };
    Ok((bytes, ext))
}

/// `bytes` checked to be a file of `kind`, as they'd be written, with the extension they go under.
fn checked(kind: FileKind, suggested: &str, bytes: Vec<u8>) -> Result<(Vec<u8>, &'static str), NotAdded> {
    if bytes.len() > MAX_ADDED_BYTES {
        return Err(NotAdded::TooLarge);
    }
    let (_, given) = split_extension(suggested);
    if kind == FileKind::Picture {
        return picture_bytes(bytes, given);
    }
    let allowed = kind.extensions();
    let ext = match given {
        None => allowed[0],
        Some(given) => *allowed.iter().find(|ext| ext.eq_ignore_ascii_case(given)).ok_or(NotAdded::NotThatKind)?,
    };
    let fits = match kind {
        FileKind::Data | FileKind::ComponentSource => std::str::from_utf8(&bytes).is_ok(),
        FileKind::ComponentBuild => bytes.starts_with(WASM_MAGIC),
        FileKind::Picture => unreachable!("a picture is checked above"),
    };
    if !fits {
        return Err(NotAdded::NotThatKind);
    }
    Ok((bytes, ext))
}

/// Where a file being added goes.
pub(crate) enum Placed {
    /// The talk has these bytes already, at this path.
    There(String),
    /// A new file, at this path, with these bytes.
    New(String, Vec<u8>),
}

/// Where the file `suggested`, holding `bytes`, goes in the talk whose files are `files`, once
/// checked to be of `kind`. Bytes it has already, in a file of that kind, are that file. A name
/// it has is told apart by a number, `-2` up to `-99`, except a component's, which must be the
/// name it has. A name counts as taken whatever the case of its letters, and when `taken` says so.
pub(crate) fn place(
    files: &dyn TalkFiles,
    kind: FileKind,
    suggested: &str,
    bytes: Vec<u8>,
    taken: impl Fn(&str) -> bool,
) -> Result<Placed, NotAdded> {
    let (bytes, ext) = checked(kind, suggested, bytes)?;
    let folder = kind.folder();
    let there = files.names_in(folder);
    let same_bytes = |path: &str| files.len(path) == Some(bytes.len() as u64) && files.read(path).is_some_and(|held| held == bytes);
    let of_this_kind = |path: &str| {
        let (_, ext) = split_extension(path);
        ext.is_some_and(|ext| kind.extensions().iter().any(|allowed| allowed.eq_ignore_ascii_case(ext)))
    };
    let stem = safe_stem(suggested);
    if !kind.is_component()
        && let Some(path) = there.iter().find(|path| of_this_kind(path) && same_bytes(path))
    {
        return Ok(Placed::There(path.clone()));
    }
    let lower: BTreeSet<String> = there.iter().map(|path| path.to_ascii_lowercase()).collect();
    let free = |path: &str| !lower.contains(path) && files.len(path).is_none() && !taken(path);
    let first = format!("{folder}/{stem}.{ext}");
    if kind.is_component() {
        if free(&first) {
            return Ok(Placed::New(first, bytes));
        }
        return match there.iter().find(|path| path.eq_ignore_ascii_case(&first)) {
            Some(path) if same_bytes(path) => Ok(Placed::There(path.clone())),
            _ => Err(NotAdded::NameTaken),
        };
    }
    if free(&first) {
        return Ok(Placed::New(first, bytes));
    }
    for number in 2..=MAX_NUMBER {
        let suffix = format!("-{number}");
        let mut base = stem.clone();
        base.truncate(MAX_STEM - suffix.len());
        let path = format!("{folder}/{}{suffix}.{ext}", base.trim_end_matches('-'));
        if free(&path) {
            return Ok(Placed::New(path, bytes));
        }
    }
    Err(NotAdded::NameTaken)
}

/// Whether `path` is one [`place`] could have made: a plain name in one of the folders a file is
/// added to, with an extension a file there can have.
pub(crate) fn is_added_path(path: &str) -> bool {
    let Some((folder, name)) = path.split_once('/') else {
        return false;
    };
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let kinds = [FileKind::Picture, FileKind::Data, FileKind::ComponentSource, FileKind::ComponentBuild];
    kinds.iter().any(|kind| kind.folder() == folder && kind.extensions().contains(&ext))
        && (1..=MAX_STEM).contains(&stem.len())
        && stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `talk` uses the file at `path`: a picture shows it, or an attribute of a tag on a slide
/// is exactly it. A component's source, `components/<name>.rs`, and its build,
/// `build/<name>.wasm`, are used when a tag on a slide has the name a file of it is given here.
/// Nothing else counts: a path only written in words or code isn't a use.
pub fn talk_uses(talk: &Talk, path: &str) -> bool {
    let component = path
        .strip_prefix("components/")
        .and_then(|name| name.strip_suffix(".rs"))
        .or_else(|| path.strip_prefix("build/").and_then(|name| name.strip_suffix(".wasm")));
    let names_it = |tag_name: &str| component.is_some_and(|stem| safe_stem(tag_name) == stem);
    let mut blocks: Vec<&Block> = Vec::new();
    for slide in &talk.slides {
        if slide.inline_tags.iter().any(|name| names_it(name)) {
            return true;
        }
        blocks.extend(&slide.blocks);
    }
    while let Some(block) = blocks.pop() {
        match block {
            Block::Picture { src, .. } if src == path => return true,
            Block::List { items, .. } => blocks.extend(items.iter().flatten()),
            Block::Tag(tag) => {
                if names_it(&tag.name) || tag.attrs.iter().any(|attr| attr.value == path) {
                    return true;
                }
                blocks.extend(&tag.children);
            }
            _ => {}
        }
    }
    false
}

/// The bytes of the file at `path`, outside any talk, to add to one. A folder, or a link to
/// anything but a file, can't be read; nor can a file that holds more than
/// [`MAX_ADDED_BYTES`], which is looked at before it's read and again as it is.
pub(crate) fn read_outside(path: &Path) -> Result<Vec<u8>, NotAdded> {
    // Asked first, as opening a named pipe waits until something writes to it.
    let metadata = std::fs::metadata(path).map_err(|_| NotAdded::Unreadable)?;
    if !metadata.is_file() {
        return Err(NotAdded::Unreadable);
    }
    let file = std::fs::File::open(path).map_err(|_| NotAdded::Unreadable)?;
    let opened = file.metadata().map_err(|_| NotAdded::Unreadable)?;
    if !opened.is_file() {
        return Err(NotAdded::Unreadable);
    }
    if opened.len() > MAX_ADDED_BYTES as u64 {
        return Err(NotAdded::TooLarge);
    }
    let mut bytes = Vec::new();
    // Bounded, in case the file grows while it is read.
    file.take(MAX_ADDED_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|_| NotAdded::Unreadable)?;
    if bytes.len() > MAX_ADDED_BYTES {
        return Err(NotAdded::TooLarge);
    }
    Ok(bytes)
}
