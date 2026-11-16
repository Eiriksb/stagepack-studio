//! What the app puts on the system clipboard, and what it takes from it. Copied text goes on
//! as plain text, for any program, with its styled paragraphs as JSON in the clipboard's
//! metadata, under the app's own name. A copied slide goes on as its Markdown, with the same
//! Markdown in the metadata, marked as a slide.
//!
//! Whatever is read from the clipboard is untrusted, the app's own metadata too: another
//! program, or another version of this app, may have put it there. It's size-checked before
//! it's parsed, parsed into plain structs of this module's own, and held to the text model's
//! rules before any of it becomes paragraphs. Anything that fails falls back to the plain
//! text, which is size-checked and cleaned as typed text is. Nothing here can panic on what the
//! clipboard holds.
//!
//! A thing copied from a slide goes on as its Markdown, the same way, marked as an element.
//! It, and a copied slide, carry the files they use that the talk has, base64 written: a
//! pasted file is trusted no more than a dropped one. Its path must be one plain name in
//! `assets`, `components` or `build`, with an extension a file there can have, and its bytes
//! must be what that kind of file holds. How many there are and how big they say they are is
//! counted before any is decoded.
//!
//! Nothing here touches the window: it's the clipboard's contents, in and out.

use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use slides_format::{
    Block, MAX_LEVEL, MAX_SLIDE_BYTES, Para, ParaKind, Span, Style, block_text, parse_talk, slide_text_for_copy, storable,
    text_flows,
};

use crate::added_files::{FileKind, MAX_ADDED_BYTES, component_stem};
use crate::text_editing::clean_typed;

/// The name the app's own metadata is kept under.
pub const APP_KEY: &str = "stagepack-studio";
/// The version of what's kept there. Anything else is taken as not the app's.
const VERSION: u32 = 1;

/// The most a paste takes, of plain text or of metadata, and the most a copied slide holds.
pub const MAX_BYTES: usize = 256 * 1024;
/// The most paragraphs a paste makes.
pub const MAX_PARAS: usize = 4096;
/// The most spans the styled paragraphs of a paste hold, all told.
pub const MAX_SPANS: usize = 16 * 1024;
/// The most bytes of files a copy carries, all told, by their length before they're written
/// as base64.
pub const MAX_CLIPBOARD_FILES_BYTES: usize = 64 * 1024 * 1024;
/// The most files a copy carries.
pub const MAX_CLIPBOARD_FILES: usize = 64;
/// The most metadata that's read at all: a thing's Markdown and its files, as base64, with room
/// to spare for the rest of the JSON.
pub const MAX_METADATA_BYTES: usize = 96 * 1024 * 1024;
/// The longest name a pasted file can have.
const MAX_PASTED_NAME: usize = 255;
/// What a WebAssembly module starts with.
const WASM_MAGIC: &[u8] = b"\0asm";
/// The most bytes a link's destination has.
const MAX_LINK: usize = 2048;
/// The sizes text can have.
const SIZES: std::ops::RangeInclusive<u32> = 1..=512;
/// The colours the talk's text takes by name, besides `#rgb` and `#rrggbb`.
const NAMED_COLORS: [&str; 9] = ["black", "white", "red", "green", "blue", "yellow", "orange", "purple", "gray"];

/// What the window says when there's too much on the clipboard to paste.
pub const TOO_LARGE: &str = "That's too much to paste at once, so nothing was pasted.";
/// What the window says when the slide list is pasted into and the clipboard holds no slide.
pub const NO_SLIDE: &str = "There's no slide on the clipboard to paste.";
/// What the window says when a slide is too big to copy.
pub const SLIDE_TOO_BIG: &str = "This slide is too big to copy, so it wasn't copied.";
/// What the window says once a pasted slide shows a picture or a component the talk lacks.
pub const MISSING_PICTURES: &str = "Some pictures on the pasted slide aren't in this talk.";

