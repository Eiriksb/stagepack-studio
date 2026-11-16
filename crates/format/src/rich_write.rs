//! Writing a slide's text model back as Markdown. Only the paragraphs that changed are
//! written: one whose words changed has its content replaced and keeps its marker, one
//! that's the same keeps every byte. Each change is read back before it's given out, and
//! one that wouldn't read back as the paragraphs it was asked for is refused.

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::edit::{Change, SourceEdit, eol_in};
use crate::model::{Block, Slide, Span, Style, Talk};
use crate::parse::parse_talk;
use crate::rich::{FlowPlace, Para, ParaKind, TextFlow, is_positioned, layout_of, text_flows};
use crate::split::split_slides;

/// The deepest a list can nest and still be read as editable.
const MAX_LEVEL: u8 = 8;

/// Markdown for one paragraph's content, as it's written on a line of its own: every
/// character Markdown would read as structure is escaped, and `**` or `*` that can't close
/// gives way to `<Text bold="true">` or `<Text italic="true">`. A line break is a hard
/// break. Spans that can't be written exactly, such as a line break in code, come out as
/// near as they can.
pub fn write_inline(spans: &[Span]) -> String {
    let spans = tidy(spans);
    let ctx = Inline { heading: false, indent: "", eol: "\n" };
    inline_for(&spans, Probe::Body, &ctx, PLAIN).unwrap_or_else(|| render(&spans, &ctx, Form::Tags, Escapes::Safe, NOTHING))
}

/// `paras` in the form they read back in: line endings made `\n`, no empty spans, and
/// neighbours with the same style merged. Nothing else is changed: an empty paragraph, of
/// any kind, and a line break at a paragraph's start or end are written so that they read
/// back as they are. This is what [`write_flow`] and [`write_into_region`] write.
pub fn storable(paras: &[Para]) -> Vec<Para> {
    paras.iter().map(|para| Para { kind: para.kind, spans: tidy(&para.spans) }).collect()
}

/// The edits that turn `flow`, a flow of slide `slide` as [`text_flows`] reads it now, into
/// `paras`. A paragraph equal to one it had keeps its bytes, and one whose spans changed but
/// whose kind didn't has only its content replaced. Added and removed paragraphs take the
/// list markers, numbering, indentation and blank lines of the paragraphs around them.
///
/// What's written is [`storable`]`(paras)`, and the change reads back as exactly that. An
/// empty change means there was nothing to change. `None` means the paragraphs can't be
/// written there, and nothing is changed: when `flow` isn't one of the slide's flows, when
/// the talk would lose the slide, or for a line break in code, a heading of more than one
/// line below level 2, or a list item deeper than the one before it allows.
pub fn write_flow(talk: &Talk, slide: usize, flow: &TextFlow, paras: &[Para]) -> Option<Change> {
    guarded(|| flow_change(talk, slide, flow, paras))
}

/// The edits that put `paras` into region `region` of slide `slide`, a region of its layout
/// that has nothing in it yet. The title's text, a single heading, goes at the top of the
/// slide. Another region's goes after its `<Region name="…" />` line, which is added at the
/// end of the slide if there isn't one, except for the layout's main region on a slide with
/// no such lines, whose text goes at the end. What's written is [`storable`]`(paras)`; if
/// that's nothing, the change is empty. It's `None` if the region isn't the layout's,
/// already has something in it, or the paragraphs can't be written exactly there.
pub fn write_into_region(talk: &Talk, slide: usize, region: &str, paras: &[Para]) -> Option<Change> {
    guarded(|| region_change(talk, slide, region, paras))
}

/// What `make` gives, or `None` if it panics.
fn guarded(make: impl FnOnce() -> Option<Change>) -> Option<Change> {
    catch_unwind(AssertUnwindSafe(make)).ok().flatten()
}

// ---------------------------------------------------------------------------------------
// Inline content

/// Where a paragraph's content is written.
struct Inline<'a> {
    /// In a heading, where a `#` anywhere can be read as the closing marker.
    heading: bool,
    /// What starts each line after a line break.
    indent: &'a str,
    eol: &'a str,
}

/// How bold and italic are written.
#[derive(Clone, Copy, PartialEq)]
enum Form {
    /// `**` and `*`, with `<Text>` for colour, size and underline.
    Marks,
    /// `<Text>` for every style but code and links.
    Tags,
}

/// How much ordinary punctuation is escaped.
#[derive(Clone, Copy, PartialEq)]
enum Escapes {
    /// Only where it could be read as something: `Q&A`, `snake_case` and `2 * 3` stay as
    /// typed.
    Least,
    /// Every character that's ever read as something.
    Safe,
}

/// The ways content may be written, tried in order.
#[derive(Clone, Copy)]
struct Ways {
    forms: &'static [Form],
    escapes: &'static [Escapes],
    /// What's written where text must be and there's none.
    nothing: &'static str,
}

/// The plainest way that reads back alone first.
const PLAIN: Ways = Ways { forms: &[Form::Marks, Form::Tags], escapes: &[Escapes::Least, Escapes::Safe], nothing: NOTHING };
/// The way that depends least on what's around it, for when the plainest doesn't read back
/// where it's put.
const GUARDED: Ways = Ways { forms: &[Form::Tags], escapes: &[Escapes::Safe], nothing: NOTHING_AFTER_ANYTHING };

/// What a paragraph's content is tried out in, to see that it reads back.
#[derive(Clone, Copy)]
enum Probe {
    Body,
    Atx,
    Setext,
    Item,
}

/// The content for `spans`, in the first way of writing it that reads back as them, if any
/// does: each form in turn, and with each, each set of escapes.
fn inline_for(spans: &[Span], probe: Probe, ctx: &Inline, ways: Ways) -> Option<String> {
    ways.forms
        .iter()
        .flat_map(|form| ways.escapes.iter().map(move |escapes| (*form, *escapes)))
        .map(|(form, escapes)| render(spans, ctx, form, escapes, ways.nothing))
        .find(|text| reads_back(text, spans, probe, ctx.eol))
}

