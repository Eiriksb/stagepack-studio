//! Lays a region's blocks out top to bottom and turns them into drawing.

use std::sync::Arc;

use slides_format::{Block, Span, Style as TextStyle, Tag};
use taffy::compute_leaf_layout;
use taffy::prelude::{
    AvailableSpace, Display, FlexDirection, NodeId, Size, Style, TaffyTree, auto, evenly_sized_tracks, length, line, zero,
};
use tiny_skia::{Path, PathBuilder, Pixmap};
use toolkit::{Color, Theme};

use crate::elements::{ShapeLook, shape_items};
use crate::placeholder::placeholder_items;
use crate::{Align, DisplayItem, Fonts, LayoutEnv, Notice, Rect, RegionDef, ShapedText, TextRun, highlight, shape_text};

const GAP: f32 = 24.0;
/// The least a list is indented, however narrow its markers.
const LIST_INDENT: f32 = 48.0;
/// The least space between a list's widest marker and its text.
const MARKER_GAP: f32 = 12.0;
const CELL_PADDING: f32 = 12.0;
const OUTLINE: f32 = 1.0;
const CODE_SIZE: f32 = 32.0;
const CODE_PADDING: f32 = 24.0;
const CODE_RADIUS: f32 = 12.0;
const CODE_PANEL: Color = Color::rgb(0xf1, 0xf5, 0xf9);
/// How far lists and tags can sit inside each other. A talk could nest them as deep as it
/// likes, and laying out each level uses up more of the stack.
const MAX_NESTING: u32 = 16;
/// The tag that styles the text inside it. It isn't a component and has nothing of its own to draw.
pub(crate) const TEXT_TAG: &str = "Text";

/// What a leaf draws behind its text.
#[derive(Clone, Copy)]
enum Frame {
    None,
    Panel,
    Outline,
}

/// What a node of the tree draws. A node with no content only arranges its children.
pub(crate) enum Content {
    Text(Leaf),
    Shape(ShapeLook),
    Picture(Arc<Pixmap>),
    Placeholder(String),
}

impl Content {
    fn measure(&self, known: Size<Option<f32>>, available: Size<AvailableSpace>, fonts: &mut Fonts) -> Size<f32> {
        match self {
            Content::Text(leaf) => leaf.measure(available, fonts),
            Content::Picture(picture) => measure_picture(picture, known, available),
            Content::Shape(_) | Content::Placeholder(_) => Size::ZERO,
        }
    }
}

/// A picture's own size, shrunk to the room there is. With one side known, the other
/// keeps the picture's shape.
fn measure_picture(picture: &Pixmap, known: Size<Option<f32>>, available: Size<AvailableSpace>) -> Size<f32> {
    let (width, height) = (picture.width() as f32, picture.height() as f32);
    let room = match available.width {
        AvailableSpace::Definite(room) => room,
        AvailableSpace::MinContent => 0.0,
        AvailableSpace::MaxContent => width,
    };
    let scale = match (known.width, known.height) {
        (Some(known), _) => known / width,
        (None, Some(known)) => known / height,
        (None, None) => (room / width).min(1.0),
    };
    Size { width: known.width.unwrap_or(width * scale), height: known.height.unwrap_or(height * scale) }
}

/// A piece of text.
pub(crate) struct Leaf {
    runs: Vec<TextRun>,
    align: Align,
    /// A list marker stays on one line, however narrow the indent is.
    wraps: bool,
    frame: Frame,
}

impl Leaf {
    fn shape(&self, width: Option<f32>, fonts: &mut Fonts) -> ShapedText {
        shape_text(fonts, &self.runs, width.filter(|_| self.wraps), self.align)
    }

    /// Takes the space Taffy offers, not its known size: only the space has had the
    /// leaf's padding taken off, so only it is the width the text will be drawn at.
    fn measure(&self, available: Size<AvailableSpace>, fonts: &mut Fonts) -> Size<f32> {
        let width = match available.width {
            AvailableSpace::Definite(width) => Some(width),
            AvailableSpace::MinContent => Some(0.0),
            AvailableSpace::MaxContent => None,
        };
        let text = self.shape(width, fonts);
        Size { width: text.width, height: text.height }
    }
}

/// The size and weight text takes before its spans have their say.
#[derive(Clone, Copy)]
struct TextLook {
    size: f32,
    bold: bool,
}