/// What the window says when the files a copied thing or slide uses were too big to go with it.
pub const FILES_TOO_LARGE: &str = "The pictures were too large to copy with it.";
/// What the window says when a thing from a slide is pasted while text is being typed.
pub const NOT_INTO_TEXT: &str = "That can't be pasted into text.";
/// What the window says when the clipboard's thing isn't one thing a slide can take.
pub const NOT_ONE_THING: &str = "That isn't one thing a slide can hold, so it wasn't pasted.";
/// What the window says when a thing is too big to copy.
pub const THING_TOO_BIG: &str = "This is too big to copy, so it wasn't copied.";

/// What the window says when a pasted component has the name of a different one the talk has,
/// called `name`.
pub fn different_component(name: &str) -> String {
    format!("This talk already has a different component called {name}. The pasted one uses this talk's.")
}

/// How the engine starts the notice for a picture it can't show, which it draws a placeholder
/// for, followed by the picture's path.
const PICTURE_NOTICE: &str = "Can't show the picture ";
/// How the engine starts the notice for a component the talk doesn't have.
const COMPONENT_NOTICE: &str = "There is no component called ";

/// Whether `notices`, a slide's, say it shows a picture or a component the talk lacks: a
/// component it doesn't have, or a picture it can't show whose file isn't there, as `has_file`
/// says. A picture whose file is there, but can't be read as one, or is too big, isn't missing.
pub fn shows_something_missing(notices: &[String], has_file: impl Fn(&str) -> bool) -> bool {
    notices.iter().any(|notice| {
        notice.starts_with(COMPONENT_NOTICE)
            || notice.strip_prefix(PICTURE_NOTICE).is_some_and(|path| !has_file(path))
    })
}

/// Which clipboard command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardOp {
    Copy,
    Cut,
    Paste,
}

/// What goes on the clipboard: plain text, and the app's metadata, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub metadata: Option<String>,
}

/// What a paste into text puts in place of the selection.
#[derive(Clone, Debug, PartialEq)]
pub enum TextPaste {
    /// Styled paragraphs, held to the model's rules.
    Styled(Vec<Para>),
    /// Plain text, cleaned as typed text is, a paragraph to a line, in the style at the caret.
    Plain(String),
    /// Nothing to paste.
    Nothing,
}

/// The clipboard holds more than a paste takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge;

// What's kept in the metadata. These are this module's own, so that what's read is only ever
// what's written here, whatever the format's types come to hold.

#[derive(Serialize, Deserialize)]
struct Envelope {
    #[serde(rename = "stagepack-studio")]
    app: Payload,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Payload {
    Text {
        version: u32,
        paras: Vec<JsonPara>,
    },
    Slide {
        version: u32,
        markdown: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<JsonFile>,
        #[serde(default, skip_serializing_if = "is_false")]
        files_too_large: bool,
    },
    Element {
        version: u32,
        markdown: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<JsonFile>,
        #[serde(default, skip_serializing_if = "is_false")]
        files_too_large: bool,
    },
}

/// A file a copy carries: its path in the talk, its bytes in base64, and, for a component's
/// build, the hash of the source the talk it came from says it was built from.
#[derive(Serialize, Deserialize)]
struct JsonFile {
    #[serde(default)]
    path: String,
    #[serde(default)]
    bytes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    built_from: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct JsonPara {
    /// `body`, `heading`, `bullet` or `numbered`.
    kind: String,
    /// A heading's level, or a list item's.
    #[serde(default, skip_serializing_if = "is_zero")]
    level: u8,
    #[serde(default)]
    spans: Vec<JsonSpan>,
}

#[derive(Serialize, Deserialize)]
struct JsonSpan {
    text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    code: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

/// Whether `metadata` is the app's own, a slide, a thing or text it copied.
pub fn is_app_data(metadata: &str) -> bool {
    !matches!(read_app_data(Some(metadata)), AppData::Not)
}

/// A file of the talk's that a copy carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopiedFile {
    /// Its path in the talk.
    pub path: String,
    pub bytes: Arc<[u8]>,
    /// For a component's build, the hash of the source the talk's index says it was built from.
    pub built_from: Option<String>,
}

/// A file a paste brings, checked to be one a talk can take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastedFile {
    /// Its path in the talk it came from: one plain name in the folder for its kind.
    pub path: String,
    pub kind: FileKind,
    pub bytes: Vec<u8>,
    /// For a component's build, the hash of the source it was built from, as the copy says: 64
    /// lowercase hex digits, or `None`.
    pub built_from: Option<String>,
}

/// The files a paste brings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PastedFiles {
    pub files: Vec<PastedFile>,
    /// Whether any that came couldn't be taken.
    pub left_out: bool,
    /// Whether they were too many or too big to come at all.
    pub too_large: bool,
}