/// Whether `text`, the content of a paragraph of the sort `probe` says, reads as `spans`.
fn reads_back(text: &str, spans: &[Span], probe: Probe, eol: &str) -> bool {
    let source = match probe {
        Probe::Body => text.to_string(),
        Probe::Atx => format!("# {text}"),
        Probe::Setext => format!("{text}{eol}==="),
        Probe::Item => format!("- {text}"),
    };
    let talk = parse_talk(&source);
    let flows = text_flows(&talk, 0);
    talk.slides.len() == 1
        && matches!(flows.as_slice(), [flow] if matches!(flow.paras.as_slice(), [para] if para.spans == spans))
}

/// Spans as they read: line endings made `\n`, the empty ones dropped, and neighbours of
/// the same style merged.
fn tidy(spans: &[Span]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        let text = span.text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&text),
            _ => out.push(Span { text, style: span.style.clone() }),
        }
    }
    out
}

/// Something that's read as text, and holds none: an empty tag in marks, as a line holding
/// only a tag is read as a block of its own.
const NOTHING: &str = "**<Text></Text>**";
/// The same, for where `**` can't open, as straight after a byte order mark.
const NOTHING_AFTER_ANYTHING: &str = "<Text></Text>**<Text></Text>**";

/// Where `spans[from]`'s run of spans that `same` holds for ends.
fn run_end(spans: &[Span], from: usize, same: impl Fn(&Span, &Span) -> bool) -> usize {
    from + 1 + spans[from + 1..].iter().take_while(|span| same(&spans[from], span)).count()
}

/// The content for `spans` in `form`.
fn render(spans: &[Span], ctx: &Inline, form: Form, escapes: Escapes, nothing: &str) -> String {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let mut writer = Writer { out: String::new(), ctx, escapes, nothing, edge: edge_whitespace(&text), at: 0, line_start: true, leading_digits: false };
    let mut index = 0;
    while index < spans.len() {
        match form {
            Form::Marks => {
                let look = |span: &Span| (span.style.color.clone(), span.style.size, span.style.underline);
                let end = run_end(spans, index, |a, b| look(a) == look(b));
                let (color, size, underline) = look(&spans[index]);
                let attrs = text_attrs(false, false, underline, color.as_deref(), size);
                writer.raw_open(&attrs);
                writer.links(&spans[index..end], |writer, span, wrapped| writer.marked(span, wrapped || !attrs.is_empty()));
                writer.raw_close(&attrs);
                index = end;
            }
            Form::Tags => {
                let end = run_end(spans, index, |a, b| a.style.link == b.style.link);
                writer.links(&spans[index..end], |writer, span, wrapped| {
                    let Style { bold, italic, underline, ref color, size, .. } = span.style;
                    let attrs = text_attrs(bold, italic, underline, color.as_deref(), size);
                    writer.raw_open(&attrs);
                    writer.content(span, wrapped || !attrs.is_empty(), false);
                    writer.raw_close(&attrs);
                });
                index = end;
            }
        }
    }
    writer.out
}

/// The attributes of a `<Text>` tag giving these styles, each with a space before it.
fn text_attrs(bold: bool, italic: bool, underline: bool, color: Option<&str>, size: Option<u32>) -> String {
    let mut out = String::new();
    for (name, on) in [("bold", bold), ("italic", italic), ("underline", underline)] {
        if on {
            out.push_str(&format!(" {name}=\"true\""));
        }
    }
    if let Some(color) = color {
        out.push_str(&format!(" color=\"{}\"", color.replace('&', "&amp;").replace('"', "&quot;")));
    }
    if let Some(size) = size {
        out.push_str(&format!(" size=\"{size}\""));
    }
    out
}

/// For each byte of `text`, whether it's a space or tab that Markdown would drop: one in a
/// run that touches the start, the end, or a line break.
fn edge_whitespace(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut edge = vec![false; bytes.len()];
    let mut start = 0;
    while start < bytes.len() {
        if !matches!(bytes[start], b' ' | b'\t') {
            start += 1;
            continue;
        }
        let end = start + bytes[start..].iter().take_while(|byte| matches!(byte, b' ' | b'\t')).count();
        let touches = start == 0 || end == bytes.len() || matches!(bytes[start - 1], b'\n' | b'\r') || matches!(bytes[end], b'\n' | b'\r');
        if touches {
            edge[start..end].fill(true);
        }
        start = end;
    }
    edge
}

struct Writer<'a> {
    out: String,
    ctx: &'a Inline<'a>,
    escapes: Escapes,
    /// What's written where text must be and there's none.
    nothing: &'a str,
    edge: Vec<bool>,
    /// Where in the paragraph's text the next character is.
    at: usize,
    /// Whether nothing has been written on this line yet.
    line_start: bool,
    /// Whether all that's been written on this line is digits.
    leading_digits: bool,
}

