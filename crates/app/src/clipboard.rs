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
//! Nothing here touches the window: it's the clipboard's contents, in and out.

use serde::{Deserialize, Serialize};
use slides_format::{Para, ParaKind, Span, Style, parse_talk, storable, text_flows};

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
/// The most bytes a link's destination has.
const MAX_LINK: usize = 2048;
/// The sizes text can have.
const SIZES: std::ops::RangeInclusive<u32> = 1..=512;
/// The deepest a list item nests, as the text model holds it.
const MAX_LEVEL: u8 = 8;
/// The colours the talk's text takes by name, besides `#rgb` and `#rrggbb`.
const NAMED_COLORS: [&str; 9] = ["black", "white", "red", "green", "blue", "yellow", "orange", "purple", "gray"];
/// The kinds of link a pasted one may be, besides one with no scheme, within the talk.
const LINK_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// What the window says when there's too much on the clipboard to paste.
pub const TOO_LARGE: &str = "That's too much to paste at once, so nothing was pasted.";
/// What the window says when the slide list is pasted into and the clipboard holds no slide.
pub const NO_SLIDE: &str = "There's no slide on the clipboard to paste.";
/// What the window says when a slide is too big to copy.
pub const SLIDE_TOO_BIG: &str = "This slide is too big to copy, so it wasn't copied.";
/// What the window says once a pasted slide shows a picture or a component the talk lacks.
pub const MISSING_PICTURES: &str = "Some pictures on the pasted slide aren't in this talk.";

/// How the engine starts the notice for a picture it can't show, and for a component the
/// talk doesn't have, each of which it draws a placeholder for.
const MISSING_NOTICES: [&str; 2] = ["Can't show the picture ", "There is no component called "];

/// Whether `notices`, a slide's, say it shows a picture or a component the talk lacks.
pub fn shows_something_missing(notices: &[String]) -> bool {
    notices.iter().any(|notice| MISSING_NOTICES.iter().any(|start| notice.starts_with(start)))
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
    Text { version: u32, paras: Vec<JsonPara> },
    Slide { version: u32, markdown: String },
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
    let envelope = Envelope { app: Payload::Slide { version: VERSION, markdown: markdown.to_string() } };
    let json = serde_json::to_string(&envelope).ok()?;
    (markdown.len() <= MAX_BYTES && json.len() <= MAX_BYTES).then(|| Entry { text: markdown.to_string(), metadata: Some(json) })
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
    if json.len() > MAX_BYTES {
        return None;
    }
    match serde_json::from_str::<Envelope>(json).ok()?.app {
        Payload::Text { version: VERSION, paras } => model_paras(paras),
        _ => None,
    }
}

/// What a paste into text takes from a clipboard holding `plain` text and `metadata`: the
/// app's styled paragraphs if the metadata holds them as the rules allow, the text of a copied
/// slide, or else the plain text. Plain text over [`MAX_BYTES`], or of more than [`MAX_PARAS`]
/// lines, is too much.
pub fn text_to_paste(plain: Option<&str>, metadata: Option<&str>) -> Result<TextPaste, TooLarge> {
    if let Some(metadata) = metadata.filter(|metadata| metadata.len() <= MAX_BYTES) {
        if let Some(paras) = decode_paras(metadata) {
            return Ok(if paras.is_empty() { TextPaste::Nothing } else { TextPaste::Styled(paras) });
        }
        if let Some(markdown) = decode_slide(metadata) {
            // A slide's text, not its Markdown, which nobody is made to see.
            return Ok(slide_words(&markdown));
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

/// The Markdown of the slide on a clipboard holding `metadata`, cleaned as typed text is, if
/// it holds one the app copied. Metadata over [`MAX_BYTES`] is too much.
pub fn slide_to_paste(metadata: Option<&str>) -> Result<Option<String>, TooLarge> {
    let Some(metadata) = metadata else { return Ok(None) };
    if metadata.len() > MAX_BYTES {
        return Err(TooLarge);
    }
    Ok(decode_slide(metadata).map(|markdown| clean_typed(&markdown)))
}

fn decode_slide(json: &str) -> Option<String> {
    match serde_json::from_str::<Envelope>(json).ok()?.app {
        Payload::Slide { version: VERSION, markdown } if markdown.len() <= MAX_BYTES => Some(markdown),
        _ => None,
    }
}

/// The text of the slide `markdown`, its runs of editable text one after another.
fn slide_words(markdown: &str) -> TextPaste {
    let talk = parse_talk(&clean_typed(markdown));
    let paras: Vec<Para> = (0..talk.slides.len()).flat_map(|slide| text_flows(&talk, slide)).flat_map(|flow| flow.paras).collect();
    let spans: usize = paras.iter().map(|para| para.spans.len()).sum();
    if paras.is_empty() || paras.len() > MAX_PARAS || spans > MAX_SPANS {
        return TextPaste::Nothing;
    }
    // Held to the rules as the app's own styled text is: the slide is as untrusted.
    match decode_paras(&encode_paras(&paras)) {
        Some(paras) if !paras.is_empty() => TextPaste::Styled(paras),
        _ => TextPaste::Nothing,
    }
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

/// `paras` as the model holds them, or `None` if any of them breaks its rules.
fn model_paras(paras: Vec<JsonPara>) -> Option<Vec<Para>> {
    if paras.len() > MAX_PARAS || paras.iter().map(|para| para.spans.len()).sum::<usize>() > MAX_SPANS {
        return None;
    }
    let paras = paras.into_iter().map(model_para).collect::<Option<Vec<Para>>>()?;
    Some(storable(&paras))
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
    if span.color.as_deref().is_some_and(|color| !is_color(color)) {
        return None;
    }
    if span.link.as_deref().is_some_and(|link| !is_link(link)) {
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

/// Whether `color` is one the talk's text takes, written as the app writes it or as a talk
/// can: `#rgb`, `#rrggbb`, or a colour's name.
fn is_color(color: &str) -> bool {
    match color.strip_prefix('#') {
        Some(hex) => matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        None => NAMED_COLORS.contains(&color),
    }
}

/// Whether `link` is a destination a pasted link may have: no control characters, quotes or
/// angle brackets, and either no scheme, so within the talk, or one of [`LINK_SCHEMES`]. A
/// link goes with the talk into what it's exported to, where a `javascript:` one would run.
fn is_link(link: &str) -> bool {
    if link.is_empty() || link.len() > MAX_LINK || link.chars().any(|c| c.is_control() || c.is_whitespace() || "\"'<>\\`".contains(c)) {
        return false;
    }
    let scheme = link.split_once(':').map(|(scheme, _)| scheme).filter(|scheme| !scheme.contains(['/', '?', '#']));
    scheme.is_none_or(|scheme| LINK_SCHEMES.iter().any(|allowed| scheme.eq_ignore_ascii_case(allowed)))
}