/// What a copied thing or slide holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    /// Its Markdown, as it came: not yet held to what a pasted thing or slide must be.
    pub markdown: String,
    pub files: PastedFiles,
}

/// What the app's metadata on the clipboard holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppData {
    /// Nothing of the app's: none, more than is read, or not as the app writes it.
    Not,
    /// Styled text.
    Text,
    /// A slide.
    Slide(Carried),
    /// A thing from a slide.
    Element(Carried),
}

/// What `metadata` holds of the app's, its files checked as [`PastedFiles`] says. More than
/// [`MAX_METADATA_BYTES`] isn't read at all.
pub fn read_app_data(metadata: Option<&str>) -> AppData {
    let Some(metadata) = metadata.filter(|metadata| metadata.len() <= MAX_METADATA_BYTES) else {
        return AppData::Not;
    };
    match serde_json::from_str::<Envelope>(metadata).map(|envelope| envelope.app) {
        Ok(Payload::Text { version: VERSION, .. }) if metadata.len() <= MAX_BYTES => AppData::Text,
        Ok(Payload::Slide { version: VERSION, markdown, files, files_too_large }) if markdown.len() <= MAX_BYTES => {
            AppData::Slide(Carried { markdown, files: pasted_files(files, files_too_large) })
        }
        Ok(Payload::Element { version: VERSION, markdown, files, files_too_large }) if markdown.len() <= MAX_BYTES => {
            AppData::Element(Carried { markdown, files: pasted_files(files, files_too_large) })
        }
        _ => AppData::Not,
    }
}

/// How many bytes `base64`, written as base64, holds at most, found without decoding it.
fn stated_len(base64: &str) -> usize {
    base64.len().div_ceil(4).saturating_mul(3)
}

/// `files` as a paste takes them. More than [`MAX_CLIPBOARD_FILES`], or more than
/// [`MAX_CLIPBOARD_FILES_BYTES`] by what their base64 says, and none are, which is too large,
/// as `too_large` says the copy found already. Each that isn't a file a talk can take is
/// left out: see [`pasted_file`].
fn pasted_files(files: Vec<JsonFile>, too_large: bool) -> PastedFiles {
    let stated = files.iter().fold(0usize, |sum, file| sum.saturating_add(stated_len(&file.bytes)));
    if too_large || files.len() > MAX_CLIPBOARD_FILES || stated > MAX_CLIPBOARD_FILES_BYTES {
        return PastedFiles { files: Vec::new(), left_out: false, too_large: true };
    }
    let mut pasted = PastedFiles::default();
    for file in files {
        match pasted_file(file).filter(|file| !pasted.files.iter().any(|other| other.path.eq_ignore_ascii_case(&file.path))) {
            Some(file) => pasted.files.push(file),
            None => pasted.left_out = true,
        }
    }
    pasted
}

