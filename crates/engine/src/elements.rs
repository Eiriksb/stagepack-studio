//! Turns what a component returned into nodes of the flow's layout tree, so that a
//! component's children lay out with the same code as the rest of the slide, and into
//! drawing.

use slides_format::{Block, Tag};
use taffy::prelude::{
    AlignItems, AlignSelf, Dimension, FlexDirection, JustifyContent, NodeId, Size, Style, auto, length, percent, zero,
};
use tiny_skia::{Path, PathBuilder};
use toolkit::{Color, Ctx, Element, Props, ShapeKind, Theme};

use crate::flow::{Content, Flow, TEXT_TAG, column, rounded_rect, rounded_rect_with};
use crate::pictures::{icon_pixmap, path_pixmap};
use crate::slide::{MAX_SIZE, SLIDE_HEIGHT, SLIDE_WIDTH, is_positioned};
use crate::{Align, DisplayItem, LayoutEnv, Notice, PLACEMENT_ATTRS, RegionDef, layout_by_name};

const DEFAULT_WIDTH: f32 = 400.0;
const DEFAULT_HEIGHT: f32 = 200.0;
const DEFAULT_RADIUS: f32 = 24.0;
const DEFAULT_LINE_WIDTH: f32 = 4.0;
/// As with sizes, so one attribute can't make a layer that needs gigabytes.
const MAX_LINE_WIDTH: f32 = 256.0;
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
fn size_when_empty(width: f32, height: f32, boxed: bool) -> Size<Dimension> {
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

/// Lays out a tag with a layer of its own, and returns the size of its box and its
/// drawing. `wanted` is the width and height the tag gives.
pub(crate) fn lay_out_positioned_tag(
    tag: &Tag,
    wanted: (Option<f32>, Option<f32>),
    theme: &Theme,
    env: &mut LayoutEnv,
    notices: &mut Vec<Notice>,
) -> ((f32, f32), Vec<DisplayItem>) {
    let mut flow = Flow::new(positioned_tag_region(), theme, env, notices);
    let element = flow.render_tag(tag);
    let natural = element.as_ref().and_then(|element| flow.natural_size(element));
    let (w, h) = box_size(wanted, natural);
    let content = match element {
        Some(element) => flow.with_text_style_of(tag, |flow| flow.element(&element, &tag.children, Align::Left, true)),
        None => Some(flow.placeholder(tag.name.clone(), size_of(0.0, 0.0, true))),
    };
    let style = Style {
        size: Size { width: length(w), height: length(h) },
        flex_direction: FlexDirection::Column,
        ..Style::default()
    };
    let root = flow.node(style, content.as_slice());
    ((w, h), flow.finish(root, w))
}

impl Flow<'_, '_> {
    /// What a tag's component returned, or `None`, with a notice, if there's no such component.
    fn render_tag(&mut self, tag: &Tag) -> Option<Element> {
        if tag.name == TEXT_TAG {
            return Some(Element::Children);
        }
        let Some(component) = self.env.registry.get(&tag.name) else {
            self.notice(format!("There is no component called {}", tag.name));
            return None;
        };
        let attrs = tag.attrs.iter().filter(|attr| !PLACEMENT_ATTRS.contains(&attr.name.as_str()));
        // Backwards, so that the first of a repeated attribute wins, as it does in `Tag::attr`.
        let props = Props(attrs.rev().map(|attr| (attr.name.clone(), attr.value.clone())).collect());
        let ctx = Ctx {
            click_step: 0,
            time: 0.0,
            slide_width: SLIDE_WIDTH,
            slide_height: SLIDE_HEIGHT,
            theme: self.theme.clone(),
        };
        Some((component.render)(&props, &ctx))
    }

    /// The size `element` has of its own, if it has one.
    fn natural_size(&mut self, element: &Element) -> Option<(f32, f32)> {
        match element {
            Element::Shape { width, height, .. } | Element::Path { width, height, .. } if *width > 0.0 && *height > 0.0 => {
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
        match self.render_tag(tag) {
            Some(element) => self.with_text_style_of(tag, |flow| flow.element(&element, &tag.children, align, false)),
            None => Some(self.block_placeholder(tag.name.clone())),
        }
    }

    pub(crate) fn placeholder(&mut self, label: String, size: Size<Dimension>) -> NodeId {
        self.leaf(Style { size, ..Style::default() }, Content::Placeholder(label))
    }

    /// The placeholder for something that takes up a block in the flow.
    pub(crate) fn block_placeholder(&mut self, label: String) -> NodeId {
        self.placeholder(label, size_when_empty(0.0, 0.0, false))
    }

    /// The node for `element`, or `None` if it has nothing in it. `blocks` is what its
    /// `Element::Children` stands for. `boxed` says it sits in a box of a known size.
    fn element(&mut self, element: &Element, blocks: &[Block], align: Align, boxed: bool) -> Option<NodeId> {
        match element {
            Element::Text { text, size, bold, color } => Some(self.plain_text(text, *size, *bold, *color, align)),
            Element::Stack { horizontal, gap, children } => self.stack(*horizontal, *gap, children, blocks, align),
            Element::Shape { kind, width, height, fill, outline, outline_width, radius, padding, child } => {
                let child = child.as_deref();
                // Whatever is in a stack starts at the corner. Anything else is centred.
                let inner_align = if matches!(child, Some(Element::Stack { .. })) { align } else { Align::Center };
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
                };
                Some(self.node_with_content(style, content.as_slice(), Content::Shape(look)))
            }
            Element::Picture { src, width, height } => Some(self.picture(src, *width, *height, align, boxed)),
            Element::Icon { name, size, color } => Some(self.icon(name, *size, *color, align)),
            Element::Path { data, width, height, fill } => Some(self.path(data, *width, *height, *fill, align)),
            Element::Children => self.child_blocks(blocks, align),
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
        Some(self.node(style, &nodes))
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
    pub(crate) fn picture(&mut self, src: &str, width: f32, height: f32, align: Align, boxed: bool) -> NodeId {
        match self.env.pictures.get(src, self.env.files) {
            Some(picture) => {
                let style =
                    Style { size: size_of(width, height, boxed), align_self: along_column(align), ..Style::default() };
                self.leaf(style, Content::Picture(picture))
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

    fn path(&mut self, data: &str, width: f32, height: f32, fill: Option<Color>, align: Align) -> NodeId {
        let (width, height) = (limited(width, MAX_SIZE).max(1.0), limited(height, MAX_SIZE).max(1.0));
        let size = Size { width: length(width), height: length(height) };
        match path_pixmap(data, width, height, fill.unwrap_or(self.theme.text)) {
            Some(path) => self.leaf(Style { size, align_self: along_column(align), ..Style::default() }, Content::Picture(path)),
            None => {
                self.notice("A component's path can't be drawn".to_string());
                self.placeholder("path".to_string(), size)
            }
        }
    }
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
    let radius = if look.radius > 0.0 { look.radius } else { DEFAULT_RADIUS };
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
    let color = look.outline.or(look.fill).unwrap_or(theme.text);
    let width = if look.outline_width > 0.0 { look.outline_width } else { DEFAULT_LINE_WIDTH };
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
