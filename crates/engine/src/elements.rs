//! Turns what a component returned into nodes of the flow's layout tree, so that a
//! component's children lay out with the same code as the rest of the slide, and into
//! drawing.

use std::cell::Cell;

use slides_format::{Block, Tag};
use taffy::prelude::{
    AlignItems, AlignSelf, Dimension, FlexDirection, JustifyContent, NodeId, Position, Size, Style, auto, length, percent,
};
use tiny_skia::{Path, PathBuilder};
use toolkit::{Color, Ctx, CtxData, Element, Placed, Props, RenderInput, ShapeKind, TextAlign, Theme};

use crate::flow::{Content, Flow, MAX_NOTICE_CHARS, TEXT_TAG, column, rounded_rect, rounded_rect_with};
use slides_components::shortened;
use crate::pictures::icon_pixmap;
use crate::slide::{MAX_SIZE, SLIDE_HEIGHT, SLIDE_WIDTH, is_positioned};
use crate::native::Semantic;
use crate::talk_components::{Rendered, read_asset};
use crate::{Align, DisplayItem, LayoutEnv, NativeContent, NativeElement, Notice, PLACEMENT_ATTRS, PathStep, RegionDef, layout_by_name};

const DEFAULT_WIDTH: f32 = 400.0;
const DEFAULT_HEIGHT: f32 = 200.0;
/// The width a positioned tag that gives none is told it has.
const POSITIONED_ROOM: f32 = 1200.0;
/// What's said for a component that didn't run, as the slide's components had used up their time.
const NOT_RUN: &str = "not run, as this slide's components took too long";
const DEFAULT_RADIUS: f32 = 24.0;
const DEFAULT_LINE_WIDTH: f32 = 4.0;
/// As with sizes, so one attribute can't make a layer that needs gigabytes.
const MAX_LINE_WIDTH: f32 = 256.0;
/// How far from its box a path's points can be. Further than any slide, and still small
/// enough that drawing it can't overflow.
const MAX_PATH_COORDINATE: f32 = 1.0e6;
const CALLOUT_TAIL: f32 = 40.0;
const MIN_ARROW_HEAD: f32 = 24.0;
const ARROW_HEAD_PER_LINE_WIDTH: f32 = 6.0;

/// What a shape draws, once it knows its size.
#[derive(Clone, Copy)]
pub(crate) struct ShapeLook {
    kind: ShapeKind,
    fill: Option<Color>,
    outline: Option<Color>,
    outline_width: f32,
    radius: f32,
    /// Whether what's inside sits in the middle, rather than starting at the corner.
    centered: bool,
}

impl ShapeLook {
    fn drawn_radius(&self) -> f32 {
        if self.radius > 0.0 { self.radius } else { DEFAULT_RADIUS }
    }

    /// The shape for export, in the colours and sizes `shape_items` draws it with, and
    /// with no text yet. `inset` is the padding the layout gave it.
    pub(crate) fn native(&self, theme: &Theme, inset: f32) -> NativeContent {
        let (fill, outline, outline_width) = if matches!(self.kind, ShapeKind::Line | ShapeKind::Arrow) {
            let (color, width) = self.line_look(theme);
            (None, Some(color), width)
        } else {
            (self.fill, self.outline.filter(|_| self.outline_width > 0.0), self.outline_width)
        };
        let radius = if matches!(self.kind, ShapeKind::Rounded | ShapeKind::Callout) { self.drawn_radius() } else { 0.0 };
        NativeContent::Shape { kind: self.kind, fill, outline, outline_width, radius, paragraphs: Vec::new(), inset, centered: self.centered }
    }

    /// The colour and width of a line: the outline colour or, with no outline, the fill.
    fn line_look(&self, theme: &Theme) -> (Color, f32) {
        let color = self.outline.or(self.fill).unwrap_or(theme.text);
        (color, if self.outline_width > 0.0 { self.outline_width } else { DEFAULT_LINE_WIDTH })
    }
}

/// A number from a talk or a component, held to what can be drawn.
fn limited(value: f32, max: f32) -> f32 {
    if value.is_finite() { value.clamp(0.0, max) } else { 0.0 }
}

/// One side of something placed in a box, where 0 means as long as the box. Outside a
/// box, 0 leaves the side to the node's content.
fn side(value: f32, boxed: bool) -> Dimension {
    let value = limited(value, MAX_SIZE);
    match (value > 0.0, boxed) {
        (true, _) => length(value),
        (false, true) => percent(1.0),
        (false, false) => auto(),
    }
}