impl Writer<'_> {
    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
        self.line_start = false;
        self.leading_digits = false;
    }

    fn raw_open(&mut self, attrs: &str) {
        if !attrs.is_empty() {
            self.raw(&format!("<Text{attrs}>"));
        }
    }

    fn raw_close(&mut self, attrs: &str) {
        if !attrs.is_empty() {
            self.raw("</Text>");
        }
    }

    /// Writes `spans`, a run with one link or none, each with `write`, which is told
    /// whether the span is inside a link.
    fn links(&mut self, spans: &[Span], mut write: impl FnMut(&mut Self, &Span, bool)) {
        let mut index = 0;
        while index < spans.len() {
            let end = run_end(spans, index, |a, b| a.style.link == b.style.link);
            let link = spans[index].style.link.clone();
            if link.is_some() {
                self.raw("[");
            }
            for span in &spans[index..end] {
                write(self, span, link.is_some());
            }
            if let Some(url) = link {
                self.raw(&format!("]({})", destination(&url)));
            }
            index = end;
        }
    }

    /// A span with `**` and `*` for bold and italic.
    fn marked(&mut self, span: &Span, wrapped: bool) {
        let marker = match (span.style.bold, span.style.italic) {
            (true, true) => "***",
            (true, false) => "**",
            (false, true) => "*",
            (false, false) => "",
        };
        if !marker.is_empty() {
            self.raw(marker);
        }
        self.content(span, wrapped || !marker.is_empty(), !marker.is_empty());
        if !marker.is_empty() {
            self.raw(marker);
        }
    }

    /// A span's text, as code or as escaped text. `wrapped` says whether something closes
    /// after it, and `in_marks` whether that's `*` or `**`.
    fn content(&mut self, span: &Span, wrapped: bool, in_marks: bool) {
        if span.style.code {
            self.code(&span.text);
        } else {
            self.text(&span.text);
        }
        let at_the_end = self.at == self.edge.len();
        if span.text.ends_with('\n') && (wrapped || at_the_end) {
            // A line can't start by closing what's open, and one with nothing on it would
            // end the paragraph, so something empty goes on it. Alone, or before a closing
            // tag, an empty tag would be read as a block of its own, so it's put in marks.
            self.raw(if in_marks { "<Text></Text>" } else { self.nothing });
        }
    }

    fn line_break(&mut self) {
        self.out.push('\\');
        self.out.push_str(self.ctx.eol);
        self.out.push_str(self.ctx.indent);
        self.line_start = true;
        self.leading_digits = false;
    }

    fn text(&mut self, text: &str) {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        for (index, &(offset, c)) in chars.iter().enumerate() {
            let at = self.at;
            self.at += c.len_utf8();
            if c == '\n' {
                self.line_break();
                continue;
            }
            if self.edge.get(at).copied().unwrap_or(false) {
                self.raw(if c == '\t' { "&#9;" } else { "&#32;" });
                continue;
            }
            let before = index.checked_sub(1).map(|i| chars[i].1);
            let after = chars.get(index + 1).map(|&(_, c)| c);
            let rest = &text[offset + c.len_utf8()..];
            // A space beside it that's written as a reference doesn't count as a space.
            let spaced = |c: Option<char>, at: Option<usize>| {
                matches!(c, Some(' ' | '\t')) && at.is_some_and(|at| !self.edge.get(at).copied().unwrap_or(true))
            };
            let least = self.escapes == Escapes::Least;
            let escape = match c {
                '&' if least => starts_a_reference(rest),
                '_' if least => !(before.is_some_and(char::is_alphanumeric) && after.is_some_and(char::is_alphanumeric)),
                '*' if least => {
                    let around = (spaced(before, at.checked_sub(1)), spaced(after, Some(at + 1)));
                    self.line_start || around != (true, true)
                }
                '\\' if least => after.is_none_or(|after| after.is_ascii_punctuation()),
                '|' if least => false,
                // Before a link, `!` would make it a picture.
                '!' if least => after.is_none() && at + 1 < self.edge.len(),
                '!' => true,
                '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '&' | '|' => true,
                '#' => self.line_start || self.ctx.heading,
                '>' | '-' | '+' | '=' | '~' => self.line_start,
                '.' | ')' => self.leading_digits,
                _ => false,
            };
            if c == '\r' {
                self.raw("&#13;");
                continue;
            }
            let digit = c.is_ascii_digit() && (self.line_start || self.leading_digits);
            if escape {
                self.out.push('\\');
            }
            self.out.push(c);
            self.line_start = false;
            self.leading_digits = digit;
        }
    }

    /// Code, as a code span on each line of it.
    fn code(&mut self, text: &str) {
        for (index, piece) in text.split('\n').enumerate() {
            if index > 0 {
                self.at += 1;
                self.line_break();
            }
            self.at += piece.len();
            if piece.is_empty() {
                continue;
            }
            let longest = piece.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat(longest + 1);
            let pad = piece.starts_with('`')
                || piece.ends_with('`')
                || (piece.starts_with(' ') && piece.ends_with(' ') && piece.contains(|c| c != ' '));
            let pad = if pad { " " } else { "" };
            if self.line_start && fence.len() >= 3 {
                // A line starting with three backticks would be taken for a fence where the
                // talk is split into slides, so something empty goes first.
                self.raw("<Text></Text>");
            }
            self.raw(&format!("{fence}{pad}{piece}{pad}{fence}"));
        }
    }
}

/// Whether `&` followed by `rest` would be read as a character reference.
fn starts_a_reference(rest: &str) -> bool {
    if rest.starts_with('#') {
        return true;
    }
    let name = rest.chars().take_while(char::is_ascii_alphanumeric).count();
    name > 0 && rest[name..].starts_with(';')
}

/// A link's destination as it's written between `(` and `)`.
fn destination(url: &str) -> String {
    let bare = !url.is_empty() && !url.contains(|c: char| c.is_whitespace() || c.is_control() || "<>()\\&".contains(c));
    if bare {
        return url.to_string();
    }
    let mut out = String::from("<");
    for c in url.chars() {
        if matches!(c, '\\' | '<' | '>' | '&') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('>');
    out
}

// ---------------------------------------------------------------------------------------
// Paragraphs and their places

/// What's known of the source a flow is written into.
struct Source<'a> {
    text: &'a str,
    eol: &'static str,
    /// What starts the line of a paragraph at the flow's top level.
    base: String,
    /// How content may be written.
    ways: Ways,
}

/// Where the line holding `at` starts. The first line starts after a byte order mark.
fn line_start(text: &str, at: usize) -> usize {
    match text[..at].rfind(['\n', '\r']) {
        Some(i) => i + 1,
        None if text.starts_with('\u{feff}') && at >= 3 => 3,
        None => 0,
    }
}

impl Source<'_> {
    fn line_start(&self, at: usize) -> usize {
        line_start(self.text, at)
    }

    /// What's before `at` on its line, if that's only spaces and tabs.
    fn prefix(&self, at: usize) -> Option<&str> {
        let prefix = self.text.get(self.line_start(at)..at)?;
        prefix.chars().all(|c| c == ' ' || c == '\t').then_some(prefix)
    }

    /// What's before `at` on its line as blank space: what a line of the paragraph after a
    /// line break starts with, to stay at its column.
    fn column(&self, at: usize) -> Option<String> {
        let before = self.text.get(self.line_start(at)..at)?;
        Some(before.chars().map(|c| if c == '\t' { '\t' } else { ' ' }).collect())
    }

    /// The end of the line `at` is on, after its line ending, and of the blank lines after
    /// it, but not past `limit`.
    fn past_blank_lines(&self, at: usize, limit: usize) -> usize {
        let mut end = self.text[at..limit].find('\n').map_or(limit, |i| at + i + 1);
        while end < limit {
            let next = self.text[end..limit].find('\n').map_or(limit, |i| end + i + 1);
            if !self.text[end..next].trim().is_empty() {
                break;
            }
            end = next;
        }
        end
    }
}

