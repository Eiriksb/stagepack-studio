//! Lays a region's blocks out top to bottom and turns them into drawing.

use slides_format::{Block, Span};
use taffy::compute_leaf_layout;
use taffy::prelude::{
    AvailableSpace, Display, FlexDirection, NodeId, Size, Style, TaffyTree, auto, evenly_sized_tracks, length, line, zero,
};
use tiny_skia::PathBuilder;
use toolkit::{Color, Theme};

use crate::{Align, DisplayItem, Fonts, Notice, RegionDef, ShapedText, TextRun, highlight, shape_text};

const GAP: f32 = 24.0;
const LIST_INDENT: f32 = 48.0;
const CELL_PADDING: f32 = 12.0;
const OUTLINE: f32 = 1.0;
const CODE_SIZE: f32 = 32.0;
const CODE_PADDING: f32 = 24.0;
const CODE_RADIUS: f32 = 12.0;
const CODE_PANEL: Color = Color::rgb(0xf1, 0xf5, 0xf9);

/// What a leaf draws behind its text.
#[derive(Clone, Copy)]
enum Frame {
    None,
    Panel,
    Outline,
}

/// A piece of text, the only thing Taffy has to measure here.
struct Leaf {
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

struct Flow<'a> {
    tree: TaffyTree<Leaf>,
    region: &'a RegionDef,
    theme: &'a Theme,
}

/// A column with the gap between blocks.
fn column() -> Style {
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
    fonts: &mut Fonts,
    notices: &mut Vec<Notice>,
) -> Vec<DisplayItem> {
    let mut flow = Flow { tree: TaffyTree::new(), region, theme };
    // Slide units aren't pixels. Rounding would also leave text a sliver narrower than it was measured at.
    flow.tree.disable_rounding();
    let children: Vec<NodeId> = blocks.iter().filter_map(|block| flow.block(block, region.align)).collect();
    let root = flow.node(Style { size: Size { width: length(width), height: auto() }, ..column() }, &children);

    let available = Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent };
    flow.tree
        .compute_layout_with_measure(root, available, |inputs, _, leaf, style| {
            compute_leaf_layout(inputs, style, |_, _| 0.0, |_, available| {
                leaf.map_or(Size::ZERO, |leaf| leaf.measure(available, fonts))
            })
        })
        .expect("every node is in the tree");

    let mut items = Vec::new();
    flow.draw(root, (0.0, 0.0), fonts, &mut items, notices);
    items
}

impl Flow<'_> {
    fn node(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        self.tree.new_with_children(style, children).expect("every child is in the tree")
    }

    fn leaf(&mut self, style: Style, leaf: Leaf) -> NodeId {
        self.tree.new_leaf_with_context(style, leaf).expect("a new leaf can always be added")
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

    fn run(&self, span: &Span, look: TextLook) -> TextRun {
        let style = &span.style;
        let color = if style.link.is_some() {
            self.theme.accent
        } else {
            style.color.as_deref().and_then(Color::parse).unwrap_or(self.theme.text)
        };
        TextRun {
            text: span.text.clone(),
            size: style.size.map_or(look.size, |size| size as f32),
            bold: look.bold || style.bold,
            italic: style.italic,
            underline: style.underline || style.link.is_some(),
            color,
            family: if style.code { self.theme.mono_font.clone() } else { self.theme.font.clone() },
        }
    }

    fn text(&mut self, spans: &[Span], look: TextLook, align: Align) -> NodeId {
        let runs = spans.iter().map(|span| self.run(span, look)).collect();
        self.leaf(Style::default(), Leaf { runs, align, wraps: true, frame: Frame::None })
    }

    /// The node for one block, if it's a block this flow draws.
    fn block(&mut self, block: &Block, align: Align) -> Option<NodeId> {
        Some(match block {
            Block::Heading { level, spans, .. } => self.text(spans, self.heading_look(*level), align),
            Block::Paragraph { spans, .. } => self.text(spans, self.region_look(), align),
            Block::List { ordered, items, .. } => self.list(*ordered, items),
            Block::Table { rows, .. } => self.table(rows, align),
            Block::Code { language, code, .. } => self.code(language.as_deref(), code),
            Block::Picture { .. } | Block::Tag(_) | Block::Region { .. } => return None,
        })
    }

    /// Each item is a row of its marker, in the indent, and a column of its blocks.
    /// A list always reads from the left, whatever the region's alignment.
    fn list(&mut self, ordered: bool, items: &[Vec<Block>]) -> NodeId {
        let mut rows = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let label = if ordered { format!("{}.", index + 1) } else { "•".to_string() };
            let marker = self.marker(label);
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

    fn marker(&mut self, label: String) -> NodeId {
        let span = Span { text: label, style: Default::default() };
        let run = self.run(&span, self.region_look());
        let style = Style {
            size: Size { width: length(LIST_INDENT), height: auto() },
            flex_shrink: 0.0,
            ..Style::default()
        };
        self.leaf(style, Leaf { runs: vec![run], align: Align::Left, wraps: false, frame: Frame::None })
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
                cells.push(self.leaf(style, Leaf { runs, align, wraps: true, frame: Frame::Outline }));
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
        let runs = highlight(code, language)
            .into_iter()
            .flatten()
            .map(|(color, text)| TextRun {
                text,
                size: CODE_SIZE,
                bold: false,
                italic: false,
                underline: false,
                color,
                family: self.theme.mono_font.clone(),
            })
            .collect();
        let style = Style { padding: length(CODE_PADDING), ..Style::default() };
        self.leaf(style, Leaf { runs, align: Align::Left, wraps: true, frame: Frame::Panel })
    }

    /// Adds what `node` and its children draw. `parent` is the corner of `node`'s parent.
    fn draw(&self, node: NodeId, parent: (f32, f32), fonts: &mut Fonts, items: &mut Vec<DisplayItem>, notices: &mut Vec<Notice>) {
        let layout = self.tree.layout(node).expect("every node is in the tree");
        let (x, y) = (parent.0 + layout.location.x, parent.1 + layout.location.y);
        if let Some(leaf) = self.tree.get_node_context(node) {
            items.extend(self.frame(leaf.frame, x, y, layout.size));
            let text = leaf.shape(Some(layout.content_box_width()), fonts);
            for notice in &text.notices {
                if !notices.contains(notice) {
                    notices.push(notice.clone());
                }
            }
            items.push(DisplayItem::Text { text, x: x + layout.padding.left, y: y + layout.padding.top });
        }
        for child in self.tree.children(node).expect("every node is in the tree") {
            self.draw(child, (x, y), fonts, items, notices);
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

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<tiny_skia::Path> {
    let r = radius.min(w / 2.0).min(h / 2.0);
    let (right, bottom) = (x + w, y + h);
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(right - r, y);
    b.quad_to(right, y, right, y + r);
    b.line_to(right, bottom - r);
    b.quad_to(right, bottom, right - r, bottom);
    b.line_to(x + r, bottom);
    b.quad_to(x, bottom, x, bottom - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}
