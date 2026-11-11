//! Adding, removing, moving and hiding slides, and their notes, as source edits. A slide's
//! bytes move as they are: only the break lines where slides now meet or part, and the
//! talk's settings when another slide becomes first, are written or taken away.

use std::ops::Range;

use crate::attrs::scan_open_tag;
use crate::edit::{Change, SourceEdit, eol_in, set_setting};
use crate::model::{Slide, Talk};
use crate::split::{RawSlide, split_slides};

/// The settings that belong to the whole talk. They live in the first slide's settings
/// block, and go with whichever slide is first.
pub const TALK_SETTINGS: [&str; 6] = ["title", "color.background", "color.text", "color.accent", "font", "font.mono"];

const HIDDEN: &str = "hidden";

fn no_change() -> Change {
    Change { edits: Vec::new() }
}

/// Adds a slide holding `text`, which has no break line of its own, after slide `after`,
/// or first when that is `None`. Its line endings become the talk's.
pub fn insert_slide(talk: &Talk, after: Option<usize>, text: &str) -> Change {
    let count = talk.slides.len();
    let mut order: Vec<Item> = (0..count).map(Item::Slide).collect();
    let at = after.map_or(0, |after| after.min(count.saturating_sub(1)) + 1);
    order.insert(at, Item::New(with_line_endings(text, eol_in(&talk.source))));
    rearrange(talk, &order)
}

/// Removes slide `index`. `None` when it's the only slide, or there is no such slide.
pub fn delete_slide(talk: &Talk, index: usize) -> Option<Change> {
    if talk.slides.len() < 2 || index >= talk.slides.len() {
        return None;
    }
    let order: Vec<Item> = (0..talk.slides.len()).filter(|&i| i != index).map(Item::Slide).collect();
    Some(rearrange(talk, &order))
}

/// Puts a copy of slide `index`, its settings and notes too, straight after it. A copy of
/// the first slide has none of the talk's settings.
pub fn duplicate_slide(talk: &Talk, index: usize) -> Change {
    if index >= talk.slides.len() {
        return no_change();
    }
    let mut order: Vec<Item> = (0..talk.slides.len()).map(Item::Slide).collect();
    order.insert(index + 1, Item::Copy(index));
    rearrange(talk, &order)
}

/// Moves slide `from` so that `to` is its index afterwards.
pub fn move_slide(talk: &Talk, from: usize, to: usize) -> Change {
    let count = talk.slides.len();
    if from == to || from >= count || to >= count {
        return no_change();
    }
    let mut order: Vec<Item> = (0..count).map(Item::Slide).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    rearrange(talk, &order)
}

pub fn is_hidden(slide: &Slide) -> bool {
    slide.settings.as_ref().and_then(|settings| settings.get(HIDDEN)) == Some("true")
}

/// Hides slide `index` with the setting `hidden: true`, or shows it by taking the setting
/// away again.
pub fn set_hidden(talk: &Talk, index: usize, hidden: bool) -> Change {
    let Some(slide) = talk.slides.get(index) else { return no_change() };
    if hidden == is_hidden(slide) && (hidden || setting_entry(slide, HIDDEN).is_none()) {
        return no_change();
    }
    if hidden {
        Change::single(set_setting(&talk.source, slide, HIDDEN, "true"))
    } else {
        remove_setting(&talk.source, slide, HIDDEN).map_or_else(no_change, Change::single)
    }
}

fn setting_entry<'a>(slide: &'a Slide, key: &str) -> Option<&'a crate::settings::SettingEntry> {
    slide.settings.as_ref()?.entries.iter().find(|entry| entry.key == key)
}

/// Takes the setting `key` out of the slide's block, with its whole line. When it was the
/// only entry the block goes too, but for the `---` that starts any slide but the first,
/// or a first slide with nothing after its block.
pub fn remove_setting(source: &str, slide: &Slide, key: &str) -> Option<SourceEdit> {
    let settings = slide.settings.as_ref()?;
    let entry = setting_entry(slide, key)?;
    if settings.entries.len() > 1 {
        return Some(SourceEdit { range: entry.line.start..line_end(source, entry.line.start), text: String::new() });
    }
    let first = slide.range.start == 0;
    if first && settings.range.end < slide.range.end {
        return Some(SourceEdit { range: settings.range.clone(), text: String::new() });
    }
    // The opening `---` stays as the slide's break line. At the very end of a file with
    // no last line ending, it's the line ending that goes, not the rule.
    let open_end = line_end(source, settings.range.start);
    let start = if settings.range.end == source.len() && !source.ends_with('\n') {
        settings.range.start + source[settings.range.start..open_end].trim_end_matches(['\r', '\n']).len()
    } else {
        open_end
    };
    Some(SourceEdit { range: start..settings.range.end, text: String::new() })
}