fn size_of(width: f32, height: f32, boxed: bool) -> Size<Dimension> {
    Size { width: side(width, boxed), height: side(height, boxed) }
}

/// The size of something with no content. Outside a box, nothing else says how big it is.
pub(crate) fn size_when_empty(width: f32, height: f32, boxed: bool) -> Size<Dimension> {
    let or_default = |value: f32, default: f32| if value > 0.0 || boxed { value } else { default };
    size_of(or_default(width, DEFAULT_WIDTH), or_default(height, DEFAULT_HEIGHT), boxed)
}

/// Where something narrower than its column sits in it.
fn along_column(align: Align) -> Option<AlignSelf> {
    Some(match align {
        Align::Left => AlignSelf::FLEX_START,
        Align::Center => AlignSelf::CENTER,
        Align::Right => AlignSelf::FLEX_END,
    })
}

/// The size of a positioned tag that gives `wanted` of its sides. A side it leaves out
/// comes from the element's natural size, kept in proportion to the side it gave.
fn box_size(wanted: (Option<f32>, Option<f32>), natural: Option<(f32, f32)>) -> (f32, f32) {
    let (w, h) = match (wanted, natural) {
        ((Some(w), Some(h)), _) => (w, h),
        ((Some(w), None), Some((natural_w, natural_h))) => (w, w * natural_h / natural_w),
        ((None, Some(h)), Some((natural_w, natural_h))) => (h * natural_w / natural_h, h),
        ((None, None), Some(natural)) => natural,
        ((w, h), None) => (w.unwrap_or(DEFAULT_WIDTH), h.unwrap_or(DEFAULT_HEIGHT)),
    };
    (w.min(MAX_SIZE), h.min(MAX_SIZE))
}

/// How text in a positioned tag looks: like the body of a blank slide.
fn positioned_tag_region() -> &'static RegionDef {
    &layout_by_name("blank").expect("the blank layout exists").regions[0]
}

/// Lays out a tag with a layer of its own, and returns the size of its box, its drawing
/// and what it is. `wanted` is the width and height the tag gives.
pub(crate) fn lay_out_positioned_tag(
    tag: &Tag,
    wanted: (Option<f32>, Option<f32>),
    theme: &Theme,
    env: &mut LayoutEnv,
    notices: &mut Vec<Notice>,
) -> ((f32, f32), Vec<DisplayItem>, Vec<NativeElement>) {
    let room = (wanted.0.unwrap_or(POSITIONED_ROOM), wanted.1.unwrap_or(0.0));
    let mut flow = Flow::new(positioned_tag_region(), room.0, theme, env, notices);
    let element = flow.render_tag(tag, room);
    let natural = element.as_ref().and_then(|element| flow.natural_size(element));
    let (w, h) = box_size(wanted, natural);
    let content = match element {
        Some(element) => flow.tag_element(tag, &element, Align::Left, true),
        None => Some(flow.placeholder(tag.name.clone(), size_of(0.0, 0.0, true))),
    };
    let content = if flow.over_budget() { Some(flow.too_much()) } else { content };
    let style = Style {
        size: Size { width: length(w), height: length(h) },
        flex_direction: FlexDirection::Column,
        ..Style::default()
    };
    let root = flow.node(style, content.as_slice());
    let (items, native) = flow.finish(root, w);
    ((w, h), items, native)
}

