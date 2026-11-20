//! A terminal recording on a slide, laid out as its screen: filled cells and real text in the
//! bundled monospaced font, so the still, thumbnails, PDF and every editing tool see ordinary
//! display items. Reading the recording, playing it to its end and laying its screen out in
//! cells is a still made aside, as a video's is: until it's made the recording is a plain dark
//! panel. What's left for the thread laying the slide out is shaping the text, once for each
//! height of a row, and placing what was laid out in the box.

use std::collections::HashMap;
use std::sync::Arc;

use slides_format::Tag;
use slides_terminal::{Cell, Colour, Screen, Wide, rgb};
use tiny_skia::PathBuilder;
use toolkit::{Color, Theme};

use crate::elements::box_size;
use crate::pictures::TerminalStill;
use crate::placeholder::placeholder_items;
use crate::text::{LINE_HEIGHT, MIN_TEXT_SIZE};
use crate::{Align, DisplayItem, Fonts, LayoutEnv, Rect, ShapedText, TextRun, shape_text};

/// The tag of a terminal recording on a slide.
pub(crate) const TERMINAL_TAG: &str = "Terminal";
/// What's said of a recording written in the text, not placed on the slide.
pub const TERMINAL_IN_TEXT: &str = "This terminal recording plays only when it's placed on the slide. Drag it out of the text to place it.";
/// What's said of a recording that names no file to play.
pub const TERMINAL_NO_FILE: &str = "This terminal recording has no file. Insert a terminal recording to choose one.";
/// What the panel of a recording in the text says.
pub(crate) const TERMINAL_LABEL: &str = "Terminal recording";
/// How high a row of the screen is at its own size, in slide units.
pub const TERMINAL_ROW: f32 = 18.0;
/// How wide a cell is for each unit of its height: the monospaced font's advance, 0.6 of its
/// size, over a line of the text, as text is shaped.
const CELL_SHAPE: f32 = 0.6 / LINE_HEIGHT;
/// The bundled monospaced font, in which every cell is as wide.
const MONO: &str = "JetBrains Mono";
/// The screen a recording is taken to have until its header has been read.
const SCREEN_UNTIL_READ: (u16, u16) = (80, 24);
/// The most runs a row is laid out as a text item each. In a row of more, the runs between wide
/// characters are one text item, as shaping each alone would take too long on a screen made of
/// them; each wide character still stands alone, as no font's is exactly two cells wide.
const MOST_RUNS_A_ROW: usize = 32;
/// The most colours a screen's backgrounds or underlines are drawn in as they are, each colour one
/// item. A screen of more has each of its colours drawn as the nearest of 4,096, sixteen levels
/// of each of red, green and blue.
const MOST_COLOURS: usize = 65_536;

/// The size of a screen of `columns` and `rows` at its own size: [`TERMINAL_ROW`] units a row.
fn own_size((columns, rows): (u16, u16)) -> (f32, f32) {
    (f32::from(columns) * TERMINAL_ROW * CELL_SHAPE, f32::from(rows) * TERMINAL_ROW)
}

fn color([r, g, b]: [u8; 3]) -> Color {
    Color::rgb(r, g, b)
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<tiny_skia::Path> {
    Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?))
}

/// A plain panel of the screen's own background over `rect`.
fn panel(rect: Rect) -> Option<DisplayItem> {
    let path = rect_path(rect.x, rect.y, rect.w, rect.h)?;
    Some(DisplayItem::Fill { path, color: color(rgb(Colour::Default, false)) })
}

/// A character as it's shown in a cell: anything the shaper would take as the end of a line, any
/// other control, and anything that turns the direction text runs in, is a blank. The cells are
/// shown left to right, as the terminal had them.
fn shown(c: char) -> char {
    let turns = matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || turns { ' ' } else { c }
}

/// How a cell is drawn: its colours, as shown, and its font's weight and slant.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    fg: [u8; 3],
    bg: [u8; 3],
    bold: bool,
    italic: bool,
    underline: bool,
}

/// How `cell` looks, with the cursor on it if `cursor`: the cursor is a block, the cell's
/// colours swapped.
fn look(cell: &Cell, cursor: bool) -> Look {
    let style = cell.style;
    let (mut fg, mut bg) = (rgb(style.fg, true), rgb(style.bg, false));
    if style.faint {
        fg = [0, 1, 2].map(|at| ((u16::from(fg[at]) + u16::from(bg[at])) / 2) as u8);
    }
    if style.inverse != cursor {
        (fg, bg) = (bg, fg);
    }
    Look { fg, bg, bold: style.bold, italic: style.italic, underline: style.underline }
}