/// Sets what slide `index`'s `<Notes>` hold: the words inside an existing element are
/// replaced, an element is added at the end of a slide with none, and the element goes
/// when `notes` is empty. Line endings become the talk's.
///
/// Notes are read back as they are written, so nothing can be escaped invisibly, and the
/// `markdown` crate refuses a zero-width character after `<` as much as `</Notes` itself.
/// So `</Notes` is written `\</Notes`: the backslash makes the `<` plain text, and is what
/// the notes read back with. One that already has an odd number of backslashes before it
/// is harmless already, and kept as it is. Nothing else is changed: notes that hold a tag
/// of their own, or a line that is only `---`, are written as they are. A slide that can't
/// be read is left alone.
pub fn set_notes(talk: &Talk, index: usize, notes: &str) -> Change {
    let source = talk.source.as_str();
    let Some(slide) = talk.slides.get(index) else { return no_change() };
    if slide.error.is_some() {
        return no_change();
    }
    let eol = eol_in(source);
    let notes = harmless(&with_line_endings(notes, eol));
    let edit = match &slide.notes {
        Some(_) if notes.trim().is_empty() => remove_notes(source, slide),
        Some(old) => {
            let (_, inner_start) = scan_open_tag(source, old.range.start);
            match source[inner_start.min(old.range.end)..old.range.end].rfind("</Notes") {
                Some(at) => {
                    let inner = inner_start..inner_start + at;
                    let words = &source[inner.clone()];
                    let lead = words.len() - words.trim_start().len();
                    let core = inner.start + lead..inner.start + words.trim_end().len();
                    if core.start >= core.end {
                        SourceEdit { range: inner, text: format!("{eol}{notes}{eol}") }
                    } else {
                        SourceEdit { range: core, text: notes }
                    }
                }
                None => SourceEdit { range: old.range.clone(), text: format!("<Notes>{eol}{notes}{eol}</Notes>") },
            }
        }
        None if notes.trim().is_empty() => return no_change(),
        None => add_notes(source, &split_slides(source)[index], &notes, eol),
    };
    if source[edit.range.clone()] == edit.text { no_change() } else { Change::single(edit) }
}

/// The edit that puts a new `<Notes>` element after the slide's last line that isn't blank.
fn add_notes(source: &str, raw: &RawSlide, notes: &str, eol: &str) -> SourceEdit {
    let content = raw.body.start.max(bom_len(source))..raw.range.end;
    let mut at = content.start;
    let mut line = content.start;
    while line < content.end {
        let end = line_end(source, line).min(content.end);
        if !source[line..end].trim().is_empty() {
            at = end;
        }
        line = end;
    }
    let ends_a_line = at == 0 || at == bom_len(source) || source[..at].ends_with('\n');
    let mut text = String::new();
    if !ends_a_line {
        text.push_str(eol);
    }
    if at > content.start {
        text.push_str(eol);
    }
    text.push_str(&format!("<Notes>{eol}{notes}{eol}</Notes>"));
    if at < source.len() || ends_a_line {
        text.push_str(eol);
    }
    SourceEdit { range: at..at, text }
}

/// The edit that takes the slide's `<Notes>` element away with its line, and the blank
/// line before it.
fn remove_notes(source: &str, slide: &Slide) -> SourceEdit {
    let Some(notes) = &slide.notes else { return SourceEdit { range: 0..0, text: String::new() } };
    let mut start = notes.range.start;
    let mut end = notes.range.end;
    let after = &source[end..];
    let ending_after = if after.starts_with("\r\n") { 2 } else { usize::from(after.starts_with('\n')) };
    end += ending_after;
    let before = &source[..start];
    let blank_before = |text: &str| text.ends_with("\r\n\r\n") || text.ends_with("\n\n");
    if blank_before(before) {
        start -= ending_len(before);
    }
    if ending_after == 0 && end == source.len() {
        // The element ended the file, with no line ending after it: so does the line before.
        start -= ending_len(&source[..start]);
    }
    SourceEdit { range: start.max(slide.range.start)..end, text: String::new() }
}

