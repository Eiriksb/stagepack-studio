//! Where a slide's editable text is drawn: each paragraph, each line and each character,
//! for a caret, a selection and a click. It's read from the shaped text the slide's layers
//! draw, so a caret can't disagree with the letters beside it.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Arc;

use cosmic_text::{Align as TextAlign, BidiParagraphs, Buffer};
use slides_format::{FlowPlace, Pos, Talk, text_flows, uneditable_reason};
use tiny_skia::{Point, Transform};
use unicode_segmentation::UnicodeSegmentation;

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

/// What's under a point of the slide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Editable text: the caret goes at `pos` of flow `flow`.
    Text { flow: usize, pos: Pos },
    /// Something whose text can't be edited, and the sentence saying why.
    Uneditable(&'static str),
    /// An empty region of the slide's layout, by name, where text can be started.
    Placeholder(String),
    Nothing,
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
    /// The layout region the layer is, if it is one.
    pub region: Option<&'static str>,
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
}

/// Where slide `index`'s editable text is, laid out as its layers draw it in its whole state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayout {
    paras: Vec<PlacedPara>,
    lines: Vec<ParaLines>,
    frames: Vec<Frame>,
    /// What can't be edited, by layer, and why: innermost first among those that overlap.
    uneditable: Vec<(usize, Rect, &'static str)>,
    /// The empty regions, their place and turn.
    placeholders: Vec<(String, Rect, Frame)>,
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

fn frame_of(rect: Rect, rotate: f32, whole: bool) -> Frame {
    Frame { rect, rotate, centre: (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0), whole }
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
    ParaLines { lines, len, empty_x, size: drawn.size }
}

impl TextLayout {
    /// The layout of slide `index` of `talk`, whose layers are `specs`, drew `drawn`, and
    /// left `empty` empty.
    pub(crate) fn build(talk: &Talk, index: usize, specs: &[LayerSpec], drawn: &[LayerText], empty: &[EmptyRegion]) -> TextLayout {
        let mut layout = TextLayout {
            frames: specs.iter().zip(drawn).map(|(spec, text)| frame_of(spec.rect, spec.rotate, text.whole)).collect(),
            placeholders: empty.iter().map(|region| (region.name.to_string(), region.rect, frame_of(region.rect, region.rotate, true))).collect(),
            ..TextLayout::default()
        };
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
                    FlowPlace::Region(name) => drawn[layer].region == Some(name.as_str()),
                    FlowPlace::Tag(range) => drawn[layer].whole && specs[layer].source.as_ref() == Some(range),
                };
                if !in_place {
                    continue;
                }
                let origin = specs[layer].rect;
                let rect = Rect { x: origin.x + text.rect.x, y: origin.y + text.rect.y, ..text.rect };
                layout.paras.push(PlacedPara { flow: f, para: p, layer, rect });
                layout.lines.push(para_lines(text, rect));
            }
        }
        for (layer, text) in drawn.iter().enumerate() {
            let origin = specs[layer].rect;
            let mut found: Vec<(Rect, &'static str)> = Vec::new();
            for block in &text.blocks {
                if let Some(reason) = uneditable_reason(talk, index, block.range.start) {
                    found.push((Rect { x: origin.x + block.rect.x, y: origin.y + block.rect.y, ..block.rect }, reason));
                }
            }
            if text.whole
                && let Some(reason) = specs[layer].source.as_ref().and_then(|source| uneditable_reason(talk, index, source.start))
            {
                found.push((origin, reason));
            }
            found.sort_by(|a, b| (a.0.w * a.0.h).total_cmp(&(b.0.w * b.0.h)));
            layout.uneditable.extend(found.into_iter().map(|(rect, reason)| (layer, rect, reason)));
        }
        layout
    }

    /// Each placed paragraph, flow by flow.
    pub fn paras(&self) -> &[PlacedPara] {
        &self.paras
    }

    fn find(&self, flow: usize, para: usize) -> Option<usize> {
        self.paras.iter().position(|placed| placed.flow == flow && placed.para == para)
    }

    /// The caret at `pos` of flow `flow`. A position past what the talk holds, as the editor
    /// can have while typing, a line break at a paragraph's end or a paragraph not yet
    /// written, is on a new line below the flow's last; `None` for a flow that isn't placed.
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
        if pos.offset > last && index == lines.lines.len() - 1 && lines.len > 0 {
            return Some(CaretRect { x: lines.empty_x, top: line.top + line.height, height: line.height, rotate: frame.rotate, centre: frame.centre });
        }
        Some(CaretRect { x: line.x_at(pos.offset), top: line.top, height: line.height, rotate: frame.rotate, centre: frame.centre })
    }

    /// The caret for a paragraph past the flow's last placed one: a line further down for each.
    fn caret_past(&self, flow: usize, pos: Pos) -> Option<CaretRect> {
        let last = self.paras.iter().rposition(|placed| placed.flow == flow)?;
        let (placed, lines) = (&self.paras[last], &self.lines[last]);
        if pos.para <= placed.para {
            return None;
        }
        let frame = self.frames[placed.layer];
        let height = lines.size * LINE_HEIGHT;
        let below = placed.rect.y.max(lines.lines.last().map_or(0.0, |line| line.top + line.height));
        let top = below + (pos.para - placed.para - 1) as f32 * height + crate::flow::GAP;
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
            let why = self.uneditable.iter().find(|(on, rect, _)| *on == layer && contains(rect, point));
            if let Some((_, _, reason)) = why {
                return Some(Hit::Uneditable(reason));
            }
            // In a box of text away from its text, the caret goes to the nearest paragraph.
            if frame.whole && contains(&frame.rect, point) {
                let distance = |rect: &Rect| (rect.y - point.1).max(point.1 - (rect.y + rect.h)).max(0.0);
                let in_layer = self.paras.iter().enumerate().filter(|(_, placed)| placed.layer == layer);
                if let Some((at, placed)) = in_layer.min_by(|a, b| distance(&a.1.rect).total_cmp(&distance(&b.1.rect))) {
                    return Some(Hit::Text { flow: placed.flow, pos: Pos { para: placed.para, offset: self.offset_at(at, point) } });
                }
            }
        }
        for (name, rect, frame) in &self.placeholders {
            if contains(rect, turned(x, y, frame.centre, -frame.rotate)) {
                return Some(Hit::Placeholder(name.clone()));
            }
        }
        Some(Hit::Nothing)
    }

    /// The offset in placed paragraph `at` nearest `point`, which is in its layer's frame.
    fn offset_at(&self, at: usize, (x, y): (f32, f32)) -> usize {
        let lines = &self.lines[at];
        let index = lines.lines.iter().position(|line| y < line.top + line.height).unwrap_or(lines.lines.len() - 1);
        lines.nearest(index, x)
    }

    /// The rectangles that show flow `flow` selected from `start` to `end`, either way round,
    /// one for each line it covers, in slide units before their layer's turn. A selection
    /// that runs on past a line's end covers a little of its break too.
    pub fn selection_rects(&self, flow: usize, start: Pos, end: Pos) -> Vec<Rect> {
        let (start, end) = if (start.para, start.offset) <= (end.para, end.offset) { (start, end) } else { (end, start) };
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
        rects
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
