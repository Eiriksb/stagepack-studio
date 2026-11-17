//! Giving things and a layout's regions a place, a click and an entrance, putting one thing in
//! front of another, lifting a block out of a region's text, and giving slides a layout or a
//! transition. Each operation changes only the values it names, or moves only one tag's lines,
//! reads the slide it changed back, and is refused where that doesn't read as what was asked.

use std::ops::Range;

use crate::edit::{Change, SourceEdit, eol_in, remove_attr, set_attr_as_typed, set_setting};
use crate::element_ops::{
    NewTag, content_end, delete_block, ending_len, guarded, insert_block, is_attr_name, is_blank, line_end, line_start, readable, reads_as,
    tag_text, with_block,
};
use crate::model::{Attr, Block, Slide, Tag, Talk};
use crate::reparse::reparse;
use crate::rich::{is_positioned, layout_of};
use crate::rich_write::{block_regions, body_start};
use crate::slide_ops::{RegionWord, blocks_alike, read_word, remove_entries, set_region_values, written_word};

/// What a value is set on: a tag at the top level of a slide, by its whole range, or a region
/// of the slide's layout, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Tag(Range<usize>),
    Region(String),
}

/// One value to set on a target, or with `None` to take away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueEdit<'a> {
    pub target: Target,
    pub name: &'a str,
    pub value: Option<&'a str>,
}

/// Which way a placed thing goes among the others: in front of the next, behind the one
/// before, in front of them all, or behind them all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Forward,
    Backward,
    Front,
    Back,
}

/// The one change that sets every value of `edits` on slide `slide`. On a tag a value is its
/// attribute, set or taken away as [`crate::set_tag_attr`] does it. On a region it's the
/// `name=value` word of the slide's `region.<name>:` settings line: the word the engine reads
/// is changed, or one added at the line's end, or the line, or the settings block, added; a
/// word taken away goes with every other of its name, and a line left with no words goes, with
/// the block when it was its only line. A value already as asked makes no edit. `None` if a
/// target isn't there, a name or value can't be written (as [`tag_text`] writes them, and a
/// region's value is letters, digits and `.`, `+` and `-`), two edits name the same target and
/// name, or the slide wouldn't read back as it did but for those values.
pub fn set_values(talk: &Talk, slide: usize, edits: &[ValueEdit]) -> Option<Change> {
    guarded(|| values_change(talk, slide, edits))
}

/// The value of the word `name` on the `region.<region>` settings line of `slide` that the
/// engine reads, if it reads one.
pub fn region_value<'a>(slide: &'a Slide, region: &str, name: &str) -> Option<&'a str> {
    guarded(|| read_word(slide.settings.as_ref()?.get(&format!("region.{region}"))?, name))
}

/// The change that puts the placed top-level tag at `tag` on slide `slide` past the next placed
/// top-level tag in the direction `order` says, or past all of them, by moving its own lines,
/// and the blank lines on the side it leaves, to the far side of that tag's lines. Nothing
/// else's bytes change, and every block of a region's text stays in the region it was in. An
/// empty change where it's there already. `None` if the tag isn't a placed one at the top
/// level, or the slide wouldn't read as the same blocks in the new order.
pub fn reorder(talk: &Talk, slide: usize, tag: &Range<usize>, order: Order) -> Option<Change> {
    guarded(|| reorder_change(talk, slide, tag, order))
}

/// Whether [`reorder`] would move the tag.
pub fn can_reorder(talk: &Talk, slide: usize, tag: &Range<usize>, order: Order) -> bool {
    reorder(talk, slide, tag, order).is_some_and(|change| !change.is_empty())
}

/// The change that gives the block at `block` on slide `slide`, lying in a region's text, the
/// place `(x, y, w, h)`. A tag at the top level gets the four values written on it. Anything
/// else, at the top level or in a list, is taken away as [`delete_block`] takes it and written
/// at the end of the slide's content, as [`insert_block`] writes a block, in a placed `<Box>`
/// holding its own bytes, with the indentation of a list it was in taken off. `None` for a
/// block inside a tag, a region's marker, a tag that has a place already, or where the slide
/// wouldn't read as its blocks without the one lifted and then the box holding it.
pub fn lift(talk: &Talk, slide: usize, block: &Range<usize>, place: (f32, f32, f32, f32)) -> Option<Change> {
    guarded(|| lift_change(talk, slide, block, place))
}