/// A layout tree being built for one layer, and what building it needs.
pub(crate) struct Flow<'a, 'env> {
    tree: TaffyTree<Content>,
    region: &'a RegionDef,
    pub(crate) theme: &'a Theme,
    pub(crate) env: &'a mut LayoutEnv<'env>,
    notices: &'a mut Vec<Notice>,
    nesting: u32,
    /// What the `<Text>` tags around the current block ask of the text in it.
    text_tag_style: TextStyle,
}

/// A column with the gap between blocks.
pub(crate) fn column() -> Style {
    Style {
        flex_direction: FlexDirection::Column,
        gap: Size { width: zero(), height: length(GAP) },
        ..Style::default()
    }
}

/// Lays `blocks` out in a column `width` wide and returns their drawing, in layer
/// coordinates. The column is as tall as its content, which can be taller than the region.
pub(crate) fn flow_region(
    blocks: &[&Block],
    region: &RegionDef,
    width: f32,
    theme: &Theme,
    env: &mut LayoutEnv,
    notices: &mut Vec<Notice>,
) -> Vec<DisplayItem> {
    let mut flow = Flow::new(region, theme, env, notices);
    let children: Vec<NodeId> = blocks.iter().filter_map(|block| flow.block(block, region.align)).collect();
    let root = flow.node(Style { size: Size { width: length(width), height: auto() }, ..column() }, &children);
    flow.finish(root, width)
}

impl<'a, 'env> Flow<'a, 'env> {
    pub(crate) fn new(
        region: &'a RegionDef,
        theme: &'a Theme,
        env: &'a mut LayoutEnv<'env>,
        notices: &'a mut Vec<Notice>,
    ) -> Flow<'a, 'env> {
        let mut tree = TaffyTree::new();
        // Slide units aren't pixels. Rounding would also leave text a sliver narrower than it was measured at.
        tree.disable_rounding();
        Flow { tree, region, theme, env, notices, nesting: 0, text_tag_style: TextStyle::default() }
    }

