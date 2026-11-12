//! A slide's text as it's edited on the slide: runs of paragraphs of styled spans, each
//! paragraph knowing the bytes it came from. Only text the model holds exactly is read
//! into it. Anything else ends a run, belongs to none, and has a sentence saying why.

use std::ops::Range;

use crate::model::{Block, Slide, Span, Tag, Talk};

/// What a paragraph is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParaKind {
    Body,
    Heading(u8),
    Bullet { level: u8 },
    Numbered { level: u8 },
}

/// One paragraph. An empty paragraph has no spans. Spans next to each other with the same
/// style are merged, and no span is empty. A line break inside the paragraph, whether the
/// Markdown has a hard break or only goes on to the next line, is a `\n` in the text, as
/// both are drawn as one.
#[derive(Clone, Debug, PartialEq)]
pub struct Para {
    pub kind: ParaKind,
    pub spans: Vec<Span>,
}

/// Where a flow's text is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowPlace {
    /// A region of the slide's layout, by name.
    Region(String),
    /// The positioned tag, by its range, whose text this is.
    Tag(Range<usize>),
}

/// Where one paragraph is in the talk's source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParaSource {
    /// The whole block, its marker included. For a list item this runs from its marker to
    /// the end of its own text, and leaves out the items nested in it.
    pub block: Range<usize>,
    /// The paragraph's content alone. For an empty list item it's empty, at the item's end.
    pub inline: Range<usize>,
}

/// One unbroken run of editable text: `source[i]` is where `paras[i]` is.
#[derive(Clone, Debug, PartialEq)]
pub struct TextFlow {
    pub place: FlowPlace,
    pub paras: Vec<Para>,
    pub source: Vec<ParaSource>,
}

const TABLE: &str = "Tables can't be edited on the slide yet.";
const CODE: &str = "Code can't be edited on the slide yet.";
const QUOTE: &str = "Quotes can't be edited on the slide yet.";
const PICTURE: &str = "Pictures can't be edited on the slide yet.";
const MIXED: &str = "Text with a component or picture in it can't be edited on the slide yet.";
const IN_A_COMPONENT: &str = "Text inside a component can't be edited on the slide yet.";
const STYLED_BLOCK: &str = "Text styled as a whole block can't be edited on the slide yet.";
const BOX_WITH_MORE: &str = "Text in a box that holds more than text can't be edited on the slide yet.";
const ODD_LIST: &str = "This list can't be edited on the slide yet.";
const UNREADABLE: &str = "This slide couldn't be read, so its text can't be edited.";

/// The deepest a list can nest and still be edited.
const MAX_LEVEL: u8 = 8;

/// The tags that draw their text as a box of its own when they're given a place.
const TEXT_BOXES: [&str; 3] = ["Text", "Box", "Shape"];

/// The layouts' regions, and the region for what nothing sends elsewhere. These are the
/// engine's layouts, which this crate can't see; `layout:` names one of them.
const LAYOUTS: [(&str, &[&str], &str); 5] = [
    ("title", &["title", "body"], "body"),
    ("content", &["title", "body"], "body"),
    ("two-columns", &["title", "left", "right"], "left"),
    ("section", &["title", "body"], "body"),
    ("blank", &["body"], "body"),
];
const DEFAULT_LAYOUT: &str = "content";

/// The regions of the slide's layout, and its main one, as the engine finds them.
fn layout_of(slide: &Slide) -> (&'static [&'static str], &'static str) {
    let name = slide.settings.as_ref().and_then(|settings| settings.get("layout")).filter(|name| !name.is_empty());
    let find = |name: &str| LAYOUTS.iter().find(|layout| layout.0 == name);
    let layout = name.and_then(find).or_else(|| find(DEFAULT_LAYOUT)).expect("the default layout is listed");
    (layout.1, layout.2)
}

