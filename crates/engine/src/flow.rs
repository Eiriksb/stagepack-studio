//! Lays a region's blocks out top to bottom and turns them into drawing.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use slides_format::{Block, CellSource, Span, Style as TextStyle, Tag};
use taffy::compute_leaf_layout;
use taffy::prelude::{
    AvailableSpace, Display, FlexDirection, NodeId, Size, Style, TaffyTree, auto, evenly_sized_tracks, length, line, zero,
};
use tiny_skia::{Path, PathBuilder, Pixmap};
use toolkit::{Color, Theme};

use crate::elements::{PathLook, ShapeLook, path_items, shape_items};
use crate::native::{MAX_TABLE_CELLS, Semantic, native_elements};
use crate::placeholder::placeholder_items;
use crate::text_geometry::{DrawnBlock, DrawnTable, DrawnText, LayerText};
use slides_components::shortened;
use crate::{
    Align, Bullet, DisplayItem, Fonts, LayoutEnv, NativeElement, NativeParagraph, NativeRun, Notice, Rect, RegionDef, ShapedText, TextRun,
    highlight,
};
use crate::text::shape_text_with;

pub(crate) const GAP: f32 = 24.0;
/// The least a list is indented, however narrow its markers.
const LIST_INDENT: f32 = 48.0;
/// The least space between a list's widest marker and its text.
const MARKER_GAP: f32 = 12.0;
const CELL_PADDING: f32 = 12.0;
const OUTLINE: f32 = 1.0;
const CODE_SIZE: f32 = 32.0;
pub(crate) const CODE_PADDING: f32 = 24.0;
const CODE_RADIUS: f32 = 12.0;
pub(crate) const CODE_PANEL: Color = Color::rgb(0xf1, 0xf5, 0xf9);
/// How far lists, tags and a component's elements can sit inside each other, all counted
/// together. A talk could nest them as deep as it likes, and laying out each level uses up
/// more of the stack.
const MAX_DEPTH: u32 = 64;
const NESTED_TOO_DEEPLY: &str = "Some content is nested too deeply to show";
const TOO_MUCH: &str = "This slide has too much on it to show";
/// The most characters a notice keeps, as many are made from a talk's or a component's text.
pub(crate) const MAX_NOTICE_CHARS: usize = 200;
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
    Path(PathLook),
    Placeholder(String),
}