impl Flow<'_, '_> {
    /// What a tag's component returned, or `None`, with a notice, if there's no such
    /// component. It looks for a built-in first, then for one of the talk's own. `room` is
    /// the width and height of the box the tag sits in, where a height of 0 grows with it.
    /// A talk's component that can't give an element gives a problem in its place.
    fn render_tag(&mut self, tag: &Tag, room: (f32, f32)) -> Option<Element> {
        if tag.name == TEXT_TAG {
            return Some(Element::Children);
        }
        let attrs = tag.attrs.iter().filter(|attr| !PLACEMENT_ATTRS.contains(&attr.name.as_str()));
        // Backwards, so that the first of a repeated attribute wins, as it does in `Tag::attr`.
        let props = attrs.rev().map(|attr| (attr.name.clone(), attr.value.clone())).collect();
        let data = CtxData {
            click_step: self.env.moment.step,
            time: self.env.moment.time,
            slide_width: SLIDE_WIDTH,
            slide_height: SLIDE_HEIGHT,
            width: room.0,
            height: room.1,
            theme: self.theme.clone(),
        };
        if let Some(component) = self.env.registry.get(&tag.name) {
            let (files, used) = (self.env.files, Cell::new(0));
            let assets = |path: &str| read_asset(files, path, &used);
            let ctx = Ctx::new(data, &assets);
            let element = (component.render)(&Props(props), &ctx);
            let output = ctx.finish(element);
            self.env.asks.note(&tag.name, output.steps, output.stepped, output.animated);
            return Some(output.element);
        }
        let name = &tag.name;
        let Some(rendered) = self.env.components.render(name, &RenderInput { props, ctx: data }) else {
            self.notice(format!("There is no component called {name}"));
            return None;
        };
        Some(match rendered {
            Rendered::Output(output) => {
                if self.env.components.is_out_of_date(name) {
                    self.notice(format!("{name} is out of date, so this is its last build"));
                }
                self.env.asks.note(name, output.steps, output.stepped, output.animated);
                output.element
            }
            Rendered::NeedsBuilding => toolkit::problem(format!("{name} needs building")),
            Rendered::Failed(error) => toolkit::problem(format!("{name}: {}", error.short())),
            Rendered::OutOfTime => toolkit::problem(format!("{name}: {NOT_RUN}")),
        })
    }

    /// The size `element` has of its own, if it has one.
    fn natural_size(&mut self, element: &Element) -> Option<(f32, f32)> {
        match element {
            Element::Shape { width, height, .. } | Element::Path { width, height, .. } | Element::Canvas { width, height, .. } if *width > 0.0 && *height > 0.0 => {
                Some((*width, *height))
            }
            Element::Picture { src, .. } => {
                let picture = self.env.pictures.get(src, self.env.files)?;
                Some((picture.width() as f32, picture.height() as f32))
            }
            Element::Icon { size, .. } => Some((icon_side(*size), icon_side(*size))),
            _ => None,
        }
    }

    /// The node for a tag that flows among other blocks.
    pub(crate) fn tag(&mut self, tag: &Tag, align: Align) -> Option<NodeId> {
        if is_positioned(tag) {
            self.notice(format!("{} is inside another tag, so the place it asks for is ignored", tag.name));
        }
        match self.render_tag(tag, (self.width, 0.0)) {
            Some(element) => self.tag_element(tag, &element, align, false),
            None => Some(self.block_placeholder(tag.name.clone())),
        }
    }

    /// The node for what `tag`'s component returned, which places the tag's content once.
    fn tag_element(&mut self, tag: &Tag, element: &Element, align: Align, boxed: bool) -> Option<NodeId> {
        self.in_tag(|flow| flow.with_text_style_of(tag, |flow| flow.element(element, &tag.children, align, boxed)))
    }

    pub(crate) fn placeholder(&mut self, label: String, size: Size<Dimension>) -> NodeId {
        self.leaf(Style { size, ..Style::default() }, Content::Placeholder(label))
    }

    /// The placeholder for something that takes up a block in the flow.
    pub(crate) fn block_placeholder(&mut self, label: String) -> NodeId {
        self.placeholder(label, size_when_empty(0.0, 0.0, false))
    }

    /// The node for `element`, or `None` if it has nothing in it or the slide already has
    /// too much on it. `blocks` is what its `Element::Children` stands for, which is laid
    /// out the first time only. `boxed` says it sits in a box of a known size.
    fn element(&mut self, element: &Element, blocks: &[Block], align: Align, boxed: bool) -> Option<NodeId> {
        if self.over_budget() {
            return None;
        }
        match self.deeper(|flow| flow.element_node(element, blocks, align, boxed)) {
            Some(node) => node,
            None => Some(self.nested_too_deeply(size_when_empty(0.0, 0.0, boxed))),
        }
    }

