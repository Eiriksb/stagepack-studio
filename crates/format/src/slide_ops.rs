//! Adding, removing, moving and hiding slides, and their notes, as source edits. A slide's
//! bytes move as they are: only the break lines where slides now meet or part, and the
//! talk's settings when another slide becomes first, are written or taken away.
//!
//! A slide of nothing but `key: value` lines straight after its `---` would be read as a
//! settings block as soon as another break follows it, and take that break for its close.
//! Wherever an operation leaves such a slide with a break after it, it adds a blank line
//! after the slide's `---`, which keeps it a slide.
//!
//! Two things are left as they are. A slide with a code fence it never closes already
//! takes in the slides after it, and does so wherever it moves. And a slide's own entry
//! with one of the `TALK_SETTINGS` keys stays its own until that slide is first; once it
//! has been first, it is one of the talk's settings and goes with them.

use std::ops::Range;

use crate::attrs::scan_open_tag;
use crate::edit::{Change, SourceEdit, add_settings, eol_in, set_attr, set_setting};
use crate::model::{Block, Notes, Slide, Talk};
use crate::rich::FlowPlace;
use crate::parse::parse_talk;
use crate::notes::escape_notes;
use crate::settings::parse_entry;
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

/// Slide `index`'s own text, as [`insert_slide`] takes it to put a copy of the slide anywhere,
/// in this talk or another: its settings and notes, but none of the talk's settings, which a
/// first slide carries, and no break line of its own. Line endings are `\n`, as
/// [`insert_slide`] makes them the talk's. Empty for a slide that isn't there.
pub fn slide_text_for_copy(talk: &Talk, index: usize) -> String {
    let source = talk.source.as_str();
    let raw = split_slides(source);
    let Some(slide) = raw.get(index) else { return String::new() };
    let parts = parts(source, slide, index == 0);
    let mut out = String::new();
    if let Head::Block { lines, close, .. } = &parts.head {
        // As a copy placed anywhere but first keeps them: see `as_later`.
        let own: Vec<_> = lines.iter().filter(|(_, key)| index > 0 || !is_talk_setting(key)).collect();
        if own.iter().any(|(_, key)| key.is_some()) {
            for (range, _) in own {
                out.push_str(&source[range.clone()]);
            }
            out.push_str(&source[close.clone()]);
        }
    }
    out.push_str(&source[parts.content.clone()]);
    out.replace("\r\n", "\n")
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

pub(crate) fn setting_entry<'a>(slide: &'a Slide, key: &str) -> Option<&'a crate::settings::SettingEntry> {
    slide.settings.as_ref()?.entries.iter().find(|entry| entry.key == key)
}

/// Takes the setting `key` out of the slide's block, with its whole line. When it was the
/// only entry the block goes too, but for the `---` that starts any slide but the first,
/// or a first slide with nothing after its block. A slide left with nothing but
/// `key: value` lines keeps a blank line after its `---`, so it isn't read as settings.
pub fn remove_setting(source: &str, slide: &Slide, key: &str) -> Option<SourceEdit> {
    let entry = setting_entry(slide, key)?;
    remove_entries(source, slide, &[entry.line.start], false)?.into_iter().next()
}

/// The edits that take the entries starting at `lines` out of the slide's block, each with its
/// whole line, or the block itself, as [`remove_setting`] takes it, when they're all it has and
/// `keep_block` doesn't say lines are to be added to it.
pub(crate) fn remove_entries(source: &str, slide: &Slide, lines: &[usize], keep_block: bool) -> Option<Vec<SourceEdit>> {
    let settings = slide.settings.as_ref()?;
    let entries: Vec<_> = settings.entries.iter().filter(|entry| lines.contains(&entry.line.start)).collect();
    if entries.len() != lines.len() {
        return None;
    }
    if keep_block || settings.entries.len() > entries.len() {
        return Some(entries.iter().map(|entry| SourceEdit { range: entry.line.start..line_end(source, entry.line.start), text: String::new() }).collect());
    }
    let first = slide.range.start == 0;
    if first && settings.range.end < slide.range.end {
        return Some(vec![SourceEdit { range: settings.range.clone(), text: String::new() }]);
    }
    // The opening `---` stays as the slide's break line. At the very end of a file with
    // no last line ending, it's the line ending that goes, not the rule.
    let open_end = line_end(source, settings.range.start);
    let start = if settings.range.end == source.len() && !source.ends_with('\n') {
        settings.range.start + source[settings.range.start..open_end].trim_end_matches(['\r', '\n']).len()
    } else {
        open_end
    };
    // A slide of `key: value` lines would be read as the settings block in its place, so a
    // blank line is left after the `---` to keep it a slide.
    let followed = slide.range.end < source.len();
    let text = if followed && only_settings_lines(&source[settings.range.end..slide.range.end]) {
        eol_in(source).to_string()
    } else {
        String::new()
    };
    Some(vec![SourceEdit { range: start..settings.range.end, text }])
}

