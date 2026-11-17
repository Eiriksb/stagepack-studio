//! Putting a block on a slide, taking one away, and changing one setting of a tag. Each
//! operation reads the slide it changed back, and is refused where that doesn't read as
//! what was asked for, so nothing else on the slide changes its meaning.

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::edit::{Change, SourceEdit, eol_in, quote_value, remove_attr, set_attr_as_typed};
use crate::model::{Attr, Block, Slide, Tag, Talk};
use crate::parse::parse_talk;
use crate::reparse::reparse;
use crate::rich::layout_of;
use crate::rich_write::{block_regions, body_start, empty_region_edit};
use crate::slide_ops::blocks_alike;

/// A tag to be written: its name, its attributes in order, and, for a tag with children,
/// the Markdown inside it. With no body it's written self-closing.
#[derive(Clone, Copy, Debug)]
pub struct NewTag<'a> {
    pub name: &'a str,
    pub attrs: &'a [(&'a str, &'a str)],
    pub body: Option<&'a str>,
}

/// The tag as it's written, with `\n` line endings: `<Name a="1" />` on one line, or the
/// opening tag, a blank line, the body, a blank line and the closing tag. Each value is
/// quoted with `"`, or with `'` when it holds a `"`. `None` for a value holding both
/// quotes, a line ending or another control character, for a name that isn't a capital and
/// then letters and digits, and for an attribute name that isn't a small letter and then
/// small letters, digits and `-`.
pub fn tag_text(tag: &NewTag) -> Option<String> {
    guarded(|| {
        if !is_tag_name(tag.name) {
            return None;
        }
        let mut open = format!("<{}", tag.name);
        for (name, value) in tag.attrs {
            if !is_attr_name(name) {
                return None;
            }
            open.push_str(&format!(" {name}={}", quote_value(value)?));
        }
        Some(match tag.body {
            None => format!("{open} />"),
            Some(body) => {
                let body = one_line_ending(body);
                let body = body.trim_matches('\n');
                if body.is_empty() { format!("{open}>\n\n</{}>", tag.name) } else { format!("{open}>\n\n{body}\n\n</{}>", tag.name) }
            }
        })
    })
}

/// The edit that adds `text`, one block of Markdown, at the end of slide `slide`'s content:
/// after its last block, so before notes that end the slide, with one blank line before it
/// and, where something follows, one after it, in the talk's line ending. On a slide with no
/// blocks it goes where typed text would. `None` when the slide can't be read, or when what
/// comes out wouldn't read as the slide's blocks as they were and then `text`'s.
pub fn insert_block(talk: &Talk, slide: usize, text: &str) -> Option<Change> {
    guarded(|| insert_change(talk, slide, None, text))
}

/// The same as [`insert_block`], at the end of the content of region `region` of the slide's
/// layout, with the region's `<Region name="…" />` line written first when it has none.
/// `None` also when the region isn't the layout's, or the block wouldn't be drawn in it.
pub fn insert_block_in_region(talk: &Talk, slide: usize, region: &str, text: &str) -> Option<Change> {
    guarded(|| insert_change(talk, slide, Some(region), text))
}

/// The edit that takes away the block or tag at exactly `range`, at any depth on slide
/// `slide`: its own lines, and one of the blank lines around it. `None` when no block has that
/// range, or when what's left wouldn't read as the slide's blocks without it, as where the
/// block was all that kept two lists apart.
pub fn delete_block(talk: &Talk, slide: usize, range: &Range<usize>) -> Option<Change> {
    guarded(|| delete_change(talk, slide, range))
}

/// The tag at exactly `range` on slide `slide`, at any depth.
pub fn find_tag<'a>(talk: &'a Talk, slide: usize, range: &Range<usize>) -> Option<&'a Tag> {
    guarded(|| tag_at(talk, slide, range))
}

fn tag_at<'a>(talk: &'a Talk, slide: usize, range: &Range<usize>) -> Option<&'a Tag> {
    match find_block(&talk.slides.get(slide)?.blocks, range)? {
        Block::Tag(tag) => Some(tag),
        _ => None,
    }
}

/// The edit that sets attribute `name` of the tag at `tag` on slide `slide` to `value`,
/// written as [`tag_text`] writes values, or with `None` takes it away with the space before
/// it. Where the attribute is written twice, the first, the one that's read, is the one set;
/// taking it away takes every one. Taking away an attribute the tag hasn't got is an empty
/// change. `None` when there's no such tag, the name or value can't be written, or the slide
/// wouldn't read back as the same with only that attribute changed.
pub fn set_tag_attr(talk: &Talk, slide: usize, tag: &Range<usize>, name: &str, value: Option<&str>) -> Option<Change> {
    guarded(|| attr_change(talk, slide, tag, name, value))
}

