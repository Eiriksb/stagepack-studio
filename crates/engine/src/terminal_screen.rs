//! A terminal recording on a slide, laid out as its screen: filled cells and real text in the
//! bundled monospaced font, so the still, thumbnails, PDF and every editing tool see ordinary
//! display items. Reading the recording and playing it to its end is a still made aside, as a
//! video's is: until it's made the recording is a plain dark panel.

use slides_format::Tag;
use slides_terminal::{Cell, Colour, Screen, Wide, rgb};
use tiny_skia::PathBuilder;
use toolkit::{Color, Theme};

use crate::elements::box_size;
use crate::pictures::TerminalStill;
use crate::placeholder::placeholder_items;
use crate::text::LINE_HEIGHT;
use crate::{Align, DisplayItem, Fonts, LayoutEnv, Rect, TextRun, shape_text};

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
/// The screen a recording is taken to have until it's been read.
const SCREEN_UNTIL_READ: (u16, u16) = (80, 24);

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

/// Whether `cell` shows nothing but its background.
fn blank(cell: &Cell) -> bool {
    cell.text == ' ' && cell.marks == [None; 2] && cell.wide != Wide::Left
}

/// The most text items a row is laid out as. A row of more runs than this is one text item of
/// them all, as shaping each alone would take too long on a screen made of them: its characters
/// are on their cells as long as none is wide.
const MOST_RUNS_A_ROW: usize = 32;

/// A run of a row's text in one look, from a column, until it's laid out, and how many cells it
/// covers.
struct Run {
    from: u16,
    look: Look,
    text: String,
    cells: u16,
}

impl Run {
    fn text_run(&self, text: String, size: f32) -> TextRun {
        let Look { fg, bold, italic, .. } = self.look;
        TextRun { text, size, bold, italic, underline: false, color: color(fg), family: MONO.to_string() }
    }
}

/// `screen` laid out over `rect`: a fill for the screen's background over the whole box, one for
/// each run of cells of another background, the text of each run of cells in one look as a text
/// item, underlines as strokes and the cursor, when it's shown, as a block. The cells are as large
/// as fit the box keeping their shape, rows on whole units once a row is a unit high, and the
/// screen sits in the middle of the box, on whole units.
pub fn screen_items(screen: &Screen, rect: Rect, fonts: &mut Fonts) -> Vec<DisplayItem> {
    let mut items: Vec<DisplayItem> = panel(rect).into_iter().collect();
    let (columns, rows) = (f32::from(screen.width()), f32::from(screen.height()));
    let fit = (rect.h / rows).min(rect.w / (columns * CELL_SHAPE));
    if !fit.is_finite() || fit <= 0.0 {
        return items;
    }
    // Floored a little above, so a box made for whole rows isn't a unit short of them.
    let (row_h, whole) = if fit >= 1.0 { ((fit + 1.0e-3).floor(), true) } else { (fit, false) };
    let cell_w = row_h * CELL_SHAPE;
    let on_unit = |at: f32| if whole { at.round() } else { at };
    let left = rect.x + on_unit((rect.w - cell_w * columns) / 2.0);
    let top = rect.y + on_unit((rect.h - row_h * rows) / 2.0);
    let size = row_h / LINE_HEIGHT;
    let cursor = screen.cursor().map(|(col, row)| (col.min(screen.width() - 1), row));
    let mut texts = Vec::new();
    let mut lines = Vec::new();
    for (row, cells) in screen.rows().enumerate() {
        let y = top + row as f32 * row_h;
        let x = |col: u16| left + f32::from(col) * cell_w;
        let looks: Vec<Look> = (0..).zip(cells).map(|(col, cell)| look(cell, cursor == Some((col, row as u16)))).collect();
        // The backgrounds, a fill for each run of one colour but the screen's own.
        let mut from = 0;
        for col in 1..=looks.len() {
            if col < looks.len() && looks[col].bg == looks[from].bg {
                continue;
            }
            if looks[from].bg != rgb(Colour::Default, false)
                && let Some(path) = rect_path(x(from as u16), y, (col - from) as f32 * cell_w, row_h)
            {
                items.push(DisplayItem::Fill { path, color: color(looks[from].bg) });
            }
            from = col;
        }
        // The text, a run for each look, a blank going with the run it's in; a wide character
        // stands alone, as no font's is exactly two cells wide.
        let mut run: Option<Run> = None;
        let mut runs = Vec::new();
        for ((col, cell), look) in (0u16..).zip(cells).zip(&looks) {
            if cell.wide == Wide::Right {
                continue;
            }
            if blank(cell) {
                if let Some(run) = &mut run {
                    run.text.push(' ');
                    run.cells += 1;
                }
                continue;
            }
            let same = |run: &Run| (run.look.fg, run.look.bold, run.look.italic) == (look.fg, look.bold, look.italic);
            if cell.wide == Wide::Left || !run.as_ref().is_some_and(same) {
                runs.extend(run.take());
                run = Some(Run { from: col, look: *look, text: String::new(), cells: 0 });
            }
            if let Some(run) = &mut run {
                run.text.push(cell.text);
                run.text.extend(cell.marks.iter().flatten());
                run.cells += if cell.wide == Wide::Left { 2 } else { 1 };
            }
            if cell.wide == Wide::Left {
                runs.extend(run.take());
            }
        }
        runs.extend(run);
        for run in &mut runs {
            let blanks = run.text.len() - run.text.trim_end().len();
            run.text.truncate(run.text.len() - blanks);
            run.cells -= blanks as u16;
        }
        if runs.len() <= MOST_RUNS_A_ROW {
            texts.extend(runs.iter().map(|run| (x(run.from), y, vec![run.text_run(run.text.clone(), size)])));
        } else if let Some(first) = runs.first() {
            // Each run but the last is followed by blanks to where the next starts.
            let mut spans: Vec<TextRun> = runs
                .windows(2)
                .map(|pair| {
                    let blanks = usize::from((pair[1].from - pair[0].from).saturating_sub(pair[0].cells));
                    pair[0].text_run(format!("{}{}", pair[0].text, " ".repeat(blanks)), size)
                })
                .collect();
            spans.extend(runs.last().map(|last| last.text_run(last.text.clone(), size)));
            texts.push((x(first.from), y, spans));
        }
        // Underlines, a line for each run of underlined cells of one colour.
        let mut from: Option<usize> = None;
        for col in 0..=looks.len() {
            let goes_on = |from: usize| col < looks.len() && looks[col].underline && looks[col].fg == looks[from].fg;
            match from {
                Some(start) if goes_on(start) => continue,
                Some(start) => {
                    lines.push((x(start as u16), x(col as u16), y + row_h * 0.86, looks[start].fg));
                    from = None;
                }
                None => {}
            }
            if col < looks.len() && looks[col].underline {
                from = Some(col);
            }
        }
    }
    for (x, y, runs) in texts {
        let text = shape_text(fonts, &runs, None, Align::Left);
        items.push(DisplayItem::Text { text, x, y });
    }
    let width = if whole { (row_h / 16.0).max(1.0) } else { row_h / 16.0 };
    for (from, to, y, fg) in lines {
        let mut line = PathBuilder::new();
        line.move_to(from, y);
        line.line_to(to, y);
        if let Some(path) = line.finish() {
            items.push(DisplayItem::Stroke { path, color: color(fg), width });
        }
    }
    items
}