/// Aligns the text in `place` on slide `index` `align`, one of `left`, `center` and `right`.
/// A region's alignment is the `align=` of its `region.<name>` settings line: the first one
/// there is changed, or one is added at the line's end, or the line, or the settings block,
/// is added, and whatever else the line says stays. A box's is its own `align` attribute.
/// Aligning as it is already changes nothing. `None` if the slide, or the box, isn't there,
/// and for a table's cell, which has no alignment of its own.
pub fn set_align(talk: &Talk, index: usize, place: &FlowPlace, align: &str) -> Option<Change> {
    let slide = talk.slides.get(index)?;
    let source = talk.source.as_str();
    match place {
        FlowPlace::Region(region) => {
            let edits = set_region_values(source, slide, &[RegionWord { region, name: "align", value: Some(align) }])?;
            Some(Change { edits })
        }
        FlowPlace::Tag(range) => {
            let tag = slide.blocks.iter().find_map(|block| match block {
                Block::Tag(tag) if tag.range == *range => Some(tag),
                _ => None,
            })?;
            let edit = (tag.attr("align") != Some(align)).then(|| set_attr(source, tag, "align", align));
            Some(edit.map_or_else(no_change, Change::single))
        }
        FlowPlace::Cell { .. } => None,
    }
}

/// One word of a `region.<name>` settings line to set to a value, or with `None` to take away.
pub(crate) struct RegionWord<'a> {
    pub region: &'a str,
    pub name: &'a str,
    pub value: Option<&'a str>,
}

/// Whether the engine reads `value` as what the word `name` of a region's line says. A word
/// it doesn't know it reads as anything.
fn engine_reads(name: &str, value: &str) -> bool {
    match name {
        "x" | "y" | "w" | "h" | "rotate" => value.parse::<f32>().is_ok_and(f32::is_finite),
        "click" => value == "each" || (value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u32>().is_ok_and(|click| (1..=999).contains(&click))),
        "enter" => matches!(value, "appear" | "fade" | "fly-up"),
        "align" => matches!(value, "left" | "center" | "right"),
        _ => true,
    }
}

/// Which of a line's `words` is the one for `name` that's written to: the one the engine reads,
/// or, where it reads none, the last. For an alignment that's the first, which is all the
/// engine looks at; for anything else the engine takes the last it can read.
pub(crate) fn written_word(words: &[&str], name: &str) -> Option<usize> {
    let prefix = format!("{name}=");
    let mut named = words.iter().enumerate().filter(|(_, word)| word.starts_with(&prefix));
    if name == "align" {
        return named.next().map(|(at, _)| at);
    }
    let named: Vec<_> = named.collect();
    named.iter().rev().find(|(_, word)| engine_reads(name, &word[prefix.len()..])).or(named.last()).map(|(at, _)| *at)
}

/// The value the engine reads from the word `name` of `line`, a region's settings value.
pub(crate) fn read_word<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let value = &words[written_word(&words, name)?][name.len() + 1..];
    engine_reads(name, value).then_some(value)
}

/// Where each word of the text at `range` is.
fn word_ranges(source: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    for (at, character) in source[range.clone()].char_indices() {
        match (character.is_whitespace(), start) {
            (true, Some(from)) => {
                out.push(range.start + from..range.start + at);
                start = None;
            }
            (false, None) => start = Some(at),
            _ => {}
        }
    }
    if let Some(from) = start {
        out.push(range.start + from..range.end);
    }
    out
}