/// The change that gives slide `slide` the layout `layout`, by its `layout:` setting, or takes
/// the setting away for `content`, which a slide has without one. A `region.` line for a region
/// the layout hasn't got stays.
pub fn set_layout(talk: &Talk, slide: usize, layout: &str) -> Option<Change> {
    let value = (layout != DEFAULT_LAYOUT).then_some(layout);
    guarded(|| settings_change(talk, &[slide], "layout", value))
}

/// The one change that gives each of `slides` the transition `transition`, by its
/// `transition:` setting, or with `None` takes the setting away.
pub fn set_transition(talk: &Talk, slides: &[usize], transition: Option<&str>) -> Option<Change> {
    guarded(|| settings_change(talk, slides, "transition", transition))
}

/// `value` as a place is written: a whole number where it rounds to one, and otherwise with
/// one decimal, rounded to the nearest; never `-0`.
pub fn number_text(value: f32) -> String {
    let tenths = (f64::from(value) * 10.0).round();
    if !tenths.is_finite() || tenths == 0.0 {
        return "0".to_string();
    }
    if tenths % 10.0 == 0.0 { format!("{:.0}", tenths / 10.0) } else { format!("{:.1}", tenths / 10.0) }
}

/// The layout a slide has without a `layout:` setting.
const DEFAULT_LAYOUT: &str = "content";

fn no_change() -> Change {
    Change { edits: Vec::new() }
}

/// `edits`, all against the same text, as one change: sorted, with insertions at one place
/// joined in the order given. `None` where two overlap.
fn merged(mut edits: Vec<SourceEdit>) -> Option<Change> {
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut out: Vec<SourceEdit> = Vec::with_capacity(edits.len());
    for edit in edits {
        if let Some(last) = out.last_mut() {
            if last.range.is_empty() && last.range == edit.range {
                last.text.push_str(&edit.text);
                continue;
            }
            if last.range.end > edit.range.start {
                return None;
            }
        }
        out.push(edit);
    }
    Some(Change { edits: out })
}

/// The tag at the top level of `slide` whose range is exactly `range`.
fn top_tag<'a>(slide: &'a Slide, range: &Range<usize>) -> Option<&'a Tag> {
    slide.blocks.iter().find_map(|block| match block {
        Block::Tag(tag) if tag.range == *range => Some(tag),
        _ => None,
    })
}

/// Whether `value` can be a word's value on a region's line.
fn is_word_value(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
}

/// What a slide's settings say, wherever they are, but for the entries `skip` says to leave out.
fn entries_but(slide: &Slide, skip: impl Fn(&str) -> bool) -> Vec<(&str, &str)> {
    slide
        .settings
        .iter()
        .flat_map(|settings| &settings.entries)
        .filter(|entry| !skip(&entry.key))
        .map(|entry| (entry.key.as_str(), entry.value.as_str()))
        .collect()
}

/// The texts of the slide's quotes, in order of their text, with `\n` line endings.
fn quotes_of(source: &str, slide: &Slide) -> Vec<String> {
    let mut quotes: Vec<String> = slide.quotes.iter().map(|quote| source[quote.clone()].replace("\r\n", "\n")).collect();
    quotes.sort();
    quotes
}

// ---------------------------------------------------------------------------------------
// Values