/// How a list item is written: what's before its marker, the marker, and the column of
/// its text.
#[derive(Clone, Debug)]
struct Shape {
    indent: String,
    marker: Marker,
    content: String,
}

#[derive(Clone, Debug)]
enum Marker {
    Bullet(char),
    Number(u64, char),
}

impl Marker {
    fn text(&self) -> String {
        match self {
            Marker::Bullet(c) => c.to_string(),
            Marker::Number(n, delim) => format!("{n}{delim}"),
        }
    }
}

/// The list item marker that starts `text`, and its length.
fn marker_at(text: &str) -> Option<(Marker, usize)> {
    let bytes = text.as_bytes();
    match bytes.first()? {
        c @ (b'-' | b'*' | b'+') => Some((Marker::Bullet(char::from(*c)), 1)),
        _ => {
            let digits = bytes.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let delim = *bytes.get(digits)?;
            if digits == 0 || digits > 9 || !matches!(delim, b'.' | b')') {
                return None;
            }
            Some((Marker::Number(text[..digits].parse().ok()?, char::from(delim)), digits + 1))
        }
    }
}

fn level_of(kind: ParaKind) -> Option<u8> {
    match kind {
        ParaKind::Bullet { level } | ParaKind::Numbered { level } => Some(level),
        _ => None,
    }
}

/// Whether a list of these paragraphs can be written at all: every item has the item it
/// sits in just before it, and an empty item holds no other.
fn writable(paras: &[Para]) -> bool {
    paras.iter().enumerate().all(|(index, para)| {
        let before = index.checked_sub(1).map(|i| &paras[i]);
        let fits = match (level_of(para.kind), before.and_then(|before| level_of(before.kind).map(|l| (l, before.spans.is_empty())))) {
            (Some(level), _) if level > MAX_LEVEL => false,
            (Some(level), None) => level == 0,
            (Some(level), Some((above, above_empty))) => level <= above + u8::from(!above_empty),
            (None, _) => true,
        };
        let heading_fits = match para.kind {
            ParaKind::Heading(level) => (1..=6).contains(&level),
            _ => true,
        };
        fits && heading_fits
    })
}

/// Where each new paragraph came from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Origin {
    /// The same as old paragraph `i`.
    Kept(usize),
    /// Old paragraph `i`, changed.
    Paired(usize),
    New,
}

/// Matches `new` against `old`: what's the same at the start and at the end is kept, and
/// what's between is paired in order. Also gives the old paragraphs that are gone, in order.
fn align(old: &[Para], new: &[Para]) -> (Vec<Origin>, Range<usize>) {
    let most = old.len().min(new.len());
    let start = (0..most).take_while(|&i| old[i] == new[i]).count();
    let end = (0..most - start).take_while(|&back| old[old.len() - 1 - back] == new[new.len() - 1 - back]).count();
    let (old_middle, new_middle) = (old.len() - start - end, new.len() - start - end);
    let pairs = old_middle.min(new_middle);
    let mut origin = Vec::with_capacity(new.len());
    origin.extend((0..start).map(Origin::Kept));
    origin.extend((start..start + pairs).map(|i| if old[i] == new[i] { Origin::Kept(i) } else { Origin::Paired(i) }));
    origin.extend((pairs..new_middle).map(|_| Origin::New));
    origin.extend((old.len() - end..old.len()).map(Origin::Kept));
    (origin, start + pairs..old.len() - end)
}

/// What a flow's paragraphs are written with that isn't in the model.
struct Look {
    /// The bullet each level's items had, or `-`.
    bullets: [char; MAX_LEVEL as usize + 1],
    /// What follows numbers.
    delim: char,
}

/// How each of `new`'s list items is written. Items that `from_old` gives a shape for keep
/// it. Others take the indentation and marker of the item before them at their level, or
/// the text column of the item they're in, and run on the numbering, but keep the marker
/// of the shape `old_shape` gives them, if any, and its spacing too where they're at its
/// column. `blank_before` says which have a blank line before them. `start` gives the number a numbered item starts a list with.
fn shapes(
    new: &[Para],
    base: &str,
    look: &Look,
    from_old: impl Fn(usize) -> Option<Option<Shape>>,
    old_shape: impl Fn(usize) -> Option<Shape>,
    start: impl Fn(usize) -> Option<u64>,
    blank_before: impl Fn(usize) -> bool,
) -> Option<Vec<Option<Shape>>> {
    let mut shapes: Vec<Option<Shape>> = Vec::with_capacity(new.len());
    for (index, para) in new.iter().enumerate() {
        let Some(level) = level_of(para.kind) else {
            shapes.push(None);
            continue;
        };
        if let Some(old) = from_old(index) {
            shapes.push(Some(old?));
            continue;
        }
        let numbered = matches!(para.kind, ParaKind::Numbered { .. });
        // Straight under another item's line, only a list starting at 1 is read as one.
        let tight = index > 0 && level_of(new[index - 1].kind).is_some() && !blank_before(index);
        let fresh = |indent: String| {
            let first = if tight { 1 } else { start(index).unwrap_or(1) };
            let marker = if numbered { Marker::Number(first, look.delim) } else { Marker::Bullet(look.bullets[usize::from(level)]) };
            (indent, marker)
        };
        let mut found = None;
        // Whether the item runs on a list from the item before it at its level.
        let mut runs_on = false;
        for back in (0..index).rev() {
            let (Some(above), Some(shape)) = (level_of(new[back].kind), shapes[back].as_ref()) else { break };
            if above > level {
                continue;
            }
            runs_on = above == level && new[back].kind == para.kind;
            found = Some(if above == level {
                match (&shape.marker, numbered, new[back].kind == para.kind) {
                    (Marker::Number(n, delim), true, true) => (shape.indent.clone(), Marker::Number(n + 1, *delim)),
                    (Marker::Bullet(c), false, true) => (shape.indent.clone(), Marker::Bullet(*c)),
                    _ => fresh(shape.indent.clone()),
                }
            } else if above + 1 == level {
                fresh(shape.content.clone())
            } else {
                return None;
            });
            break;
        }
        let (indent, mut marker) = match found {
            Some(found) => found,
            None if level == 0 => fresh(base.to_string()),
            None => return None,
        };
        let old = old_shape(index);
        let mut kept = false;
        match old.as_ref().map(|old| &old.marker) {
            // A list that starts straight under an item's line is only read as one from 1.
            Some(Marker::Number(n, delim)) if numbered => {
                kept = runs_on || !tight || *n == 1;
                marker = Marker::Number(if kept { *n } else { 1 }, *delim);
            }
            Some(Marker::Bullet(c)) if !numbered => {
                kept = true;
                marker = Marker::Bullet(*c);
            }
            _ => {}
        }
        if let Some(old) = old.filter(|old| kept && old.indent == indent) {
            shapes.push(Some(old));
            continue;
        }
        let content = format!("{indent}{}", " ".repeat(marker.text().len() + 1));
        shapes.push(Some(Shape { indent, marker, content }));
    }
    Some(shapes)
}

