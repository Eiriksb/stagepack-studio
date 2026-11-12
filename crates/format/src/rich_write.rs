//! Writing a slide's text model back as Markdown. Only the paragraphs that changed are
//! written: one whose words changed has its content replaced and keeps its marker, one
//! that's the same keeps every byte. Each change is read back before it's given out, and
//! one that wouldn't read back as the paragraphs it was asked for is refused: the change
//! that comes back is then empty.

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
/// break. Spans that can't be written exactly, such as a line break at the very end, come
/// out as near as they can.
pub fn write_inline(spans: &[Span]) -> String {
    let spans = tidy(spans);
    let ctx = Inline { heading: false, indent: "", eol: "\n" };
    inline_for(&spans, Probe::Body, &ctx).unwrap_or_else(|| render(&spans, &ctx, Form::Tags))
}

/// The edits that turn `flow`, a flow of slide `slide` as [`text_flows`] reads it now, into
/// `paras`. A paragraph equal to one it had keeps its bytes, and one whose spans changed but
/// whose kind didn't has only its content replaced. Added and removed paragraphs take the
/// list markers, numbering, indentation and blank lines of the paragraphs around them.
///
/// If `paras` can't be written so that they read back exactly, or `flow` isn't one of the
/// slide's flows, the change is empty, and nothing is changed. Nothing can be written for an
/// empty body paragraph, a line break at the very end of a paragraph, a line break in code,
/// a heading of more than one line below level 2, or a list item deeper than the one before
/// it allows.
pub fn write_flow(talk: &Talk, slide: usize, flow: &TextFlow, paras: &[Para]) -> Change {
    guarded(|| flow_change(talk, slide, flow, paras))
}

/// The edits that put `paras` into region `region` of slide `slide`, a region of its layout
/// that has nothing in it yet. The title's text, a single heading, goes at the top of the
/// slide. Another region's goes after its `<Region name="…" />` line, which is added at the
/// end of the slide if there isn't one, except for the layout's main region on a slide with
/// no such lines, whose text goes at the end. The change is empty if the region isn't the
/// layout's, already has something in it, or `paras` can't be written exactly there.
pub fn write_into_region(talk: &Talk, slide: usize, region: &str, paras: &[Para]) -> Change {
    guarded(|| region_change(talk, slide, region, paras))
}

/// What `make` gives, or an empty change if it gives nothing or panics.
fn guarded(make: impl FnOnce() -> Option<Change>) -> Change {
    catch_unwind(AssertUnwindSafe(make)).ok().flatten().unwrap_or(Change { edits: Vec::new() })
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

/// What a paragraph's content is tried out in, to see that it reads back.
#[derive(Clone, Copy)]
enum Probe {
    Body,
    Atx,
    Setext,
    Item,
}

/// The content for `spans`, in the first form that reads back as them, if any does.
fn inline_for(spans: &[Span], probe: Probe, ctx: &Inline) -> Option<String> {
    [Form::Marks, Form::Tags].into_iter().map(|form| render(spans, ctx, form)).find(|text| reads_back(text, spans, probe, ctx.eol))
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

/// Spans with the empty ones dropped and neighbours of the same style merged, as they read.
fn tidy(spans: &[Span]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans.iter().filter(|span| !span.text.is_empty()) {
        match out.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&span.text),
            _ => out.push(span.clone()),
        }
    }
    out
}

fn tidy_paras(paras: &[Para]) -> Vec<Para> {
    paras.iter().map(|para| Para { kind: para.kind, spans: tidy(&para.spans) }).collect()
}

/// Where `spans[from]`'s run of spans that `same` holds for ends.
fn run_end(spans: &[Span], from: usize, same: impl Fn(&Span, &Span) -> bool) -> usize {
    from + 1 + spans[from + 1..].iter().take_while(|span| same(&spans[from], span)).count()
}

