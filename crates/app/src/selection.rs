//! A thing picked on a slide as a whole: a picture, a shape, an icon, a table, a component or a
//! positioned box. It's the editor's view of the talk, never part of it: it follows its thing
//! through every edit by where that thing's text moves to, and goes once its text does.

use std::ops::Range;

use slides_format::{Block, Change, Talk, text_flows};

/// The thing selected: slide `slide`'s block or tag whose whole source is `source`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    pub slide: usize,
    pub source: Range<usize>,
}

/// What a step of the history selects once it's undone, `before`, or made again, `after`: the
/// whole source of a thing, in the text as that leaves it, or nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectMark {
    pub before: Option<Range<usize>>,
    pub after: Option<Range<usize>>,
}

/// Where `range` is once `change` is made, if its bytes are still there: moved along by edits
/// before it, and grown or shrunk by edits inside it. An edit that reaches past either end, or
/// takes it all, takes it away.
pub(crate) fn moved(range: &Range<usize>, change: &Change) -> Option<Range<usize>> {
    let (mut start, mut end) = (range.start as isize, range.end as isize);
    for edit in &change.edits {
        let grown = edit.text.len() as isize - edit.range.len() as isize;
        let (from, to) = (edit.range.start, edit.range.end);
        if to < range.start || (from == to && from == range.start) {
            // Before it, or written just in front of it.
            start += grown;
            end += grown;
        } else if from > range.end || (from == to && from == range.end) {
            // After it, or written just behind it.
        } else if range.start < from && to < range.end {
            end += grown;
        } else if to == range.start && from < to {
            // Up to its first byte, which stays.
            start += grown;
            end += grown;
        } else if from == range.end && from < to {
            // From just past its last byte.
        } else {
            return None;
        }
    }
    (start >= 0 && start < end).then_some(start as usize..end as usize)
}

/// The slide of `talk` whose text holds all of `range`.
pub(crate) fn slide_of(talk: &Talk, range: &Range<usize>) -> Option<usize> {
    talk.slides
        .iter()
        .position(|slide| slide.range.start <= range.start && range.end <= slide.range.end)
}

/// Whether slide `slide` of `talk` has a block or tag, at any depth, whose source is `range`.
pub(crate) fn has_block(talk: &Talk, slide: usize, range: &Range<usize>) -> bool {
    fn within(blocks: &[Block], range: &Range<usize>) -> bool {
        blocks.iter().any(|block| {
            block.range() == range
                || match block {
                    Block::List { items, .. } => items.iter().any(|item| within(item, range)),
                    Block::Tag(tag) => within(&tag.children, range),
                    _ => false,
                }
        })
    }
    talk.slides
        .get(slide)
        .is_some_and(|slide| slide.error.is_none() && within(&slide.blocks, range))
}

/// The block or tag of `blocks`, at any depth, whose source is `range`.
pub(crate) fn block_at<'a>(blocks: &'a [Block], range: &Range<usize>) -> Option<&'a Block> {
    blocks.iter().find_map(|block| {
        if block.range() == range {
            return Some(block);
        }
        match block {
            Block::List { items, .. } => items.iter().find_map(|item| block_at(item, range)),
            Block::Tag(tag) => block_at(&tag.children, range),
            _ => None,
        }
    })
}

/// Whether paragraph `para` of flow `flow` of slide `slide` is written inside `range`.
pub(crate) fn holds_para(talk: &Talk, slide: usize, flow: usize, para: usize, range: &Range<usize>) -> bool {
    let flows = text_flows(talk, slide);
    flows
        .get(flow)
        .and_then(|flow| flow.source.get(para))
        .is_some_and(|source| range.start <= source.block.start && source.block.end <= range.end)
}

/// What the status line says of the thing at `range` on slide `slide` once it's selected, such
/// as "Picture selected".
pub fn said_of(talk: &Talk, slide: usize, range: &Range<usize>) -> Option<String> {
    Some(format!("{} selected", name_of(talk, slide, range)?))
}

/// What the thing at `range` on slide `slide` is called, such as "Picture" or "Text box".
pub fn name_of(talk: &Talk, slide: usize, range: &Range<usize>) -> Option<String> {
    let slide = talk.slides.get(slide)?;
    let what = match block_at(&slide.blocks, range)? {
        Block::Picture { .. } => "Picture".to_string(),
        Block::Table { .. } => "Table".to_string(),
        Block::Tag(tag) => match tag.name.as_str() {
            "Picture" | "Shape" | "Icon" | "Table" | "Chart" => tag.name.clone(),
            // Text boxes, as the Insert tab and the format name them.
            "Text" | "Box" => "Text box".to_string(),
            name => spaced(name),
        },
        _ => return None,
    };
    Some(what)
}

/// `name` with a space before each capital letter but the first: "StatCard" is "Stat Card".
pub(crate) fn spaced(name: &str) -> String {
    let mut spaced = String::with_capacity(name.len() + 4);
    for (index, letter) in name.chars().enumerate() {
        if index > 0 && letter.is_uppercase() {
            spaced.push(' ');
        }
        spaced.push(letter);
    }
    spaced
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_format::SourceEdit;

    fn change(range: Range<usize>, text: &str) -> Change {
        Change::single(SourceEdit { range, text: text.to_string() })
    }

    #[test]
    fn a_range_moves_with_the_text_around_it() {
        let thing = 10..20;
        assert_eq!(moved(&thing, &change(0..2, "abcd")), Some(12..22), "before");
        assert_eq!(moved(&thing, &change(10..10, "ab")), Some(12..22), "written in front");
        assert_eq!(moved(&thing, &change(8..10, "")), Some(8..18), "up to it");
        assert_eq!(moved(&thing, &change(20..20, "ab")), Some(10..20), "written behind");
        assert_eq!(moved(&thing, &change(20..25, "")), Some(10..20), "from its end");
        assert_eq!(moved(&thing, &change(30..31, "")), Some(10..20), "after");
        assert_eq!(moved(&thing, &change(12..14, "abcde")), Some(10..23), "inside");
        assert_eq!(moved(&thing, &change(10..20, "")), None, "taken away");
        assert_eq!(moved(&thing, &change(8..20, "x")), None, "taken with more");
        assert_eq!(moved(&thing, &change(5..12, "x")), None, "cut into at the start");
        assert_eq!(moved(&thing, &change(18..22, "x")), None, "cut into at the end");
        assert_eq!(moved(&thing, &change(0..100, "x")), None, "the whole text");
    }

    #[test]
    fn a_name_is_spaced_at_its_capitals() {
        assert_eq!(spaced("StatCard"), "Stat Card");
        assert_eq!(spaced("Chart"), "Chart");
        assert_eq!(spaced("myThing"), "my Thing");
    }
}