/// What goes between two paragraphs written one after the other.
fn separator(before: &Para, after: &Para, eol: &str) -> String {
    let tight = match (level_of(before.kind), level_of(after.kind)) {
        // An empty item straight under its item's text would make that text a heading.
        (Some(above), Some(level)) => !(after.spans.is_empty() && level > above),
        _ => false,
    };
    if tight { eol.to_string() } else { format!("{eol}{eol}") }
}

/// A paragraph written out, without what's before it on its first line. `keep` is content
/// to use as it is in place of writing the spans.
fn block_text(para: &Para, shape: Option<&Shape>, src: &Source, keep: Option<&str>) -> Option<String> {
    let inline = |probe: Probe, heading: bool, indent: &str| match keep {
        Some(kept) => Some(kept.to_string()),
        None => inline_for(&para.spans, probe, &Inline { heading, indent, eol: src.eol }, src.ways),
    };
    match para.kind {
        ParaKind::Body if para.spans.is_empty() => Some(src.ways.nothing.to_string()),
        ParaKind::Body => inline(Probe::Body, false, &src.base),
        ParaKind::Heading(level) => {
            let hashes = "#".repeat(usize::from(level));
            if para.spans.is_empty() {
                return Some(hashes);
            }
            if !para.spans.iter().any(|span| span.text.contains('\n')) {
                return Some(format!("{hashes} {}", inline(Probe::Atx, true, &src.base)?));
            }
            let underline = match level {
                1 => "===",
                2 => "--",
                _ => return None,
            };
            Some(format!("{}{}{}{underline}", inline(Probe::Setext, false, &src.base)?, src.eol, src.base))
        }
        ParaKind::Bullet { .. } | ParaKind::Numbered { .. } => {
            let shape = shape?;
            let marker = shape.marker.text();
            if para.spans.is_empty() {
                return Some(marker);
            }
            // The spaces between the marker and the text, as the shape has them.
            let spaces = shape.content.len().saturating_sub(shape.indent.len() + marker.len()).clamp(1, 4);
            Some(format!("{marker}{}{}", " ".repeat(spaces), inline(Probe::Item, false, &shape.content)?))
        }
    }
}

/// How old paragraph `index` of `flow`, a list item, is written now.
fn old_shape(src: &Source, flow: &TextFlow, index: usize) -> Option<Shape> {
    let source = &flow.source[index];
    let indent = src.prefix(source.block.start)?.to_string();
    let rest = src.text.get(source.block.start..)?;
    let (marker, length) = marker_at(rest)?;
    // The spaces after the marker, up to the text on its line. With none, or more than an
    // item's text can be indented by, the text is one space after it.
    let after = &rest[length..];
    let spaces = after.bytes().take_while(|byte| *byte == b' ').count();
    let text_follows = after[spaces..].chars().next().is_some_and(|c| c != '\n' && c != '\r');
    let spaces = if text_follows && (1..=4).contains(&spaces) { spaces } else { 1 };
    let content = format!("{indent}{}", " ".repeat(length + spaces));
    Some(Shape { indent, marker, content })
}

/// The look of `flow`'s list items.
fn look_of(src: &Source, flow: &TextFlow) -> Look {
    let mut look = Look { bullets: ['-'; MAX_LEVEL as usize + 1], delim: '.' };
    let (mut bullet_seen, mut delim_seen) = ([false; MAX_LEVEL as usize + 1], false);
    for (index, para) in flow.paras.iter().enumerate() {
        let Some(level) = level_of(para.kind).map(usize::from).filter(|&level| level <= usize::from(MAX_LEVEL)) else { continue };
        match old_shape(src, flow, index).map(|shape| shape.marker) {
            Some(Marker::Bullet(c)) if !bullet_seen[level] => {
                look.bullets[level] = c;
                bullet_seen[level] = true;
            }
            Some(Marker::Number(_, delim)) if !delim_seen => {
                look.delim = delim;
                delim_seen = true;
            }
            _ => {}
        }
    }
    look
}

/// The number old paragraph `i` is written with, if it's a numbered item.
fn old_number(src: &Source, flow: &TextFlow, origin: Origin) -> Option<u64> {
    let (Origin::Kept(i) | Origin::Paired(i)) = origin else { return None };
    match old_shape(src, flow, i)?.marker {
        Marker::Number(n, _) => Some(n),
        Marker::Bullet(_) => None,
    }
}

/// Whether everything between `flow`'s paragraphs is blank, so nothing but them is lost
/// when they're written again.
fn gaps_are_blank(src: &Source, flow: &TextFlow, between: Range<usize>) -> bool {
    between.into_iter().all(|i| {
        let (Some(before), Some(after)) = (i.checked_sub(1).and_then(|before| flow.source.get(before)), flow.source.get(i)) else { return true };
        src.text.get(before.block.end..after.block.start).is_some_and(|gap| gap.trim().is_empty())
    })
}

