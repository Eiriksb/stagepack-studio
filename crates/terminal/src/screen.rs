//! The screen a recording is played on: a grid of cells, a cursor, and the few sequences
//! recordings use to put characters and colours on it.
//!
//! A recording is data, and all it can do here is change its own screen. The screen has no way
//! to send anything: it answers no question, keeps no title, link or clipboard, and changes no
//! colour but its own cells'. What it does:
//!
//! - text, wrapped at the right edge (the cursor waits on the last cell until the next
//!   character, as terminals do); wide characters take two cells; a combining mark takes none;
//! - carriage return, line feed (and vertical tab and form feed, as line feeds), backspace, tab
//!   (stops every 8 cells);
//! - the cursor moved up, down, forward, back, to a row and column, a column, a row, the next or
//!   previous line's start (CUU, CUD, CUF, CUB, CUP, HVP, CHA, VPA, CNL, CPL), saved and brought
//!   back (`ESC 7`, `ESC 8`, `CSI s`, `CSI u`), shown and hidden (mode 25);
//! - erasing in the screen and the line (ED, EL; ED 3 does nothing as there's no scrollback),
//!   inserting and deleting lines and characters, erasing characters (IL, DL, ICH, DCH, ECH);
//! - scrolling up and down (SU, SD, IND, RI, NEL) within a scroll region (DECSTBM);
//! - the alternate screen (modes 47, 1047 and 1049), wrapping (mode 7), starting again (RIS);
//! - styles (SGR): bold, faint, italic, underline, inverse, and colours in 16, 256 and full
//!   colour; and the line-drawing characters (`ESC ( 0`), as full-screen programs draw boxes.
//!
//! Everything else is read and does nothing: every operating system command (titles, the
//! clipboard, links, colour changes), every device control, application, privacy and other
//! string, every query, the bell, and every other mode and sequence.

use std::fmt;
use std::sync::Arc;

use unicode_width::UnicodeWidthChar;

use crate::cast::{MOST_HEIGHT, MOST_WIDTH};
use crate::parse::{Action, Csi, Parser};

/// One cell of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub text: char,
    pub wide: Wide,
    pub style: Style,
}

/// Whether a cell holds a character of its own, or half of a wide one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Wide {
    /// A character one cell wide, or a blank.
    #[default]
    Narrow,
    /// The first cell of a character two cells wide; it holds the character.
    Left,
    /// The second cell of a character two cells wide; its text is a space, not drawn.
    Right,
}

/// How a cell's character is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Colour,
    pub bg: Colour,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    /// The foreground and background swapped as it's drawn.
    pub inverse: bool,
}

/// A colour as a recording names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Colour {
    /// The screen's own foreground or background.
    #[default]
    Default,
    /// One of the 256 colours: the 16 named ones, a cube of 216, and 24 greys.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Default for Cell {
    fn default() -> Self {
        Cell { text: ' ', wide: Wide::Narrow, style: Style::default() }
    }
}

/// The default foreground, a light grey, and background, near black.
const DEFAULT_FG: [u8; 3] = [0xcc, 0xcc, 0xcc];
const DEFAULT_BG: [u8; 3] = [0x18, 0x18, 0x18];

/// The 16 named colours.
const NAMED: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xcd, 0x31, 0x31],
    [0x0d, 0xbc, 0x79],
    [0xe5, 0xe5, 0x10],
    [0x24, 0x72, 0xc8],
    [0xbc, 0x3f, 0xbc],
    [0x11, 0xa8, 0xcd],
    [0xe5, 0xe5, 0xe5],
    [0x66, 0x66, 0x66],
    [0xf1, 0x4c, 0x4c],
    [0x23, 0xd1, 0x8b],
    [0xf5, 0xf5, 0x43],
    [0x3b, 0x8e, 0xea],
    [0xd6, 0x70, 0xd6],
    [0x29, 0xb8, 0xdb],
    [0xff, 0xff, 0xff],
];