/// `notes` with every `</Notes` that would end the element written `\</Notes`.
fn harmless(notes: &str) -> String {
    let mut out = String::with_capacity(notes.len());
    let mut rest = notes;
    while let Some(at) = rest.find("</Notes") {
        out.push_str(&rest[..at]);
        let backslashes = out.bytes().rev().take_while(|&byte| byte == b'\\').count();
        if backslashes % 2 == 0 {
            out.push('\\');
        }
        out.push_str("</Notes");
        rest = &rest[at + "</Notes".len()..];
    }
    out.push_str(rest);
    out
}

fn with_line_endings(text: &str, eol: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', eol)
}

fn bom_len(source: &str) -> usize {
    if source.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 }
}

/// The offset just past the line ending of the line that `at` is in.
fn line_end(source: &str, at: usize) -> usize {
    source[at..].find('\n').map_or(source.len(), |i| at + i + 1)
}

/// The length of the line ending `text` ends with.
fn ending_len(text: &str) -> usize {
    if text.ends_with("\r\n") { 2 } else { usize::from(text.ends_with('\n')) }
}

// How a new order of slides becomes edits.

/// A slide in the order a talk is to have.
enum Item {
    /// The slide at this index, where it moves to.
    Slide(usize),
    /// A second copy of the slide at this index.
    Copy(usize),
    /// A new slide's text, without its break line.
    New(String),
}

/// A run of the new text: bytes of the old one, or text written new.
enum Piece {
    Old(Range<usize>),
    New(String),
}

impl Piece {
    fn text<'a>(&'a self, source: &'a str) -> &'a str {
        match self {
            Piece::Old(range) => &source[range.clone()],
            Piece::New(text) => text,
        }
    }
}

/// How a slide starts in the file.
enum Head {
    /// Nothing: a first slide whose content starts the file.
    Nothing,
    /// A `---` line with its line ending.
    Rule(Range<usize>),
    /// A settings block: its opening line, the lines between with the key of each entry,
    /// and its closing line, each with its line ending.
    Block { open: Range<usize>, lines: Vec<(Range<usize>, Option<String>)>, close: Range<usize> },
}

struct Parts {
    head: Head,
    content: Range<usize>,
}

fn parts(source: &str, raw: &RawSlide, first: bool) -> Parts {
    let start = if first { bom_len(source) } else { raw.range.start };
    let content = raw.body.start.max(start)..raw.range.end;
    let Some(settings) = &raw.settings else {
        let head = if content.start > start { Head::Rule(start..content.start) } else { Head::Nothing };
        return Parts { head, content };
    };
    let open = settings.range.start..line_end(source, settings.range.start);
    let block = &source[settings.range.clone()];
    let close_start = settings.range.start + block.trim_end_matches(['\r', '\n']).rfind('\n').map_or(0, |i| i + 1);
    let mut lines = Vec::new();
    let mut line = open.end;
    while line < close_start {
        let end = line_end(source, line);
        let key = settings.entries.iter().find(|entry| entry.line.start == line).map(|entry| entry.key.clone());
        lines.push((line..end, key));
        line = end;
    }
    Parts { head: Head::Block { open, lines, close: close_start..settings.range.end }, content }
}

fn is_talk_setting(key: &Option<String>) -> bool {
    key.as_deref().is_some_and(|key| TALK_SETTINGS.contains(&key))
}

/// The talk's settings lines in the first slide's block, in their order.
fn talk_lines(first: &Parts) -> Vec<Range<usize>> {
    match &first.head {
        Head::Block { lines, .. } => lines.iter().filter(|(_, key)| is_talk_setting(key)).map(|(range, _)| range.clone()).collect(),
        _ => Vec::new(),
    }
}

/// The pieces of slide `index` exactly as it is.
fn as_it_is(parts: &Parts) -> Vec<Piece> {
    let mut out = Vec::new();
    match &parts.head {
        Head::Nothing => {}
        Head::Rule(rule) => out.push(Piece::Old(rule.clone())),
        Head::Block { open, lines, close } => {
            out.push(Piece::Old(open.clone()));
            out.extend(lines.iter().map(|(range, _)| Piece::Old(range.clone())));
            out.push(Piece::Old(close.clone()));
        }
    }
    out.push(Piece::Old(parts.content.clone()));
    out
}