/// The change that edits only what changed. When every paragraph goes, the blank lines
/// after them go too, unless `keep_blank`.
fn least_change(
    src: &Source,
    flow: &TextFlow,
    new: &[Para],
    origin: &[Origin],
    gone: &Range<usize>,
    slide_end: usize,
    keep_blank: bool,
) -> Option<Vec<SourceEdit>> {
    let old = &flow.paras;
    let look = look_of(src, flow);
    let keeps_shape = |j: usize| match origin[j] {
        Origin::Kept(i) | Origin::Paired(i) if old[i].kind == new[j].kind => Some(old_shape(src, flow, i)),
        _ => None,
    };
    let blank = |j: usize| j > 0 && separator(&new[j - 1], &new[j], src.eol).len() > src.eol.len();
    let shapes = shapes(new, &src.base, &look, keeps_shape, |_| None, |j| old_number(src, flow, origin[j]), blank)?;
    let mut edits = Vec::new();
    let mut j = 0;
    while j < new.len() {
        let para = &new[j];
        match origin[j] {
            Origin::Kept(_) => {}
            Origin::Paired(i) => edits.push(paired_edit(src, flow, i, para, shapes[j].as_ref())?),
            Origin::New => {
                let end = j + origin[j..].iter().take_while(|origin| **origin == Origin::New).count();
                let written = |k: usize| Some(format!("{}{}", shapes[k].as_ref().map_or(src.base.as_str(), |shape| &shape.indent), block_text(&new[k], shapes[k].as_ref(), src, None)?));
                let mut text = String::new();
                let at = match j.checked_sub(1).map(|before| origin[before]) {
                    Some(Origin::Kept(i) | Origin::Paired(i)) => {
                        for k in j..end {
                            text.push_str(&separator(&new[k - 1], &new[k], src.eol));
                            text.push_str(&written(k)?);
                        }
                        // What follows was tight against the paragraph before, and may need
                        // a blank line between it and what's added.
                        if let (Some(next), Some(next_source)) = (new.get(end), flow.source.get(i + 1)) {
                            let gap = src.text.get(flow.source[i].block.end..next_source.block.start)?;
                            let blank_wanted = separator(&new[end - 1], next, src.eol).len() > src.eol.len();
                            if blank_wanted && gap.matches('\n').count() < 2 {
                                text.push_str(src.eol);
                            }
                        }
                        flow.source[i].block.end
                    }
                    None => {
                        let next = flow.source.first()?.block.start;
                        src.prefix(next)?;
                        for k in j..end {
                            text.push_str(&written(k)?);
                            text.push_str(&separator(&new[k], new.get(k + 1)?, src.eol));
                        }
                        src.line_start(next)
                    }
                    Some(Origin::New) => return None,
                };
                edits.push(SourceEdit { range: at..at, text });
                j = end;
                continue;
            }
        }
        j += 1;
    }
    if !gone.is_empty() {
        if !gaps_are_blank(src, flow, gone.start..gone.end + 1) {
            return None;
        }
        let range = match (gone.start.checked_sub(1), flow.source.get(gone.end)) {
            (Some(before), _) => flow.source[before].block.end..flow.source[gone.end - 1].block.end,
            (None, Some(after)) => {
                src.prefix(after.block.start)?;
                src.line_start(flow.source[0].block.start)..src.line_start(after.block.start)
            }
            (None, None) => {
                let first = flow.source.first()?.block.start;
                src.prefix(first)?;
                let last = flow.source.last()?.block.end;
                let end = if keep_blank { last } else { src.past_blank_lines(last, slide_end) };
                src.line_start(first)..end
            }
        };
        edits.push(SourceEdit { range, text: String::new() });
    }
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    Some(edits)
}

/// The edit that turns old paragraph `i` of `flow` into `para`.
fn paired_edit(src: &Source, flow: &TextFlow, i: usize, para: &Para, shape: Option<&Shape>) -> Option<SourceEdit> {
    let (old, source) = (&flow.paras[i], &flow.source[i]);
    let has_break = para.spans.iter().any(|span| span.text.contains('\n'));
    let setext = src.text.get(source.inline.end..source.block.end).is_some_and(|after| after.contains(['\n', '\r']));
    let content_fits = match para.kind {
        ParaKind::Heading(_) => !has_break || setext,
        _ => true,
    };
    if old.kind == para.kind && !old.spans.is_empty() && !para.spans.is_empty() && content_fits {
        let probe = match para.kind {
            ParaKind::Body => Probe::Body,
            ParaKind::Heading(_) if setext => Probe::Setext,
            ParaKind::Heading(_) => Probe::Atx,
            _ => Probe::Item,
        };
        let indent = src.column(source.inline.start)?;
        let heading = matches!(probe, Probe::Atx);
        let text = inline_for(&para.spans, probe, &Inline { heading, indent: &indent, eol: src.eol }, src.ways)?;
        return Some(SourceEdit { range: source.inline.clone(), text });
    }
    // The whole paragraph is written again, from the start of its line.
    let start = src.line_start(source.block.start);
    src.prefix(source.block.start)?;
    let indent = shape.map_or(src.base.as_str(), |shape| &shape.indent);
    Some(SourceEdit { range: start..source.block.end, text: format!("{indent}{}", block_text(para, shape, src, None)?) })
}

/// The change that writes the whole flow's markers, indentation and spacing again.
/// Paragraphs whose words didn't change keep their content's bytes, and items whose kind
/// didn't change keep their markers.
fn whole_change(src: &Source, flow: &TextFlow, new: &[Para], origin: &[Origin]) -> Option<Vec<SourceEdit>> {
    let blank_line = format!("{}{}", src.eol, src.eol);
    let first = flow.source.first()?.block.start;
    src.prefix(first)?;
    if new.is_empty() || !gaps_are_blank(src, flow, 0..flow.source.len()) {
        return None;
    }
    let look = look_of(src, flow);
    let keeps_marker = |j: usize| match origin[j] {
        Origin::Kept(i) | Origin::Paired(i) if flow.paras[i].kind == new[j].kind => old_shape(src, flow, i),
        _ => None,
    };
    // A blank line that stood between two paragraphs that are still next to each other
    // stays, as what comes after it may need it to be read as it was.
    let blank: Vec<bool> = (0..new.len())
        .map(|j| {
            if j == 0 {
                return false;
            }
            let kept_gap = match (origin[j - 1], origin[j]) {
                (Origin::Kept(a) | Origin::Paired(a), Origin::Kept(b) | Origin::Paired(b)) if a + 1 == b => {
                    src.text.get(flow.source[a].block.end..flow.source[b].block.start).is_some_and(|gap| gap.matches('\n').count() >= 2)
                }
                _ => false,
            };
            kept_gap || separator(&new[j - 1], &new[j], src.eol).len() > src.eol.len()
        })
        .collect();
    let shapes = shapes(new, &src.base, &look, |_| None, keeps_marker, |j| old_number(src, flow, origin[j]), |j| blank[j])?;
    let mut text = String::new();
    for (j, para) in new.iter().enumerate() {
        if j > 0 {
            text.push_str(if blank[j] { &blank_line } else { src.eol });
        }
        let keep = match origin[j] {
            Origin::Kept(i) | Origin::Paired(i) if !para.spans.is_empty() && flow.paras[i].spans == para.spans => {
                src.text.get(flow.source[i].inline.clone())
            }
            _ => None,
        };
        text.push_str(shapes[j].as_ref().map_or(src.base.as_str(), |shape| &shape.indent));
        text.push_str(&block_text(para, shapes[j].as_ref(), src, keep)?);
    }
    Some(vec![SourceEdit { range: src.line_start(first)..flow.source.last()?.block.end, text }])
}