/// A piece of a text item in one look.
struct Span {
    text: String,
    look: Look,
}

/// A text item of the screen: the cell it starts at, and its pieces.
struct CellText {
    row: u16,
    col: u16,
    spans: Vec<Span>,
}

/// Cells in a row: the row, the first column and how many.
type Cells = (u16, u16, u16);

/// A screen laid out in cells, ready to be placed in a box of any size: made from the screen
/// where the recording is played, so nothing of that is left for the thread laying out.
pub(crate) struct LaidScreen {
    pub(crate) columns: u16,
    pub(crate) rows: u16,
    /// Each background colour other than the screen's own, and the cells it fills.
    fills: Vec<([u8; 3], Vec<Cells>)>,
    texts: Vec<CellText>,
    /// Each colour of underline, and the cells it's under.
    lines: Vec<([u8; 3], Vec<Cells>)>,
    /// About how many bytes it holds.
    pub(crate) bytes: usize,
    /// How many characters its text has.
    pub(crate) glyphs: usize,
    /// How many runs of cells it fills or underlines.
    pub(crate) runs: usize,
}

impl LaidScreen {
    /// How much there is to shape, place and draw of it: its characters and its runs of cells.
    pub(crate) fn pieces(&self) -> usize {
        self.glyphs + self.runs
    }

    fn count(&self) -> usize {
        let cells = |groups: &[([u8; 3], Vec<Cells>)]| groups.iter().map(|(_, cells)| cells.len() * size_of::<Cells>() + 32).sum::<usize>();
        let texts: usize = self.texts.iter().map(|text| 32 + text.spans.iter().map(|span| span.text.len() + size_of::<Span>()).sum::<usize>()).sum();
        size_of::<LaidScreen>() + cells(&self.fills) + cells(&self.lines) + texts
    }
}

/// A run of a row's text in one look, from a column, until it's laid out, and how many cells it
/// covers.
struct Run {
    from: u16,
    look: Look,
    text: String,
    cells: u16,
    wide: bool,
}

/// `runs`, the runs of one row, merged into one text item from the first's column, each but the
/// last followed by blanks to where the next starts.
fn merged(row: u16, runs: &[Run]) -> CellText {
    let mut spans: Vec<Span> = runs
        .windows(2)
        .map(|pair| {
            let blanks = usize::from((pair[1].from - pair[0].from).saturating_sub(pair[0].cells));
            Span { text: format!("{}{}", pair[0].text, " ".repeat(blanks)), look: pair[0].look }
        })
        .collect();
    spans.extend(runs.last().map(|last| Span { text: last.text.clone(), look: last.look }));
    CellText { row, col: runs.first().map_or(0, |first| first.from), spans }
}

/// The runs of a row as its text items: each alone if there are few enough, or else those
/// between wide characters merged, each wide character standing alone, as no font's is exactly
/// two cells wide.
fn row_texts(row: u16, runs: Vec<Run>) -> Vec<CellText> {
    let alone = |run: &Run| CellText { row, col: run.from, spans: vec![Span { text: run.text.clone(), look: run.look }] };
    if runs.len() <= MOST_RUNS_A_ROW {
        return runs.iter().map(alone).collect();
    }
    let mut texts = Vec::new();
    for piece in runs.split_inclusive(|run| run.wide) {
        let (narrow, wide) = match piece.split_last() {
            Some((last, narrow)) if last.wide => (narrow, Some(last)),
            _ => (piece, None),
        };
        if !narrow.is_empty() {
            texts.push(merged(row, narrow));
        }
        texts.extend(wide.map(alone));
    }
    texts
}

/// The nearest of sixteen levels, from none to all, to a channel.
fn coarse_level(channel: u8) -> u8 {
    ((u16::from(channel) + 8) / 17 * 17) as u8
}

/// Cells by their colour, in the order each colour was first seen: as each is, if there are few
/// enough colours, or else each the nearest of 4,096.
fn by_colour(cells: Vec<([u8; 3], Cells)>) -> Vec<([u8; 3], Vec<Cells>)> {
    let mut colours: HashMap<[u8; 3], usize> = HashMap::new();
    let distinct = {
        for (colour, _) in &cells {
            let next = colours.len();
            colours.entry(*colour).or_insert(next);
        }
        colours.len()
    };
    let shown = |colour: [u8; 3]| if distinct > MOST_COLOURS { colour.map(coarse_level) } else { colour };
    let mut grouped: Vec<([u8; 3], Vec<Cells>)> = Vec::new();
    let mut at: HashMap<[u8; 3], usize> = HashMap::new();
    for (colour, cells) in cells {
        let colour = shown(colour);
        let index = *at.entry(colour).or_insert_with(|| {
            grouped.push((colour, Vec::new()));
            grouped.len() - 1
        });
        grouped[index].1.push(cells);
    }
    grouped
}