    fn element_node(&mut self, element: &Element, blocks: &[Block], align: Align, boxed: bool) -> Option<NodeId> {
        match element {
            Element::Text { text, size, bold, color } => Some(self.plain_text(text, *size, *bold, *color, align)),
            Element::Stack { horizontal, gap, children } => self.stack(*horizontal, *gap, children, blocks, align),
            Element::Shape { kind, width, height, fill, outline, outline_width, radius, padding, child } => {
                let child = child.as_deref();
                // Whatever is in a stack starts at the corner. Anything else is centred.
                let centered = !matches!(child, Some(Element::Stack { .. }));
                let inner_align = if centered { Align::Center } else { align };
                let content = child.and_then(|child| self.element(child, blocks, inner_align, boxed));
                let size = match content {
                    Some(_) => size_of(*width, *height, boxed),
                    None => size_when_empty(*width, *height, boxed),
                };
                let tail = if *kind == ShapeKind::Callout { CALLOUT_TAIL } else { 0.0 };
                let pad = length(limited(*padding, MAX_SIZE));
                let style = Style {
                    size,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::CENTER,
                    align_items: AlignItems::CENTER,
                    padding: taffy::Rect { left: pad, right: pad, top: pad, bottom: length(limited(*padding, MAX_SIZE) + tail) },
                    ..Style::default()
                };
                let look = ShapeLook {
                    kind: *kind,
                    fill: *fill,
                    outline: *outline,
                    outline_width: limited(*outline_width, MAX_LINE_WIDTH),
                    radius: limited(*radius, MAX_SIZE),
                    centered,
                };
                Some(self.node_with_content(style, content.as_slice(), Content::Shape(look)))
            }
            Element::Picture { src, width, height } => Some(self.picture(src, "", *width, *height, align, boxed)),
            Element::Icon { name, size, color } => Some(self.icon(name, *size, *color, align)),
            Element::Path { data, width, height, fill, stroke, stroke_width } => {
                Some(self.path(data, (*width, *height), (*fill, *stroke, *stroke_width), align))
            }
            Element::Canvas { width, height, items } => Some(self.canvas(*width, *height, items, blocks, align)),
            Element::Problem { message } => {
                let message = shortened(message, MAX_NOTICE_CHARS);
                self.notice(message.clone());
                Some(self.placeholder(message, size_when_empty(0.0, 0.0, boxed)))
            }
            Element::Children if self.place_children() => self.child_blocks(blocks, align),
            Element::Children => None,
            Element::Empty => None,
        }
    }

    fn stack(&mut self, horizontal: bool, gap: f32, children: &[Element], blocks: &[Block], align: Align) -> Option<NodeId> {
        let nodes: Vec<NodeId> = children.iter().filter_map(|child| self.element(child, blocks, align, false)).collect();
        if nodes.is_empty() {
            return None;
        }
        let gap = limited(gap, MAX_SIZE);
        let style = Style {
            flex_direction: if horizontal { FlexDirection::Row } else { FlexDirection::Column },
            gap: Size { width: length(gap), height: length(gap) },
            // All the room there is, even inside a parent that centres.
            flex_grow: 1.0,
            align_self: Some(AlignSelf::STRETCH),
            ..Style::default()
        };
        let stack = self.node(style, &nodes);
        self.note(stack, Semantic::Stack);
        Some(stack)
    }

    fn child_blocks(&mut self, blocks: &[Block], align: Align) -> Option<NodeId> {
        let nodes: Vec<NodeId> = blocks.iter().filter_map(|block| self.block(block, align)).collect();
        if nodes.is_empty() {
            return None;
        }
        Some(self.node(column(), &nodes))
    }

    /// A picture, or its placeholder. With no size set and no box, it is the size of the
    /// picture, shrunk to fit its column.
    pub(crate) fn picture(&mut self, src: &str, alt: &str, width: f32, height: f32, align: Align, boxed: bool) -> NodeId {
        match self.env.pictures.get(src, self.env.files) {
            Some(picture) => {
                let style =
                    Style { size: size_of(width, height, boxed), align_self: along_column(align), ..Style::default() };
                let node = self.leaf(style, Content::Picture(picture));
                self.note(node, Semantic::File { src: src.to_string(), alt: alt.to_string() });
                node
            }
            None => {
                self.notice(format!("Can't show the picture {src}"));
                self.placeholder(src.to_string(), size_when_empty(width, height, boxed))
            }
        }
    }

    fn icon(&mut self, name: &str, size: f32, color: Option<Color>, align: Align) -> NodeId {
        let size = icon_side(size);
        let box_size = Size { width: length(size), height: length(size) };
        match icon_pixmap(name, size, color.unwrap_or(self.theme.text)) {
            Some(icon) => {
                self.leaf(Style { size: box_size, align_self: along_column(align), ..Style::default() }, Content::Picture(icon))
            }
            None => {
                self.notice(format!("There is no icon called {name}"));
                self.placeholder(name.to_string(), box_size)
            }
        }
    }