/// The content for `spans` in `form`.
fn render(spans: &[Span], ctx: &Inline, form: Form) -> String {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let mut writer = Writer { out: String::new(), ctx, edge: edge_whitespace(&text), at: 0, line_start: true, leading_digits: false };
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
                    writer.content(span, wrapped || !attrs.is_empty());
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
        self.content(span, wrapped || !marker.is_empty());
        if !marker.is_empty() {
            self.raw(marker);
        }
    }

    /// A span's text, as code or as escaped text. `wrapped` says whether something closes
    /// after it, which a line can't start with.
    fn content(&mut self, span: &Span, wrapped: bool) {
        if span.style.code {
            self.code(&span.text);
        } else {
            self.text(&span.text);
        }
        if wrapped && span.text.ends_with('\n') {
            // A line can't start by closing what's open, so something empty goes first.
            self.raw("<Text></Text>");
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
        for c in text.chars() {
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
            let escape = match c {
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
            self.raw(&format!("{fence}{pad}{piece}{pad}{fence}"));
        }
    }
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
}

impl Source<'_> {
    fn line_start(&self, at: usize) -> usize {
        self.text[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1)
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
/// sits in just before it, an empty item holds no other, and there's no empty body text.
fn writable(paras: &[Para]) -> bool {
    paras.iter().enumerate().all(|(index, para)| {
        let before = index.checked_sub(1).map(|i| &paras[i]);
        let fits = match (level_of(para.kind), before.and_then(|before| level_of(before.kind).map(|l| (l, before.spans.is_empty())))) {
            (Some(level), _) if level > MAX_LEVEL => false,
            (Some(level), None) => level == 0,
            (Some(level), Some((above, above_empty))) => level <= above + u8::from(!above_empty),
            (None, _) => true,
        };
        let ends_in_a_break = para.spans.last().is_some_and(|span| span.text.ends_with('\n'));
        let heading_fits = match para.kind {
            ParaKind::Heading(level) => (1..=6).contains(&level),
            _ => true,
        };
        fits && heading_fits && !ends_in_a_break && !(para.kind == ParaKind::Body && para.spans.is_empty())
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
/// the text column of the item they're in, and run on the numbering. `start` gives the
/// number a numbered item starts a list with.
fn shapes(
    new: &[Para],
    base: &str,
    look: &Look,
    from_old: impl Fn(usize) -> Option<Option<Shape>>,
    start: impl Fn(usize) -> Option<u64>,
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
        let tight = index > 0 && level_of(new[index - 1].kind).is_some();
        let fresh = |indent: String| {
            let first = if tight { 1 } else { start(index).unwrap_or(1) };
            let marker = if numbered { Marker::Number(first, look.delim) } else { Marker::Bullet(look.bullets[usize::from(level)]) };
            (indent, marker)
        };
        let mut found = None;
        for back in (0..index).rev() {
            let (Some(above), Some(shape)) = (level_of(new[back].kind), shapes[back].as_ref()) else { break };
            if above > level {
                continue;
            }
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
        let (indent, marker) = match found {
            Some(found) => found,
            None if level == 0 => fresh(base.to_string()),
            None => return None,
        };
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
        None => inline_for(&para.spans, probe, &Inline { heading, indent, eol: src.eol }),
    };
    match para.kind {
        ParaKind::Body if para.spans.is_empty() => None,
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
            Some(format!("{marker} {}", inline(Probe::Item, false, &shape.content)?))
        }
    }
}

/// How old paragraph `index` of `flow`, a list item, is written now.
fn old_shape(src: &Source, flow: &TextFlow, index: usize) -> Option<Shape> {
    let source = &flow.source[index];
    let indent = src.prefix(source.block.start)?.to_string();
    let (marker, length) = marker_at(src.text.get(source.block.start..)?)?;
    let same_line = !source.inline.is_empty() && src.line_start(source.inline.start) == src.line_start(source.block.start);
    let content = if same_line { src.column(source.inline.start)? } else { format!("{indent}{}", " ".repeat(length + 1)) };
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

/// The change that edits only what changed.
fn least_change(src: &Source, flow: &TextFlow, new: &[Para], origin: &[Origin], gone: &Range<usize>, slide_end: usize) -> Option<Vec<SourceEdit>> {
    let old = &flow.paras;
    let look = look_of(src, flow);
    let keeps_shape = |j: usize| match origin[j] {
        Origin::Kept(i) | Origin::Paired(i) if old[i].kind == new[j].kind => Some(old_shape(src, flow, i)),
        _ => None,
    };
    let shapes = shapes(new, &src.base, &look, keeps_shape, |j| old_number(src, flow, origin[j]))?;
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
                src.line_start(first)..src.past_blank_lines(flow.source.last()?.block.end, slide_end)
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
        let text = inline_for(&para.spans, probe, &Inline { heading, indent: &indent, eol: src.eol })?;
        return Some(SourceEdit { range: source.inline.clone(), text });
    }
    // The whole paragraph is written again, from the start of its line.
    let start = src.line_start(source.block.start);
    src.prefix(source.block.start)?;
    let indent = shape.map_or(src.base.as_str(), |shape| &shape.indent);
    Some(SourceEdit { range: start..source.block.end, text: format!("{indent}{}", block_text(para, shape, src, None)?) })
}

/// The change that writes the whole flow again, keeping the content of paragraphs whose
/// words didn't change if `keep_words`.
fn whole_change(src: &Source, flow: &TextFlow, new: &[Para], origin: &[Origin], keep_words: bool) -> Option<Vec<SourceEdit>> {
    let first = flow.source.first()?.block.start;
    src.prefix(first)?;
    if new.is_empty() || !gaps_are_blank(src, flow, 0..flow.source.len()) {
        return None;
    }
    let look = look_of(src, flow);
    let shapes = shapes(new, &src.base, &look, |_| None, |j| old_number(src, flow, origin[j]))?;
    let mut text = String::new();
    for (j, para) in new.iter().enumerate() {
        if j > 0 {
            text.push_str(&separator(&new[j - 1], para, src.eol));
        }
        let keep = match origin[j] {
            Origin::Kept(i) | Origin::Paired(i) if keep_words && !para.spans.is_empty() && flow.paras[i].spans == para.spans => {
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
    let new = tidy_paras(paras);
    if new == flow.paras {
        return Some(Change { edits: Vec::new() });
    }
    if !writable(&new) {
        return None;
    }
    let first = flow.source.first()?.block.start;
    let src = Source { text: &talk.source, eol: eol_in(&talk.source), base: src_base(&talk.source, first)? };
    let (origin, gone) = align(&flow.paras, &new);
    let mut expected: Vec<(FlowPlace, Vec<Para>)> = flows.iter().map(|flow| (flow.place.clone(), flow.paras.clone())).collect();
    if new.is_empty() {
        expected.remove(at);
    } else {
        expected[at].1 = new.clone();
    }
    let tries: [&dyn Fn() -> Option<Vec<SourceEdit>>; 3] = [
        &|| least_change(&src, flow, &new, &origin, &gone, slide.range.end),
        &|| whole_change(&src, flow, &new, &origin, true),
        &|| whole_change(&src, flow, &new, &origin, false),
    ];
    tries.iter().filter_map(|attempt| attempt()).map(|edits| Change { edits }).find(|change| reads_as(talk, slide, change, &expected))
}

/// What starts the line of `at`, if it's blank.
fn src_base(source: &str, at: usize) -> Option<String> {
    let start = source[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
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
    // The slide still ends where the next one starts: no break is added, and no fence is
    // left open to swallow the next slide's break.
    let eol = eol_in(&talk.source);
    let ending = if new_text.ends_with('\n') || new_text.is_empty() { "" } else { eol };
    let probe = format!("{new_text}{ending}---{eol}after");
    let raw = split_slides(&probe);
    if raw.len() != 2 || raw[0].range.end != new_text.len() + ending.len() {
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
    let new = tidy_paras(paras);
    if new.is_empty() {
        return Some(Change { edits: Vec::new() });
    }
    let (regions, main) = layout_of(slide);
    if !regions.contains(&region) || has_content(slide, region) || !writable(&new) {
        return None;
    }
    if region == "title" && !matches!(new.as_slice(), [Para { kind: ParaKind::Heading(_), .. }]) {
        return None;
    }
    let eol = eol_in(&talk.source);
    let src = Source { text: &talk.source, eol, base: String::new() };
    let look = Look { bullets: ['-'; MAX_LEVEL as usize + 1], delim: '.' };
    let shapes = shapes(&new, "", &look, |_| None, |_| None)?;
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
        let gap = if rest.trim().is_empty() || rest.starts_with(['\n', '\r']) { "" } else { eol };
        SourceEdit { range: body..body, text: format!("{text}{eol}{gap}") }
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