/// The edit that gives the tag at exactly `tag` on slide `slide` the body `body`, Markdown, in
/// place of what it holds: after its opening tag, a blank line, the body, a blank line and its
/// closing tag, as [`tag_text`] writes one, in the talk's line ending and on lines indented as
/// the tag's is. A self-closing tag is written open, its `/>` made `>`. Nothing outside the tag
/// changes, nor does its opening tag but for that. `None` when there's no such tag, or the slide
/// wouldn't read as it did with the tag holding what `body` reads as alone.
pub fn set_tag_body(talk: &Talk, slide: usize, tag: &Range<usize>, body: &str) -> Option<Change> {
    guarded(|| body_change(talk, slide, tag, body))
}

/// The bytes of the block at exactly `range` on slide `slide`, at any depth, with `\n` line
/// endings.
pub fn block_text(talk: &Talk, slide: usize, range: &Range<usize>) -> Option<String> {
    guarded(|| {
        find_block(&talk.slides.get(slide)?.blocks, range)?;
        Some(talk.source.get(range.clone())?.replace("\r\n", "\n"))
    })
}

/// `text` with every line ending, a lone carriage return too, made `\n`.
fn one_line_ending(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// What `make` gives, or `None` if it panics.
pub(crate) fn guarded<T>(make: impl FnOnce() -> Option<T>) -> Option<T> {
    catch_unwind(AssertUnwindSafe(make)).ok().flatten()
}

fn is_tag_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_alphanumeric())
}

pub(crate) fn is_attr_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The block at exactly `range` among `blocks`, at any depth.
fn find_block<'a>(blocks: &'a [Block], range: &Range<usize>) -> Option<&'a Block> {
    blocks.iter().find_map(|block| {
        if block.range() == range {
            return Some(block);
        }
        match block {
            Block::List { items, .. } => items.iter().find_map(|item| find_block(item, range)),
            Block::Tag(tag) => find_block(&tag.children, range),
            _ => None,
        }
    })
}

/// Calls `change` on the block at exactly `range` among `blocks`, at any depth, with the list
/// that holds it and its place there. Whether there was one.
pub(crate) fn with_block(blocks: &mut Vec<Block>, range: &Range<usize>, change: &mut impl FnMut(&mut Vec<Block>, usize)) -> bool {
    if let Some(at) = blocks.iter().position(|block| block.range() == range) {
        change(blocks, at);
        return true;
    }
    blocks.iter_mut().any(|block| match block {
        Block::List { items, .. } => items.iter_mut().any(|item| with_block(item, range, change)),
        Block::Tag(tag) => with_block(&mut tag.children, range, change),
        _ => false,
    })
}