/// `screen` laid out in cells: a fill for each run of cells of a background other than the
/// screen's own, the text of each run of cells in one look, underlines, and the cursor, when it's
/// shown, as a block over both halves of a wide character.
pub(crate) fn lay_out_screen(screen: &Screen) -> LaidScreen {
    let cursor = screen.cursor().map(|(col, row)| (col.min(screen.width() - 1), row));
    let (mut fills, mut lines, mut texts) = (Vec::new(), Vec::new(), Vec::new());
    for (row, cells) in (0u16..).zip(screen.rows()) {
        let under_cursor = |col: u16| {
            let Some((at, _)) = cursor.filter(|(_, on)| *on == row) else { return false };
            let wide = |col: u16| cells.get(usize::from(col)).map(|cell| cell.wide);
            col == at || (col + 1 == at && wide(at) == Some(Wide::Right)) || (col == at + 1 && wide(at) == Some(Wide::Left))
        };
        let looks: Vec<Look> = (0u16..).zip(cells).map(|(col, cell)| look(cell, under_cursor(col))).collect();
        // The backgrounds, a run for each colour but the screen's own.
        let mut from = 0;
        for col in 1..=looks.len() {
            if col < looks.len() && looks[col].bg == looks[from].bg {
                continue;
            }
            if looks[from].bg != rgb(Colour::Default, false) {
                fills.push((looks[from].bg, (row, from as u16, (col - from) as u16)));
            }
            from = col;
        }
        // The text, a run for each look, a blank going with the run it's in.
        let mut run: Option<Run> = None;
        let mut runs = Vec::new();
        for ((col, cell), look) in (0u16..).zip(cells).zip(&looks) {
            if cell.wide == Wide::Right {
                continue;
            }
            let text = shown(cell.text);
            let marks: Vec<char> = cell.marks.iter().flatten().copied().filter(|mark| shown(*mark) != ' ').collect();
            let width = if cell.wide == Wide::Left { 2 } else { 1 };
            if text == ' ' && marks.is_empty() {
                if let Some(run) = &mut run {
                    run.text.push_str(if width == 2 { "  " } else { " " });
                    run.cells += width;
                }
                continue;
            }
            let wide = cell.wide == Wide::Left;
            let same = |run: &Run| !run.wide && (run.look.fg, run.look.bold, run.look.italic) == (look.fg, look.bold, look.italic);
            if wide || !run.as_ref().is_some_and(same) {
                runs.extend(run.take());
                run = Some(Run { from: col, look: *look, text: String::new(), cells: 0, wide });
            }
            if let Some(run) = &mut run {
                run.text.push(text);
                run.text.extend(marks);
                run.cells += width;
            }
            if wide {
                runs.extend(run.take());
            }
        }
        runs.extend(run);
        // Blanks at a run's end are left out: each is a space of a cell.
        for run in &mut runs {
            let kept = run.text.trim_end_matches(' ').len();
            run.cells = run.cells.saturating_sub((run.text.len() - kept) as u16);
            run.text.truncate(kept);
        }
        texts.extend(row_texts(row, runs));
        // Underlines, a line for each run of underlined cells of one colour.
        let mut from: Option<usize> = None;
        for col in 0..=looks.len() {
            let goes_on = |from: usize| col < looks.len() && looks[col].underline && looks[col].fg == looks[from].fg;
            match from {
                Some(start) if goes_on(start) => continue,
                Some(start) => {
                    lines.push((looks[start].fg, (row, start as u16, (col - start) as u16)));
                    from = None;
                }
                None => {}
            }
            if col < looks.len() && looks[col].underline {
                from = Some(col);
            }
        }
    }
    let glyphs = texts.iter().flat_map(|text| &text.spans).map(|span| span.text.chars().count()).sum();
    let runs = fills.len() + lines.len();
    let mut laid = LaidScreen { columns: screen.width(), rows: screen.height(), fills: by_colour(fills), texts, lines: by_colour(lines), bytes: 0, glyphs, runs };
    laid.bytes = laid.count();
    laid
}