/// The edits that set or take away `words` on the slide's `region.<name>` settings lines, all
/// against the text as it is. A word set changes the one [`written_word`] picks, or is added
/// at the line's end; one taken away goes with the white space before it, and every word of
/// that name goes. A line that's missing is added, and the settings block with it, as
/// [`set_setting`] adds them; one left with no words goes, and the block with it when that was
/// its only line, as [`remove_setting`] takes them. A word already as asked makes no edit.
/// `None` where the edits can't be made.
pub(crate) fn set_region_values(source: &str, slide: &Slide, words: &[RegionWord]) -> Option<Vec<SourceEdit>> {
    let mut regions: Vec<&str> = Vec::new();
    for word in words {
        if !regions.contains(&word.region) {
            regions.push(word.region);
        }
    }
    let mut edits = Vec::new();
    let mut new_lines: Vec<(String, String)> = Vec::new();
    let mut gone = Vec::new();
    for region in regions {
        let key = format!("region.{region}");
        let asked = words.iter().filter(|word| word.region == region);
        let Some(entry) = setting_entry(slide, &key) else {
            let set: Vec<String> = asked.filter_map(|word| Some(format!("{}={}", word.name, word.value?))).collect();
            if !set.is_empty() {
                new_lines.push((key, set.join(" ")));
            }
            continue;
        };
        let ranges = word_ranges(source, entry.value_range.clone());
        let texts: Vec<&str> = ranges.iter().map(|range| &source[range.clone()]).collect();
        let mut line_edits = Vec::new();
        let mut added = String::new();
        let mut removed = 0;
        for word in asked {
            let prefix = format!("{}=", word.name);
            match word.value {
                Some(value) => match written_word(&texts, word.name) {
                    Some(at) if &texts[at][prefix.len()..] == value => {}
                    Some(at) => line_edits.push(SourceEdit { range: ranges[at].start + prefix.len()..ranges[at].end, text: value.to_string() }),
                    None => added.push_str(&format!(" {prefix}{value}")),
                },
                None => {
                    for (range, _) in ranges.iter().zip(&texts).filter(|(_, text)| text.starts_with(&prefix)) {
                        let before = source[entry.line.start..range.start].trim_end_matches(char::is_whitespace).len();
                        line_edits.push(SourceEdit { range: entry.line.start + before..range.end, text: String::new() });
                        removed += 1;
                    }
                }
            }
        }
        if removed > 0 && removed == texts.len() && added.is_empty() {
            gone.push(entry.line.start);
            continue;
        }
        edits.extend(line_edits);
        if !added.is_empty() {
            let end = entry.value_range.end;
            edits.push(SourceEdit { range: end..end, text: added });
        }
    }
    if !gone.is_empty() {
        edits.extend(remove_entries(source, slide, &gone, !new_lines.is_empty())?);
    }
    if !new_lines.is_empty() {
        let lines: Vec<(&str, &str)> = new_lines.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
        edits.push(add_settings(source, slide, &lines));
    }
    Some(edits)
}