/// Whether a tag asks for a place on the slide, as the engine decides it.
fn is_positioned(tag: &Tag) -> bool {
    ["x", "y", "w", "h"]
        .iter()
        .any(|name| tag.attr(name).and_then(|value| value.trim().parse::<f32>().ok()).is_some_and(f32::is_finite))
}

/// Each run of editable text on slide `slide`, in the order it's in the file. A region's
/// text is one flow per unbroken run of headings, paragraphs and lists. Anything else ends
/// the run and belongs to no flow, as does a positioned tag, whose own text is a flow if
/// it's a box of editable text.
pub fn text_flows(talk: &Talk, slide: usize) -> Vec<TextFlow> {
    let Some(slide) = talk.slides.get(slide) else { return Vec::new() };
    if slide.error.is_some() {
        return Vec::new();
    }
    let (regions, main) = layout_of(slide);
    let mut title = regions.contains(&"title").then_some("title");
    let mut current = main;
    let mut flows = Vec::new();
    let mut run: Option<TextFlow> = None;
    let end_run = |run: &mut Option<TextFlow>, flows: &mut Vec<TextFlow>| flows.extend(run.take());
    for block in &slide.blocks {
        match block {
            Block::Region { name, .. } => {
                end_run(&mut run, &mut flows);
                title = None;
                current = regions.iter().find(|region| *region == name).copied().unwrap_or(main);
            }
            Block::Tag(tag) if is_positioned(tag) => {
                end_run(&mut run, &mut flows);
                flows.extend(box_flow(slide, tag));
            }
            _ => {
                let region = match block {
                    Block::Heading { .. } => title.take().unwrap_or(current),
                    _ => current,
                };
                let mut read = Vec::new();
                if !read_block(slide, block, &mut read) {
                    end_run(&mut run, &mut flows);
                    continue;
                }
                let place = FlowPlace::Region(region.to_string());
                if run.as_ref().is_some_and(|run| run.place != place) {
                    end_run(&mut run, &mut flows);
                }
                let flow = run.get_or_insert_with(|| TextFlow { place, paras: Vec::new(), source: Vec::new() });
                for (para, source) in read {
                    flow.paras.push(para);
                    flow.source.push(source);
                }
            }
        }
    }
    end_run(&mut run, &mut flows);
    flows
}

/// The flow of a positioned tag's text, if it's a box holding only editable text.
fn box_flow(slide: &Slide, tag: &Tag) -> Option<TextFlow> {
    if !TEXT_BOXES.contains(&tag.name.as_str()) || is_quoted(slide, &tag.range) || tag.children.is_empty() {
        return None;
    }
    let mut read = Vec::new();
    for child in &tag.children {
        if !read_block(slide, child, &mut read) {
            return None;
        }
    }
    let (paras, source) = read.into_iter().unzip();
    Some(TextFlow { place: FlowPlace::Tag(tag.range.clone()), paras, source })
}

fn is_quoted(slide: &Slide, range: &Range<usize>) -> bool {
    slide.quotes.iter().any(|quote| quote.start < range.end && range.start < quote.end)
}

/// Adds the paragraphs of `block` to `out`, or returns false, having added nothing, if
/// it isn't text the model holds exactly.
fn read_block(slide: &Slide, block: &Block, out: &mut Vec<(Para, ParaSource)>) -> bool {
    if is_quoted(slide, block.range()) {
        return false;
    }
    match block {
        Block::Heading { level, spans, range, inline, only_text: true } => {
            out.push((para(ParaKind::Heading(*level), spans), ParaSource { block: range.clone(), inline: inline.clone() }));
            true
        }
        Block::Paragraph { spans, range, inline, only_text: true } => {
            out.push((para(ParaKind::Body, spans), ParaSource { block: range.clone(), inline: inline.clone() }));
            true
        }
        Block::List { ordered, items, item_ranges, .. } => {
            let mut read = Vec::new();
            let fits = read_list(*ordered, items, item_ranges, 0, &mut read);
            if fits {
                out.extend(read);
            }
            fits
        }
        _ => false,
    }
}