/// A colour as red, green and blue, from a fixed palette; `foreground` says which the default
/// colour is.
pub fn rgb(colour: Colour, foreground: bool) -> [u8; 3] {
    match colour {
        Colour::Default if foreground => DEFAULT_FG,
        Colour::Default => DEFAULT_BG,
        Colour::Rgb(r, g, b) => [r, g, b],
        Colour::Indexed(index @ 0..=15) => NAMED[usize::from(index)],
        Colour::Indexed(index @ 16..=231) => {
            let level = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            let n = index - 16;
            [level(n / 36), level(n / 6 % 6), level(n % 6)]
        }
        Colour::Indexed(index) => {
            let grey = 8 + (index - 232) * 10;
            [grey; 3]
        }
    }
}

/// A row of the screen. A blank row shares its cells with others of its colour, so clearing
/// and scrolling cost a row at a time rather than a cell at a time.
#[derive(Clone)]
enum Row {
    Blank(Arc<[Cell]>),
    Cells(Vec<Cell>),
}

impl Row {
    fn cells(&self) -> &[Cell] {
        match self {
            Row::Blank(cells) => cells,
            Row::Cells(cells) => cells,
        }
    }
}

/// Where the cursor is and what it writes with, as saved and brought back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pen {
    col: u16,
    row: u16,
    style: Style,
    /// Whether the line-drawing characters are in use.
    lines: bool,
}

/// One of the two screens a program may write on: its rows, and the cursor saved on it.
#[derive(Clone)]
struct Grid {
    rows: Vec<Row>,
    saved: Pen,
}

impl PartialEq for Grid {
    fn eq(&self, other: &Self) -> bool {
        self.saved == other.saved
            && self.rows.len() == other.rows.len()
            && self.rows.iter().zip(&other.rows).all(|(a, b)| a.cells() == b.cells())
    }
}

/// A terminal screen. See the module's comment for what it does.
#[derive(Clone)]
pub struct Screen {
    width: u16,
    height: u16,
    main: Grid,
    /// The alternate screen, once a program has used it.
    alternate: Option<Grid>,
    on_alternate: bool,
    pen: Pen,
    /// The cursor is on the last cell and the next character goes on the next line.
    wrap_next: bool,
    shown: bool,
    wraps: bool,
    /// The scroll region's first and last rows.
    top: u16,
    bottom: u16,
    parser: Parser,
    /// The last blank row made, shared by the rows cleared to its colour.
    blank: Arc<[Cell]>,
}

impl PartialEq for Screen {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.main == other.main
            && self.alternate == other.alternate
            && self.on_alternate == other.on_alternate
            && self.pen == other.pen
            && self.wrap_next == other.wrap_next
            && self.shown == other.shown
            && self.wraps == other.wraps
            && self.top == other.top
            && self.bottom == other.bottom
            && self.parser == other.parser
    }
}

impl fmt::Debug for Screen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Screen {} by {}, cursor at {}, {}", self.width, self.height, self.pen.col, self.pen.row)?;
        for row in self.rows() {
            let text: String = row.iter().filter(|cell| cell.wide != Wide::Right).map(|cell| cell.text).collect();
            writeln!(f, "|{}|", text.trim_end())?;
        }
        Ok(())
    }
}

/// A screen's side, kept between one and `most`.
fn side(side: u16, most: u16) -> u16 {
    side.clamp(1, most)
}