fn values_change(talk: &Talk, index: usize, asked: &[ValueEdit]) -> Option<Change> {
    let slide = readable(talk, index)?;
    let source = talk.source.as_str();
    for (at, edit) in asked.iter().enumerate() {
        if !is_attr_name(edit.name) || asked[..at].iter().any(|other| other.target == edit.target && other.name == edit.name) {
            return None;
        }
    }

    let mut edits = Vec::new();
    let mut expected = slide.blocks.clone();
    let mut words = Vec::new();
    for edit in asked {
        match &edit.target {
            Target::Tag(range) => {
                let tag = top_tag(slide, range)?;
                if tag.attr(edit.name) == edit.value {
                    continue;
                }
                let made = match edit.value {
                    Some(value) => vec![set_attr_as_typed(source, tag, edit.name, value)?],
                    None => remove_attr(source, tag, edit.name),
                };
                if made.iter().any(|made| made.range.start <= tag.range.start || made.range.end >= tag.open_end) {
                    return None;
                }
                edits.extend(made);
                with_block(&mut expected, range, &mut |blocks, at| {
                    let Block::Tag(tag) = &mut blocks[at] else { return };
                    match edit.value {
                        Some(value) => match tag.attrs.iter_mut().find(|attr| attr.name == edit.name) {
                            Some(attr) => attr.value = value.to_string(),
                            None => tag.attrs.push(Attr { name: edit.name.to_string(), value: value.to_string(), name_range: 0..0, value_range: 0..0 }),
                        },
                        None => tag.attrs.retain(|attr| attr.name != edit.name),
                    }
                });
            }
            Target::Region(region) => {
                if !layout_of(slide).0.contains(&region.as_str()) || edit.value.is_some_and(|value| !is_word_value(value)) {
                    return None;
                }
                words.push(RegionWord { region, name: edit.name, value: edit.value });
            }
        }
    }
    edits.extend(set_region_values(source, slide, &words)?);
    let change = merged(edits)?;
    if change.is_empty() {
        return Some(change);
    }

    let after = reparse(talk, change.apply(source));
    let new = after.slides.get(index)?;
    let regions: Vec<String> = words.iter().map(|word| format!("region.{}", word.region)).collect();
    let touched = |key: &str| regions.iter().any(|region| region == key);
    let same = after.slides.len() == talk.slides.len()
        && new.error.is_none()
        && new.notes.as_ref().map(|notes| &notes.text) == slide.notes.as_ref().map(|notes| &notes.text)
        && blocks_alike(&new.blocks, &expected)
        && first_lines_but(slide, &touched) == first_lines_but(new, &touched)
        && words.iter().all(|word| region_reads(slide, new, word.region, &words));
    same.then_some(change)
}

/// What a slide's settings say, with the first line of each key `touched` says yes to left out.
fn first_lines_but<'a>(slide: &'a Slide, touched: &dyn Fn(&str) -> bool) -> Vec<(&'a str, &'a str)> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for entry in slide.settings.iter().flat_map(|settings| &settings.entries) {
        if touched(&entry.key) && !seen.contains(&entry.key.as_str()) {
            seen.push(&entry.key);
            continue;
        }
        out.push((entry.key.as_str(), entry.value.as_str()));
    }
    out
}

/// Whether `region`'s line on `new` says what it said on `old`, but for `words`, which it says
/// as asked.
fn region_reads(old: &Slide, new: &Slide, region: &str, words: &[RegionWord]) -> bool {
    let key = format!("region.{region}");
    let line = |slide: &Slide| -> Vec<String> {
        slide.settings.as_ref().and_then(|settings| settings.get(&key)).unwrap_or_default().split_whitespace().map(str::to_string).collect()
    };
    let asked: Vec<&RegionWord> = words.iter().filter(|word| word.region == region).collect();
    let others = |words: Vec<String>| -> Vec<String> {
        words.into_iter().filter(|word| !asked.iter().any(|asked| word.starts_with(&format!("{}=", asked.name)))).collect()
    };
    let now = line(new);
    let texts: Vec<&str> = now.iter().map(String::as_str).collect();
    others(line(old)) == others(now.clone())
        && asked.iter().all(|word| match word.value {
            Some(value) => written_word(&texts, word.name).is_some_and(|at| texts[at][word.name.len() + 1..] == *value),
            None => !texts.iter().any(|text| text.starts_with(&format!("{}=", word.name))),
        })
}

// ---------------------------------------------------------------------------------------
// Order