/// Sets what slide `index`'s `<Notes>` hold: the words inside an existing element are
/// replaced, an element is added at the end of a slide with none, and the element goes
/// when `notes` is empty. Where taking the element away would join the blocks on either
/// side of it, such as two lists, it stays with nothing in it instead, and reads as empty
/// notes. Line endings become the talk's.
///
/// The notes are escaped where they would otherwise be read as something else, a tag, a
/// code fence or a slide break, and the parser reads them back unescaped (see
/// `escape_notes`), so `Notes::text` gives back what was typed. A slide that can't be read
/// is left alone.
pub fn set_notes(talk: &Talk, index: usize, notes: &str) -> Change {
    let source = talk.source.as_str();
    let Some(slide) = talk.slides.get(index) else { return no_change() };
    if slide.error.is_some() {
        return no_change();
    }
    let eol = eol_in(source);
    // The parser trims what an element holds, so space around the words means nothing. Kept,
    // it would stay behind each time the words were replaced, as the notes field sends them
    // after every key: a space typed between two words would be left at the end.
    let notes = with_line_endings(&escape_notes(notes.replace("\r\n", "\n").trim()), eol);
    let edit = match &slide.notes {
        Some(old) if notes.trim().is_empty() => {
            let raw = &split_slides(source)[index];
            let removal = without_notes(source, raw, remove_notes(source, raw, old), eol);
            let after = parse_talk(&removal.apply(source));
            if after.slides.len() == talk.slides.len() && blocks_alike(&after.slides[index].blocks, &slide.blocks) {
                return removal;
            }
            // The element is all that keeps the blocks around it apart, as with two lists:
            // it stays, with nothing in it.
            let (_, inner_start) = scan_open_tag(source, old.range.start);
            let inner_end = source[inner_start.min(old.range.end)..old.range.end].rfind("</Notes").map_or(old.range.end, |at| inner_start + at);
            SourceEdit { range: inner_start..inner_end, text: eol.to_string() }
        }
        Some(old) => {
            let (_, inner_start) = scan_open_tag(source, old.range.start);
            match source[inner_start.min(old.range.end)..old.range.end].rfind("</Notes") {
                Some(at) => {
                    let inner = inner_start..inner_start + at;
                    let words = &source[inner.clone()];
                    let lead = words.len() - words.trim_start().len();
                    let core = inner.start + lead..inner.start + words.trim_end().len();
                    // An indented `</Notes>` would be taken into a list the notes end with, so
                    // the closing tag goes to the start of its line.
                    let last_line = words.rfind('\n').map(|i| inner.start + i + 1);
                    let closing_indent = last_line.filter(|&at| at < inner.end && source[at..inner.end].chars().all(|c| c == ' ' || c == '\t'));
                    if core.start >= core.end {
                        SourceEdit { range: inner, text: format!("{eol}{notes}{eol}") }
                    } else if let Some(at) = closing_indent {
                        SourceEdit { range: core.start..inner.end, text: format!("{notes}{}", &source[core.end..at]) }
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

/// `removal` of a slide's notes, and a blank line after the slide's `---` if what's left of
/// it would otherwise read as a settings block.
fn without_notes(source: &str, raw: &RawSlide, mut removal: SourceEdit, eol: &str) -> Change {
    let head_start = if raw.range.start == 0 { bom_len(source) } else { raw.range.start };
    let after_a_rule = raw.settings.is_none() && raw.body.start > head_start;
    let left = format!("{}{}", &source[raw.body.start..removal.range.start], &source[removal.range.end..raw.range.end]);
    if !(after_a_rule && raw.range.end < source.len() && only_settings_lines(&left)) {
        return Change::single(removal);
    }
    if removal.range.start == raw.body.start {
        removal.text = eol.to_string();
        return Change::single(removal);
    }
    let blank = SourceEdit { range: raw.body.start..raw.body.start, text: eol.to_string() };
    Change { edits: vec![blank, removal] }
}

/// Whether two lists of blocks say the same, wherever they are in the file.
pub(crate) fn blocks_alike(a: &[Block], b: &[Block]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| block_alike(a, b))
}

fn block_alike(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::Heading { level: l1, spans: s1, .. }, Block::Heading { level: l2, spans: s2, .. }) => l1 == l2 && s1 == s2,
        (Block::Paragraph { spans: s1, .. }, Block::Paragraph { spans: s2, .. }) => s1 == s2,
        (Block::List { ordered: o1, items: i1, .. }, Block::List { ordered: o2, items: i2, .. }) => {
            o1 == o2 && i1.len() == i2.len() && i1.iter().zip(i2).all(|(x, y)| blocks_alike(x, y))
        }
        (Block::Table { rows: r1, .. }, Block::Table { rows: r2, .. }) => r1 == r2,
        (Block::Code { language: l1, code: c1, .. }, Block::Code { language: l2, code: c2, .. }) => l1 == l2 && c1 == c2,
        (Block::Picture { src: s1, alt: a1, .. }, Block::Picture { src: s2, alt: a2, .. }) => s1 == s2 && a1 == a2,
        (Block::Region { name: n1, .. }, Block::Region { name: n2, .. }) => n1 == n2,
        (Block::Tag(t1), Block::Tag(t2)) => {
            t1.name == t2.name
                && t1.attrs.len() == t2.attrs.len()
                && t1.attrs.iter().zip(&t2.attrs).all(|(x, y)| x.name == y.name && x.value == y.value)
                && blocks_alike(&t1.children, &t2.children)
        }
        _ => false,
    }
}

/// Whether `text`, a slide that starts with a `---` line, would be read as a settings block
/// if a break came after it: a slide of `key: value` lines only.
fn reads_as_settings(text: &str) -> bool {
    text.split_once('\n').is_some_and(|(first, rest)| first.trim_end_matches('\r').trim_end_matches(' ') == "---" && only_settings_lines(rest))
}

/// Whether the lines of `text` would be read as a settings block's entries, if they came
/// straight after a `---` and a break came after them.
fn only_settings_lines(text: &str) -> bool {
    let mut lines = text.lines();
    lines.next().is_some_and(|first| parse_entry(first, 0).is_some()) && lines.all(|line| line.trim().is_empty() || parse_entry(line, 0).is_some())
}

/// The edit that takes the slide's `<Notes>` element away: its whole lines, the spaces or
/// tabs before and after it included, and the blank line before it when nothing but the
/// slide's end or another blank line comes after it, so the blocks on either side stay
/// apart. It never reaches back past the start of the slide's content.
fn remove_notes(source: &str, raw: &RawSlide, notes: &Notes) -> SourceEdit {
    let content_start = raw.body.start.max(raw.range.start).max(bom_len(source));
    let is_space = |c: char| c == ' ' || c == '\t';
    let line_start = source[..notes.range.start].rfind('\n').map_or(0, |i| i + 1).max(content_start);
    let indent = &source[line_start..notes.range.start];
    let mut start = if indent.chars().all(is_space) { line_start } else { notes.range.start };
    let trailing = source[notes.range.end..].len() - source[notes.range.end..].trim_start_matches(is_space).len();
    let mut end = notes.range.end;
    let after = &source[end + trailing..];
    let ending_after = if after.starts_with("\r\n") { 2 } else { usize::from(after.starts_with('\n')) };
    if ending_after > 0 || end + trailing == source.len() {
        end += trailing + ending_after;
    }
    let next_line = &source[end..line_end(source, end).min(raw.range.end)];
    let ends_or_blank_after = end >= raw.range.end || next_line.trim().is_empty();
    let before = &source[..start];
    if ends_or_blank_after && (before.ends_with("\r\n\r\n") || before.ends_with("\n\n")) {
        start -= ending_len(before);
    }
    if ending_after == 0 && end == source.len() {
        // The element ended the file, with no line ending after it: so does the line before.
        start -= ending_len(&source[..start]);
    }
    // A slide whose content starts with its notes keeps the line ending of its `---` or
    // settings block, which `start` may have stepped back over.
    SourceEdit { range: start.max(content_start)..end, text: String::new() }
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

/// The pieces of a slide exactly as it is.
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
fn as_first(source: &str, item: &Item, all: &[Parts], talk: &[Range<usize>], eol: &str) -> Vec<Piece> {
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
            // A bare `---` that ends the file has no line ending for the talk's lines to follow.
            let open = if source[rule_line.clone()].ends_with('\n') { Piece::Old(rule_line.clone()) } else { rule() };
            let mut out = vec![open];
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
        Item::New(text) => return vec![Piece::New(format!("---{eol}")), Piece::New(text.clone())],
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
        .map(|(at, item)| if at == 0 { as_first(source, item, &all, &talk_settings, eol) } else { as_later(item, &all, eol) })
        .collect();
    if order.len() > 1 && joined(source, &slides[0]).is_empty() {
        slides[0].push(Piece::New(format!("---{eol}")));
    }
    let last = slides.len() - 1;
    for slide in &mut slides[..last] {
        if !joined(source, slide).ends_with('\n') {
            slide.push(Piece::New(eol.to_string()));
        }
        // Every slide's head is its first piece, so the blank line goes straight after it.
        if reads_as_settings(&joined(source, slide)) {
            slide.insert(1, Piece::New(eol.to_string()));
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