fn flow_change(talk: &Talk, index: usize, flow: &TextFlow, paras: &[Para]) -> Option<Change> {
    let slide = talk.slides.get(index)?;
    let flows = text_flows(talk, index);
    let at = flows.iter().position(|candidate| candidate == flow)?;
    let new = storable(paras);
    if new == flow.paras {
        return Some(Change { edits: Vec::new() });
    }
    if !writable(&new) {
        return None;
    }
    let first = flow.source.first()?.block.start;
    let base = src_base(&talk.source, first)?;
    let (origin, gone) = align(&flow.paras, &new);
    let mut expected: Vec<(FlowPlace, Vec<Para>)> = flows.iter().map(|flow| (flow.place.clone(), flow.paras.clone())).collect();
    if new.is_empty() {
        expected.remove(at);
    } else {
        expected[at].1 = new.clone();
    }
    // Content is written as plainly as reads back alone, and if that reads as something
    // else on the slide, in tags and with every escape.
    for ways in [PLAIN, GUARDED] {
        let src = Source { text: &talk.source, eol: eol_in(&talk.source), base: base.clone(), ways };
        let tries: [&dyn Fn() -> Option<Vec<SourceEdit>>; 3] = [
            &|| least_change(&src, flow, &new, &origin, &gone, slide.range.end, false),
            &|| least_change(&src, flow, &new, &origin, &gone, slide.range.end, true).filter(|_| new.is_empty()),
            &|| whole_change(&src, flow, &new, &origin),
        ];
        let found = tries.iter().filter_map(|attempt| attempt()).map(|edits| Change { edits }).find(|change| reads_as(talk, slide, change, &expected));
        if found.is_some() {
            return found;
        }
    }
    None
}

/// What starts the line of `at`, if it's blank.
fn src_base(source: &str, at: usize) -> Option<String> {
    let start = line_start(source, at);
    let prefix = source.get(start..at)?;
    prefix.chars().all(|c| c == ' ' || c == '\t').then(|| prefix.to_string())
}

// ---------------------------------------------------------------------------------------
// Checking a change

/// Whether `change`, made inside `slide` of `talk`, leaves it one slide whose flows are
/// `expected` and whose other content is as it was.
fn reads_as(talk: &Talk, slide: &Slide, change: &Change, expected: &[(FlowPlace, Vec<Para>)]) -> bool {
    let range = &slide.range;
    let inside = change.edits.iter().all(|edit| range.start <= edit.range.start && edit.range.end <= range.end);
    let ordered = change.edits.windows(2).all(|pair| pair[0].range.end <= pair[1].range.start);
    if !inside || !ordered || change.is_empty() {
        return false;
    }
    let Some(old_text) = talk.source.get(range.clone()) else { return false };
    let local = Change {
        edits: change
            .edits
            .iter()
            .map(|edit| SourceEdit { range: edit.range.start - range.start..edit.range.end - range.start, text: edit.text.clone() })
            .collect(),
    };
    let new_text = local.apply(old_text);
    // The slide still ends where the next one starts, and is still a slide: no break is
    // added or lost, and no fence is left open to swallow the next slide's break.
    let next_line = talk.source[range.end..].find('\n').map_or(talk.source.len(), |i| range.end + i + 1);
    let probe = format!("{new_text}{}", &talk.source[range.end..next_line]);
    let raw = split_slides(&probe);
    let slides = if next_line > range.end { 2 } else { 1 };
    if raw.len() != slides || raw[0].range.end != new_text.len() {
        return false;
    }
    let read = parse_talk(&new_text);
    let [new_slide] = read.slides.as_slice() else { return false };
    if new_slide.error.is_some() {
        return false;
    }
    let flows = text_flows(&read, 0);
    let same_flows = flows.len() == expected.len()
        && flows.iter().zip(expected).all(|(flow, (place, paras))| same_place(&flow.place, place) && flow.paras == *paras);
    same_flows && the_rest(new_slide) == the_rest(slide)
}

fn same_place(a: &FlowPlace, b: &FlowPlace) -> bool {
    match (a, b) {
        (FlowPlace::Region(a), FlowPlace::Region(b)) => a == b,
        (FlowPlace::Tag(_), FlowPlace::Tag(_)) => true,
        _ => false,
    }
}

/// What's on a slide besides its editable text, without positions.
fn the_rest(slide: &Slide) -> (Vec<String>, Option<String>, usize, Vec<String>) {
    fn sketch(block: &Block, out: &mut Vec<String>) {
        match block {
            Block::Heading { .. } | Block::Paragraph { .. } | Block::List { .. } => {}
            Block::Table { rows, .. } => out.push(format!("table {rows:?}")),
            Block::Code { language, code, .. } => out.push(format!("code {language:?} {code:?}")),
            Block::Picture { src, alt, .. } => out.push(format!("picture {src:?} {alt:?}")),
            // Where a region starts is checked by where the flows read.
            Block::Region { .. } => {}
            Block::Tag(tag) => {
                let attrs: Vec<_> = tag.attrs.iter().map(|attr| (&attr.name, &attr.value)).collect();
                out.push(format!("tag {} {attrs:?}", tag.name));
                for child in &tag.children {
                    sketch(child, out);
                }
            }
        }
    }
    let mut blocks = Vec::new();
    for block in &slide.blocks {
        sketch(block, &mut blocks);
    }
    (blocks, slide.notes.as_ref().map(|notes| notes.text.clone()), slide.quotes.len(), slide.inline_tags.clone())
}