/// A terminal recording placed on a slide, laid out: the size of its box, for the sides `wanted`
/// gives and its screen's own shape for the rest, what it draws, how many times it stops, and
/// why it can't be played if it can't. Until it's been read, it's a plain dark panel, of the
/// shape of a screen of 80 by 24 where no size says otherwise.
pub(crate) fn lay_out_terminal(tag: &Tag, wanted: (Option<f32>, Option<f32>), theme: &Theme, env: &mut LayoutEnv) -> ((f32, f32), Vec<DisplayItem>, usize, Option<String>) {
    let src = tag.attr("src").unwrap_or_default().trim();
    // One with no file is never looked for.
    let still = (!src.is_empty()).then(|| env.pictures.terminal_still(src, env.files));
    let screen_size = match &still {
        Some(TerminalStill::Screen(screen, _)) => (screen.width(), screen.height()),
        _ => SCREEN_UNTIL_READ,
    };
    let (w, h) = box_size(wanted, Some(own_size(screen_size)));
    let rect = Rect { x: 0.0, y: 0.0, w, h };
    let (items, stops, problem) = match still {
        None => (placeholder_items(TERMINAL_NO_FILE, rect, theme, env.fonts), 0, Some(TERMINAL_NO_FILE.to_string())),
        Some(TerminalStill::Cannot(why)) => (placeholder_items(&why.to_string(), rect, theme, env.fonts), 0, Some(why.to_string())),
        Some(TerminalStill::Coming) => (panel(rect).into_iter().collect(), 0, None),
        Some(TerminalStill::Screen(screen, stops)) => {
            let items = env.pictures.terminal_items(src, (w, h)).unwrap_or_else(|| {
                let items = screen_items(&screen, rect, env.fonts);
                env.pictures.keep_terminal_items(src, (w, h), &items);
                items
            });
            (items, stops, None)
        }
    };
    ((w, h), items, stops, problem)
}
