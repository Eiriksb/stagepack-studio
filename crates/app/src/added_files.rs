//! Files added to a talk: pictures, data and components brought in from outside it. What may be
//! added, under what name, and which of them the talk's text uses, so that a save writes those.
//! The bytes are kept in memory, in the talk's [`AddedSet`], until a save writes them.
//!
//! [`AddedSet`]: slides_engine::AddedSet

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use slides_components::{BuildIndex, INDEX_FILE};
use slides_engine::{MAX_INDEX_BYTES, MAX_SOURCE_BYTES, PictureFormat, TalkFiles, normalise, picture_as_png, picture_format};
use slides_format::{Block, Change, SourceEdit, Talk, parse_talk, set_tag_attr};

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
/// What the window says when a file kept from last time has the name of a different one the talk
/// has now, and the text brought back couldn't be made to name the kept one instead.
pub const KEPT_FILE_CLASH: &str =
    "A file kept from last time has the same name as a different one in the talk, so a slide may show the talk's own instead.";
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

    pub(crate) fn is_component(self) -> bool {
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

/// The name a component's file is given for `suggested`, without its extension: the name as it
/// is, as a component's file is never renamed. The engine pairs a source, `components/<stem>.rs`,
/// with its build, `build/<stem>.wasm`, by that stem, so making it "safe" would part them. `None`
/// if it isn't a stem the engine takes (letters, digits, `_` and `-`, at most 64), or is a name
/// Windows keeps for a device.
pub(crate) fn component_stem(suggested: &str) -> Option<&str> {
    let (stem, _) = split_extension(suggested);
    let device = DEVICE_NAMES.iter().any(|device| device.eq_ignore_ascii_case(stem));
    (slides_components::safe_stem(stem) && !device).then_some(stem)
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
        FileKind::Data => std::str::from_utf8(&bytes).is_ok(),
        // The engine finds the component's tag in its source: one it can't is no component.
        FileKind::ComponentSource => std::str::from_utf8(&bytes).ok().and_then(slides_components::component_tag).is_some(),
        FileKind::ComponentBuild => bytes.starts_with(WASM_MAGIC),
        FileKind::Picture => unreachable!("a picture is checked above"),
    };
    if !fits || (kind.is_component() && component_stem(suggested).is_none()) {
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
    let lower: BTreeSet<String> = there.iter().map(|path| path.to_ascii_lowercase()).collect();
    let free = |path: &str| !lower.contains(&path.to_ascii_lowercase()) && files.len(path).is_none() && !taken(path);
    if kind.is_component() {
        let stem = component_stem(suggested).expect("checked above");
        let path = format!("{folder}/{stem}.{ext}");
        if free(&path) {
            return Ok(Placed::New(path, bytes));
        }
        return match there.iter().find(|other| other.eq_ignore_ascii_case(&path)) {
            Some(other) if same_bytes(other) => Ok(Placed::There(other.clone())),
            _ => Err(NotAdded::NameTaken),
        };
    }
    if let Some(path) = there.iter().find(|path| of_this_kind(path) && same_bytes(path)) {
        return Ok(Placed::There(path.clone()));
    }
    let stem = safe_stem(suggested);
    let first = format!("{folder}/{stem}.{ext}");
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

/// The kind of file that the path `path` is, if it's one [`place`] could have made: a plain name
/// in one of the folders a file is added to, with an extension a file there can have.
pub(crate) fn kind_of_path(path: &str) -> Option<FileKind> {
    let (folder, name) = path.split_once('/')?;
    let (stem, ext) = name.rsplit_once('.')?;
    let kinds = [FileKind::Picture, FileKind::Data, FileKind::ComponentSource, FileKind::ComponentBuild];
    let kind = kinds.into_iter().find(|kind| kind.folder() == folder && kind.extensions().contains(&ext))?;
    let plain = if kind.is_component() {
        component_stem(stem) == Some(stem) && !stem.contains('.')
    } else {
        (1..=MAX_STEM).contains(&stem.len()) && stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    plain.then_some(kind)
}

/// Whether `path` is one [`place`] could have made.
pub(crate) fn is_added_path(path: &str) -> bool {
    kind_of_path(path).is_some()
}

/// The tag the component whose source is `components/<stem>.rs` in `files` is, as the engine
/// finds it in the source.
fn component_tag_of(files: &dyn TalkFiles, stem: &str) -> Option<String> {
    let path = format!("components/{stem}.rs");
    if files.len(&path).is_some_and(|len| len > MAX_SOURCE_BYTES) {
        return None;
    }
    let source = files.read(&path)?;
    slides_components::component_tag(std::str::from_utf8(&source).ok()?)
}

/// The talk's build index in `files`, if it has one that can be read.
pub(crate) fn build_index(files: &dyn TalkFiles) -> Option<BuildIndex> {
    if files.len(INDEX_FILE).is_some_and(|len| len > MAX_INDEX_BYTES) {
        return None;
    }
    files.read(INDEX_FILE).and_then(|bytes| serde_json::from_slice::<BuildIndex>(&bytes).ok())
}

/// The tags the talk's build index in `files` gives the build `build/<file>.wasm`.
fn indexed_tags(files: &dyn TalkFiles, file: &str) -> Vec<String> {
    let Some(index) = build_index(files) else {
        return Vec::new();
    };
    index.components.into_iter().filter(|entry| entry.file == file).map(|entry| entry.tag).collect()
}

/// Whether `talk`, whose files are `files`, uses the file at `path`. Paths are compared as the
/// talk's files read them, so `./assets/a.png` is `assets/a.png`.
///
/// - A picture shows it, or an attribute of a tag on a slide names it.
/// - A component's source, `components/<stem>.rs`, is used when a tag on a slide is the tag the
///   engine finds in that source.
/// - A build, `build/<stem>.wasm`, is used when a tag on a slide is the tag the talk's build index
///   gives it, as the engine pairs them, or the tag of the source of the same stem, which is the
///   build that source is built to.
///
/// Nothing else counts: a path written only in words or code isn't a use.
pub fn talk_uses(talk: &Talk, files: &dyn TalkFiles, path: &str) -> bool {
    let Some(path) = normalise(path) else {
        return false;
    };
    let names_it = |written: &str| normalise(written).as_ref() == Some(&path);
    let mut tags: BTreeSet<&str> = BTreeSet::new();
    let mut blocks: Vec<&Block> = Vec::new();
    for slide in &talk.slides {
        tags.extend(slide.inline_tags.iter().map(String::as_str));
        blocks.extend(&slide.blocks);
    }
    while let Some(block) = blocks.pop() {
        match block {
            Block::Picture { src, .. } if names_it(src) => return true,
            Block::List { items, .. } => blocks.extend(items.iter().flatten()),
            Block::Tag(tag) => {
                if tag.attrs.iter().any(|attr| names_it(&attr.value)) {
                    return true;
                }
                tags.insert(&tag.name);
                blocks.extend(&tag.children);
            }
            _ => {}
        }
    }
    let on_a_slide = |tag: Option<String>| tag.is_some_and(|tag| tags.contains(tag.as_str()));
    if let Some(stem) = path.strip_prefix("components/").and_then(|name| name.strip_suffix(".rs")) {
        return on_a_slide(component_tag_of(files, stem));
    }
    if let Some(stem) = path.strip_prefix("build/").and_then(|name| name.strip_suffix(".wasm")) {
        return indexed_tags(files, stem).into_iter().any(|tag| on_a_slide(Some(tag))) || on_a_slide(component_tag_of(files, stem));
    }
    false
}

/// The bytes of the file at `path`, outside any talk, to add to it. A folder, or a link to
/// anything but a file, can't be read; nor can a file that holds more than
/// [`MAX_ADDED_BYTES`], which is looked at before it's read and again as it is.
pub(crate) fn read_outside(path: &Path) -> Result<Vec<u8>, NotAdded> {
    // Asked first, as opening a named pipe waits until something writes to it.
    let metadata = std::fs::metadata(path).map_err(|_| NotAdded::Unreadable)?;
    if !metadata.is_file() {
        return Err(NotAdded::Unreadable);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // And opened without waiting, in case it was swapped for one since.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| NotAdded::Unreadable)?;
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

/// Every file name the talk's slides write where a file is named, as written, in order: the
/// source of each picture and the value of each attribute of each tag, at any depth.
pub(crate) fn written_names(talk: &Talk) -> Vec<&str> {
    let mut names = Vec::new();
    for slide in &talk.slides {
        let mut blocks: Vec<&Block> = slide.blocks.iter().rev().collect();
        while let Some(block) = blocks.pop() {
            match block {
                Block::Picture { src, .. } => names.push(src.as_str()),
                Block::List { items, .. } => blocks.extend(items.iter().flatten().rev()),
                Block::Tag(tag) => {
                    names.extend(tag.attrs.iter().map(|attr| attr.value.as_str()));
                    blocks.extend(tag.children.iter().rev());
                }
                _ => {}
            }
        }
    }
    names
}

/// The edit that makes the first use of a renamed file in `talk` name its new path, as
/// `renamed_to` gives it for what's written: `Some(None)` if there's none left, and `None` if
/// that use can't be rewritten exactly.
fn rename_first_use(talk: &Talk, renamed_to: &dyn Fn(&str) -> Option<String>) -> Option<Option<Change>> {
    for (index, slide) in talk.slides.iter().enumerate() {
        let mut blocks: Vec<&Block> = slide.blocks.iter().rev().collect();
        while let Some(block) = blocks.pop() {
            match block {
                Block::Picture { src, range, .. } => {
                    let Some(to) = renamed_to(src) else { continue };
                    // The source's own bytes, as written: just after `(` or `(<`.
                    let written = talk.source.get(range.clone())?;
                    let mut at = written.match_indices(src.as_str()).map(|(at, _)| at).filter(|&at| {
                        let before = &written[..at];
                        before.ends_with("](") || before.ends_with("](<")
                    });
                    let (Some(at), None) = (at.next(), at.next()) else {
                        return None;
                    };
                    let start = range.start + at;
                    return Some(Some(Change::single(SourceEdit { range: start..start + src.len(), text: to })));
                }
                Block::List { items, .. } => blocks.extend(items.iter().flatten().rev()),
                Block::Tag(tag) => {
                    if let Some((attr, to)) = tag.attrs.iter().find_map(|attr| Some((attr, renamed_to(&attr.value)?))) {
                        return Some(Some(set_tag_attr(talk, index, &tag.range, &attr.name, Some(&to))?));
                    }
                    blocks.extend(tag.children.iter().rev());
                }
                _ => {}
            }
        }
    }
    Some(None)
}

/// `text` with every use of a file that `renames` gives a new path, `(from, to)`, naming `to`
/// instead: each picture's source and attribute of a tag that names `from`, however it's
/// written, rewritten in place and nothing else. Words that only mention it are left. `None`
/// if any use can't be rewritten exactly, so that the text stays whole as it was.
pub(crate) fn rename_uses(text: &str, renames: &[(String, String)]) -> Option<String> {
    let renamed_to = |written: &str| {
        let path = normalise(written)?;
        renames.iter().find(|(from, _)| *from == path).map(|(_, to)| to.clone())
    };
    let before = parse_talk(text);
    let expected: Vec<String> = written_names(&before).into_iter().map(|name| renamed_to(name).unwrap_or_else(|| name.to_string())).collect();
    let mut text = text.to_string();
    // Each edit takes one use away, so there are at most as many as there are names written.
    for _ in 0..=expected.len() {
        let talk = parse_talk(&text);
        match rename_first_use(&talk, &renamed_to)? {
            Some(change) => text = change.apply(&text),
            None => {
                let after: Vec<&str> = written_names(&talk);
                let same_slides = talk.slides.len() == before.slides.len();
                return (same_slides && after == expected).then_some(text);
            }
        }
    }
    None
}