/// Where a screen's cells go in a box: as large as fit it keeping their shape, rows on whole
/// units once a row is a unit high, and the screen in the middle of the box, on whole units.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub(crate) row: f32,
    cell: f32,
    left: f32,
    top: f32,
    whole: bool,
    /// The box.
    rect: Rect,
}

/// What tells one grid from another of the same screen: the box it's in.
pub(crate) type GridKey = [u32; 4];

impl Grid {
    pub(crate) fn key(&self) -> GridKey {
        [self.rect.x, self.rect.y, self.rect.w, self.rect.h].map(f32::to_bits)
    }
}

/// Where the cells of a screen of `columns` and `rows` go in `rect`, if they fit at all.
pub(crate) fn grid(columns: u16, rows: u16, rect: Rect) -> Option<Grid> {
    let (columns, rows) = (f32::from(columns), f32::from(rows));
    let fit = (rect.h / rows).min(rect.w / (columns * CELL_SHAPE));
    if !fit.is_finite() || fit <= 0.0 {
        return None;
    }
    // Floored a little above, so a box made for whole rows isn't a unit short of them.
    let (row, whole) = if fit >= 1.0 { ((fit + 1.0e-3).floor(), true) } else { (fit, false) };
    let cell = row * CELL_SHAPE;
    let on_unit = |at: f32| if whole { at.round() } else { at };
    Some(Grid { row, cell, left: rect.x + on_unit((rect.w - cell * columns) / 2.0), top: rect.y + on_unit((rect.h - row * rows) / 2.0), whole, rect })
}

/// The text items of `laid` shaped for rows `row` units high, in its order: the same wherever the
/// box is, and however large, as long as its rows are as high. Rows too small for text have none.
pub(crate) fn shape(laid: &LaidScreen, row: f32, fonts: &mut Fonts) -> Vec<ShapedText> {
    let size = row / LINE_HEIGHT;
    // Text is no smaller than this, and would be higher than such rows are.
    if size < MIN_TEXT_SIZE {
        return Vec::new();
    }
    laid.texts
        .iter()
        .map(|text| {
            let runs: Vec<TextRun> = text
                .spans
                .iter()
                .map(|span| TextRun { text: span.text.clone(), size, bold: span.look.bold, italic: span.look.italic, underline: false, color: color(span.look.fg), family: MONO.to_string() })
                .collect();
            shape_text(fonts, &runs, None, Align::Left)
        })
        .collect()
}

/// About how many bytes shaped text holds.
pub(crate) fn shaped_bytes(shaped: &[ShapedText]) -> usize {
    // Each glyph is held as it's shaped and as it's laid out, and the line's text with it.
    let glyph = 2 * size_of::<cosmic_text::LayoutGlyph>() + 32;
    shaped.iter().map(|text| 512 + text.buffer.layout_runs().map(|run| 2 * run.text.len() + run.glyphs.len() * glyph).sum::<usize>()).sum()
}

/// About how many bytes placed items hold: each item, and each path's points and verbs.
pub(crate) fn placed_bytes(items: &[DisplayItem]) -> usize {
    let path = |path: &tiny_skia::Path| size_of_val(path.points()) + path.verbs().len();
    items
        .iter()
        .map(|item| {
            size_of::<DisplayItem>()
                + match item {
                    DisplayItem::Fill { path: drawn, .. } | DisplayItem::Stroke { path: drawn, .. } => path(drawn),
                    _ => 0,
                }
        })
        .sum()
}

/// `laid` placed on `grid`, with its text `shaped` for the grid's rows: a fill of the screen's
/// background over the whole box, a fill for each background colour but the screen's own, the
/// text, and a stroke for each colour of underline.
pub(crate) fn place(laid: &LaidScreen, grid: &Grid, shaped: &[ShapedText]) -> Vec<DisplayItem> {
    let mut items: Vec<DisplayItem> = panel(grid.rect).into_iter().collect();
    let x = |col: u16| grid.left + f32::from(col) * grid.cell;
    let y = |row: u16| grid.top + f32::from(row) * grid.row;
    for (colour, cells) in &laid.fills {
        let mut path = PathBuilder::with_capacity(cells.len() * 5, cells.len() * 4);
        for &(row, col, count) in cells {
            if let Some(rect) = tiny_skia::Rect::from_xywh(x(col), y(row), f32::from(count) * grid.cell, grid.row) {
                path.push_rect(rect);
            }
        }
        if let Some(path) = path.finish() {
            items.push(DisplayItem::Fill { path, color: color(*colour) });
        }
    }
    for (text, shaped) in laid.texts.iter().zip(shaped) {
        items.push(DisplayItem::Text { text: shaped.clone(), x: x(text.col), y: y(text.row) });
    }
    let width = if grid.whole { (grid.row / 16.0).max(1.0) } else { grid.row / 16.0 };
    for (colour, cells) in &laid.lines {
        let mut path = PathBuilder::with_capacity(cells.len() * 2, cells.len() * 2);
        for &(row, col, count) in cells {
            let at = y(row) + grid.row * 0.86;
            path.move_to(x(col), at);
            path.line_to(x(col + count), at);
        }
        if let Some(path) = path.finish() {
            items.push(DisplayItem::Stroke { path, color: color(*colour), width });
        }
    }
    items
}