/// The start of the line `at` is in.
pub(crate) fn line_start(source: &str, at: usize) -> usize {
    source[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// Just past the line ending of the line `at` is in, or the end of the text.
pub(crate) fn line_end(source: &str, at: usize) -> usize {
    source[at..].find('\n').map_or(source.len(), |i| at + i + 1)
}

pub(crate) fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// The length of the line ending `text` ends with.
pub(crate) fn ending_len(text: &str) -> usize {
    if text.ends_with("\r\n") { 2 } else { usize::from(text.ends_with('\n')) }
}

/// Whether slide `index` of `after`, `talk` changed, reads as `blocks` with nothing else
/// about it changed: the same number of slides, the same notes, and no error.
pub(crate) fn reads_as(talk: &Talk, after: &Talk, index: usize, blocks: &[Block]) -> bool {
    let (Some(old), Some(new)) = (talk.slides.get(index), after.slides.get(index)) else { return false };
    after.slides.len() == talk.slides.len()
        && new.error.is_none()
        && new.notes.as_ref().map(|notes| &notes.text) == old.notes.as_ref().map(|notes| &notes.text)
        && settings_of(new) == settings_of(old)
        && blocks_alike(&new.blocks, blocks)
}

/// What a slide's settings say, wherever they are.
fn settings_of(slide: &Slide) -> Option<Vec<(&str, &str)>> {
    slide.settings.as_ref().map(|settings| settings.entries.iter().map(|entry| (entry.key.as_str(), entry.value.as_str())).collect())
}

/// A readable slide of the talk, by index.
pub(crate) fn readable(talk: &Talk, index: usize) -> Option<&Slide> {
    talk.slides.get(index).filter(|slide| slide.error.is_none())
}

// ---------------------------------------------------------------------------------------
// Inserting

fn insert_change(talk: &Talk, index: usize, region: Option<&str>, text: &str) -> Option<Change> {
    let slide = readable(talk, index)?;
    let source = talk.source.as_str();
    let eol = eol_in(source);
    let text = one_line_ending(text);
    let text = text.trim_matches('\n');
    if text.trim().is_empty() {
        return None;
    }
    let text = text.replace('\n', eol);
    // What `text` reads as alone: one block, and nothing else.
    let alone = parse_talk(&text);
    let [own] = alone.slides.as_slice() else { return None };
    if own.error.is_some() || own.notes.is_some() || own.settings.is_some() || own.blocks.len() != 1 {
        return None;
    }

    let regions = block_regions(slide);
    let edit = match region {
        None => match slide.blocks.last() {
            Some(last) => after_block(source, slide, last.range().end, &text, eol)?,
            None => empty_region_edit(source, slide, layout_of(slide).1, &text, eol)?,
        },
        Some(region) => {
            let (names, main) = layout_of(slide);
            if !names.contains(&region) {
                return None;
            }
            let last_in = regions.iter().rposition(|place| *place == Some(region));
            let marker = slide.blocks.iter().find_map(|block| match block {
                Block::Region { name, range } if name == region => Some(range.end),
                _ => None,
            });
            let has_markers = slide.blocks.iter().any(|block| matches!(block, Block::Region { .. }));
            match (last_in, marker, slide.blocks.last()) {
                (Some(at), _, _) => after_block(source, slide, slide.blocks[at].range().end, &text, eol)?,
                (None, Some(end), _) => after_block(source, slide, end, &text, eol)?,
                (None, None, Some(last)) if region != "title" => {
                    let text = if region == main && !has_markers { text } else { format!("<Region name=\"{region}\" />{eol}{eol}{text}") };
                    after_block(source, slide, last.range().end, &text, eol)?
                }
                _ => empty_region_edit(source, slide, region, &text, eol)?,
            }
        }
    };
    if !slide.range.contains(&edit.range.start) && edit.range.start != slide.range.end {
        return None;
    }

    // The blocks the edit's text reads as, a `<Region>` line and the new block, go in among
    // the old ones where it's put.
    let added = parse_talk(&edit.text);
    let [added] = added.slides.as_slice() else { return None };
    let new_blocks = &added.blocks;
    let (new_block, markers) = new_blocks.split_last()?;
    if !blocks_alike(std::slice::from_ref(new_block), &own.blocks) || !markers.iter().all(|block| matches!(block, Block::Region { .. })) {
        return None;
    }
    let before = slide.blocks.iter().take_while(|block| block.range().start < edit.range.start).count();
    let mut expected = slide.blocks.clone();
    expected.splice(before..before, new_blocks.iter().cloned());

    let change = Change::single(edit);
    let after = reparse(talk, change.apply(source));
    if !reads_as(talk, &after, index, &expected) {
        return None;
    }
    if let Some(region) = region {
        // The new block is drawn in the region, unless it's a tag with a place of its own,
        // and everything else is drawn where it was.
        let now = block_regions(&after.slides[index]);
        let own = *now.get(before + markers.len())?;
        let mut places = block_regions(slide);
        places.splice(before..before, markers.iter().map(|_| None).chain([own]));
        if (own.is_some() && own != Some(region)) || now != places {
            return None;
        }
    }
    Some(change)
}

/// The edit that puts `text` on lines of its own after the block ending at `end`, with a
/// blank line before it and, where something follows on the slide, one after it.
fn after_block(source: &str, slide: &Slide, end: usize, text: &str, eol: &str) -> Option<SourceEdit> {
    let end = content_end(source, end);
    let stop = line_end(source, end);
    let rest_of_line = source[end..stop].trim_end_matches(['\n', '\r']);
    if !rest_of_line.trim().is_empty() {
        return None;
    }
    let at = end + rest_of_line.len();
    let mut out = format!("{eol}{eol}{text}");
    let next = source.get(stop..line_end(source, stop).min(slide.range.end))?;
    if stop < slide.range.end && !is_blank(next) {
        out.push_str(eol);
    }
    Some(SourceEdit { range: at..at, text: out })
}

/// `end`, the end of a block's range, moved back over the line endings and spaces it ends
/// with: the `markdown` crate takes the blank lines after a list into the list.
pub(crate) fn content_end(source: &str, end: usize) -> usize {
    source[..end].trim_end_matches([' ', '\t', '\r', '\n']).len()
}

// ---------------------------------------------------------------------------------------
// Deleting

fn delete_change(talk: &Talk, index: usize, range: &Range<usize>) -> Option<Change> {
    let slide = readable(talk, index)?;
    let source = talk.source.as_str();
    find_block(&slide.blocks, range)?;
    let content = body_start(source, slide)?;
    if range.start < content || range.end > slide.range.end {
        return None;
    }

    // The blank lines a list's range runs over aren't its own.
    let own = range.start..content_end(source, range.end).max(range.start);
    let first = line_start(source, own.start).max(content);
    let whole_lines = source[first..own.start].chars().all(|c| matches!(c, ' ' | '\t' | '>'))
        && is_blank(&source[own.end..line_end(source, own.end).min(slide.range.end)]);
    let (mut start, mut end) = (own.start, own.end);
    if whole_lines {
        start = first;
        end = line_end(source, own.end).min(slide.range.end);
        let next = line_end(source, end).min(slide.range.end);
        if end < slide.range.end && is_blank(&source[end..next]) {
            end = next;
        } else if start > content {
            let previous = line_start(source, start - 1).max(content);
            if is_blank(&source[previous..start]) {
                start = previous;
            }
        }
        // A block that ended the talk with no line ending after it: so does the line before.
        if end == source.len() && !source.ends_with('\n') && start > content {
            start -= ending_len(&source[..start]);
        }
    }

    let mut expected = slide.blocks.clone();
    if !with_block(&mut expected, range, &mut |blocks, at| {
        blocks.remove(at);
    }) {
        return None;
    }
    let change = Change::single(SourceEdit { range: start..end, text: String::new() });
    let after = reparse(talk, change.apply(source));
    reads_as(talk, &after, index, &expected).then_some(change)
}

// ---------------------------------------------------------------------------------------
// Attributes

fn attr_change(talk: &Talk, index: usize, range: &Range<usize>, name: &str, value: Option<&str>) -> Option<Change> {
    let slide = readable(talk, index)?;
    let tag = find_tag(talk, index, range)?;
    if !is_attr_name(name) {
        return None;
    }
    let source = talk.source.as_str();
    let edits = match value {
        Some(value) => vec![set_attr_as_typed(source, tag, name, value)?],
        None => remove_attr(source, tag, name),
    };
    let edits: Vec<SourceEdit> = edits.into_iter().filter(|edit| source.get(edit.range.clone()) != Some(edit.text.as_str())).collect();
    if edits.is_empty() {
        return Some(Change { edits });
    }
    if edits.iter().any(|edit| edit.range.start <= tag.range.start || edit.range.end >= tag.open_end) {
        return None;
    }

    let mut expected = slide.blocks.clone();
    with_block(&mut expected, range, &mut |blocks, at| {
        let Block::Tag(tag) = &mut blocks[at] else { return };
        match value {
            Some(value) => match tag.attrs.iter_mut().find(|attr| attr.name == name) {
                Some(attr) => attr.value = value.to_string(),
                None => tag.attrs.push(Attr { name: name.to_string(), value: value.to_string(), name_range: 0..0, value_range: 0..0 }),
            },
            None => tag.attrs.retain(|attr| attr.name != name),
        }
    });
    let change = Change { edits };
    let after = reparse(talk, change.apply(source));
    reads_as(talk, &after, index, &expected).then_some(change)
}

// ---------------------------------------------------------------------------------------
// A tag's body

fn body_change(talk: &Talk, index: usize, range: &Range<usize>, body: &str) -> Option<Change> {
    let slide = readable(talk, index)?;
    let tag = tag_at(talk, index, range)?;
    let source = talk.source.as_str();
    let eol = eol_in(source);
    let indent = source.get(line_start(source, tag.range.start)..tag.range.start)?;
    if !indent.chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    // What the body reads as alone: blocks, and nothing a slide has besides.
    let body = one_line_ending(body);
    let body = body.trim_matches('\n');
    let alone = parse_talk(body);
    let [own] = alone.slides.as_slice() else { return None };
    if own.error.is_some() || own.notes.is_some() || own.settings.is_some() {
        return None;
    }

    let open = source.get(tag.range.start..tag.open_end)?;
    let (from, opening) = match open.strip_suffix("/>") {
        // The space before `/>` goes with it.
        Some(kept) => (tag.range.start + kept.trim_end_matches([' ', '\t', '\r', '\n']).len(), ">"),
        None => (tag.open_end, ""),
    };
    let mut text = format!("{opening}{eol}{eol}");
    if !own.blocks.is_empty() {
        for line in body.split('\n') {
            if !line.is_empty() {
                text.push_str(indent);
                text.push_str(line);
            }
            text.push_str(eol);
        }
        text.push_str(eol);
    }
    text.push_str(indent);
    text.push_str(&format!("</{}>", tag.name));

    let mut expected = slide.blocks.clone();
    with_block(&mut expected, range, &mut |blocks, at| {
        if let Block::Tag(tag) = &mut blocks[at] {
            tag.children = own.blocks.clone();
        }
    });
    let change = Change::single(SourceEdit { range: from..tag.range.end, text });
    let after = reparse(talk, change.apply(source));
    reads_as(talk, &after, index, &expected).then_some(change)
}
