//! Where a slide's editable text is drawn: each paragraph, each line and each character,
//! for a caret, a selection and a click. It's read from the shaped text the slide's layers
//! draw, so a caret can't disagree with the letters beside it.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Arc;

use cosmic_text::{Align as TextAlign, BidiParagraphs, Buffer};
use slides_format::{Block, FlowPlace, Pos, Talk, text_flows, uneditable_reason};
use tiny_skia::{Point, Transform};
use unicode_segmentation::UnicodeSegmentation;

use crate::flow::TEXT_TAG;
use crate::slide::EmptyRegion;
use crate::text::LINE_HEIGHT;
use crate::{Align, LayerSpec, Rect};

/// One paragraph of a flow, where it's drawn: `rect` is in slide units, before the turn of
/// layer `layer`, an index into the slide's layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedPara {
    pub flow: usize,
    pub para: usize,
    pub layer: usize,
    pub rect: Rect,
}

/// Where a caret is drawn: a line at `x` from `top`, `height` tall, in slide units before its
/// layer is turned. The layer turns by `rotate` degrees around `centre`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretRect {
    pub x: f32,
    pub top: f32,
    pub height: f32,
    pub rotate: f32,
    pub centre: (f32, f32),
}

impl CaretRect {
    /// Where the point (`x`, `y`) of the caret's layer, before it's turned, is on the slide.
    pub fn on_slide(&self, x: f32, y: f32) -> (f32, f32) {
        turned(x, y, self.centre, self.rotate)
    }
}

/// Something on the slide that can be picked as a whole: a positioned tag, a picture, an icon,
/// a shape, a component or a table.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementBox {
    /// Its tag's or its block's whole source.
    pub source: Range<usize>,
    /// Where it's drawn, in slide units before it's turned.
    pub rect: Rect,
    /// The degrees it's turned by, about the centre of `rect`.
    pub rotate: f32,
    /// Whether there's text in it that can be edited.
    pub holds_text: bool,
}

/// What's under a point of the slide.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// Editable text: the caret goes at `pos` of flow `flow`.
    Text { flow: usize, pos: Pos },
    /// A thing of its own whose text, if it has any, can't be edited where the point is.
    Element(ElementBox),
    /// Something whose text can't be edited, and the sentence saying why.
    Uneditable(&'static str),
    /// An empty region of the slide's layout, by name, where text can be started.
    Placeholder(String),
    Nothing,
}

/// The rectangles that show a selection, in slide units before their layer is turned, and
/// how that layer turns: by `rotate` degrees around `centre`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionRects {
    pub rects: Vec<Rect>,
    pub rotate: f32,
    pub centre: (f32, f32),
}

/// Where a caret goes from where it is, by lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineMove {
    Up,
    Down,
    LineStart,
    LineEnd,
}

/// What a text leaf of a layer drew, for the editor. Ranges are into the talk's source.
#[derive(Clone)]
pub(crate) struct DrawnText {
    /// The block whose text it is: a heading or a paragraph, or an empty list item.
    pub range: Range<usize>,
    /// That block's content.
    pub inline: Range<usize>,
    /// Where the text is, relative to its layer.
    pub rect: Rect,
    /// The text's size before its spans have their say, for a caret in an empty paragraph.
    pub size: f32,
    pub align: Align,
    /// The text, and the text shaped. An empty list item has neither.
    pub shaped: Option<(String, Arc<Buffer>)>,
}

/// A block drawn in a layer, relative to the layer.
#[derive(Clone)]
pub(crate) struct DrawnBlock {
    pub range: Range<usize>,
    pub rect: Rect,
}