// ---------------------------------------------------------------------------------------
// Text for an empty region

fn region_change(talk: &Talk, index: usize, region: &str, paras: &[Para]) -> Option<Change> {
    let slide = talk.slides.get(index)?;
    if slide.error.is_some() {
        return None;
    }
    let new = storable(paras);
    let (regions, main) = layout_of(slide);
    if !regions.contains(&region) || has_content(slide, region) {
        return None;
    }
    if new.is_empty() {
        return Some(Change { edits: Vec::new() });
    }
    if !writable(&new) {
        return None;
    }
    if region == "title" && !matches!(new.as_slice(), [Para { kind: ParaKind::Heading(_), .. }]) {
        return None;
    }
    let eol = eol_in(&talk.source);
    let src = Source { text: &talk.source, eol, base: String::new(), ways: PLAIN };
    let look = Look { bullets: ['-'; MAX_LEVEL as usize + 1], delim: '.' };
    let blank = |j: usize| j > 0 && separator(&new[j - 1], &new[j], eol).len() > eol.len();
    let shapes = shapes(&new, "", &look, |_| None, |_| None, |_| None, blank)?;
    let mut text = String::new();
    for (j, para) in new.iter().enumerate() {
        if j > 0 {
            text.push_str(&separator(&new[j - 1], para, eol));
        }
        text.push_str(shapes[j].as_ref().map_or("", |shape| &shape.indent));
        text.push_str(&block_text(para, shapes[j].as_ref(), &src, None)?);
    }
    let source = &talk.source;
    let body = body_start(source, slide)?;
    let after_blocks = slide.blocks.last().map(|block| block.range().end);
    // At the end of a line that may have no line ending, and before what follows.
    let at_end = |at: usize, what: String| {
        let lead = if at == 0 || source[..at].ends_with('\n') { "" } else { eol };
        SourceEdit { range: at..at, text: format!("{lead}{what}{eol}") }
    };
    let marker_for = |name: &str| {
        slide.blocks.iter().find_map(|block| match block {
            Block::Region { name: found, range } if found == name => Some(range.end),
            _ => None,
        })
    };
    let has_markers = slide.blocks.iter().any(|block| matches!(block, Block::Region { .. }));
    let edit = if region == "title" {
        let rest = source.get(body..slide.range.end)?;
        // A blank line under the slide's opening lines stays above the title, as a person
        // would write it.
        let skip = if source[..body].ends_with('\n') && rest.starts_with(eol) { eol.len() } else { 0 };
        let rest = &rest[skip..];
        let gap = if rest.starts_with(['\n', '\r']) {
            ""
        } else if rest.is_empty() {
            // A blank line before the next slide's, if there is one.
            if slide.range.end < source.len() { eol } else { "" }
        } else if rest.trim().is_empty() {
            ""
        } else {
            eol
        };
        SourceEdit { range: body + skip..body + skip, text: format!("{text}{eol}{gap}") }
    } else if let Some(marker) = marker_for(region) {
        let after = if source.get(marker..slide.range.end)?.trim().is_empty() { "" } else { eol };
        SourceEdit { range: marker..marker, text: format!("{eol}{eol}{text}{after}") }
    } else if region == main && !has_markers {
        match after_blocks {
            Some(end) => SourceEdit { range: end..end, text: format!("{eol}{eol}{text}") },
            None => {
                let rest = source.get(body..slide.range.end)?;
                if rest.trim().is_empty() {
                    at_end(body, text)
                } else {
                    SourceEdit { range: body..body, text: format!("{text}{eol}{eol}") }
                }
            }
        }
    } else {
        let marker = format!("<Region name=\"{region}\" />{eol}{eol}{text}");
        match after_blocks {
            Some(end) => SourceEdit { range: end..end, text: format!("{eol}{eol}{marker}") },
            None => at_end(body, marker),
        }
    };
    let change = Change::single(edit);
    let mut expected: Vec<(FlowPlace, Vec<Para>)> =
        text_flows(talk, index).into_iter().map(|flow| (flow.place, flow.paras)).collect();
    // The new flow sits wherever it reads: find it, and check the rest are as they were.
    let new_text = {
        let local = SourceEdit {
            range: change.edits[0].range.start - slide.range.start..change.edits[0].range.end - slide.range.start,
            text: change.edits[0].text.clone(),
        };
        local.apply(source.get(slide.range.clone())?)
    };
    let read = parse_talk(&new_text);
    let place = FlowPlace::Region(region.to_string());
    let at = text_flows(&read, 0).iter().position(|flow| flow.place == place && flow.paras == new)?;
    expected.insert(at.min(expected.len()), (place, new));
    reads_as(talk, slide, &change, &expected).then_some(change)
}

/// Where a slide's body starts: after its settings, or its break line, if it has either.
fn body_start(source: &str, slide: &Slide) -> Option<usize> {
    if let Some(settings) = &slide.settings {
        return Some(settings.range.end);
    }
    let start = slide.range.start;
    let bom = if start == 0 && source.starts_with('\u{feff}') { 3 } else { 0 };
    let rest = source.get(start + bom..slide.range.end)?;
    let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
    let line = rest[..line_end].trim_end_matches(['\n', '\r']);
    Some(if line.trim_end_matches(' ') == "---" { start + bom + line_end } else { start + bom })
}

/// Whether anything on `slide` is drawn in `region`, as the engine sends blocks to regions.
fn has_content(slide: &Slide, region: &str) -> bool {
    let (regions, main) = layout_of(slide);
    let mut title = regions.contains(&"title").then_some("title");
    let mut current = main;
    for block in &slide.blocks {
        let place = match block {
            Block::Region { name, .. } => {
                title = None;
                current = regions.iter().find(|known| *known == name).copied().unwrap_or(main);
                continue;
            }
            Block::Tag(tag) if is_positioned(tag) => continue,
            Block::Heading { .. } => title.take().unwrap_or(current),
            _ => current,
        };
        if place == region {
            return true;
        }
    }
    false
}