fn reorder_change(talk: &Talk, index: usize, range: &Range<usize>, order: Order) -> Option<Change> {
    let slide = readable(talk, index)?;
    let source = talk.source.as_str();
    let placed: Vec<usize> = (0..slide.blocks.len()).filter(|&at| matches!(&slide.blocks[at], Block::Tag(tag) if is_positioned(tag))).collect();
    let k = placed.iter().position(|&at| slide.blocks[at].range() == range)?;
    let last = placed.len() - 1;
    let to = match order {
        Order::Forward => (k < last).then_some(k + 1),
        Order::Backward => k.checked_sub(1),
        Order::Front => (k < last).then_some(last),
        Order::Back => (k > 0).then_some(0),
    };
    let Some(to) = to else { return Some(no_change()) };

    let content = body_start(source, slide)?;
    let end = slide.range.end;
    let moved = own_lines(source, content, end, range)?;
    let past = own_lines(source, content, end, slide.blocks[placed[to]].range())?;
    let eol = eol_in(source);
    let lines = &source[moved.clone()];
    let edits = if to > k {
        // The blank lines after the tag go with it, and come before it where it goes.
        let gap_end = blanks_after(source, moved.end, end);
        let gap = &source[moved.end..gap_end];
        let text = if source[..past.end].ends_with('\n') { format!("{gap}{lines}") } else { format!("{eol}{gap}{}", lines.trim_end_matches(['\r', '\n'])) };
        vec![SourceEdit { range: moved.start..gap_end, text: String::new() }, SourceEdit { range: past.end..past.end, text }]
    } else {
        // The blank lines before it go with it, and come after it where it goes.
        let gap_start = blanks_before(source, moved.start, content);
        let gap = &source[gap_start..moved.start];
        let ends_a_line = lines.ends_with('\n');
        let text = if ends_a_line { format!("{lines}{gap}") } else { format!("{lines}{eol}{gap}") };
        let mut from = gap_start;
        if !ends_a_line {
            // It ended the talk with no line ending after it: so does the line before.
            from -= ending_len(&source[..from]);
        }
        vec![SourceEdit { range: past.start..past.start, text }, SourceEdit { range: from..moved.end, text: String::new() }]
    };
    let change = merged(edits)?;

    let mut expected = slide.blocks.clone();
    let tag = expected.remove(placed[k]);
    expected.insert(placed[to], tag);
    let after = reparse(talk, change.apply(source));
    if !reads_as(talk, &after, index, &expected) {
        return None;
    }
    let new = &after.slides[index];
    // Every block of a region's text reads into the region it did, and quotes are as they were.
    let mut was = block_regions(slide);
    was.remove(placed[k]);
    let mut now = block_regions(new);
    now.remove(placed[to]);
    (was == now && quotes_of(source, slide) == quotes_of(&after.source, new)).then_some(change)
}

/// The whole lines of the block at `range`, from the start of its first to past the line
/// ending of its last, if it has them to itself and they're in the slide's content, from
/// `content` to `end`.
fn own_lines(source: &str, content: usize, end: usize, range: &Range<usize>) -> Option<Range<usize>> {
    let start = line_start(source, range.start);
    if start < content || !source[start..range.start].chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    let own_end = content_end(source, range.end).max(range.start);
    let stop = line_end(source, own_end).min(end);
    is_blank(&source[own_end..stop]).then_some(start..stop)
}

/// Past the blank lines from `at`, the start of a line, stopping at `end`.
fn blanks_after(source: &str, mut at: usize, end: usize) -> usize {
    while at < end {
        let next = line_end(source, at).min(end);
        if !is_blank(&source[at..next]) {
            break;
        }
        at = next;
    }
    at
}

/// The start of the blank lines that end just before `at`, the start of a line, stopping at
/// `content`.
fn blanks_before(source: &str, mut at: usize, content: usize) -> usize {
    while at > content {
        let previous = line_start(source, at - 1).max(content);
        if !is_blank(&source[previous..at]) {
            break;
        }
        at = previous;
    }
    at
}

// ---------------------------------------------------------------------------------------
// Lifting