impl Content {
    fn measure(&self, known: Size<Option<f32>>, available: Size<AvailableSpace>, fonts: &mut Fonts) -> Size<f32> {
        match self {
            Content::Text(leaf) => leaf.measure(available, fonts),
            Content::Picture(picture) => measure_picture(picture, known, available),
            Content::Shape(_) | Content::Path(_) | Content::Placeholder(_) => Size::ZERO,
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
    /// Whether a line break at the end of the text is drawn as an empty last line, as it is
    /// at the end of a heading or a paragraph.
    end_line: bool,
    frame: Frame,
}

impl Leaf {
    fn shape(&self, width: Option<f32>, fonts: &mut Fonts) -> ShapedText {
        shape_text_with(fonts, &self.runs, width.filter(|_| self.wraps), self.align, self.end_line)
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

/// What a text leaf is of the slide's text: the block it draws and that block's content,
/// with the size its text takes before its spans have their say.
struct ParaKey {
    range: Range<usize>,
    inline: Range<usize>,
    size: f32,
    /// Whether it's a table's cell.
    cell: bool,
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
    /// What the tree's nodes are, for export.
    semantics: HashMap<NodeId, Semantic>,
    region: &'a RegionDef,
    /// How wide the flow is, which a component is told as the room it has.
    pub(crate) width: f32,
    pub(crate) theme: &'a Theme,
    pub(crate) env: &'a mut LayoutEnv<'env>,
    notices: &'a mut Vec<Notice>,
    /// How deep the block or element being laid out is, counting both.
    depth: u32,
    /// Whether the tag being laid out has already placed its content.
    children_placed: bool,
    /// Whether the slide had too much on it, so nothing more is laid out in this layer.
    stopped: bool,
    /// What the `<Text>` tags around the current block ask of the text in it.
    text_tag_style: TextStyle,
    /// The alignment a positioned tag asks for, which wins over a shape's centring.
    pub(crate) box_align: Option<Align>,
    /// The text leaves that are a heading's or a paragraph's text.
    paras: HashMap<NodeId, ParaKey>,
    /// The content of each empty list item, by the item's bytes.
    empty_items: HashMap<NodeId, Range<usize>>,
    /// The node of each block, by its bytes.
    blocks: HashMap<NodeId, Range<usize>>,
    /// The cells of each table, row by row, by the table's node, with the table's bytes.
    tables: HashMap<NodeId, (Range<usize>, Vec<Vec<NodeId>>)>,
    /// Where `draw` found those, for the editor.
    drawn: LayerText,
}

/// A layer's drawing, what it is for export, and where its text is.
pub(crate) struct Drawing {
    pub items: Vec<DisplayItem>,
    pub native: Vec<NativeElement>,
    pub text: LayerText,
}

/// A column with the gap between blocks.
pub(crate) fn column() -> Style {
    Style {
        flex_direction: FlexDirection::Column,
        gap: Size { width: zero(), height: length(GAP) },
        ..Style::default()
    }
}

/// Lays `blocks` out in a column `width` wide and returns their drawing and what they
/// are, in layer coordinates. The column is as tall as its content, which can be taller
/// than the region.
pub(crate) fn flow_region(
    blocks: &[&Block],
    region: &RegionDef,
    width: f32,
    theme: &Theme,
    env: &mut LayoutEnv,
    notices: &mut Vec<Notice>,
) -> Drawing {
    let mut flow = Flow::new(region, width, theme, env, notices);
    let children: Vec<NodeId> = blocks.iter().filter_map(|block| flow.block(block, region.align)).collect();
    let root = flow.node(Style { size: Size { width: length(width), height: auto() }, ..column() }, &children);
    flow.finish(root, width)
}

/// One block, or one item of a list, drawn on its own: `rect` is its box in the region's
/// coordinates, and the items and native elements are relative to that box.
pub(crate) struct Piece {
    pub rect: Rect,
    pub source: Range<usize>,
    pub items: Vec<DisplayItem>,
    pub native: Vec<NativeElement>,
    pub text: LayerText,
}

/// As `flow_region`, laid out the same way, but each top-level block and each item of a
/// top-level list is drawn on its own.
pub(crate) fn flow_region_each(
    blocks: &[&Block],
    region: &RegionDef,
    width: f32,
    theme: &Theme,
    env: &mut LayoutEnv,
    notices: &mut Vec<Notice>,
) -> Vec<Piece> {
    let mut flow = Flow::new(region, width, theme, env, notices);
    let placed: Vec<(&Block, NodeId)> =
        blocks.iter().filter_map(|block| flow.block(block, region.align).map(|node| (*block, node))).collect();
    let children: Vec<NodeId> = placed.iter().map(|(_, node)| *node).collect();
    let root = flow.node(Style { size: Size { width: length(width), height: auto() }, ..column() }, &children);
    flow.compute(root, width);

    let mut pieces = Vec::new();
    for (block, node) in placed {
        match block {
            Block::List { items, .. } => {
                let rows = flow.tree.children(node).expect("every node is in the tree");
                let list_at = flow.tree.layout(node).expect("every node is in the tree").location;
                for (row, item) in rows.into_iter().zip(items) {
                    let source = match (item.first(), item.last()) {
                        (Some(first), Some(last)) => first.range().start..last.range().end,
                        _ => block.range().clone(),
                    };
                    pieces.push(flow.piece(row, (list_at.x, list_at.y), source));
                }
            }
            _ => pieces.push(flow.piece(node, (0.0, 0.0), block.range().clone())),
        }
    }
    pieces
}

impl<'a, 'env> Flow<'a, 'env> {
    /// `node` drawn on its own. `parent` is the corner of its parent.
    fn piece(&mut self, node: NodeId, parent: (f32, f32), source: Range<usize>) -> Piece {
        let layout = *self.tree.layout(node).expect("every node is in the tree");
        let (x, y) = (parent.0 + layout.location.x, parent.1 + layout.location.y);
        let inside = (parent.0 - x, parent.1 - y);
        let mut items = Vec::new();
        self.draw(node, inside, &mut items);
        let native = self.native(node, inside);
        let text = std::mem::take(&mut self.drawn);
        Piece { rect: Rect { x, y, w: layout.size.width, h: layout.size.height }, source, items, native, text }
    }

    pub(crate) fn new(
        region: &'a RegionDef,
        width: f32,
        theme: &'a Theme,
        env: &'a mut LayoutEnv<'env>,
        notices: &'a mut Vec<Notice>,
    ) -> Flow<'a, 'env> {
        let mut tree = TaffyTree::new();
        // Slide units aren't pixels. Rounding would also leave text a sliver narrower than it was measured at.
        tree.disable_rounding();
        Flow {
            tree,
            semantics: HashMap::new(),
            region,
            width,
            theme,
            env,
            notices,
            depth: 0,
            children_placed: false,
            stopped: false,
            text_tag_style: TextStyle::default(),
            box_align: None,
            paras: HashMap::new(),
            empty_items: HashMap::new(),
            blocks: HashMap::new(),
            tables: HashMap::new(),
            drawn: LayerText::default(),
        }
    }

    /// Lays out the tree under `root`, `width` wide, and returns its drawing, what it is and
    /// where its text is.
    pub(crate) fn finish(mut self, root: NodeId, width: f32) -> Drawing {
        self.compute(root, width);
        let mut items = Vec::new();
        self.draw(root, (0.0, 0.0), &mut items);
        let native = self.native(root, (0.0, 0.0));
        Drawing { items, native, text: self.drawn }
    }

    /// What `node` and its children are, for export. Call it once, after laying out.
    fn native(&mut self, node: NodeId, parent: (f32, f32)) -> Vec<NativeElement> {
        native_elements(&self.tree, &mut self.semantics, self.theme, node, parent)
    }

    /// Notes what `node` is, for export.
    pub(crate) fn note(&mut self, node: NodeId, semantic: Semantic) {
        self.semantics.insert(node, semantic);
    }

    fn compute(&mut self, root: NodeId, width: f32) {
        let available = Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent };
        let fonts = &mut *self.env.fonts;
        self.tree
            .compute_layout_with_measure(root, available, |inputs, _, content, style| {
                compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| {
                    content.map_or(Size::ZERO, |content| content.measure(known, available, fonts))
                })
            })
            .expect("every node is in the tree");
    }

    pub(crate) fn node(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        self.env.components.count_node();
        self.tree.new_with_children(style, children).expect("every child is in the tree")
    }

    pub(crate) fn node_with_content(&mut self, style: Style, children: &[NodeId], content: Content) -> NodeId {
        let node = self.node(style, children);
        self.tree.set_node_context(node, Some(content)).expect("the node was just added");
        node
    }

    pub(crate) fn leaf(&mut self, style: Style, content: Content) -> NodeId {
        self.env.components.count_node();
        self.tree.new_leaf_with_context(style, content).expect("a new leaf can always be added")
    }

    /// Whether the slide has made more nodes than it can have.
    pub(crate) fn over_budget(&self) -> bool {
        self.env.components.too_many_nodes()
    }

    /// The box that takes the place of a block once the slide has too much on it, after
    /// which nothing more is laid out in this layer.
    pub(crate) fn too_much(&mut self) -> NodeId {
        self.stopped = true;
        self.notice(TOO_MUCH.to_string());
        self.block_placeholder(TOO_MUCH.to_string())
    }

    /// The box for something nested deeper than the limit.
    pub(crate) fn nested_too_deeply(&mut self, size: Size<taffy::prelude::Dimension>) -> NodeId {
        self.notice(NESTED_TOO_DEEPLY.to_string());
        self.placeholder("Nested too deeply".to_string(), size)
    }

    /// Runs `lay_out` one level deeper, or gives `None` past the limit.
    pub(crate) fn deeper<T>(&mut self, lay_out: impl FnOnce(&mut Self) -> T) -> Option<T> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let laid = lay_out(self);
        self.depth -= 1;
        Some(laid)
    }

    /// Runs `lay_out` for a tag, whose content can be placed once.
    pub(crate) fn in_tag<T>(&mut self, lay_out: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.children_placed, false);
        let laid = lay_out(self);
        self.children_placed = outer;
        laid
    }