impl Screen {
    /// A blank screen of this many columns and rows, each kept between one and the most a
    /// recording may have.
    pub fn new(width: u16, height: u16) -> Screen {
        let (width, height) = (side(width, MOST_WIDTH), side(height, MOST_HEIGHT));
        let blank: Arc<[Cell]> = vec![Cell::default(); usize::from(width)].into();
        Screen {
            width,
            height,
            main: Grid { rows: vec![Row::Blank(blank.clone()); usize::from(height)], saved: Pen::default() },
            alternate: None,
            on_alternate: false,
            pen: Pen::default(),
            wrap_next: false,
            shown: true,
            wraps: true,
            top: 0,
            bottom: height - 1,
            parser: Parser::default(),
            blank,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The cell at this column and row, which must be on the screen.
    pub fn cell(&self, col: u16, row: u16) -> &Cell {
        &self.grid().rows[usize::from(row)].cells()[usize::from(col)]
    }

    /// Where the cursor is, as column and row, or nothing while it's hidden.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.shown.then_some((self.pen.col, self.pen.row))
    }

    /// Each row's cells, top to bottom; each row is as wide as the screen.
    pub fn rows(&self) -> impl Iterator<Item = &[Cell]> {
        self.grid().rows.iter().map(Row::cells)
    }

    /// How many cells the screen holds, both its screens counted.
    pub(crate) fn cells_held(&self) -> usize {
        let grids = if self.alternate.is_some() { 2 } else { 1 };
        usize::from(self.width) * usize::from(self.height) * grids
    }

    /// Reads what a program wrote. A sequence cut off at the end is finished by what's fed next.
    pub fn feed(&mut self, text: &str) {
        for c in text.chars() {
            match self.parser.advance(c) {
                Action::None => {}
                Action::Print(c) => self.print(c),
                Action::Control(c) => self.control(c),
                Action::Escape { intermediate, last } => self.escape(intermediate, last),
                Action::Csi(csi) => self.csi(&csi),
            }
        }
    }

    /// Makes the screen this many columns and rows, each kept within the limits. What fits
    /// stays; if the cursor's row would be cut off, rows go from the top instead.
    pub fn resize(&mut self, width: u16, height: u16) {
        let (width, height) = (side(width, MOST_WIDTH), side(height, MOST_HEIGHT));
        if (width, height) == (self.width, self.height) {
            return;
        }
        let dropped = (self.pen.row + 1).saturating_sub(height);
        let on_alternate = self.on_alternate;
        self.width = width;
        self.height = height;
        let mut main = std::mem::take(&mut self.main.rows);
        self.resize_rows(&mut main, if on_alternate { 0 } else { dropped });
        self.main.rows = main;
        if let Some(mut alternate) = self.alternate.as_mut().map(|grid| std::mem::take(&mut grid.rows)) {
            self.resize_rows(&mut alternate, if on_alternate { dropped } else { 0 });
            if let Some(grid) = self.alternate.as_mut() {
                grid.rows = alternate;
            }
        }
        self.pen.row = self.pen.row.saturating_sub(dropped).min(height - 1);
        self.pen.col = self.pen.col.min(width - 1);
        for grid in std::iter::once(&mut self.main).chain(self.alternate.as_mut()) {
            grid.saved.col = grid.saved.col.min(width - 1);
            grid.saved.row = grid.saved.row.min(height - 1);
        }
        self.wrap_next = false;
        self.top = 0;
        self.bottom = height - 1;
    }

    /// Fits rows to the screen's size, the first `dropped` going.
    fn resize_rows(&mut self, rows: &mut Vec<Row>, dropped: u16) {
        let (width, height) = (usize::from(self.width), usize::from(self.height));
        rows.drain(..usize::from(dropped).min(rows.len()));
        rows.truncate(height);
        for row in rows.iter_mut() {
            match row {
                Row::Blank(cells) => {
                    let bg = cells[0].style.bg;
                    *row = Row::Blank(self.blank_cells(bg));
                }
                Row::Cells(cells) => {
                    cells.resize(width, Cell::default());
                    // A wide character cut in half by the new edge goes.
                    if let Some(last) = cells.last_mut()
                        && last.wide == Wide::Left
                    {
                        *last = Cell { text: ' ', wide: Wide::Narrow, style: last.style };
                    }
                }
            }
        }
        let blank = self.blank_cells(Colour::Default);
        rows.resize(height, Row::Blank(blank));
    }

    fn grid(&self) -> &Grid {
        match &self.alternate {
            Some(alternate) if self.on_alternate => alternate,
            _ => &self.main,
        }
    }

    fn grid_mut(&mut self) -> &mut Grid {
        match &mut self.alternate {
            Some(alternate) if self.on_alternate => alternate,
            _ => &mut self.main,
        }
    }

    /// A blank row's cells of this background, shared with the last made if it's the same.
    fn blank_cells(&mut self, bg: Colour) -> Arc<[Cell]> {
        if self.blank.len() != usize::from(self.width) || self.blank[0].style.bg != bg {
            self.blank = vec![self.erased_with(bg); usize::from(self.width)].into();
        }
        self.blank.clone()
    }

    /// A cell as erasing leaves it: blank, with the background colour in use.
    fn erased(&self) -> Cell {
        self.erased_with(self.pen.style.bg)
    }

    fn erased_with(&self, bg: Colour) -> Cell {
        Cell { text: ' ', wide: Wide::Narrow, style: Style { bg, ..Style::default() } }
    }

    /// A blank row in the background colour in use.
    fn blank_row(&mut self) -> Row {
        Row::Blank(self.blank_cells(self.pen.style.bg))
    }

    /// A row's cells to change, made its own if it was blank.
    fn row_mut(&mut self, row: u16) -> &mut [Cell] {
        let row = &mut self.grid_mut().rows[usize::from(row)];
        if let Row::Blank(cells) = row {
            *row = Row::Cells(cells.to_vec());
        }
        match row {
            Row::Cells(cells) => cells,
            Row::Blank(_) => unreachable!("the row was just made its own"),
        }
    }

    fn print(&mut self, c: char) {
        let c = if self.pen.lines { line_drawing(c) } else { c };
        let wide = match c {
            ' '..='~' => 1,
            _ => c.width().unwrap_or(0),
        };
        // A combining mark has no cell of its own; one cell holds one character, so it's
        // joined to the cell before by being left out.
        if wide == 0 {
            return;
        }
        let width = self.width;
        if wide == 2 && width < 2 {
            return;
        }
        if self.wrap_next && self.wraps {
            self.pen.col = 0;
            self.line_feed();
        }
        self.wrap_next = false;
        if wide == 2 && self.pen.col + 1 >= width {
            if self.wraps {
                self.pen.col = 0;
                self.line_feed();
            } else {
                self.pen.col = width - 2;
            }
        }
        let (col, style) = (usize::from(self.pen.col), self.pen.style);
        let cells = self.row_mut(self.pen.row);
        // Writing over half of a wide character leaves no half behind.
        if col > 0 && cells[col].wide == Wide::Right {
            cells[col - 1] = Cell { text: ' ', wide: Wide::Narrow, style: cells[col - 1].style };
        }
        let end = col + wide;
        if cells[end - 1].wide == Wide::Left && end < cells.len() {
            cells[end] = Cell { text: ' ', wide: Wide::Narrow, style: cells[end].style };
        }
        if wide == 2 {
            cells[col] = Cell { text: c, wide: Wide::Left, style };
            cells[col + 1] = Cell { text: ' ', wide: Wide::Right, style };
        } else {
            cells[col] = Cell { text: c, wide: Wide::Narrow, style };
        }
        if end >= usize::from(width) {
            self.pen.col = width - 1;
            self.wrap_next = self.wraps;
        } else {
            self.pen.col = end as u16;
        }
    }

    fn control(&mut self, c: char) {
        match c {
            '\u{8}' => {
                self.wrap_next = false;
                self.pen.col = self.pen.col.saturating_sub(1);
            }
            '\t' => {
                self.wrap_next = false;
                self.pen.col = ((self.pen.col / 8 + 1) * 8).min(self.width - 1);
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{84}' => self.line_feed(),
            '\r' => self.carriage_return(),
            '\u{85}' => {
                self.carriage_return();
                self.line_feed();
            }
            '\u{8d}' => self.reverse_line_feed(),
            // The bell and every other control do nothing.
            _ => {}
        }
    }

    fn carriage_return(&mut self) {
        self.wrap_next = false;
        self.pen.col = 0;
    }

    /// Down a row, scrolling the region if the cursor is on its last row.
    fn line_feed(&mut self) {
        self.wrap_next = false;
        if self.pen.row == self.bottom {
            self.scroll_up(self.top, self.bottom, 1);
        } else if self.pen.row + 1 < self.height {
            self.pen.row += 1;
        }
    }

    /// Up a row, scrolling the region down if the cursor is on its first row.
    fn reverse_line_feed(&mut self) {
        self.wrap_next = false;
        if self.pen.row == self.top {
            self.scroll_down(self.top, self.bottom, 1);
        } else {
            self.pen.row = self.pen.row.saturating_sub(1);
        }
    }

    /// Moves the rows from `top` to `bottom` up by `count`, blank rows coming in below.
    fn scroll_up(&mut self, top: u16, bottom: u16, count: u16) {
        let (top, bottom) = (usize::from(top), usize::from(bottom));
        let count = usize::from(count).min(bottom + 1 - top);
        let blank = self.blank_row();
        let rows = &mut self.grid_mut().rows[top..=bottom];
        rows.rotate_left(count);
        let len = rows.len();
        for row in &mut rows[len - count..] {
            *row = blank.clone();
        }
    }

    /// Moves the rows from `top` to `bottom` down by `count`, blank rows coming in above.
    fn scroll_down(&mut self, top: u16, bottom: u16, count: u16) {
        let (top, bottom) = (usize::from(top), usize::from(bottom));
        let count = usize::from(count).min(bottom + 1 - top);
        let blank = self.blank_row();
        let rows = &mut self.grid_mut().rows[top..=bottom];
        rows.rotate_right(count);
        for row in &mut rows[..count] {
            *row = blank.clone();
        }
    }

    /// Erases the cells of a row from `from` up to `to`; a wide character partly erased goes
    /// whole.
    fn erase(&mut self, row: u16, from: u16, to: u16) {
        let (from, to) = (usize::from(from), usize::from(to.min(self.width)));
        if from >= to {
            return;
        }
        if from == 0 && to == usize::from(self.width) {
            return self.erase_rows(row, row + 1);
        }
        let erased = self.erased();
        let cells = self.row_mut(row);
        cells[from..to].fill(erased);
        if from > 0 && cells[from - 1].wide == Wide::Left {
            cells[from - 1] = erased;
        }
        if to < cells.len() && cells[to].wide == Wide::Right {
            cells[to] = erased;
        }
    }

    /// Erases whole rows from `from` up to `to`. A row already the same blank is left as it is,
    /// so clearing a clear screen costs next to nothing.
    fn erase_rows(&mut self, from: u16, to: u16) {
        let blank = self.blank_cells(self.pen.style.bg);
        let (from, to) = (usize::from(from), usize::from(to.min(self.height)));
        for row in &mut self.grid_mut().rows[from.min(to)..to] {
            if !matches!(row, Row::Blank(cells) if Arc::ptr_eq(cells, &blank)) {
                *row = Row::Blank(blank.clone());
            }
        }
    }

    /// Starts again, as a new screen of the same size.
    fn reset(&mut self) {
        let (width, height) = (self.width, self.height);
        let blank = self.blank_cells(Colour::Default);
        let main = std::mem::take(&mut self.main.rows);
        *self = Screen { main: Grid { rows: main, saved: Pen::default() }, blank, ..Screen::new(1, 1) };
        (self.width, self.height, self.bottom) = (width, height, height - 1);
        self.erase_rows(0, height);
    }

    fn escape(&mut self, intermediate: u8, last: u8) {
        match (intermediate, last) {
            (0, b'7') => self.save(),
            (0, b'8') => self.restore(),
            (0, b'D') => self.line_feed(),
            (0, b'E') => {
                self.carriage_return();
                self.line_feed();
            }
            (0, b'M') => self.reverse_line_feed(),
            (0, b'c') => self.reset(),
            (b'(', last) => self.pen.lines = last == b'0',
            _ => {}
        }
    }

    fn save(&mut self) {
        let pen = self.pen;
        self.grid_mut().saved = pen;
    }

    fn restore(&mut self) {
        self.pen = self.grid().saved;
        self.pen.col = self.pen.col.min(self.width - 1);
        self.pen.row = self.pen.row.min(self.height - 1);
        self.wrap_next = false;
    }

    fn csi(&mut self, csi: &Csi) {
        if csi.intermediate != 0 {
            return;
        }
        match csi.private {
            0 => self.csi_plain(csi),
            b'?' => match csi.last {
                b'h' => self.modes(csi, true),
                b'l' => self.modes(csi, false),
                _ => {}
            },
            _ => {}
        }
    }

    fn csi_plain(&mut self, csi: &Csi) {
        let n = csi.param(0, 1);
        let (col, row) = (self.pen.col, self.pen.row);
        let (width, height) = (self.width, self.height);
        match csi.last {
            b'A' => {
                let stop = if row >= self.top { self.top } else { 0 };
                self.move_to(col, row.saturating_sub(n).max(stop));
            }
            b'B' => {
                let stop = if row <= self.bottom { self.bottom } else { height - 1 };
                self.move_to(col, row.saturating_add(n).min(stop));
            }
            b'C' => self.move_to(col.saturating_add(n), row),
            b'D' => self.move_to(col.saturating_sub(n), row),
            b'E' => {
                let stop = if row <= self.bottom { self.bottom } else { height - 1 };
                self.move_to(0, row.saturating_add(n).min(stop));
            }
            b'F' => {
                let stop = if row >= self.top { self.top } else { 0 };
                self.move_to(0, row.saturating_sub(n).max(stop));
            }
            b'G' => self.move_to(n - 1, row),
            b'H' | b'f' => self.move_to(csi.param(1, 1) - 1, n - 1),
            b'd' => self.move_to(col, n - 1),
            b'J' => match csi.param(0, 0) {
                0 => {
                    self.erase(row, col, width);
                    self.erase_rows(row + 1, height);
                }
                1 => {
                    self.erase_rows(0, row);
                    self.erase(row, 0, col + 1);
                }
                2 => self.erase_rows(0, height),
                // There's no scrollback to clear.
                _ => {}
            },
            b'K' => match csi.param(0, 0) {
                0 => self.erase(row, col, width),
                1 => self.erase(row, 0, col + 1),
                2 => self.erase(row, 0, width),
                _ => {}
            },
            b'L' | b'M' if (self.top..=self.bottom).contains(&row) => {
                if csi.last == b'L' {
                    self.scroll_down(row, self.bottom, n);
                } else {
                    self.scroll_up(row, self.bottom, n);
                }
                self.move_to(0, row);
            }
            b'@' | b'P' => {
                self.wrap_next = false;
                let erased = self.erased();
                let (col, n) = (usize::from(col), usize::from(n).min(usize::from(width - col)));
                let cells = self.row_mut(row);
                let len = cells.len();
                if csi.last == b'@' {
                    cells[col..].rotate_right(n);
                    cells[col..col + n].fill(erased);
                } else {
                    cells[col..].rotate_left(n);
                    cells[len - n..].fill(erased);
                }
                mend(cells);
            }
            b'X' => {
                self.wrap_next = false;
                self.erase(row, col, col.saturating_add(n));
            }
            b'S' => self.scroll_up(self.top, self.bottom, n),
            // With more than one parameter, this asks to follow the mouse.
            b'T' if csi.params().len() <= 1 => self.scroll_down(self.top, self.bottom, n),
            b'm' => self.style(csi),
            b'r' => {
                let (top, bottom) = (csi.param(0, 1) - 1, csi.param(1, height).min(height) - 1);
                if top < bottom {
                    self.top = top;
                    self.bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            b's' if csi.params().is_empty() => self.save(),
            b'u' if csi.params().is_empty() => self.restore(),
            _ => {}
        }
    }

    /// Moves the cursor, kept on the screen.
    fn move_to(&mut self, col: u16, row: u16) {
        self.wrap_next = false;
        self.pen.col = col.min(self.width - 1);
        self.pen.row = row.min(self.height - 1);
    }

    fn modes(&mut self, csi: &Csi, on: bool) {
        for mode in csi.params() {
            match mode {
                7 => self.wraps = on,
                25 => self.shown = on,
                47 | 1047 => {
                    if !on && *mode == 1047 && self.on_alternate {
                        self.erase_rows(0, self.height);
                    }
                    self.switch(on);
                }
                1049 => {
                    if on {
                        if !self.on_alternate {
                            self.save();
                        }
                        self.switch(true);
                        self.erase_rows(0, self.height);
                    } else {
                        self.switch(false);
                        self.restore();
                    }
                }
                _ => {}
            }
        }
    }

    /// To the alternate screen, or back to the main one.
    fn switch(&mut self, alternate: bool) {
        if alternate && self.alternate.is_none() {
            let blank = self.blank_cells(Colour::Default);
            self.alternate = Some(Grid { rows: vec![Row::Blank(blank); usize::from(self.height)], saved: Pen::default() });
        }
        self.on_alternate = alternate;
        self.wrap_next = false;
    }

    fn style(&mut self, csi: &Csi) {
        let params = csi.params();
        let style = &mut self.pen.style;
        if params.is_empty() {
            *style = Style::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            // The parameters joined to this one by colons.
            let joined = (i + 1..params.len()).take_while(|j| csi.joined(*j)).count();
            let group = &params[i + 1..i + 1 + joined];
            let mut next = i + 1 + joined;
            match params[i] {
                0 => *style = Style::default(),
                1 => style.bold = true,
                2 => style.faint = true,
                3 => style.italic = true,
                4 => style.underline = group.first() != Some(&0),
                7 => style.inverse = true,
                21 => style.underline = true,
                22 => {
                    style.bold = false;
                    style.faint = false;
                }
                23 => style.italic = false,
                24 => style.underline = false,
                27 => style.inverse = false,
                n @ 30..=37 => style.fg = Colour::Indexed((n - 30) as u8),
                39 => style.fg = Colour::Default,
                n @ 40..=47 => style.bg = Colour::Indexed((n - 40) as u8),
                49 => style.bg = Colour::Default,
                n @ 90..=97 => style.fg = Colour::Indexed((n - 90 + 8) as u8),
                n @ 100..=107 => style.bg = Colour::Indexed((n - 100 + 8) as u8),
                n @ (38 | 48) => {
                    let (colour, used) = if joined > 0 { (joined_colour(group), joined) } else { colour(&params[i + 1..]) };
                    if let Some(colour) = colour {
                        if n == 38 {
                            style.fg = colour;
                        } else {
                            style.bg = colour;
                        }
                    }
                    next = i + 1 + used;
                }
                // Blinking, hidden and crossed-out text, and every other, aren't drawn here.
                _ => {}
            }
            i = next;
        }
    }
}

/// A colour written after 38 or 48 with semicolons: `5;n` or `2;r;g;b`. Says how many
/// parameters it used.
fn colour(params: &[u16]) -> (Option<Colour>, usize) {
    match params {
        [5, n, ..] => (u8::try_from(*n).ok().map(Colour::Indexed), 2),
        [5] => (None, 1),
        [2, r, g, b, ..] => (rgb_of(*r, *g, *b), 4),
        [2, ..] => (None, params.len()),
        [] => (None, 0),
        // Anything else isn't a colour, and what follows can't be read.
        _ => (None, params.len()),
    }
}

/// A colour written after 38 or 48 with colons: `5:n`, `2:r:g:b`, or `2:space:r:g:b`.
fn joined_colour(group: &[u16]) -> Option<Colour> {
    match group {
        [5, n] => u8::try_from(*n).ok().map(Colour::Indexed),
        [2, r, g, b] | [2, _, r, g, b, ..] => rgb_of(*r, *g, *b),
        _ => None,
    }
}

fn rgb_of(r: u16, g: u16, b: u16) -> Option<Colour> {
    Some(Colour::Rgb(u8::try_from(r).ok()?, u8::try_from(g).ok()?, u8::try_from(b).ok()?))
}

/// Mends a row after its cells were moved: half of a wide character without its other half
/// becomes a blank.
fn mend(cells: &mut [Cell]) {
    let mut col = 0;
    while col < cells.len() {
        match cells[col].wide {
            Wide::Left if cells.get(col + 1).is_some_and(|next| next.wide == Wide::Right) => col += 2,
            Wide::Narrow => col += 1,
            _ => {
                cells[col] = Cell { text: ' ', wide: Wide::Narrow, style: cells[col].style };
                col += 1;
            }
        }
    }
}

/// The line-drawing character for one written while they're in use.
fn line_drawing(c: char) -> char {
    const LINES: [char; 32] = [
        ' ', '\u{25c6}', '\u{2592}', '\u{2409}', '\u{240c}', '\u{240d}', '\u{240a}', '\u{b0}', '\u{b1}', '\u{2424}', '\u{240b}',
        '\u{2518}', '\u{2510}', '\u{250c}', '\u{2514}', '\u{253c}', '\u{23ba}', '\u{23bb}', '\u{2500}', '\u{23bc}', '\u{23bd}',
        '\u{251c}', '\u{2524}', '\u{2534}', '\u{252c}', '\u{2502}', '\u{2264}', '\u{2265}', '\u{3c0}', '\u{2260}', '\u{a3}',
        '\u{b7}',
    ];
    match c {
        '_'..='~' => LINES[c as usize - '_' as usize],
        c => c,
    }
}