fn lift_change(talk: &Talk, index: usize, range: &Range<usize>, place: (f32, f32, f32, f32)) -> Option<Change> {
    let slide = readable(talk, index)?;
    let (x, y, w, h) = place;
    let values = [x, y, w, h].map(|value| value.is_finite().then(|| number_text(value)));
    let [Some(x), Some(y), Some(w), Some(h)] = values else { return None };
    let at_top = slide.blocks.iter().position(|block| block.range() == range);
    let block = match at_top {
        Some(at) => &slide.blocks[at],
        None => in_lists(&slide.blocks, range)?,
    };
    match block {
        Block::Region { .. } => return None,
        Block::Tag(tag) if at_top.is_some() => {
            if is_positioned(tag) {
                return None;
            }
            let target = || Target::Tag(range.clone());
            let edits = [("x", &x), ("y", &y), ("w", &w), ("h", &h)].map(|(name, value)| ValueEdit { target: target(), name, value: Some(value) });
            return values_change(talk, index, &edits);
        }
        _ => {}
    }
    if slide.quotes.iter().any(|quote| quote.start < range.end && range.start < quote.end) {
        return None;
    }

    // The block's own bytes, the indentation of a list it's in taken off.
    let source = talk.source.as_str();
    let own_end = content_end(source, range.end).max(range.start);
    let indent = range.start - line_start(source, range.start);
    let own = source[range.start..own_end].replace("\r\n", "\n");
    let text: Vec<&str> = own
        .split('\n')
        .enumerate()
        .map(|(i, line)| {
            let lead = if i == 0 { 0 } else { line.bytes().take(indent).take_while(|byte| *byte == b' ' || *byte == b'\t').count() };
            &line[lead..]
        })
        .collect();
    let attrs = [("x", x.as_str()), ("y", y.as_str()), ("w", w.as_str()), ("h", h.as_str())];
    let boxed = tag_text(&NewTag { name: "Box", attrs: &attrs, body: Some(&text.join("\n")) })?;

    let removal = delete_block(talk, index, range)?;
    let [cut] = removal.edits.as_slice() else { return None };
    let between = reparse(talk, removal.apply(source));
    let insertion = insert_block(&between, index, &boxed)?;
    let [put] = insertion.edits.as_slice() else { return None };
    if !put.range.is_empty() {
        return None;
    }
    // Where the box goes in the text as it was.
    let at = if put.range.start < cut.range.start { put.range.start } else { put.range.start + cut.range.len() };
    let change = merged(vec![cut.clone(), SourceEdit { range: at..at, text: put.text.clone() }])?;

    let mut expected = slide.blocks.clone();
    with_block(&mut expected, range, &mut |blocks, at| {
        blocks.remove(at);
    });
    let attrs = attrs.map(|(name, value)| Attr { name: name.to_string(), value: value.to_string(), name_range: 0..0, value_range: 0..0 });
    expected.push(Block::Tag(Tag { name: "Box".to_string(), attrs: attrs.to_vec(), children: vec![block.clone()], range: 0..0, open_end: 0 }));
    let after = reparse(talk, change.apply(source));
    reads_as(talk, &after, index, &expected).then_some(change)
}

/// The block at exactly `range` in a list among `blocks`, at any depth, but never inside a tag.
fn in_lists<'a>(blocks: &'a [Block], range: &Range<usize>) -> Option<&'a Block> {
    blocks.iter().find_map(|block| match block {
        Block::List { items, .. } => items.iter().find_map(|item| item.iter().find(|block| block.range() == range).or_else(|| in_lists(item, range))),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------------
// Settings

fn settings_change(talk: &Talk, slides: &[usize], key: &str, value: Option<&str>) -> Option<Change> {
    if value.is_some_and(|value| value.is_empty() || value.trim() != value || value.chars().any(char::is_control)) {
        return None;
    }
    let source = talk.source.as_str();
    let mut named = slides.to_vec();
    named.sort_unstable();
    named.dedup();
    let mut edits = Vec::new();
    for &index in &named {
        let slide = readable(talk, index)?;
        match value {
            Some(value) => {
                if slide.settings.as_ref().and_then(|settings| settings.get(key)) != Some(value) {
                    edits.push(set_setting(source, slide, key, value));
                }
            }
            None => {
                let lines: Vec<usize> = slide.settings.iter().flat_map(|settings| &settings.entries).filter(|entry| entry.key == key).map(|entry| entry.line.start).collect();
                if !lines.is_empty() {
                    edits.extend(remove_entries(source, slide, &lines, false)?);
                }
            }
        }
    }
    let change = merged(edits)?;
    if change.is_empty() {
        return Some(change);
    }

    let after = reparse(talk, change.apply(source));
    let same = after.slides.len() == talk.slides.len()
        && talk.slides.iter().zip(&after.slides).enumerate().all(|(index, (old, new))| {
            let settings = if named.contains(&index) {
                entries_but(old, |other| other == key) == entries_but(new, |other| other == key) && new.settings.as_ref().and_then(|settings| settings.get(key)) == value
            } else {
                entries_but(old, |_| false) == entries_but(new, |_| false)
            };
            settings
                && new.error.is_none() == old.error.is_none()
                && new.notes.as_ref().map(|notes| &notes.text) == old.notes.as_ref().map(|notes| &notes.text)
                && blocks_alike(&new.blocks, &old.blocks)
        });
    same.then_some(change)
}