/// `screen` laid out over `rect`, as a recording's last screen is: a fill of the screen's
/// background over the whole box, and its cells as [`lay_out_screen`], [`grid`] and [`place`] lay
/// them out.
pub fn screen_items(screen: &Screen, rect: Rect, fonts: &mut Fonts) -> Vec<DisplayItem> {
    let laid = lay_out_screen(screen);
    match grid(laid.columns, laid.rows, rect) {
        Some(grid) => place(&laid, &grid, &shape(&laid, grid.row, fonts)),
        None => panel(rect).into_iter().collect(),
    }
}

/// A plain panel over `rect`, alone, as a recording is laid out until its screen is.
pub(crate) fn panel_alone(rect: Rect) -> Placed {
    panel(rect).into_iter().collect()
}

/// A terminal recording placed on a slide, laid out.
pub(crate) struct LaidTerminal {
    /// The size of its box.
    pub(crate) size: (f32, f32),
    /// What it draws, shared with what's kept of the recording.
    pub(crate) items: Placed,
    /// How many times it stops.
    pub(crate) stops: usize,
    /// Why it can't be played, if it can't.
    pub(crate) problem: Option<String>,
    /// Whether its screen is being shaped and placed aside, its panel alone until then.
    pub(crate) pending: bool,
}

/// A terminal recording placed on a slide, laid out: the size of its box, for the sides `wanted`
/// gives and the shape its header says its screen has for the rest, what it draws, how many
/// times it stops, and why it can't be played if it can't. Until it's been read, it's a plain dark
/// panel, and stops nowhere. One being resized, `placing`, is its panel alone: what's shown of it
/// meanwhile is its picture as it was drawn, stretched, and it's laid out once it's let go. One
/// whose screen is being shaped and placed aside is its panel alone too, until it's taken in.
pub(crate) fn lay_out_terminal(tag: &Tag, wanted: (Option<f32>, Option<f32>), placing: bool, theme: &Theme, env: &mut LayoutEnv) -> LaidTerminal {
    let src = tag.attr("src").unwrap_or_default().trim();
    // One with no file is never looked for.
    let (still, screen_size) = if src.is_empty() { (None, None) } else {
        let (still, size) = env.pictures.terminal_still(src, env.files);
        (Some(still), size)
    };
    let (w, h) = box_size(wanted, Some(own_size(screen_size.unwrap_or(SCREEN_UNTIL_READ))));
    let rect = Rect { x: 0.0, y: 0.0, w, h };
    let laid = |items: Placed, stops: usize, problem: Option<String>, pending: bool| LaidTerminal { size: (w, h), items, stops, problem, pending };
    match still {
        None => laid(placeholder_items(TERMINAL_NO_FILE, rect, theme, env.fonts).into(), 0, Some(TERMINAL_NO_FILE.to_string()), false),
        Some(TerminalStill::Cannot(why)) => laid(placeholder_items(&why.to_string(), rect, theme, env.fonts).into(), 0, Some(why.to_string()), false),
        Some(TerminalStill::Coming) => laid(panel_alone(rect), 0, None, false),
        Some(TerminalStill::Laid(_, stops)) if placing => laid(panel_alone(rect), stops, None, false),
        Some(TerminalStill::Laid(_, stops)) => match env.pictures.terminal_items(src, rect, env.fonts) {
            Some(items) => laid(items, stops, None, false),
            None => laid(panel_alone(rect), stops, None, true),
        },
    }
}

/// Shaped text and placed items, kept for a recording as it's laid out again and again.
pub(crate) type Shaped = Arc<[ShapedText]>;
pub(crate) type Placed = Arc<[DisplayItem]>;