/// `file` as a paste takes it, if its path is one [`pasted_kind`] gives a kind and its bytes are
/// base64 of no more than [`MAX_ADDED_BYTES`] that hold that kind of file: a picture the engine
/// reads, UTF-8 data, a component's source the check lets be built, or a WebAssembly module.
fn pasted_file(file: JsonFile) -> Option<PastedFile> {
    let kind = pasted_kind(&file.path)?;
    if stated_len(&file.bytes) > MAX_ADDED_BYTES.saturating_add(3) {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(file.bytes.as_bytes()).ok()?;
    if bytes.len() > MAX_ADDED_BYTES {
        return None;
    }
    let fits = match kind {
        FileKind::Picture => slides_engine::picture_format(&bytes).is_some(),
        FileKind::Data => std::str::from_utf8(&bytes).is_ok(),
        FileKind::ComponentSource => {
            bytes.len() as u64 <= slides_engine::MAX_SOURCE_BYTES
                && std::str::from_utf8(&bytes).is_ok_and(|source| slides_components::check_source(&file.path, source).is_ok())
        }
        FileKind::ComponentBuild => bytes.starts_with(WASM_MAGIC),
    };
    let built_from = file
        .built_from
        .filter(|hash| kind == FileKind::ComponentBuild && hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    fits.then_some(PastedFile { path: file.path, kind, bytes, built_from })
}

/// The kind of file a pasted file at `path` is, if it's one a talk can take: `assets/`,
/// `components/` or `build/` followed by one name, with no slash, backslash, colon or control
/// character in it, not starting with a dot, at most 255 bytes, ending in an extension a file of
/// that kind can have. A component's file must be named as a component's can be.
pub fn pasted_kind(path: &str) -> Option<FileKind> {
    let (folder, name) = path.split_once('/')?;
    let plain = !name.is_empty()
        && name.len() <= MAX_PASTED_NAME
        && !name.starts_with('.')
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
    let (stem, ext) = name.rsplit_once('.')?;
    if !plain || stem.is_empty() || slides_engine::normalise(path).as_deref() != Some(path) {
        return None;
    }
    let kinds: &[FileKind] = match folder {
        "assets" => &[FileKind::Picture, FileKind::Data],
        "components" => &[FileKind::ComponentSource],
        "build" => &[FileKind::ComponentBuild],
        _ => return None,
    };
    let kind = kinds.iter().copied().find(|kind| kind.extensions().iter().any(|allowed| allowed.eq_ignore_ascii_case(ext)))?;
    (!kind.is_component() || component_stem(name) == Some(stem)).then_some(kind)
}

/// What copying a thing from a slide, whose own text is `markdown`, and which uses `files`,
/// puts on the clipboard: see [`slide_entry_with`].
pub fn element_entry(markdown: &str, files: &[CopiedFile]) -> Option<Entry> {
    carried_entry(markdown, files, |markdown, files, files_too_large| Payload::Element { version: VERSION, markdown, files, files_too_large })
}

/// What copying a slide whose own text is `markdown`, and which uses `files`, puts on the
/// clipboard, or `None` if its text is more than a paste takes. Files more than
/// [`MAX_CLIPBOARD_FILES`] or [`MAX_CLIPBOARD_FILES_BYTES`] aren't carried, and the paste says so.
pub fn slide_entry_with(markdown: &str, files: &[CopiedFile]) -> Option<Entry> {
    carried_entry(markdown, files, |markdown, files, files_too_large| Payload::Slide { version: VERSION, markdown, files, files_too_large })
}

fn carried_entry(markdown: &str, files: &[CopiedFile], payload: impl Fn(String, Vec<JsonFile>, bool) -> Payload) -> Option<Entry> {
    if markdown.len() > MAX_BYTES {
        return None;
    }
    let total = files.iter().fold(0usize, |sum, file| sum.saturating_add(file.bytes.len()));
    let too_large = files.len() > MAX_CLIPBOARD_FILES || total > MAX_CLIPBOARD_FILES_BYTES;
    let carried = if too_large {
        Vec::new()
    } else {
        files
            .iter()
            .map(|file| JsonFile {
                path: file.path.clone(),
                bytes: base64::engine::general_purpose::STANDARD.encode(&file.bytes),
                built_from: file.built_from.clone(),
            })
            .collect()
    };
    let envelope = Envelope { app: payload(markdown.to_string(), carried, too_large) };
    let json = serde_json::to_string(&envelope).ok()?;
    (json.len() <= MAX_METADATA_BYTES).then(|| Entry { text: markdown.to_string(), metadata: Some(json) })
}

/// `text` as the one thing on a slide it holds, as every thing that's pasted is made: cleaned
/// as typed text is, it must read, after a break line, as one slide holding exactly one block,
/// a tag or a table, within [`MAX_SLIDE_BYTES`], with no settings, notes or quote, and nothing
/// else. What's given is that block's own text, with `\n` line endings.
pub fn one_element(text: &str) -> Option<String> {
    if text.len() > MAX_SLIDE_BYTES {
        return None;
    }
    let text = clean_typed(text);
    std::panic::catch_unwind(|| {
        let talk = parse_talk(&format!("---\n{text}"));
        let [slide] = talk.slides.as_slice() else { return None };
        let alone = slide.settings.is_none() && slide.notes.is_none() && slide.error.is_none() && slide.inline_tags.is_empty() && slide.quotes.is_empty();
        let [block] = slide.blocks.as_slice() else { return None };
        let a_thing = matches!(block, Block::Tag(tag) if tag.name != "Notes") || matches!(block, Block::Table { .. });
        (alone && a_thing).then(|| block_text(&talk, 0, block.range())).flatten()
    })
    .ok()
    .flatten()
}

/// What a paste into the slide list makes of `markdown`, a copied slide's: see [`one_slide`].
pub fn carried_slide(markdown: &str) -> SlidePaste {
    // A slide the app copied may be one with nothing on it, as a new slide is.
    one_slide(&clean_typed(markdown)).map_or(SlidePaste::NotOneSlide, |slide| SlidePaste::Slide(slide.text))
}

/// What copying `paras` puts on the clipboard: their text, a paragraph to a line, and the
/// paragraphs themselves in the metadata, unless that would be more than a paste takes.
pub fn text_entry(paras: &[Para]) -> Entry {
    let text = paras.iter().map(para_text).collect::<Vec<_>>().join("\n");
    let metadata = Some(encode_paras(paras)).filter(|json| json.len() <= MAX_BYTES);
    Entry { text, metadata }
}

/// What copying a slide whose own text is `markdown` puts on the clipboard, or `None` if it's
/// more than a paste takes.
pub fn slide_entry(markdown: &str) -> Option<Entry> {
    slide_entry_with(markdown, &[])
}

/// `paras` as the metadata keeps them.
pub fn encode_paras(paras: &[Para]) -> String {
    let paras = storable(paras).iter().map(json_para).collect();
    let envelope = Envelope { app: Payload::Text { version: VERSION, paras } };
    serde_json::to_string(&envelope).unwrap_or_default()
}

/// The styled paragraphs `json` holds, if it's the app's text and keeps to the model's rules.
/// Text in it is cleaned as typed text is; anything else that's out of the rules gives `None`.
pub fn decode_paras(json: &str) -> Option<Vec<Para>> {
    match decode_text(json) {
        Decoded::Paras(paras) => Some(paras),
        Decoded::OverCaps | Decoded::Not => None,
    }
}

/// What the metadata held as the app's text.
enum Decoded {
    Paras(Vec<Para>),
    /// The app's text, but more paragraphs or spans than a paste makes.
    OverCaps,
    /// Not the app's text, or not as the rules allow.
    Not,
}

fn decode_text(json: &str) -> Decoded {
    if json.len() > MAX_BYTES {
        return Decoded::Not;
    }
    match serde_json::from_str::<Envelope>(json).map(|envelope| envelope.app) {
        Ok(Payload::Text { version: VERSION, paras }) => model_paras(paras),
        _ => Decoded::Not,
    }
}

/// What a paste into text takes from a clipboard holding `plain` text and `metadata`: the
/// app's styled paragraphs if the metadata holds them as the rules allow, the text of a copied
/// slide, or else the plain text. Styled paragraphs with no words at all give way to plain text
/// that has some. More than [`MAX_PARAS`] paragraphs or [`MAX_SPANS`] spans, styled or plain,
/// or plain text over [`MAX_BYTES`], is too much.
pub fn text_to_paste(plain: Option<&str>, metadata: Option<&str>) -> Result<TextPaste, TooLarge> {
    let plain_has_words = plain.is_some_and(|plain| !plain.trim().is_empty());
    if let Some(metadata) = metadata.filter(|metadata| metadata.len() <= MAX_METADATA_BYTES) {
        match decode_text(metadata) {
            Decoded::OverCaps => return Err(TooLarge),
            Decoded::Paras(paras) if paras.iter().any(|para| !para.spans.is_empty()) || !plain_has_words => {
                return Ok(if paras.is_empty() { TextPaste::Nothing } else { TextPaste::Styled(paras) });
            }
            Decoded::Paras(_) | Decoded::Not => {}
        }
        if let Some(markdown) = decode_slide(metadata) {
            // A slide's text, not its Markdown, which nobody is made to see.
            return slide_words(&markdown);
        }
    }
    let Some(plain) = plain.filter(|plain| !plain.is_empty()) else {
        return Ok(TextPaste::Nothing);
    };
    if plain.len() > MAX_BYTES {
        return Err(TooLarge);
    }
    let text = clean_typed(plain);
    if text.split(['\n', '\u{2029}']).count() > MAX_PARAS {
        return Err(TooLarge);
    }
    Ok(if text.is_empty() { TextPaste::Nothing } else { TextPaste::Plain(text) })
}

/// What a paste into the slide list takes from the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlidePaste {
    /// A slide's own text, as [`one_slide`] gives it.
    Slide(String),
    /// Nothing that holds a slide.
    NoSlide,
    /// Text that doesn't read as one slide, which isn't pasted.
    NotOneSlide,
}

/// A slide's own text, as every slide that's pasted is made: see [`one_slide`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneSlide {
    pub text: String,
    /// Whether the slide has nothing on it, settings aside.
    pub holds_nothing: bool,
}