/// Adds a list's items, at `level`, to `out`. An item is editable if it's empty, or holds a
/// paragraph of text followed by lists, which are a level deeper.
fn read_list(ordered: bool, items: &[Vec<Block>], item_ranges: &[Range<usize>], level: u8, out: &mut Vec<(Para, ParaSource)>) -> bool {
    if level > MAX_LEVEL || items.len() != item_ranges.len() {
        return false;
    }
    let kind = if ordered { ParaKind::Numbered { level } } else { ParaKind::Bullet { level } };
    for (item, item_range) in items.iter().zip(item_ranges) {
        let nested = match item.as_slice() {
            [] => {
                let end = item_range.end;
                out.push((Para { kind, spans: Vec::new() }, ParaSource { block: item_range.clone(), inline: end..end }));
                continue;
            }
            [Block::Paragraph { spans, range, inline, only_text: true }, nested @ ..] if item_range.start <= range.start => {
                out.push((para(kind, spans), ParaSource { block: item_range.start..range.end, inline: inline.clone() }));
                nested
            }
            _ => return false,
        };
        for block in nested {
            let Block::List { ordered, items, item_ranges, .. } = block else { return false };
            if !read_list(*ordered, items, item_ranges, level + 1, out) {
                return false;
            }
        }
    }
    true
}

/// A paragraph of `kind` holding `spans`, tidied: line endings made `\n`, no empty spans,
/// and neighbours with the same style merged.
fn para(kind: ParaKind, spans: &[Span]) -> Para {
    let mut tidy: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        let text = span.text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            continue;
        }
        match tidy.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&text),
            _ => tidy.push(Span { text, style: span.style.clone() }),
        }
    }
    Para { kind, spans: tidy }
}

/// For a byte position `at` on slide `slide` that's inside text in no flow, the sentence
/// saying why it can't be edited. `None` for a position in a flow, or in nothing drawn.
pub fn uneditable_reason(talk: &Talk, slide: usize, at: usize) -> Option<&'static str> {
    let index = slide;
    let slide = talk.slides.get(index)?;
    if !slide.range.contains(&at) {
        return None;
    }
    if slide.error.is_some() {
        return Some(UNREADABLE);
    }
    let in_a_flow = text_flows(talk, index)
        .iter()
        .flat_map(|flow| &flow.source)
        .any(|source| source.block.start <= at && at <= source.block.end);
    if in_a_flow {
        return None;
    }
    if slide.quotes.iter().any(|quote| quote.contains(&at)) {
        return Some(QUOTE);
    }
    let block = slide.blocks.iter().find(|block| block.range().contains(&at))?;
    reason_in(block, at)
}

/// Why the text at `at`, inside `block`, isn't editable, or `None` if what's there is
/// editable text in itself, and only what holds it isn't.
fn reason_in(block: &Block, at: usize) -> Option<&'static str> {
    match block {
        Block::Table { .. } => Some(TABLE),
        Block::Code { .. } => Some(CODE),
        Block::Picture { .. } => Some(PICTURE),
        Block::Region { .. } => None,
        Block::Heading { only_text, .. } | Block::Paragraph { only_text, .. } => (!only_text).then_some(MIXED),
        Block::List { items, .. } => {
            let inner = items.iter().flatten().find(|block| block.range().contains(&at));
            Some(inner.and_then(|block| reason_in(block, at)).unwrap_or(ODD_LIST))
        }
        Block::Tag(tag) => {
            let inner = tag.children.iter().find(|block| block.range().contains(&at));
            let positioned = is_positioned(tag);
            let fallback = match tag.name.as_str() {
                "Text" if !positioned => STYLED_BLOCK,
                name if positioned && TEXT_BOXES.contains(&name) => BOX_WITH_MORE,
                _ => IN_A_COMPONENT,
            };
            Some(inner.and_then(|block| reason_in(block, at)).unwrap_or(fallback))
        }
    }
}