/// A slide as the first of the talk: it carries the talk's settings, and starts the file
/// with no `---` unless it has a settings block or nothing else.
fn as_first(item: &Item, all: &[Parts], talk: &[Range<usize>], eol: &str) -> Vec<Piece> {
    let rule = || Piece::New(format!("---{eol}"));
    let talk_pieces = || talk.iter().map(|range| Piece::Old(range.clone()));
    let index = match item {
        Item::Slide(0) | Item::Copy(0) => return as_it_is(&all[0]),
        Item::Slide(index) | Item::Copy(index) => *index,
        Item::New(text) if talk.is_empty() => return vec![if text.is_empty() { rule() } else { Piece::New(text.clone()) }],
        Item::New(text) => {
            let mut out = vec![rule()];
            out.extend(talk_pieces());
            out.extend([rule(), Piece::New(text.clone())]);
            return out;
        }
    };
    let parts = &all[index];
    let content = Piece::Old(parts.content.clone());
    match &parts.head {
        Head::Block { open, lines, close } => {
            let mut out = vec![Piece::Old(open.clone())];
            out.extend(talk_pieces());
            out.extend(lines.iter().map(|(range, _)| Piece::Old(range.clone())));
            out.extend([Piece::Old(close.clone()), content]);
            out
        }
        Head::Rule(rule_line) if !talk.is_empty() => {
            let mut out = vec![Piece::Old(rule_line.clone())];
            out.extend(talk_pieces());
            out.extend([rule(), content]);
            out
        }
        Head::Rule(rule_line) if parts.content.is_empty() => vec![Piece::Old(rule_line.clone()), content],
        Head::Rule(_) | Head::Nothing => vec![content],
    }
}

/// A slide anywhere but first: it starts with a `---`, and has none of the talk's settings.
fn as_later(item: &Item, all: &[Parts], eol: &str) -> Vec<Piece> {
    let index = match item {
        Item::New(text) => return vec![Piece::New(format!("---{eol}{text}"))],
        Item::Slide(index) | Item::Copy(index) => *index,
    };
    let parts = &all[index];
    if index > 0 {
        return as_it_is(parts);
    }
    let content = Piece::Old(parts.content.clone());
    match &parts.head {
        Head::Nothing => vec![Piece::New(format!("---{eol}")), content],
        Head::Rule(rule) => vec![Piece::Old(rule.clone()), content],
        Head::Block { open, lines, close } => {
            let own: Vec<_> = lines.iter().filter(|(_, key)| !is_talk_setting(key)).collect();
            let mut out = vec![Piece::Old(open.clone())];
            if own.iter().any(|(_, key)| key.is_some()) {
                out.extend(own.iter().map(|(range, _)| Piece::Old(range.clone())));
                out.push(Piece::Old(close.clone()));
            }
            out.push(content);
            out
        }
    }
}

/// The change that gives the talk the slides of `order`.
fn rearrange(talk: &Talk, order: &[Item]) -> Change {
    let source = talk.source.as_str();
    let eol = eol_in(source);
    let raw = split_slides(source);
    let all: Vec<Parts> = raw.iter().enumerate().map(|(i, raw)| parts(source, raw, i == 0)).collect();
    let talk_settings = talk_lines(&all[0]);

    let mut slides: Vec<Vec<Piece>> = order
        .iter()
        .enumerate()
        .map(|(at, item)| if at == 0 { as_first(item, &all, &talk_settings, eol) } else { as_later(item, &all, eol) })
        .collect();
    if order.len() > 1 && joined(source, &slides[0]).is_empty() {
        slides[0].push(Piece::New(format!("---{eol}")));
    }
    let last = slides.len() - 1;
    for slide in &mut slides[..last] {
        if !joined(source, slide).ends_with('\n') {
            slide.push(Piece::New(eol.to_string()));
        }
    }
    // A new last slide ends as the file did.
    let ends_a_line = source[bom_len(source)..].is_empty() || source.ends_with('\n');
    let text = joined(source, &slides[last]);
    match order.last() {
        Some(Item::New(_)) if !ends_a_line => drop_last_line_ending(&mut slides[last]),
        _ if ends_a_line && !text.is_empty() && !text.ends_with('\n') => slides[last].push(Piece::New(eol.to_string())),
        _ => {}
    }

    let mut pieces = Vec::new();
    if bom_len(source) > 0 {
        pieces.push(Piece::Old(0..bom_len(source)));
    }
    pieces.extend(slides.into_iter().flatten());
    edits_for(source, &pieces)
}