    /// Lays out the tree under `root`, `width` wide, and returns its drawing.
    pub(crate) fn finish(mut self, root: NodeId, width: f32) -> Vec<DisplayItem> {
        let available = Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent };
        let fonts = &mut *self.env.fonts;
        self.tree
            .compute_layout_with_measure(root, available, |inputs, _, content, style| {
                compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| {
                    content.map_or(Size::ZERO, |content| content.measure(known, available, fonts))
                })
            })
            .expect("every node is in the tree");

        let mut items = Vec::new();
        self.draw(root, (0.0, 0.0), &mut items);
        items
    }

    pub(crate) fn node(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        self.tree.new_with_children(style, children).expect("every child is in the tree")
    }

    pub(crate) fn node_with_content(&mut self, style: Style, children: &[NodeId], content: Content) -> NodeId {
        let node = self.node(style, children);
        self.tree.set_node_context(node, Some(content)).expect("the node was just added");
        node
    }

    pub(crate) fn leaf(&mut self, style: Style, content: Content) -> NodeId {
        self.tree.new_leaf_with_context(style, content).expect("a new leaf can always be added")
    }

    /// Says `text` to the user, once however often it comes up.
    pub(crate) fn notice(&mut self, text: String) {
        let notice = Notice(text);
        if !self.notices.contains(&notice) {
            self.notices.push(notice);
        }
    }

    fn region_look(&self) -> TextLook {
        TextLook { size: self.region.size, bold: self.region.bold }
    }

    fn heading_look(&self, level: u8) -> TextLook {
        if self.region.name == "title" {
            return self.region_look();
        }
        let size = match level {
            1 => 64.0,
            2 => 48.0,
            3 => 40.0,
            _ => self.region.size,
        };
        TextLook { size, bold: true }
    }

    /// Runs `draw`, with what's drawn styled by `tag` if it's a `<Text>` tag.
    pub(crate) fn with_text_style_of<T>(&mut self, tag: &Tag, draw: impl FnOnce(&mut Self) -> T) -> T {
        if tag.name != TEXT_TAG {
            return draw(self);
        }
        let outer = self.text_tag_style.clone();
        for attr in &tag.attrs {
            self.text_tag_style.apply_text_attr(&attr.name, &attr.value);
        }
        let drawn = draw(self);
        self.text_tag_style = outer;
        drawn
    }

    /// A span's own style wins over the `<Text>` tags around it, as it does inside an inline one.
    fn run(&self, span: &Span, look: TextLook) -> TextRun {
        let style = &span.style;
        let tag = &self.text_tag_style;
        let color = if style.link.is_some() {
            self.theme.accent
        } else {
            style.color.as_deref().or(tag.color.as_deref()).and_then(Color::parse).unwrap_or(self.theme.text)
        };
        TextRun {
            text: span.text.clone(),
            size: style.size.or(tag.size).map_or(look.size, |size| size as f32),
            bold: look.bold || style.bold || tag.bold,
            italic: style.italic || tag.italic,
            underline: style.underline || tag.underline || style.link.is_some(),
            color,
            family: if style.code { self.theme.mono_font.clone() } else { self.theme.font.clone() },
        }
    }

    fn text(&mut self, spans: &[Span], look: TextLook, align: Align) -> NodeId {
        let runs = spans.iter().map(|span| self.run(span, look)).collect();
        self.leaf(Style::default(), Content::Text(Leaf { runs, align, wraps: true, frame: Frame::None }))
    }

    /// Text from a component. Whatever it leaves out comes from the region.
    pub(crate) fn plain_text(&mut self, text: &str, size: Option<f32>, bold: bool, color: Option<Color>, align: Align) -> NodeId {
        let look = TextLook { size: size.unwrap_or(self.region.size), bold: bold || self.region.bold };
        let mut run = self.run(&Span { text: text.to_string(), style: Default::default() }, look);
        run.color = color.unwrap_or(run.color);
        run.size = size.unwrap_or(run.size);
        self.leaf(Style::default(), Content::Text(Leaf { runs: vec![run], align, wraps: true, frame: Frame::None }))
    }

    /// The node for one block, if it's a block this flow draws.
    pub(crate) fn block(&mut self, block: &Block, align: Align) -> Option<NodeId> {
        if self.nesting == MAX_NESTING {
            self.notice("Some content is nested too deeply to show".to_string());
            return Some(self.block_placeholder("Nested too deeply".to_string()));
        }
        self.nesting += 1;
        let node = match block {
            Block::Heading { level, spans, .. } => Some(self.text(spans, self.heading_look(*level), align)),
            Block::Paragraph { spans, .. } => Some(self.text(spans, self.region_look(), align)),
            Block::List { ordered, items, .. } => Some(self.list(*ordered, items)),
            Block::Table { rows, .. } => Some(self.table(rows, align)),
            Block::Code { language, code, .. } => Some(self.code(language.as_deref(), code)),
            Block::Picture { src, .. } => Some(self.picture(src, 0.0, 0.0, align, false)),
            Block::Tag(tag) => self.tag(tag, align),
            Block::Region { .. } => None,
        };
        self.nesting -= 1;
        node
    }

    /// Each item is a row of its marker, in the indent, and a column of its blocks.
    /// A list always reads from the left, whatever the region's alignment.
    fn list(&mut self, ordered: bool, items: &[Vec<Block>]) -> NodeId {
        let markers: Vec<Leaf> = (1..=items.len())
            .map(|number| self.marker(if ordered { format!("{number}.") } else { "•".to_string() }))
            .collect();
        let widest = markers.iter().map(|marker| marker.shape(None, self.env.fonts).width).fold(0.0, f32::max);
        let indent = LIST_INDENT.max(widest + MARKER_GAP);
        let mut rows = Vec::with_capacity(items.len());
        for (marker, item) in markers.into_iter().zip(items) {
            let in_the_indent = Style {
                size: Size { width: length(indent), height: auto() },
                flex_shrink: 0.0,
                ..Style::default()
            };
            let marker = self.leaf(in_the_indent, Content::Text(marker));
            let blocks: Vec<NodeId> = item.iter().filter_map(|block| self.block(block, Align::Left)).collect();
            let content = Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size { width: length(0.0), height: auto() },
                ..column()
            };
            let content = self.node(content, &blocks);
            rows.push(self.node(Style::default(), &[marker, content]));
        }
        self.node(column(), &rows)
    }

    fn marker(&self, label: String) -> Leaf {
        let span = Span { text: label, style: Default::default() };
        let run = self.run(&span, self.region_look());
        Leaf { runs: vec![run], align: Align::Left, wraps: false, frame: Frame::None }
    }

    /// Columns share the width evenly. Cells are placed by number, so a row with too
    /// few cells leaves the end of the row empty.
    fn table(&mut self, rows: &[Vec<Vec<Span>>], align: Align) -> NodeId {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut cells = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            let look = TextLook { bold: r == 0 || self.region.bold, ..self.region_look() };
            for (c, spans) in row.iter().enumerate() {
                let style = Style {
                    grid_row: line(r as i16 + 1),
                    grid_column: line(c as i16 + 1),
                    padding: length(CELL_PADDING),
                    ..Style::default()
                };
                let runs = spans.iter().map(|span| self.run(span, look)).collect();
                cells.push(self.leaf(style, Content::Text(Leaf { runs, align, wraps: true, frame: Frame::Outline })));
            }
        }
        let style = Style {
            display: Display::Grid,
            grid_template_columns: evenly_sized_tracks(columns as u16),
            // Room for the last row's and last column's outlines, which sit just past their cells.
            padding: taffy::Rect { left: zero(), top: zero(), right: length(OUTLINE), bottom: length(OUTLINE) },
            ..Style::default()
        };
        self.node(style, &cells)
    }

    fn code(&mut self, language: Option<&str>, code: &str) -> NodeId {
        let tag = &self.text_tag_style;
        let tag_color = tag.color.as_deref().and_then(Color::parse);
        let runs = highlight(code, language)
            .into_iter()
            .flatten()
            .map(|(color, text)| TextRun {
                text,
                size: tag.size.map_or(CODE_SIZE, |size| size as f32),
                bold: tag.bold,
                italic: tag.italic,
                underline: tag.underline,
                color: tag_color.unwrap_or(color),
                family: self.theme.mono_font.clone(),
            })
            .collect();
        let style = Style { padding: length(CODE_PADDING), ..Style::default() };
        self.leaf(style, Content::Text(Leaf { runs, align: Align::Left, wraps: true, frame: Frame::Panel }))
    }

    /// Adds what `node` and its children draw. `parent` is the corner of `node`'s parent.
    fn draw(&mut self, node: NodeId, parent: (f32, f32), items: &mut Vec<DisplayItem>) {
        let layout = *self.tree.layout(node).expect("every node is in the tree");
        let (x, y) = (parent.0 + layout.location.x, parent.1 + layout.location.y);
        let (w, h) = (layout.size.width, layout.size.height);
        match self.tree.get_node_context(node) {
            Some(Content::Text(leaf)) => {
                items.extend(self.frame(leaf.frame, x, y, layout.size));
                let text = leaf.shape(Some(layout.content_box_width()), self.env.fonts);
                for notice in &text.notices {
                    self.notice(notice.0.clone());
                }
                items.push(DisplayItem::Text { text, x: x + layout.padding.left, y: y + layout.padding.top });
            }
            Some(Content::Shape(look)) => items.extend(shape_items(look, x, y, w, h, self.theme)),
            Some(Content::Picture(picture)) if w > 0.0 && h > 0.0 => {
                items.push(DisplayItem::Picture { pixmap: picture.clone(), rect: Rect { x, y, w, h } });
            }
            Some(Content::Placeholder(label)) => {
                items.extend(placeholder_items(label, Rect { x, y, w, h }, self.theme, self.env.fonts));
            }
            _ => {}
        }
        for child in self.tree.children(node).expect("every node is in the tree") {
            self.draw(child, (x, y), items);
        }
    }

    fn frame(&self, frame: Frame, x: f32, y: f32, size: Size<f32>) -> Option<DisplayItem> {
        match frame {
            Frame::None => None,
            Frame::Panel => {
                let path = rounded_rect(x, y, size.width, size.height, CODE_RADIUS)?;
                Some(DisplayItem::Fill { path, color: CODE_PANEL })
            }
            Frame::Outline => {
                // Half a unit along, so at scale 1 the line covers whole pixels and
                // neighbouring cells draw their shared edge in the same place.
                let rect = tiny_skia::Rect::from_xywh(x + OUTLINE / 2.0, y + OUTLINE / 2.0, size.width, size.height)?;
                Some(DisplayItem::Stroke { path: PathBuilder::from_rect(rect), color: self.theme.text, width: OUTLINE })
            }
        }
    }
}

pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    rounded_rect_with(x, y, w, h, radius, |_| {})
}

/// A rounded rectangle with `along_bottom` adding to its outline, which runs right to
/// left along the bottom edge.
pub(crate) fn rounded_rect_with(x: f32, y: f32, w: f32, h: f32, radius: f32, along_bottom: impl FnOnce(&mut PathBuilder)) -> Option<Path> {
    let r = radius.min(w / 2.0).min(h / 2.0);
    let (right, bottom) = (x + w, y + h);
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(right - r, y);
    b.quad_to(right, y, right, y + r);
    b.line_to(right, bottom - r);
    b.quad_to(right, bottom, right - r, bottom);
    along_bottom(&mut b);
    b.line_to(x + r, bottom);
    b.quad_to(x, bottom, x, bottom - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}