/// What to paste into the slide list from a clipboard holding `plain` text and `metadata`: the
/// slide the app copied, if the metadata holds one, or else the plain text, as a slide copied
/// by another running copy of the app arrives, or Markdown copied from an editor. Either is
/// cleaned as typed text is and goes through [`one_slide`]. Plain text over [`MAX_BYTES`] is
/// too much; plain text that's empty, only whitespace, or a slide with nothing on it but
/// settings, is no slide. Of plain text, one break line at its end, and the line ending after
/// it, is also left out, if it isn't the close of a settings block it starts with.
pub fn slide_to_paste(plain: Option<&str>, metadata: Option<&str>) -> Result<SlidePaste, TooLarge> {
    if let AppData::Slide(slide) = read_app_data(metadata) {
        return Ok(carried_slide(&slide.markdown));
    }
    let Some(plain) = plain else { return Ok(SlidePaste::NoSlide) };
    if plain.len() > MAX_BYTES {
        return Err(TooLarge);
    }
    let text = clean_typed(plain);
    if text.trim().is_empty() {
        return Ok(SlidePaste::NoSlide);
    }
    // As it is first: a break line at its end may close a settings block it starts with. Then
    // without a break line at its end.
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let last_start = body.rfind('\n').map_or(0, |at| at + 1);
    let without_end = is_break_line(&body[last_start..]).then(|| &text[..last_start]);
    let slide = one_slide(&text).or_else(|| without_end.and_then(one_slide));
    Ok(match slide {
        Some(slide) if slide.holds_nothing => SlidePaste::NoSlide,
        Some(slide) => SlidePaste::Slide(slide.text),
        None => SlidePaste::NotOneSlide,
    })
}