fn joined(source: &str, pieces: &[Piece]) -> String {
    pieces.iter().map(|piece| piece.text(source)).collect()
}

/// Takes the line ending off the end of a new slide's text.
fn drop_last_line_ending(pieces: &mut [Piece]) {
    if let Some(Piece::New(text)) = pieces.last_mut() {
        text.truncate(text.len() - ending_len(text));
    }
}

/// The edits that turn `source` into the pieces joined. The most bytes of old pieces that
/// can stay where they are do; everything between them is replaced.
fn edits_for(source: &str, pieces: &[Piece]) -> Change {
    let mut kept = vec![false; pieces.len()];
    for index in kept_in_place(pieces) {
        kept[index] = true;
    }
    let mut edits = Vec::new();
    let mut at = 0;
    let mut text = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Old(range) if kept[index] => {
                push_edit(source, &mut edits, at..range.start, std::mem::take(&mut text));
                at = range.end;
            }
            _ => text.push_str(piece.text(source)),
        }
    }
    push_edit(source, &mut edits, at..source.len(), text);
    Change { edits }
}

/// Replaces `range` with `text`, leaving out what they start and end with alike.
fn push_edit(source: &str, edits: &mut Vec<SourceEdit>, range: Range<usize>, text: String) {
    let old = &source[range.clone()];
    if old == text {
        return;
    }
    let mut prefix = old.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
    while !(old.is_char_boundary(prefix) && text.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let room = old.len().min(text.len()) - prefix;
    let mut suffix = old.bytes().rev().zip(text.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    while !(old.is_char_boundary(old.len() - suffix) && text.is_char_boundary(text.len() - suffix)) {
        suffix -= 1;
    }
    edits.push(SourceEdit {
        range: range.start + prefix..range.end - suffix,
        text: text[prefix..text.len() - suffix].to_string(),
    });
}

/// Which old pieces stay where they are: those in the same order as in the old text,
/// chosen to keep the most bytes. Old pieces are whole parts of slides, so any two either
/// start at different places and don't overlap, or start at the same place.
fn kept_in_place(pieces: &[Piece]) -> Vec<usize> {
    let old: Vec<(usize, Range<usize>)> = pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| match piece {
            Piece::Old(range) if !range.is_empty() => Some((index, range.clone())),
            _ => None,
        })
        .collect();
    let mut starts: Vec<usize> = old.iter().map(|(_, range)| range.start).collect();
    starts.sort_unstable();
    starts.dedup();
    // best[k]: the most bytes kept by a run ending with old[k], and the one before it.
    let mut best: Vec<(usize, Option<usize>)> = Vec::with_capacity(old.len());
    let mut tree = MaxTree::new(starts.len());
    for (k, (_, range)) in old.iter().enumerate() {
        let rank = starts.partition_point(|&start| start < range.start);
        let (before, previous) = tree.best_below(rank);
        best.push((before + range.len(), previous));
        tree.raise(rank, best[k].0, k);
    }
    let mut kept = Vec::new();
    let mut at = (0..best.len()).max_by_key(|&k| best[k].0);
    while let Some(k) = at {
        kept.push(old[k].0);
        at = best[k].1;
    }
    kept.reverse();
    kept
}

/// A Fenwick tree that keeps the highest value, and where it came from, of every prefix.
struct MaxTree {
    nodes: Vec<(usize, Option<usize>)>,
}

impl MaxTree {
    fn new(len: usize) -> MaxTree {
        MaxTree { nodes: vec![(0, None); len + 1] }
    }

    /// The best of positions `0..rank`.
    fn best_below(&self, rank: usize) -> (usize, Option<usize>) {
        let mut best = (0, None);
        let mut i = rank;
        while i > 0 {
            if self.nodes[i].0 > best.0 {
                best = self.nodes[i];
            }
            i &= i - 1;
        }
        best
    }

    fn raise(&mut self, rank: usize, value: usize, from: usize) {
        let mut i = rank + 1;
        while i < self.nodes.len() {
            if value > self.nodes[i].0 {
                self.nodes[i] = (value, Some(from));
            }
            i += i.isolate_lowest_one();
        }
    }
}