/// What one layer drew of the slide's blocks.
#[derive(Clone, Default)]
pub(crate) struct LayerText {
    pub texts: Vec<DrawnText>,
    pub blocks: Vec<DrawnBlock>,
    /// The layout region the layer is, if it is one, and where that region is on the slide.
    pub region: Option<(&'static str, Rect)>,
    /// Whether the layer stands for one thing, a positioned tag or a slide that couldn't be
    /// read, so that a click on it away from its text is about the start of its source.
    pub whole: bool,
}

impl LayerText {
    /// Moves the ranges along with the slide's text, from `from` to `to`.
    pub(crate) fn moved(&mut self, from: usize, to: usize) {
        let shift = |range: &mut Range<usize>| *range = range.start - from + to..range.end - from + to;
        for text in &mut self.texts {
            shift(&mut text.range);
            shift(&mut text.inline);
        }
        for block in &mut self.blocks {
            shift(&mut block.range);
        }
    }

    /// Every source range it holds, for tests.
    pub(crate) fn sources(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let texts = self.texts.iter().flat_map(|text| [text.range.clone(), text.inline.clone()]);
        texts.chain(self.blocks.iter().map(|block| block.range.clone()))
    }
}

/// One drawn line of a paragraph, in slide units before its layer's turn.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    top: f32,
    height: f32,
    /// The offsets a caret can be at on the line, and the `x` of each, in order.
    stops: Vec<(usize, f32)>,
}

impl Line {
    fn start(&self) -> usize {
        self.stops[0].0
    }

    /// The `x` of the last stop at or before `offset`.
    fn x_at(&self, offset: usize) -> f32 {
        self.stops.iter().rev().find(|stop| stop.0 <= offset).unwrap_or(&self.stops[0]).1
    }
}

/// A placed paragraph's lines. There is always at least one, and each has a stop.
#[derive(Clone, Debug, PartialEq)]
struct ParaLines {
    lines: Vec<Line>,
    /// How long the paragraph's text is. It can go on past the last stop, with a line break
    /// at its end, which isn't drawn.
    len: usize,
    /// Where a line with nothing on it starts.
    empty_x: f32,
    size: f32,
    align: Align,
}

impl ParaLines {
    /// The line `offset` is on: a line holds its own offsets up to where the next starts.
    fn line_of(&self, offset: usize) -> usize {
        self.lines.iter().rposition(|line| line.start() <= offset).unwrap_or(0)
    }

    /// The stops a caret on line `index` can take: not the one where the next line starts.
    fn own_stops(&self, index: usize) -> &[(usize, f32)] {
        let stops = &self.lines[index].stops;
        let Some(next) = self.lines.get(index + 1) else { return stops };
        let own = stops.iter().take_while(|stop| stop.0 < next.start()).count();
        &stops[..own.max(1)]
    }

    /// The offset on line `index` nearest `x`.
    fn nearest(&self, index: usize, x: f32) -> usize {
        let stops = self.own_stops(index);
        stops.iter().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |stop| stop.0)
    }
}

/// Where a layer is and how it turns.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    rect: Rect,
    rotate: f32,
    centre: (f32, f32),
    /// Whether it's a box of its own, a click anywhere in which is about it.
    whole: bool,
    /// The layout region it's in, and where that region is, for a click in the region away
    /// from its text.
    region: Option<(&'static str, Rect)>,
}

/// Where slide `index`'s editable text is, laid out as its layers draw it in its whole state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayout {
    paras: Vec<PlacedPara>,
    lines: Vec<ParaLines>,
    frames: Vec<Frame>,
    /// What can't be edited, by layer, and why, with the element it is if it's one: innermost
    /// first among those that overlap.
    uneditable: Vec<(usize, Rect, &'static str, Option<usize>)>,
    /// Every element, in the order it's drawn.
    elements: Vec<ElementBox>,
    /// The empty regions, their place and turn, and the size and alignment of their text.
    placeholders: Vec<Placeholder>,
}

/// An empty region of the slide's layout.
#[derive(Clone, Debug, PartialEq)]
struct Placeholder {
    name: String,
    rect: Rect,
    frame: Frame,
    size: f32,
    align: Align,
}

/// `(x, y)` turned by `degrees` around `centre`, as a layer is drawn.
fn turned(x: f32, y: f32, centre: (f32, f32), degrees: f32) -> (f32, f32) {
    if degrees == 0.0 {
        return (x, y);
    }
    let mut point = [Point::from_xy(x, y)];
    Transform::from_rotate_at(degrees, centre.0, centre.1).map_points(&mut point);
    (point[0].x, point[0].y)
}