/// Whether `line`, without its line ending, is a break between slides.
fn is_break_line(line: &str) -> bool {
    line.trim_end_matches([' ', '\r']) == "---"
}

/// `text` as the slide it holds, as every slide that's pasted is made, from the clipboard or
/// anywhere else: blank lines before a break line at its start go with that break line, and
/// what's left is read as a slide that follows another, so a settings block straight after the
/// break line, or at the start without one, is the slide's. It must read as exactly one slide,
/// or it's `None`. What's given is that slide's own text, as
/// [`slides_format::slide_text_for_copy`] gives it, with none of the talk's settings: a pasted
/// slide never sets them, wherever it lands. Giving it back to this gives it back as it is.
pub fn one_slide(text: &str) -> Option<OneSlide> {
    let blank: usize = text.split_inclusive('\n').take_while(|line| line.trim().is_empty()).map(str::len).sum();
    let starts_with_break = text[blank..].split('\n').next().is_some_and(is_break_line);
    let as_talk = if starts_with_break { text[blank..].to_string() } else { format!("---\n{text}") };
    std::panic::catch_unwind(|| {
        let talk = parse_talk(&as_talk);
        let slide = talk.slides.first().filter(|_| talk.slides.len() == 1)?;
        let holds_nothing = slide.blocks.is_empty() && slide.notes.is_none() && slide.error.is_none();
        let text = slide_text_for_copy(&talk, 0);
        Some(OneSlide { text, holds_nothing })
    })
    .ok()
    .flatten()
}

