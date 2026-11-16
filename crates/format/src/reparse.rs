//! Reading a talk again after an edit, parsing only the slides whose text changed.

use std::ops::Range;

use crate::model::{Attr, Block, Notes, Slide, Tag, Talk};
use crate::parse::read_slides;
use crate::settings::{SettingEntry, Settings};
use crate::split::{RawSlide, split_slides};

/// The talk `new_source` reads as, given `old`, a talk as [`crate::parse_talk`] read it. Each
/// slide whose bytes are those of a slide of `old`, matched in order from the start and then
/// from the end, is taken from `old`, moved to where it now sits; only the others are parsed.
/// What comes out is always what `parse_talk(&new_source)` gives, as a slide reads the same
/// wherever its bytes sit.
pub fn reparse(old: &Talk, new_source: String) -> Talk {
    let raw_slides = split_slides(&new_source);
    let same = |old_index: usize, new_index: usize| {
        old.source.get(old.slides[old_index].range.clone()) == Some(&new_source[raw_slides[new_index].range.clone()])
    };
    let (old_count, new_count) = (old.slides.len(), raw_slides.len());
    let most = old_count.min(new_count);
    let from_start = (0..most).take_while(|&index| same(index, index)).count();
    let from_end = (0..most - from_start).take_while(|&back| same(old_count - 1 - back, new_count - 1 - back)).count();

    let changed: Vec<&RawSlide> = raw_slides[from_start..new_count - from_end].iter().collect();
    let parsed = read_slides(&new_source, &changed);
    let mut slides = Vec::with_capacity(new_count);
    // The slides before the first change sit where they sat.
    slides.extend(old.slides[..from_start].iter().cloned());
    slides.extend(parsed);
    for (old_slide, raw) in old.slides[old_count - from_end..].iter().zip(&raw_slides[new_count - from_end..]) {
        slides.push(moved(old_slide, old_slide.range.start, raw.range.start));
    }
    Talk { source: new_source, slides }
}

/// `slide` with every position in it moved from where it started, `from`, to `to`.
fn moved(slide: &Slide, from: usize, to: usize) -> Slide {
    let shift = Shift { from, to };
    Slide {
        range: shift.range(&slide.range),
        settings: slide.settings.as_ref().map(|settings| Settings {
            range: shift.range(&settings.range),
            entries: settings
                .entries
                .iter()
                .map(|entry| SettingEntry { line: shift.range(&entry.line), value_range: shift.range(&entry.value_range), ..entry.clone() })
                .collect(),
        }),
        blocks: shift.blocks(&slide.blocks),
        notes: slide.notes.as_ref().map(|notes| Notes { text: notes.text.clone(), range: shift.range(&notes.range) }),
        error: slide.error.clone(),
        inline_tags: slide.inline_tags.clone(),
        quotes: slide.quotes.iter().map(|quote| shift.range(quote)).collect(),
    }
}

/// Moves positions of a slide that started at `from` to where it starts now, `to`.
struct Shift {
    from: usize,
    to: usize,
}

impl Shift {
    fn at(&self, position: usize) -> usize {
        position - self.from + self.to
    }

    fn range(&self, range: &Range<usize>) -> Range<usize> {
        self.at(range.start)..self.at(range.end)
    }

    fn blocks(&self, blocks: &[Block]) -> Vec<Block> {
        blocks.iter().map(|block| self.block(block)).collect()
    }

    fn block(&self, block: &Block) -> Block {
        match block {
            Block::Heading { level, spans, range, inline, only_text } => Block::Heading {
                level: *level,
                spans: spans.clone(),
                range: self.range(range),
                inline: self.range(inline),
                only_text: *only_text,
            },
            Block::Paragraph { spans, range, inline, only_text } => {
                Block::Paragraph { spans: spans.clone(), range: self.range(range), inline: self.range(inline), only_text: *only_text }
            }
            Block::List { ordered, items, range, item_ranges } => Block::List {
                ordered: *ordered,
                items: items.iter().map(|item| self.blocks(item)).collect(),
                range: self.range(range),
                item_ranges: item_ranges.iter().map(|item| self.range(item)).collect(),
            },
            Block::Table { rows, range } => Block::Table { rows: rows.clone(), range: self.range(range) },
            Block::Code { language, code, range } => Block::Code { language: language.clone(), code: code.clone(), range: self.range(range) },
            Block::Picture { src, alt, range } => Block::Picture { src: src.clone(), alt: alt.clone(), range: self.range(range) },
            Block::Region { name, range } => Block::Region { name: name.clone(), range: self.range(range) },
            Block::Tag(tag) => Block::Tag(Tag {
                name: tag.name.clone(),
                attrs: tag
                    .attrs
                    .iter()
                    .map(|attr| Attr { name_range: self.range(&attr.name_range), value_range: self.range(&attr.value_range), ..attr.clone() })
                    .collect(),
                children: self.blocks(&tag.children),
                range: self.range(&tag.range),
                open_end: self.at(tag.open_end),
            }),
        }
    }
}