    fn path(&mut self, data: &str, (width, height): (f32, f32), (fill, stroke, stroke_width): (Option<Color>, Option<Color>, f32), align: Align) -> NodeId {
        let (width, height) = (limited(width, MAX_SIZE).max(1.0), limited(height, MAX_SIZE).max(1.0));
        let size = Size { width: length(width), height: length(height) };
        let stroke_width = limited(stroke_width, MAX_LINE_WIDTH);
        let stroke = stroke.filter(|_| stroke_width > 0.0);
        // With neither, the path is still shown, in the text colour.
        let fill = fill.or_else(|| stroke.is_none().then_some(self.theme.text));
        match parse_path(data) {
            Some(steps) => {
                let style = Style { size, align_self: along_column(align), ..Style::default() };
                self.leaf(style, Content::Path(PathLook { steps, fill, stroke, stroke_width }))
            }
            None => {
                self.notice("A component's path can't be drawn".to_string());
                self.placeholder("path".to_string(), size)
            }
        }
    }

    /// A node of a fixed size, with each of `items` at its own place in it.
    fn canvas(&mut self, width: f32, height: f32, items: &[Placed], blocks: &[Block], align: Align) -> NodeId {
        let nodes: Vec<NodeId> = items.iter().filter_map(|placed| self.placed(placed, blocks)).collect();
        let style = Style {
            size: Size { width: length(limited(width, MAX_SIZE)), height: length(limited(height, MAX_SIZE)) },
            // Taller than the room there is, it still keeps its size.
            flex_shrink: 0.0,
            align_self: along_column(align),
            ..Style::default()
        };
        self.node(style, &nodes)
    }

    /// `placed` as a node that stands alone in an export, or `None` if it has nothing in it.
    fn placed(&mut self, placed: &Placed, blocks: &[Block]) -> Option<NodeId> {
        let align = match placed.align {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        };
        let content = self.element(&placed.element, blocks, align, false)?;
        let at = |value: f32| if value.is_finite() { length(value.clamp(-MAX_SIZE, MAX_SIZE)) } else { length(0.0) };
        let style = Style {
            position: Position::Absolute,
            inset: taffy::Rect { left: at(placed.x), top: at(placed.y), right: auto(), bottom: auto() },
            size: Size { width: side(placed.width, false), height: auto() },
            flex_direction: FlexDirection::Column,
            ..Style::default()
        };
        let node = self.node(style, &[content]);
        self.note(node, Semantic::Stack);
        Some(node)
    }
}

/// The steps of SVG path `data`, with arcs and the rest as curves, or `None` if it can't
/// be read, has nothing in it or reaches beyond what can be drawn.
fn parse_path(data: &str) -> Option<Vec<PathStep>> {
    use svgtypes::SimplePathSegment as Segment;
    let mut steps = Vec::new();
    for segment in svgtypes::SimplifyingPathParser::from(data) {
        let n = |value: f64| value as f32;
        steps.push(match segment.ok()? {
            Segment::MoveTo { x, y } => PathStep::Move(n(x), n(y)),
            Segment::LineTo { x, y } => PathStep::Line(n(x), n(y)),
            Segment::Quadratic { x1, y1, x, y } => PathStep::Quad(n(x1), n(y1), n(x), n(y)),
            Segment::CurveTo { x1, y1, x2, y2, x, y } => PathStep::Cubic(n(x1), n(y1), n(x2), n(y2), n(x), n(y)),
            Segment::ClosePath => PathStep::Close,
        });
    }
    let reachable = |value: f32| value.is_finite() && value.abs() <= MAX_PATH_COORDINATE;
    let all_reachable = steps.iter().all(|step| match *step {
        PathStep::Move(a, b) | PathStep::Line(a, b) => reachable(a) && reachable(b),
        PathStep::Quad(a, b, c, d) => [a, b, c, d].into_iter().all(reachable),
        PathStep::Cubic(a, b, c, d, e, f) => [a, b, c, d, e, f].into_iter().all(reachable),
        PathStep::Close => true,
    });
    (!steps.is_empty() && all_reachable).then_some(steps)
}

/// What a path draws, with its steps relative to its box.
pub(crate) struct PathLook {
    steps: Vec<PathStep>,
    fill: Option<Color>,
    stroke: Option<Color>,
    stroke_width: f32,
}