    /// Whether the tag being laid out can still place its content, which it can do once.
    pub(crate) fn place_children(&mut self) -> bool {
        !std::mem::replace(&mut self.children_placed, true)
    }

    /// Says `text` to the user, once however often it comes up.
    pub(crate) fn notice(&mut self, text: String) {
        let notice = Notice(shortened(&text, MAX_NOTICE_CHARS));
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
            // Line endings as the text model has them, so that a caret's offset is the drawn text's.
            text: if span.text.contains('\r') { span.text.replace("\r\n", "\n").replace('\r', "\n") } else { span.text.clone() },
            size: style.size.or(tag.size).map_or(look.size, |size| size as f32),
            bold: look.bold || style.bold || tag.bold,
            italic: style.italic || tag.italic,
            underline: style.underline || tag.underline || style.link.is_some(),
            color,
            family: if style.code { self.theme.mono_font.clone() } else { self.theme.font.clone() },
        }
    }

    /// The runs of `spans`, for drawing and for export.
    fn runs(&self, spans: &[Span], look: TextLook) -> (Vec<TextRun>, Vec<NativeRun>) {
        spans
            .iter()
            .map(|span| {
                let run = self.run(span, look);
                let native = NativeRun::new(&run, span.style.link.clone());
                (run, native)
            })
            .unzip()
    }

    /// A heading's or a paragraph's text. One with nothing in it is still a line of its
    /// look's height, and a break at its end is an empty last line.
    fn text(&mut self, spans: &[Span], look: TextLook, align: Align) -> NodeId {
        let (mut runs, mut native) = self.runs(spans, look);
        if runs.iter().all(|run| run.text.is_empty()) {
            let run = self.run(&Span { text: String::new(), style: Default::default() }, look);
            native = vec![NativeRun::new(&run, None)];
            runs = vec![run];
        }
        let node = self.leaf(Style::default(), Content::Text(Leaf { runs, align, wraps: true, end_line: true, frame: Frame::None }));
        self.note(node, Semantic::Paragraph(NativeParagraph::new(native, align)));
        node
    }

    /// How big the text of `node`, a leaf of text, is on lines `width` wide, or on one line with no
    /// width. A node that isn't text has no size.
    pub(crate) fn text_size(&mut self, node: NodeId, width: Option<f32>) -> (f32, f32) {
        let fonts = &mut *self.env.fonts;
        match self.tree.get_node_context(node) {
            Some(Content::Text(leaf)) => {
                let text = leaf.shape(width, fonts);
                (text.width, text.height)
            }
            _ => (0.0, 0.0),
        }
    }

    /// Text from a component. Whatever it leaves out comes from the region.
    pub(crate) fn plain_text(&mut self, text: &str, size: Option<f32>, bold: bool, color: Option<Color>, align: Align) -> NodeId {
        let look = TextLook { size: size.unwrap_or(self.region.size), bold: bold || self.region.bold };
        let mut run = self.run(&Span { text: text.to_string(), style: Default::default() }, look);
        run.color = color.unwrap_or(run.color);
        run.size = size.unwrap_or(run.size);
        let paragraph = NativeParagraph::new(vec![NativeRun::new(&run, None)], align);
        let node = self.leaf(Style::default(), Content::Text(Leaf { runs: vec![run], align, wraps: true, end_line: false, frame: Frame::None }));
        self.note(node, Semantic::Paragraph(paragraph));
        node
    }

    /// The node for one block, if it's a block this flow draws.
    /// Once the slide has too much on it, the top-level block being laid out becomes a box
    /// saying so, and nothing more is laid out in its layer.
    pub(crate) fn block(&mut self, block: &Block, align: Align) -> Option<NodeId> {
        if self.stopped {
            return None;
        }
        let top = self.depth == 0;
        if self.over_budget() {
            return top.then(|| self.too_much());
        }
        let laid = self.deeper(|flow| flow.block_node(block, align));
        let Some(node) = laid else {
            return Some(self.nested_too_deeply(crate::elements::size_when_empty(0.0, 0.0, false)));
        };
        if top && self.over_budget() {
            return Some(self.too_much());
        }
        if let Some(node) = node {
            self.blocks.insert(node, block.range().clone());
        }
        node
    }

    fn block_node(&mut self, block: &Block, align: Align) -> Option<NodeId> {
        match block {
            Block::Heading { level, spans, range, inline, .. } => Some(self.para(spans, self.heading_look(*level), align, range, inline)),
            Block::Paragraph { spans, range, inline, .. } => Some(self.para(spans, self.region_look(), align, range, inline)),
            Block::List { ordered, items, item_ranges, .. } => Some(self.list(*ordered, items, item_ranges)),
            Block::Table { rows, cells, range, .. } => Some(self.table(rows, cells, range, align)),
            Block::Code { language, code, .. } => Some(self.code(language.as_deref(), code)),
            Block::Picture { src, alt, .. } => Some(self.picture(src, alt, 0.0, 0.0, align, false)),
            Block::Tag(tag) => self.tag(tag, align),
            Block::Region { .. } => None,
        }
    }

    /// A heading's or a paragraph's text, noted as the text of the block at `range`.
    fn para(&mut self, spans: &[Span], look: TextLook, align: Align, range: &Range<usize>, inline: &Range<usize>) -> NodeId {
        let node = self.text(spans, look, align);
        self.paras.insert(node, ParaKey { range: range.clone(), inline: inline.clone(), size: look.size, cell: false });
        node
    }

    /// Each item is a row of its marker, in the indent, and a column of its blocks.
    /// A list always reads from the left, whatever the region's alignment.
    fn list(&mut self, ordered: bool, items: &[Vec<Block>], item_ranges: &[Range<usize>]) -> NodeId {
        let markers: Vec<Leaf> = (1..=items.len())
            .map(|number| self.marker(if ordered { format!("{number}.") } else { "•".to_string() }))
            .collect();
        let widest = markers.iter().map(|marker| marker.shape(None, self.env.fonts).width).fold(0.0, f32::max);
        let indent = LIST_INDENT.max(widest + MARKER_GAP);
        let mut rows = Vec::with_capacity(items.len());
        for (index, (marker, item)) in markers.into_iter().zip(items).enumerate() {
            if self.over_budget() {
                break;
            }
            let in_the_indent = Style {
                size: Size { width: length(indent), height: auto() },
                flex_shrink: 0.0,
                ..Style::default()
            };
            let color = marker.runs[0].color;
            let marker = self.leaf(in_the_indent, Content::Text(marker));
            let blocks: Vec<NodeId> = item.iter().filter_map(|block| self.block(block, Align::Left)).collect();
            let content = Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size { width: length(0.0), height: auto() },
                ..column()
            };
            let content = self.node(content, &blocks);
            if let Some(range) = item_ranges.get(index).filter(|_| item.is_empty()) {
                self.empty_items.insert(content, range.clone());
            }
            let row = self.node(Style::default(), &[marker, content]);
            self.note(row, Semantic::ListItem(if ordered { Bullet::Number(index as u32 + 1) } else { Bullet::Dot }, color));
            rows.push(row);
        }
        self.node(column(), &rows)
    }

    fn marker(&self, label: String) -> Leaf {
        let span = Span { text: label, style: Default::default() };
        let run = self.run(&span, self.region_look());
        Leaf { runs: vec![run], align: Align::Left, wraps: false, end_line: false, frame: Frame::None }
    }

    /// Columns share the width evenly. Cells are placed by number, so a row with too
    /// few cells leaves the end of the row empty. Each cell's text is noted as the text of its
    /// bytes, `sources` beside `rows`, for the editor.
    fn table(&mut self, rows: &[Vec<Vec<Span>>], sources: &[Vec<CellSource>], range: &Range<usize>, align: Align) -> NodeId {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut cells = Vec::new();
        let mut grid = Vec::with_capacity(rows.len());
        let mut native_rows = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let look = TextLook { bold: r == 0 || self.region.bold, ..self.region_look() };
            let mut native_row = Vec::with_capacity(row.len());
            if self.over_budget() {
                break;
            }
            for (c, spans) in row.iter().enumerate() {
                let style = Style {
                    grid_row: line(r as i16 + 1),
                    grid_column: line(c as i16 + 1),
                    padding: length(CELL_PADDING),
                    ..Style::default()
                };
                let (runs, native) = self.runs(spans, look);
                let cell = self.leaf(style, Content::Text(Leaf { runs, align, wraps: true, end_line: false, frame: Frame::Outline }));
                if let Some(source) = sources.get(r).and_then(|row| row.get(c)) {
                    self.paras.insert(cell, ParaKey { range: source.range.clone(), inline: source.inline.clone(), size: look.size, cell: true });
                }
                native_row.push((cell, NativeParagraph::new(native, align)));
                cells.push(cell);
            }
            grid.push(native_row.iter().map(|(cell, _)| *cell).collect());
            native_rows.push(native_row);
        }
        let style = Style {
            display: Display::Grid,
            grid_template_columns: evenly_sized_tracks(columns as u16),
            // Room for the last row's and last column's outlines, which sit just past their cells.
            padding: taffy::Rect { left: zero(), top: zero(), right: length(OUTLINE), bottom: length(OUTLINE) },
            ..Style::default()
        };
        let table = self.node(style, &cells);
        self.tables.insert(table, (range.clone(), grid));
        if rows.len() * columns > MAX_TABLE_CELLS {
            self.notice("This table is too big to export, so it's shown as text".to_string());
            self.note(table, Semantic::TableTooBig);
        } else {
            self.note(table, Semantic::Table(native_rows));
        }
        table
    }

    fn code(&mut self, language: Option<&str>, code: &str) -> NodeId {
        let tag = &self.text_tag_style;
        let tag_color = tag.color.as_deref().and_then(Color::parse);
        let mut runs = Vec::new();
        let mut lines = Vec::new();
        for line in highlight(code, language) {
            let mut native = Vec::with_capacity(line.len());
            for (color, text) in line {
                let run = TextRun {
                    text,
                    size: tag.size.map_or(CODE_SIZE, |size| size as f32),
                    bold: tag.bold,
                    italic: tag.italic,
                    underline: tag.underline,
                    color: tag_color.unwrap_or(color),
                    family: self.theme.mono_font.clone(),
                };
                native.push(NativeRun::new(&run, None));
                runs.push(run);
            }
            lines.push(NativeParagraph::new(without_line_ending(native), Align::Left));
        }
        let style = Style { padding: length(CODE_PADDING), ..Style::default() };
        let node = self.leaf(style, Content::Text(Leaf { runs, align: Align::Left, wraps: true, end_line: false, frame: Frame::Panel }));
        self.note(node, Semantic::Code(lines));
        node
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
                let align = leaf.align;
                let drawn: String = leaf.runs.iter().map(|run| run.text.as_str()).collect();
                for notice in &text.notices {
                    self.notice(notice.0.clone());
                }
                if let Some(key) = self.paras.get(&node) {
                    let rect = Rect { x: x + layout.padding.left, y: y + layout.padding.top, w: layout.content_box_width(), h: layout.content_box_height() };
                    let (range, inline, size) = (key.range.clone(), key.inline.clone(), key.size);
                    let cell = key.cell.then_some(Rect { x, y, w, h });
                    self.drawn.texts.push(DrawnText { range, inline, rect, size, align, shaped: Some((drawn, text.buffer.clone())), cell });
                }
                items.push(DisplayItem::Text { text, x: x + layout.padding.left, y: y + layout.padding.top });
            }
            Some(Content::Shape(look)) => items.extend(shape_items(look, x, y, w, h, self.theme)),
            Some(Content::Path(look)) => items.extend(path_items(look, x, y)),
            Some(Content::Picture(picture)) if w > 0.0 && h > 0.0 => {
                let src = match self.semantics.get(&node) {
                    Some(Semantic::File { src, .. }) => Some(src.clone()),
                    _ => None,
                };
                items.push(DisplayItem::Picture { pixmap: picture.clone(), src, rect: Rect { x, y, w, h } });
            }
            Some(Content::Placeholder(label)) => {
                items.extend(placeholder_items(label, Rect { x, y, w, h }, self.theme, self.env.fonts));
            }
            _ => {}
        }
        if let Some(range) = self.blocks.get(&node) {
            self.drawn.blocks.push(DrawnBlock { range: range.clone(), rect: Rect { x, y, w, h } });
        }
        if let Some((range, grid)) = self.tables.get(&node) {
            let cells = grid
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| {
                            let cell = self.tree.layout(*cell).expect("every node is in the tree");
                            Rect { x: x + cell.location.x, y: y + cell.location.y, w: cell.size.width, h: cell.size.height }
                        })
                        .collect()
                })
                .collect();
            self.drawn.tables.push(DrawnTable { range: range.clone(), cells });
        }
        if let Some(range) = self.empty_items.get(&node) {
            let inline = range.end..range.end;
            let rect = Rect { x, y, w, h };
            let size = self.region.size;
            self.drawn.texts.push(DrawnText { range: range.clone(), inline, rect, size, align: Align::Left, shaped: None, cell: None });
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

/// One line of code's runs, with its line ending gone. An empty line keeps one empty run,
/// so that it stays as tall as the others.
fn without_line_ending(mut runs: Vec<NativeRun>) -> Vec<NativeRun> {
    while let Some(last) = runs.last_mut() {
        last.text.truncate(last.text.trim_end_matches(['\r', '\n']).len());
        if !last.text.is_empty() || runs.len() == 1 {
            break;
        }
        runs.pop();
    }
    runs
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