fn contains(rect: &Rect, (x, y): (f32, f32)) -> bool {
    x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h
}

/// The block of `blocks`, at any depth, whose source is `range`.
fn block_at<'a>(blocks: &'a [Block], range: &Range<usize>) -> Option<&'a Block> {
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

/// Whether `block`, drawn among others, is a thing of its own. A `<Text>` tag only styles
/// the text in it.
fn is_element(block: &Block) -> bool {
    match block {
        Block::Picture { .. } | Block::Table { .. } => true,
        Block::Tag(tag) => tag.name != TEXT_TAG,
        _ => false,
    }
}

fn frame_of(rect: Rect, rotate: f32, whole: bool, region: Option<(&'static str, Rect)>) -> Frame {
    Frame { rect, rotate, centre: (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0), whole, region }
}

fn align_x(align: Align, width: f32) -> f32 {
    match align {
        Align::Left => 0.0,
        Align::Center => width / 2.0,
        Align::Right => width,
    }
}

/// The lines of `drawn`, whose layer's corner is at `origin`.
fn para_lines(drawn: &DrawnText, rect: Rect) -> ParaLines {
    let empty_x = rect.x + align_x(drawn.align, rect.w);
    let mut lines = Vec::new();
    let mut len = 0;
    if let Some((text, buffer)) = &drawn.shaped {
        // Where each of the buffer's lines starts in the text: it splits the text as this does.
        let bases: Vec<usize> = BidiParagraphs::new(text).map(|line| line.as_ptr() as usize - text.as_ptr() as usize).collect();
        len = text.len();
        for run in buffer.layout_runs() {
            let base = bases.get(run.line_i).copied().unwrap_or(len);
            let mut stops = BTreeMap::new();
            // Each cluster's glyphs together: a cluster can be several glyphs, or one glyph
            // for several characters, whose edges inside it are spread across it.
            let mut clusters: Vec<(usize, usize, f32, f32)> = Vec::new();
            for glyph in run.glyphs {
                match clusters.last_mut() {
                    Some(last) if (last.0, last.1) == (glyph.start, glyph.end) => {
                        last.2 = last.2.min(glyph.x);
                        last.3 = last.3.max(glyph.x + glyph.w);
                    }
                    _ => clusters.push((glyph.start, glyph.end, glyph.x, glyph.x + glyph.w)),
                }
            }
            for (start, end, left, right) in clusters {
                let x = |at: f32| rect.x + at;
                stops.insert(base + start, x(left));
                stops.entry(base + end).or_insert(x(right));
                let text = run.text.get(start..end).unwrap_or_default();
                let count = text.graphemes(true).count();
                for (k, (inside, _)) in text.grapheme_indices(true).enumerate().skip(1) {
                    stops.entry(base + start + inside).or_insert(x(left + (right - left) * k as f32 / count as f32));
                }
            }
            if stops.is_empty() {
                let align = buffer.lines[run.line_i].align();
                let align = match align {
                    Some(TextAlign::Center) => Align::Center,
                    Some(TextAlign::Right | TextAlign::End) => Align::Right,
                    _ => Align::Left,
                };
                stops.insert(base, rect.x + align_x(align, rect.w));
            }
            lines.push(Line { top: rect.y + run.line_top, height: run.line_height, stops: stops.into_iter().collect() });
        }
    }
    if len == 0 || lines.is_empty() {
        len = 0;
        lines = vec![Line { top: rect.y, height: drawn.size * LINE_HEIGHT, stops: vec![(0, empty_x)] }];
    }
    ParaLines { lines, len, empty_x, size: drawn.size, align: drawn.align }
}

impl TextLayout {
    /// The layout of slide `index` of `talk`, whose layers are `specs`, drew `drawn`, and
    /// left `empty` empty.
    pub(crate) fn build(talk: &Talk, index: usize, specs: &[LayerSpec], drawn: &[LayerText], empty: &[EmptyRegion]) -> TextLayout {
        let mut layout = TextLayout {
            frames: specs.iter().zip(drawn).map(|(spec, text)| frame_of(spec.rect, spec.rotate, text.whole, text.region)).collect(),
            placeholders: empty
                .iter()
                .map(|region| Placeholder {
                    name: region.name.to_string(),
                    rect: region.rect,
                    frame: frame_of(region.rect, region.rotate, true, None),
                    size: region.size,
                    align: region.align,
                })
                .collect(),
            ..TextLayout::default()
        };
        // The block of each paragraph placed, for what holds editable text.
        let mut placed_text: Vec<Range<usize>> = Vec::new();
        // Each drawn text by the end of its block and its content, which tell paragraphs apart.
        let mut found: HashMap<(usize, usize, usize), (usize, &DrawnText)> = HashMap::new();
        for (layer, text) in drawn.iter().enumerate() {
            for drawn in &text.texts {
                found.insert((drawn.range.end, drawn.inline.start, drawn.inline.end), (layer, drawn));
            }
        }
        for (f, flow) in text_flows(talk, index).iter().enumerate() {
            for (p, source) in flow.source.iter().enumerate() {
                let Some(&(layer, text)) = found.get(&(source.block.end, source.inline.start, source.inline.end)) else { continue };
                // The flow is where the format crate says it is, or it isn't placed at all.
                let in_place = match &flow.place {
                    FlowPlace::Region(name) => drawn[layer].region.is_some_and(|(region, _)| region == name.as_str()),
                    FlowPlace::Tag(range) => drawn[layer].whole && specs[layer].source.as_ref() == Some(range),
                };
                if !in_place {
                    continue;
                }
                let origin = specs[layer].rect;
                let rect = Rect { x: origin.x + text.rect.x, y: origin.y + text.rect.y, ..text.rect };
                layout.paras.push(PlacedPara { flow: f, para: p, layer, rect });
                layout.lines.push(para_lines(text, rect));
                placed_text.push(source.block.clone());
            }
        }
        let blocks = talk.slides.get(index).filter(|slide| slide.error.is_none()).map_or(&[][..], |slide| &slide.blocks[..]);
        let holds_text = |source: &Range<usize>| placed_text.iter().any(|block| source.start <= block.start && block.end <= source.end);
        for (layer, text) in drawn.iter().enumerate() {
            let origin = specs[layer].rect;
            let frame = layout.frames[layer];
            // An element of the layer, `rect` relative to the slide before the layer's turn.
            let mut element = |source: &Range<usize>, rect: Rect| {
                let (x, y) = turned(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, frame.centre, frame.rotate);
                let rect = Rect { x: x - rect.w / 2.0, y: y - rect.h / 2.0, ..rect };
                layout.elements.push(ElementBox { source: source.clone(), rect, rotate: frame.rotate, holds_text: holds_text(source) });
                layout.elements.len() - 1
            };
            // A positioned tag is drawn first in its layer, then what's in it.
            let whole = specs[layer].source.as_ref().filter(|source| text.whole && matches!(block_at(blocks, source), Some(Block::Tag(_))));
            let whole = whole.map(|source| element(source, origin));
            let mut found: Vec<(Rect, &'static str, Option<usize>)> = Vec::new();
            for block in &text.blocks {
                let rect = Rect { x: origin.x + block.rect.x, y: origin.y + block.rect.y, ..block.rect };
                let at = block_at(blocks, &block.range).filter(|block| is_element(block)).map(|_| element(&block.range, rect));
                if let Some(reason) = uneditable_reason(talk, index, block.range.start) {
                    found.push((rect, reason, at));
                }
            }
            if text.whole
                && let Some(reason) = specs[layer].source.as_ref().and_then(|source| uneditable_reason(talk, index, source.start))
            {
                found.push((origin, reason, whole));
            }
            found.sort_by(|a, b| (a.0.w * a.0.h).total_cmp(&(b.0.w * b.0.h)));
            layout.uneditable.extend(found.into_iter().map(|(rect, reason, element)| (layer, rect, reason, element)));
        }
        layout
    }

    /// Each placed paragraph, flow by flow.
    pub fn paras(&self) -> &[PlacedPara] {
        &self.paras
    }

    /// Where the empty region `region` is, in slide units before its turn, and the degrees it
    /// turns by around its centre: for a caret in it before it holds any text.
    pub fn placeholder(&self, region: &str) -> Option<(Rect, f32)> {
        let placeholder = self.find_placeholder(region)?;
        Some((placeholder.rect, placeholder.frame.rotate))
    }

    fn find_placeholder(&self, region: &str) -> Option<&Placeholder> {
        self.placeholders.iter().find(|placeholder| placeholder.name == region)
    }

    /// The size and alignment text typed into the empty region `region` takes, before its
    /// spans have their say.
    pub fn placeholder_text(&self, region: &str) -> Option<(f32, Align)> {
        let placeholder = self.find_placeholder(region)?;
        Some((placeholder.size, placeholder.align))
    }

    /// The caret in the empty region `region`, before it holds any text: where its first line
    /// starts, as tall as that line, held within the region.
    pub fn placeholder_caret(&self, region: &str) -> Option<CaretRect> {
        let Placeholder { rect, frame, size, align, .. } = self.find_placeholder(region)?;
        Some(CaretRect {
            x: rect.x + align_x(*align, rect.w),
            top: rect.y,
            height: (size * LINE_HEIGHT).min(rect.h),
            rotate: frame.rotate,
            centre: frame.centre,
        })
    }

    /// The size paragraph `para` of flow `flow` is drawn at before its spans have their say:
    /// what text in it with no size of its own is drawn at. `None` if it isn't placed.
    pub fn text_size(&self, flow: usize, para: usize) -> Option<f32> {
        self.find(flow, para).map(|at| self.lines[at].size)
    }

    /// How flow `flow`'s text is aligned, as its first placed paragraph is. `None` if none of
    /// it is placed.
    pub fn align(&self, flow: usize) -> Option<Align> {
        let at = self.paras.iter().position(|placed| placed.flow == flow)?;
        Some(self.lines[at].align)
    }

    fn find(&self, flow: usize, para: usize) -> Option<usize> {
        self.paras.iter().position(|placed| placed.flow == flow && placed.para == para)
    }

    /// The caret at `pos` of flow `flow`. A position just past what the talk holds, one
    /// offset past a paragraph's end or the paragraph after the flow's last, is on a new line
    /// below it; `None` for anything further, or a flow that isn't placed.
    pub fn caret(&self, flow: usize, pos: Pos) -> Option<CaretRect> {
        let at = match self.find(flow, pos.para) {
            Some(at) => at,
            None => return self.caret_past(flow, pos),
        };
        let (placed, lines) = (&self.paras[at], &self.lines[at]);
        let frame = self.frames[placed.layer];
        let index = lines.line_of(pos.offset);
        let line = &lines.lines[index];
        let last = line.stops[line.stops.len() - 1].0;
        if pos.offset > lines.len + 1 {
            return None;
        }
        if pos.offset > last && index == lines.lines.len() - 1 && lines.len > 0 {
            return Some(CaretRect { x: lines.empty_x, top: line.top + line.height, height: line.height, rotate: frame.rotate, centre: frame.centre });
        }
        Some(CaretRect { x: line.x_at(pos.offset), top: line.top, height: line.height, rotate: frame.rotate, centre: frame.centre })
    }

    /// The caret for a paragraph past the flow's last placed one: a line further down for each.
    fn caret_past(&self, flow: usize, pos: Pos) -> Option<CaretRect> {
        let last = self.paras.iter().rposition(|placed| placed.flow == flow)?;
        let (placed, lines) = (&self.paras[last], &self.lines[last]);
        if pos.para != placed.para + 1 || pos.offset != 0 {
            return None;
        }
        let frame = self.frames[placed.layer];
        let height = lines.size * LINE_HEIGHT;
        let below = placed.rect.y.max(lines.lines.last().map_or(0.0, |line| line.top + line.height));
        let top = below + crate::flow::GAP;
        Some(CaretRect { x: lines.empty_x, top, height, rotate: frame.rotate, centre: frame.centre })
    }

    /// What's under the point (`x`, `y`) of the slide, in slide units: the topmost layer's
    /// text, or why it can't be edited, then an empty region. `None` for a point that isn't one.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        for layer in (0..self.frames.len()).rev() {
            let frame = self.frames[layer];
            let point = turned(x, y, frame.centre, -frame.rotate);
            let text = self.paras.iter().position(|placed| placed.layer == layer && contains(&placed.rect, point));
            if let Some(at) = text {
                let placed = &self.paras[at];
                return Some(Hit::Text { flow: placed.flow, pos: Pos { para: placed.para, offset: self.offset_at(at, point) } });
            }
            let why = self.uneditable.iter().find(|(on, rect, ..)| *on == layer && contains(rect, point));
            if let Some((_, _, reason, element)) = why {
                return Some(match element {
                    Some(at) => Hit::Element(self.elements[*at].clone()),
                    None => Hit::Uneditable(reason),
                });
            }
            // In a box of text away from its text, the caret goes to the nearest paragraph.
            if frame.whole
                && contains(&frame.rect, point)
                && let Some(hit) = self.nearest(|placed| placed.layer == layer, (x, y))
            {
                return Some(hit);
            }
        }
        // So it does in a region of text away from its text: between paragraphs, on a marker,
        // below the last.
        for frame in self.frames.iter().rev() {
            let Some((name, area)) = frame.region else { continue };
            if contains(&area, turned(x, y, frame.centre, -frame.rotate))
                && let Some(hit) = self.nearest(|placed| self.frames[placed.layer].region.is_some_and(|(other, _)| other == name), (x, y))
            {
                return Some(hit);
            }
        }
        for Placeholder { name, rect, frame, .. } in &self.placeholders {
            if contains(rect, turned(x, y, frame.centre, -frame.rotate)) {
                return Some(Hit::Placeholder(name.clone()));
            }
        }
        Some(Hit::Nothing)
    }

    /// The topmost element under the point (`x`, `y`) of the slide, whether or not the point
    /// is on its text: the one drawn last, each in its turned shape.
    pub fn element_at(&self, x: f32, y: f32) -> Option<ElementBox> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let under = |element: &&ElementBox| {
            let Rect { x: left, y: top, w, h } = element.rect;
            contains(&element.rect, turned(x, y, (left + w / 2.0, top + h / 2.0), -element.rotate))
        };
        self.elements.iter().rev().find(under).cloned()
    }

    /// The element whose source is `source`, as `element_at` would give it.
    pub fn element(&self, source: &Range<usize>) -> Option<ElementBox> {
        self.elements.iter().rev().find(|element| element.source == *source).cloned()
    }

    /// The place in the paragraphs `among` takes nearest the point (`x`, `y`) of the slide:
    /// the nearest line by height, then the nearest offset on it.
    fn nearest(&self, among: impl Fn(&PlacedPara) -> bool, (x, y): (f32, f32)) -> Option<Hit> {
        let mut best: Option<(f32, f32, usize, (f32, f32))> = None;
        for (at, (placed, lines)) in self.paras.iter().zip(&self.lines).enumerate() {
            if !among(placed) {
                continue;
            }
            let frame = self.frames[placed.layer];
            let point = turned(x, y, frame.centre, -frame.rotate);
            for line in &lines.lines {
                let down = (line.top - point.1).max(point.1 - (line.top + line.height)).max(0.0);
                let across = (placed.rect.x - point.0).max(point.0 - (placed.rect.x + placed.rect.w)).max(0.0);
                if best.is_none_or(|(d, a, ..)| (down, across) < (d, a)) {
                    best = Some((down, across, at, point));
                }
            }
        }
        let (_, _, at, point) = best?;
        let placed = &self.paras[at];
        let lines = &self.lines[at];
        // The nearest line, with the point held within the paragraph's height.
        let index = lines.lines.iter().enumerate().min_by(|a, b| {
            let distance = |line: &Line| (line.top - point.1).max(point.1 - (line.top + line.height)).max(0.0);
            distance(a.1).total_cmp(&distance(b.1))
        });
        let index = index.map_or(0, |(index, _)| index);
        Some(Hit::Text { flow: placed.flow, pos: Pos { para: placed.para, offset: lines.nearest(index, point.0) } })
    }

    /// The offset in placed paragraph `at` nearest `point`, which is in its layer's frame.
    fn offset_at(&self, at: usize, (x, y): (f32, f32)) -> usize {
        let lines = &self.lines[at];
        let index = lines.lines.iter().position(|line| y < line.top + line.height).unwrap_or(lines.lines.len() - 1);
        lines.nearest(index, x)
    }

    /// The rectangles that show flow `flow` selected from `start` to `end`, either way round,
    /// one for each line it covers, with the turn of the flow's layer. A selection that runs
    /// on past a line's end covers a little of its break too.
    pub fn selection_rects(&self, flow: usize, start: Pos, end: Pos) -> SelectionRects {
        let (start, end) = if (start.para, start.offset) <= (end.para, end.offset) { (start, end) } else { (end, start) };
        let Some(first) = self.paras.iter().find(|placed| placed.flow == flow) else { return SelectionRects::default() };
        let frame = self.frames[first.layer];
        let mut rects = Vec::new();
        for (placed, lines) in self.paras.iter().zip(&self.lines) {
            if placed.flow != flow || placed.para < start.para || placed.para > end.para {
                continue;
            }
            let from = if placed.para == start.para { start.offset } else { 0 };
            let to = if placed.para == end.para { end.offset } else { usize::MAX };
            for (index, line) in lines.lines.iter().enumerate() {
                let own = lines.own_stops(index);
                let (first, last) = (own[0].0, own[own.len() - 1].0);
                let next = lines.lines.get(index + 1).map_or(usize::MAX, Line::start);
                // The line holds its offsets from `first` up to where the next line starts.
                if from >= next || to <= first {
                    continue;
                }
                let left = line.x_at(from.max(first));
                let mut right = line.x_at(to.min(last));
                if to > last {
                    right = right.max(left) + line.height * 0.25;
                }
                if right > left {
                    rects.push(Rect { x: left, y: line.top, w: right - left, h: line.height });
                }
            }
        }
        SelectionRects { rects, rotate: frame.rotate, centre: frame.centre }
    }

    /// Where the caret goes from `pos` of flow `flow`: up or down a line, keeping to
    /// `wanted_x` if it's given and to the caret's own `x` if not, or to the start or the end
    /// of its line. Up from the first line goes to its start, and down from the last to its end.
    pub fn line_move(&self, flow: usize, pos: Pos, direction: LineMove, wanted_x: Option<f32>) -> Pos {
        let Some(at) = self.find(flow, pos.para) else { return pos };
        let lines = &self.lines[at];
        let index = lines.line_of(pos.offset);
        let own = lines.own_stops(index);
        let in_para = |offset| Pos { para: pos.para, offset };
        let x = wanted_x.unwrap_or_else(|| lines.lines[index].x_at(pos.offset));
        match direction {
            LineMove::LineStart => in_para(own[0].0),
            LineMove::LineEnd => in_para(own[own.len() - 1].0),
            LineMove::Up if index > 0 => in_para(lines.nearest(index - 1, x)),
            LineMove::Down if index + 1 < lines.lines.len() => in_para(lines.nearest(index + 1, x)),
            LineMove::Up => match self.paras[..at].iter().rposition(|placed| placed.flow == flow) {
                Some(before) => Pos { para: self.paras[before].para, offset: self.lines[before].nearest(self.lines[before].lines.len() - 1, x) },
                None => in_para(own[0].0),
            },
            LineMove::Down => match self.paras[at + 1..].iter().position(|placed| placed.flow == flow) {
                Some(after) => {
                    let after = at + 1 + after;
                    Pos { para: self.paras[after].para, offset: self.lines[after].nearest(0, x) }
                }
                None => in_para(own[own.len() - 1].0),
            },
        }
    }
}
