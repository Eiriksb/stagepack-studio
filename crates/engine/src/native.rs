//! What a layer is made of, for export: text boxes, shapes, tables and pictures. It is read
//! from the layout tree the drawing comes from, so the export and the screen can't disagree.

use std::collections::HashMap;
use std::sync::Arc;

use taffy::prelude::{NodeId, TaffyTree};
use tiny_skia::Pixmap;
use toolkit::{Color, ShapeKind, Theme};

use crate::elements::ShapeLook;
use crate::flow::{CODE_PADDING, CODE_PANEL, Content};
use crate::placeholder::placeholder_run;
use crate::text::{LINE_HEIGHT, limited_size};
use crate::{Align, Rect, TextRun};

#[derive(Clone, Debug, PartialEq)]
pub struct NativeRun {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Color,
    pub family: String,
    pub link: Option<String>,
}

impl NativeRun {
    /// `run` as it's drawn, with the link of the span it came from.
    pub(crate) fn new(run: &TextRun, link: Option<String>) -> NativeRun {
        NativeRun {
            text: run.text.clone(),
            size: limited_size(run.size),
            bold: run.bold,
            italic: run.italic,
            underline: run.underline,
            color: run.color,
            family: run.family.clone(),
            link,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bullet {
    None,
    Dot,
    Number(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeParagraph {
    pub runs: Vec<NativeRun>,
    pub align: Align,
    pub level: u8,
    pub bullet: Bullet,
    /// From one line to the next, in slide units. With none, the program showing it decides.
    pub line_height: Option<f32>,
    /// The gap above it, in slide units. With none, the program showing it decides.
    pub space_before: Option<f32>,
    /// The colour of its marker. With none, a bullet takes the accent and a number its text's colour.
    pub bullet_color: Option<Color>,
}

impl NativeParagraph {
    /// A paragraph on its own, with lines as far apart as `shape_text` puts them.
    pub(crate) fn new(runs: Vec<NativeRun>, align: Align) -> NativeParagraph {
        let line_height = runs.iter().map(|run| run.size).reduce(f32::max).map(|biggest| biggest * LINE_HEIGHT);
        NativeParagraph { runs, align, level: 0, bullet: Bullet::None, line_height, space_before: None, bullet_color: None }
    }
}

#[derive(Clone)]
pub enum NativeContent {
    Text {
        paragraphs: Vec<NativeParagraph>,
        fill: Option<Color>,
        inset: f32,
    },
    Shape {
        kind: ShapeKind,
        fill: Option<Color>,
        outline: Option<Color>,
        outline_width: f32,
        radius: f32,
        paragraphs: Vec<NativeParagraph>,
        /// The space kept clear between the shape's edge and its text.
        inset: f32,
        /// Whether its text sits in the middle, rather than starting at the corner.
        centered: bool,
    },
    Table {
        rows: Vec<Vec<Vec<NativeParagraph>>>,
        column_widths: Vec<f32>,
        row_heights: Vec<f32>,
        line: Color,
    },
    Picture {
        src: Option<String>,
        alt: String,
        pixmap: Arc<Pixmap>,
    },
}

/// One thing in a layer. `rect` is relative to the layer's own rect.
#[derive(Clone)]
pub struct NativeElement {
    pub rect: Rect,
    pub content: NativeContent,
}

/// The most cells a table can have once its rows are filled out. A talk's short rows cost
/// it almost nothing to write, but each one filled out would take memory for every column.
pub(crate) const MAX_TABLE_CELLS: usize = 10_000;

/// What a node of a layout tree is, where what it draws doesn't say. The tree's builder
/// notes this as it goes, so that one layout gives both the drawing and the export.
pub(crate) enum Semantic {
    /// A heading, a paragraph or a component's text.
    Paragraph(NativeParagraph),
    /// A list item: a row of its marker and a column of its blocks, and the marker's colour.
    ListItem(Bullet, Color),
    /// A table's cells, row by row: each one's node and its text.
    Table(Vec<Vec<(NodeId, NativeParagraph)>>),
    /// A table with more cells than `MAX_TABLE_CELLS`.
    TableTooBig,
    /// A block of code, one paragraph per line.
    Code(Vec<NativeParagraph>),
    /// A stack from a component, whose children each stand alone.
    Stack,
    /// A picture from a file. A picture with no such note is an icon or a path.
    File { src: String, alt: String },
}

/// A placeholder's label, in `rect`, as it's drawn.
pub(crate) fn placeholder_element(label: &str, rect: Rect, theme: &Theme) -> NativeElement {
    let run = NativeRun::new(&placeholder_run(label, theme), None);
    let paragraphs = vec![NativeParagraph::new(vec![run], Align::Center)];
    NativeElement { rect, content: NativeContent::Text { paragraphs, fill: None, inset: 0.0 } }
}

/// What `node` and everything under it are, in drawing order. `parent` is the corner of
/// `node`'s parent, in the coordinates the elements' rects are to use.
pub(crate) fn native_elements(
    tree: &TaffyTree<Content>,
    semantics: &mut HashMap<NodeId, Semantic>,
    theme: &Theme,
    node: NodeId,
    parent: (f32, f32),
) -> Vec<NativeElement> {
    let mut walk = Walk { tree, semantics, theme, elements: Vec::new(), text: None, marker: None, lists: 0 };
    walk.visit(node, parent);
    walk.end_text();
    walk.elements
}

/// A list item's marker, until the text it belongs to comes along.
#[derive(Clone, Copy)]
struct Marker {
    bullet: Bullet,
    color: Color,
    rect: Rect,
}

/// Paragraphs on their way to being one text box.
struct RunningText {
    paragraphs: Vec<NativeParagraph>,
    around: Rect,
    /// Where the last paragraph's own text ends. Its marker can reach further, as a marker's
    /// row is as tall as everything nested in its item.
    bottom: f32,
}

/// One pass over a laid-out tree, the same way `Flow::draw` goes.
struct Walk<'a> {
    tree: &'a TaffyTree<Content>,
    semantics: &'a mut HashMap<NodeId, Semantic>,
    theme: &'a Theme,
    elements: Vec<NativeElement>,
    /// The text so far, until something ends the run.
    text: Option<RunningText>,
    marker: Option<Marker>,
    /// How many lists the current node is inside.
    lists: u8,
}

impl Walk<'_> {
    fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.tree.children(node).expect("every node is in the tree")
    }

    fn rect_of(&self, node: NodeId, parent: (f32, f32)) -> Rect {
        let layout = self.tree.layout(node).expect("every node is in the tree");
        Rect { x: parent.0 + layout.location.x, y: parent.1 + layout.location.y, w: layout.size.width, h: layout.size.height }
    }

    fn visit(&mut self, node: NodeId, parent: (f32, f32)) {
        let rect = self.rect_of(node, parent);
        let corner = (rect.x, rect.y);
        let tree = self.tree;
        match (self.semantics.remove(&node), tree.get_node_context(node)) {
            (Some(Semantic::Paragraph(paragraph)), _) => self.add_paragraph(paragraph, rect),
            (Some(Semantic::ListItem(bullet, color)), _) => self.list_item(node, corner, bullet, color),
            (Some(Semantic::Table(cells)), _) => self.table(cells, rect),
            (Some(Semantic::TableTooBig), _) => self.label("This table is too big to export", rect),
            (Some(Semantic::Code(paragraphs)), _) => {
                self.push(rect, NativeContent::Text { paragraphs, fill: Some(CODE_PANEL), inset: CODE_PADDING });
            }
            (Some(Semantic::Stack), _) => {
                for child in self.children(node) {
                    self.end_text();
                    self.visit(child, corner);
                }
                self.end_text();
            }
            (file, Some(Content::Picture(pixmap))) if rect.w > 0.0 && rect.h > 0.0 => {
                let (src, alt) = match file {
                    Some(Semantic::File { src, alt }) => (Some(src), alt),
                    _ => (None, String::new()),
                };
                self.push(rect, NativeContent::Picture { src, alt, pixmap: pixmap.clone() });
            }
            (_, Some(Content::Shape(look))) => self.shape(node, rect, look),
            (_, Some(Content::Placeholder(label))) => self.label(label, rect),
            _ => self.visit_children(node, corner),
        }
    }

    fn visit_children(&mut self, node: NodeId, corner: (f32, f32)) {
        for child in self.children(node) {
            self.visit(child, corner);
        }
    }

    /// Adds to the text so far, taking the marker of the list item it starts, if any.
    fn add_paragraph(&mut self, mut paragraph: NativeParagraph, mut rect: Rect) {
        let (top, bottom) = (rect.y, rect.y + rect.h);
        if let Some(marker) = self.marker.take() {
            paragraph.bullet = marker.bullet;
            paragraph.bullet_color = Some(marker.color);
            rect = union(rect, marker.rect);
        }
        paragraph.level = self.lists.saturating_sub(1);
        match &mut self.text {
            Some(text) => {
                // As far from the text above it as the layout put it.
                paragraph.space_before = Some((top - text.bottom).max(0.0));
                text.paragraphs.push(paragraph);
                text.around = union(text.around, rect);
                text.bottom = bottom;
            }
            None => self.text = Some(RunningText { paragraphs: vec![paragraph], around: rect, bottom }),
        }
    }

    /// A list item with no text to put its marker on still shows the marker.
    fn show_marker_alone(&mut self) {
        if let Some(marker) = self.marker {
            self.add_paragraph(NativeParagraph::new(Vec::new(), Align::Left), marker.rect);
        }
    }

    /// Makes the text so far into a text box.
    fn end_text(&mut self) {
        self.show_marker_alone();
        if let Some(RunningText { paragraphs, around, .. }) = self.text.take() {
            self.elements.push(NativeElement { rect: around, content: NativeContent::Text { paragraphs, fill: None, inset: 0.0 } });
        }
    }

    /// Adds something that stands on its own.
    fn push(&mut self, rect: Rect, content: NativeContent) {
        self.end_text();
        self.elements.push(NativeElement { rect, content });
    }

    /// Adds a placeholder's label.
    fn label(&mut self, label: &str, rect: Rect) {
        self.end_text();
        self.elements.push(placeholder_element(label, rect, self.theme));
    }

    fn list_item(&mut self, node: NodeId, corner: (f32, f32), bullet: Bullet, color: Color) {
        let children = self.children(node);
        let [marker, content] = children[..] else {
            return;
        };
        self.show_marker_alone();
        self.marker = Some(Marker { bullet, color, rect: self.rect_of(marker, corner) });
        self.lists += 1;
        self.visit(content, corner);
        self.show_marker_alone();
        self.lists -= 1;
    }

    /// Columns and rows are as big as the layout made their cells. A row with too few
    /// cells is filled out with empty ones, as a table must have the same cells in each row.
    fn table(&mut self, cells: Vec<Vec<(NodeId, NativeParagraph)>>, rect: Rect) {
        let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let size_of = |cell: &(NodeId, NativeParagraph)| self.tree.layout(cell.0).expect("every node is in the tree").size;
        let column_widths = (0..columns)
            .map(|column| cells.iter().find_map(|row| row.get(column)).map_or(0.0, |cell| size_of(cell).width))
            .collect();
        let row_heights = cells.iter().map(|row| row.first().map_or(0.0, |cell| size_of(cell).height)).collect();
        let rows = cells
            .into_iter()
            .map(|row| {
                let mut row: Vec<Vec<NativeParagraph>> = row.into_iter().map(|(_, paragraph)| vec![paragraph]).collect();
                row.resize(columns, vec![NativeParagraph::new(Vec::new(), Align::Left)]);
                row
            })
            .collect();
        self.push(rect, NativeContent::Table { rows, column_widths, row_heights, line: self.theme.text });
    }

    /// A shape, with its text in it if that's all it holds. Otherwise what it holds comes
    /// after it, placed by the layout.
    fn shape(&mut self, node: NodeId, rect: Rect, look: &ShapeLook) {
        let only_text = self.children(node).into_iter().all(|child| self.is_only_text(child));
        let inset = self.tree.layout(node).expect("every node is in the tree").padding.left;
        self.push(rect, look.native(self.theme, inset));
        let shape = self.elements.len() - 1;
        self.visit_children(node, (rect.x, rect.y));
        self.end_text();
        if !only_text {
            return;
        }
        let held = self.elements.drain(shape + 1..).flat_map(|element| match element.content {
            NativeContent::Text { paragraphs, .. } => paragraphs,
            _ => Vec::new(),
        });
        let held: Vec<NativeParagraph> = held.collect();
        if let NativeContent::Shape { paragraphs, .. } = &mut self.elements[shape].content {
            *paragraphs = held;
        }
    }

    /// Whether `node` is nothing but text reading from top to bottom.
    fn is_only_text(&self, node: NodeId) -> bool {
        let children = self.children(node);
        let breaks_up_text = match (self.semantics.get(&node), self.tree.get_node_context(node)) {
            (Some(Semantic::Table(_) | Semantic::TableTooBig | Semantic::Code(_)), _) => true,
            (Some(Semantic::Stack), _) => children.len() > 1,
            (_, Some(Content::Shape(_) | Content::Picture(_) | Content::Placeholder(_))) => true,
            _ => false,
        };
        !breaks_up_text && children.into_iter().all(|child| self.is_only_text(child))
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect { x, y, w: (a.x + a.w).max(b.x + b.w) - x, h: (a.y + a.h).max(b.y + b.h) - y }
}