fn decode_slide(json: &str) -> Option<String> {
    match read_app_data(Some(json)) {
        AppData::Slide(slide) => Some(slide.markdown),
        _ => None,
    }
}

/// The text of the slide `markdown`, its runs of editable text one after another. Its links,
/// sizes and colours that a pasted one may not have are left off, and what's left is held to
/// the rules as the app's own styled text is: the slide is as untrusted. More paragraphs or
/// spans than a paste makes is too much.
fn slide_words(markdown: &str) -> Result<TextPaste, TooLarge> {
    let read = std::panic::catch_unwind(|| {
        let talk = parse_talk(&clean_typed(markdown));
        (0..talk.slides.len()).flat_map(|slide| text_flows(&talk, slide)).flat_map(|flow| flow.paras).collect::<Vec<Para>>()
    });
    let Ok(mut paras) = read else { return Ok(TextPaste::Nothing) };
    let spans: usize = paras.iter().map(|para| para.spans.len()).sum();
    if paras.len() > MAX_PARAS || spans > MAX_SPANS {
        return Err(TooLarge);
    }
    for span in paras.iter_mut().flat_map(|para| &mut para.spans) {
        let style = &mut span.style;
        style.link = style.link.take().filter(|link| link_allowed(link));
        style.color = style.color.take().filter(|color| color_allowed(color));
        style.size = style.size.filter(|size| SIZES.contains(size));
    }
    Ok(match decode_paras(&encode_paras(&paras)) {
        Some(paras) if !paras.is_empty() => TextPaste::Styled(paras),
        _ => TextPaste::Nothing,
    })
}

fn para_text(para: &Para) -> String {
    para.spans.iter().map(|span| span.text.as_str()).collect()
}

fn json_para(para: &Para) -> JsonPara {
    let (kind, level) = match para.kind {
        ParaKind::Body => ("body", 0),
        ParaKind::Heading(level) => ("heading", level),
        ParaKind::Bullet { level } => ("bullet", level),
        ParaKind::Numbered { level } => ("numbered", level),
    };
    let spans = para
        .spans
        .iter()
        .map(|span| {
            let style = &span.style;
            JsonSpan {
                text: span.text.clone(),
                bold: style.bold,
                italic: style.italic,
                underline: style.underline,
                code: style.code,
                color: style.color.clone(),
                size: style.size,
                link: style.link.clone(),
            }
        })
        .collect();
    JsonPara { kind: kind.to_string(), level, spans }
}