impl PathLook {
    pub(crate) fn native(&self) -> NativeContent {
        NativeContent::Path { steps: self.steps.clone(), fill: self.fill, stroke: self.stroke, stroke_width: self.stroke_width }
    }
}

/// The drawing of a path whose box starts at (`x`, `y`).
pub(crate) fn path_items(look: &PathLook, x: f32, y: f32) -> Vec<DisplayItem> {
    let mut b = PathBuilder::new();
    for step in &look.steps {
        match *step {
            PathStep::Move(a, c) => b.move_to(x + a, y + c),
            PathStep::Line(a, c) => b.line_to(x + a, y + c),
            PathStep::Quad(a, c, d, e) => b.quad_to(x + a, y + c, x + d, y + e),
            PathStep::Cubic(a, c, d, e, f, g) => b.cubic_to(x + a, y + c, x + d, y + e, x + f, y + g),
            PathStep::Close => b.close(),
        }
    }
    let Some(path) = b.finish() else {
        return Vec::new();
    };
    let fill = look.fill.map(|color| DisplayItem::Fill { path: path.clone(), color });
    let stroke = look.stroke.map(|color| DisplayItem::Stroke { path, color, width: look.stroke_width });
    fill.into_iter().chain(stroke).collect()
}

fn icon_side(size: f32) -> f32 {
    limited(size, MAX_SIZE).max(1.0)
}

/// The drawing of a shape at (`x`, `y`), `w` by `h`.
pub(crate) fn shape_items(look: &ShapeLook, x: f32, y: f32, w: f32, h: f32, theme: &Theme) -> Vec<DisplayItem> {
    if matches!(look.kind, ShapeKind::Line | ShapeKind::Arrow) {
        return line_items(look, x, y, w, h, theme);
    }
    let Some(path) = closed_path(look, x, y, w, h) else {
        return Vec::new();
    };
    let fill = look.fill.map(|color| DisplayItem::Fill { path: path.clone(), color });
    let stroke = look.outline.filter(|_| look.outline_width > 0.0).map(|color| DisplayItem::Stroke { path, color, width: look.outline_width });
    fill.into_iter().chain(stroke).collect()
}

fn closed_path(look: &ShapeLook, x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let radius = look.drawn_radius();
    match look.kind {
        ShapeKind::Rounded => rounded_rect(x, y, w, h, radius),
        ShapeKind::Ellipse => PathBuilder::from_oval(tiny_skia::Rect::from_xywh(x, y, w, h)?),
        ShapeKind::Callout => {
            let tail = CALLOUT_TAIL.min(h / 2.0);
            let bottom = y + h - tail;
            rounded_rect_with(x, y, w, h - tail, radius, |b| {
                b.line_to(x + w * 0.35, bottom);
                b.line_to(x + w * 0.15, y + h);
                b.line_to(x + w * 0.2, bottom);
            })
        }
        _ => Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?)),
    }
}

fn line_path(from: (f32, f32), to: (f32, f32)) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(from.0, from.1);
    b.line_to(to.0, to.1);
    b.finish()
}

/// A line from the top-left corner to the bottom-right one, in the outline colour or,
/// with no outline, the fill. An arrow has a filled head at the bottom-right end.
fn line_items(look: &ShapeLook, x: f32, y: f32, w: f32, h: f32, theme: &Theme) -> Vec<DisplayItem> {
    let length = w.hypot(h);
    if length == 0.0 {
        return Vec::new();
    }
    let (color, width) = look.line_look(theme);
    let (start, end) = ((x, y), (x + w, y + h));
    let mut items = Vec::new();
    let mut shaft_end = end;
    if look.kind == ShapeKind::Arrow {
        let head = (width * ARROW_HEAD_PER_LINE_WIDTH).max(MIN_ARROW_HEAD).min(length);
        let (along_x, along_y) = (w / length, h / length);
        shaft_end = (end.0 - along_x * head, end.1 - along_y * head);
        let (half_x, half_y) = (-along_y * head / 2.0, along_x * head / 2.0);
        let mut b = PathBuilder::new();
        b.move_to(end.0, end.1);
        b.line_to(shaft_end.0 + half_x, shaft_end.1 + half_y);
        b.line_to(shaft_end.0 - half_x, shaft_end.1 - half_y);
        b.close();
        items.extend(b.finish().map(|path| DisplayItem::Fill { path, color }));
    }
    items.extend(line_path(start, shaft_end).map(|path| DisplayItem::Stroke { path, color, width }));
    items
}