/// `paras` as the model holds them, unless there are too many or any of them breaks its rules.
fn model_paras(paras: Vec<JsonPara>) -> Decoded {
    if paras.len() > MAX_PARAS || paras.iter().map(|para| para.spans.len()).sum::<usize>() > MAX_SPANS {
        return Decoded::OverCaps;
    }
    match paras.into_iter().map(model_para).collect::<Option<Vec<Para>>>() {
        Some(paras) => Decoded::Paras(storable(&paras)),
        None => Decoded::Not,
    }
}

fn model_para(para: JsonPara) -> Option<Para> {
    let level = para.level;
    let kind = match para.kind.as_str() {
        "body" if level == 0 => ParaKind::Body,
        "heading" if (1..=6).contains(&level) => ParaKind::Heading(level),
        "bullet" if level <= MAX_LEVEL => ParaKind::Bullet { level },
        "numbered" if level <= MAX_LEVEL => ParaKind::Numbered { level },
        _ => return None,
    };
    let spans = para.spans.into_iter().map(model_span).collect::<Option<Vec<Span>>>()?;
    Some(Para { kind, spans })
}

fn model_span(span: JsonSpan) -> Option<Span> {
    if span.size.is_some_and(|size| !SIZES.contains(&size)) {
        return None;
    }
    if span.color.as_deref().is_some_and(|color| !color_allowed(color)) {
        return None;
    }
    if span.link.as_deref().is_some_and(|link| !link_allowed(link)) {
        return None;
    }
    // Line and paragraph separators are line breaks here: a paragraph ends only between spans'
    // paragraphs.
    let text = clean_typed(&span.text).replace(['\u{2028}', '\u{2029}'], "\n");
    let style = Style {
        bold: span.bold,
        italic: span.italic,
        underline: span.underline,
        code: span.code,
        color: span.color,
        size: span.size,
        link: span.link,
    };
    Some(Span { text, style })
}

/// Whether `color` is one a pasted span may have: one the talk's text takes, written as the app
/// writes it or as a talk can: `#rgb` or `#rrggbb` in ASCII hex digits, or one of the names
/// `black`, `white`, `red`, `green`, `blue`, `yellow`, `orange`, `purple` and `gray`, exactly.
pub fn color_allowed(color: &str) -> bool {
    match color.strip_prefix('#') {
        Some(hex) => matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        None => NAMED_COLORS.contains(&color),
    }
}

/// Whether `link` is a destination a pasted link may have. It's at most 2048 bytes, with no
/// control characters, whitespace, quotes, angle brackets, backslash or backtick, and it's one
/// of: an `http://` or `https://` address; a `mailto:` address; or, with no scheme, a place
/// within the talk, such as `#next` or `notes/more.html`, that doesn't start with `//` (which
/// would be another site) and has no `..` segment, written plainly or as `%2e%2e`. A link goes
/// with the talk into what it's exported to, where a `javascript:` one would run.
pub fn link_allowed(link: &str) -> bool {
    if link.is_empty() || link.len() > MAX_LINK || link.chars().any(|c| c.is_control() || c.is_whitespace() || "\"'<>\\`".contains(c)) {
        return false;
    }
    let scheme = link.split_once(':').map(|(scheme, _)| scheme).filter(|scheme| !scheme.contains(['/', '?', '#']));
    match scheme.map(str::to_ascii_lowercase).as_deref() {
        Some("http" | "https") => link.split_once(':').is_some_and(|(_, rest)| rest.starts_with("//") && rest.len() > 2),
        Some("mailto") => true,
        Some(_) => false,
        None => {
            let path = link.split(['?', '#']).next().unwrap_or_default();
            !link.starts_with("//") && !path.split('/').any(|segment| segment.to_ascii_lowercase().replace("%2e", ".") == "..")
        }
    }
}
