//! Growing and shrinking a table by rows and columns. Each operation changes only the lines it
//! must, and only inside them: a new row is a line of its own, a new column is a cell put into
//! each line, and every other cell keeps its bytes. Each change is read back before it's given
//! out, and one that wouldn't read as the table meant is refused.

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::edit::{Change, SourceEdit, eol_in};
use crate::model::{Block, CellSource, Slide, Span, Talk};
use crate::parse::parse_talk;
use crate::rich::typed_tables;
use crate::rich_write::changed_slide;
use crate::slide_ops::blocks_alike;

/// The most columns a table can have.
pub const MAX_TABLE_COLUMNS: usize = 20;
/// The most rows a table can have, its header with them.
pub const MAX_TABLE_ROWS: usize = 51;

/// A new table of `columns` columns, 1 to 20, and `rows` rows under its header, 1 to 50: every
/// cell a single space, the alignment row `| --- |`, and each line starting and ending with
/// `|`. Its lines end with `\n`, and the last has none.
pub fn new_table(columns: usize, rows: usize) -> Option<String> {
    if !(1..=MAX_TABLE_COLUMNS).contains(&columns) || !(1..MAX_TABLE_ROWS).contains(&rows) {
        return None;
    }
    let empty = empty_row(columns);
    let rule = format!("|{}", " --- |".repeat(columns));
    let lines: Vec<&str> = [empty.as_str(), &rule].into_iter().chain(std::iter::repeat_n(empty.as_str(), rows)).collect();
    Some(lines.join("\n"))
}

/// How many columns the table at exactly `table` on slide `slide` has, as its header does, and
/// how many rows, the header with them. `None` if there's no table there whose cells can be
/// typed in.
pub fn table_size(talk: &Talk, slide: usize, table: &Range<usize>) -> Option<(usize, usize)> {
    let found = find(talk, slide, table)?;
    Some((found.rows[0].len(), found.rows.len()))
}

/// The edit that adds an empty row after row `after`, the header being row 0, of the table at
/// `table` on slide `slide`: a line of its own, as many cells as the header has, indented as
/// the table's lines are. `None` past the last row, past 51 rows, or where the table isn't
/// one whose cells can be typed in.
pub fn add_row(talk: &Talk, slide: usize, table: &Range<usize>, after: usize) -> Option<Change> {
    guarded(|| {
        let found = find(talk, slide, table)?;
        let rows = found.rows.len();
        if after >= rows || rows + 1 > MAX_TABLE_ROWS {
            return None;
        }
        let source = talk.source.as_str();
        let columns = found.rows[0].len();
        // The line the new one goes under: the alignment row's, under the header.
        let above = if after == 0 { found.rule.end } else { found.cells[after].last()?.range.end };
        let at = content_end(source, above);
        let text = format!("{}{}{}", eol_in(source), found.indent(source), empty_row(columns));
        let mut expected = found.rows.clone();
        expected.insert(after + 1, vec![Vec::new(); columns]);
        checked(talk, &found, vec![SourceEdit { range: at..at, text }], &expected)
    })
}

/// The edit that takes row `row` of the table at `table` on slide `slide` away: its line, and
/// for the last row, the line ending before it in place of its own.
/// `None` for the header, row 0, for the table's only row under it, and past its last row.
pub fn remove_row(talk: &Talk, slide: usize, table: &Range<usize>, row: usize) -> Option<Change> {
    guarded(|| {
        let found = find(talk, slide, table)?;
        if row == 0 || row >= found.rows.len() || found.rows.len() <= 2 {
            return None;
        }
        let source = talk.source.as_str();
        let first = found.cells[row].first()?.range.start;
        let start = line_start(source, first);
        // The last row's line goes with the line ending before it, which ends the table.
        let range = if row + 1 == found.rows.len() {
            let ending = if source[..start].ends_with("\r\n") { 2 } else { 1 };
            start.checked_sub(ending)?..content_end(source, first)
        } else {
            start..next_line(source, first)
        };
        let mut expected = found.rows.clone();
        expected.remove(row);
        checked(talk, &found, vec![SourceEdit { range, text: String::new() }], &expected)
    })
}

/// The edit that adds an empty column after column `after`, or first for `None`, to the table
/// at `table` on slide `slide`: an empty cell in each row and `---` in its alignment row. A
/// row with fewer cells than that needs is first given empty ones up to it, and no more.
/// `None` past the last column, past 20 columns, for a table with a row of more cells than its
/// header, whose cells past the header's are in no column, and where the table isn't one
/// whose cells can be typed in.
pub fn add_column(talk: &Talk, slide: usize, table: &Range<usize>, after: Option<usize>) -> Option<Change> {
    guarded(|| {
        let found = find(talk, slide, table)?;
        let columns = found.rows[0].len();
        let at = after.map_or(0, |after| after + 1);
        if at > columns || columns + 1 > MAX_TABLE_COLUMNS || found.has_a_long_row() {
            return None;
        }
        let source = talk.source.as_str();
        let rule = rule_cells(source, &found.rule)?;
        if rule.cells() != columns {
            return None;
        }
        let mut edits = Vec::new();
        for (r, cells) in found.cells.iter().enumerate() {
            edits.push(insert_cell(&row_cells(source, cells)?, at, " "));
            if r == 0 {
                edits.push(insert_cell(&rule, at, " --- "));
            }
        }
        let expected: Vec<Vec<Vec<Span>>> = found
            .rows
            .iter()
            .map(|row| {
                let mut row = row.clone();
                if row.len() < at {
                    row.resize(at, Vec::new());
                }
                row.insert(at, Vec::new());
                row
            })
            .collect();
        checked(talk, &found, edits, &expected)
    })
}

/// The edit that takes column `column` away from the table at `table` on slide `slide`: that
/// cell from each row that has it, and from its alignment row. `None` for the table's only
/// column, past its last column, for a table with a row of more cells than its header, and
/// where the table isn't one whose cells can be typed in.
pub fn remove_column(talk: &Talk, slide: usize, table: &Range<usize>, column: usize) -> Option<Change> {
    guarded(|| {
        let found = find(talk, slide, table)?;
        let columns = found.rows[0].len();
        if columns <= 1 || column >= columns || found.has_a_long_row() {
            return None;
        }
        let source = talk.source.as_str();
        let rule = rule_cells(source, &found.rule)?;
        if rule.cells() != columns {
            return None;
        }
        let mut edits = Vec::new();
        let mut expected = Vec::new();
        for (r, (cells, spans)) in found.cells.iter().zip(found.rows).enumerate() {
            let row = row_cells(source, cells)?;
            let mut spans = spans.clone();
            if column < spans.len() {
                edits.push(remove_cell(&row, column));
                spans.remove(column);
                if spans.is_empty() {
                    // A row's only cell is emptied: a row can't have none.
                    spans.push(Vec::new());
                }
            }
            if r == 0 {
                edits.push(remove_cell(&rule, column));
            }
            expected.push(spans);
        }
        checked(talk, &found, edits, &expected)
    })
}

/// What `make` gives, or `None` if it panics.
fn guarded(make: impl FnOnce() -> Option<Change>) -> Option<Change> {
    catch_unwind(AssertUnwindSafe(make)).ok().flatten()
}

/// An empty row of `columns` cells, each a single space, with a `|` at each end.
fn empty_row(columns: usize) -> String {
    format!("|{}", " |".repeat(columns))
}

/// A table found on a slide.
struct Found<'a> {
    slide: &'a Slide,
    range: Range<usize>,
    rows: &'a Vec<Vec<Vec<Span>>>,
    cells: &'a Vec<Vec<CellSource>>,
    rule: Range<usize>,
}

impl Found<'_> {
    /// Whether a row has more cells than the header.
    fn has_a_long_row(&self) -> bool {
        self.rows.iter().any(|row| row.len() > self.rows[0].len())
    }

    /// What starts the table's lines before its `|`s: the alignment row's, which, unlike the
    /// header's, can't start with a list's marker.
    fn indent<'s>(&self, source: &'s str) -> &'s str {
        &source[line_start(source, self.rule.start)..self.rule.start]
    }
}

/// The table at exactly `range` on slide `index`, if its cells can be typed in.
fn find<'a>(talk: &'a Talk, index: usize, range: &Range<usize>) -> Option<Found<'a>> {
    let slide = talk.slides.get(index)?;
    let table = typed_tables(slide).into_iter().find(|table| table.range() == range)?;
    let Block::Table { rows, cells, rule, .. } = table else { return None };
    let shaped = !rows.is_empty() && rows.len() == cells.len() && rows.iter().zip(cells).all(|(row, cells)| !row.is_empty() && row.len() == cells.len());
    (shaped && !rule.is_empty()).then(|| Found { slide, range: range.clone(), rows, cells, rule: rule.clone() })
}

/// Where a row's cells are: each from its `|`, or the row's start for a first cell with none,
/// to the next cell's start, and whether the last is closed by a `|` of its own.
struct RowCells {
    starts: Vec<usize>,
    /// Where the last cell ends: after its `|`, if it's closed.
    end: usize,
    closed: bool,
    /// Whether the first cell starts with a `|`.
    opened: bool,
}

impl RowCells {
    fn cells(&self) -> usize {
        self.starts.len()
    }
}

/// A content row's cells, as the parser found them.
fn row_cells(source: &str, cells: &[CellSource]) -> Option<RowCells> {
    let (first, last) = (cells.first()?, cells.last()?);
    let bytes = source.as_bytes();
    let opened = bytes.get(first.range.start) == Some(&b'|');
    // A last cell's `|` that's part of its content, escaped, isn't the row's closing one.
    let closed = last.range.len() >= 2 && bytes.get(last.range.end - 1) == Some(&b'|') && last.inline.end < last.range.end;
    Some(RowCells { starts: cells.iter().map(|cell| cell.range.start).collect(), end: last.range.end, closed, opened })
}

/// The alignment row's cells, from its `|`s: it holds nothing else that could be one.
fn rule_cells(source: &str, rule: &Range<usize>) -> Option<RowCells> {
    let text = source.get(rule.clone())?;
    if !text.bytes().all(|byte| matches!(byte, b'|' | b'-' | b':' | b' ' | b'\t')) {
        return None;
    }
    let pipes: Vec<usize> = text.bytes().enumerate().filter(|(_, byte)| *byte == b'|').map(|(at, _)| rule.start + at).collect();
    let opened = text.starts_with('|');
    let closed = text.len() > 1 && text.ends_with('|');
    let mut starts: Vec<usize> = if opened { Vec::new() } else { vec![rule.start] };
    starts.extend(pipes.iter().copied().filter(|&at| at + 1 != rule.end || !closed));
    if starts.is_empty() {
        return None;
    }
    Some(RowCells { starts, end: rule.end, closed, opened })
}

/// The edit that puts a cell whose bytes between its `|`s are `inside` at column `at` of
/// `row`, after empty cells, ` ` between `|`s, up to it where the row ends before it.
fn insert_cell(row: &RowCells, at: usize, inside: &str) -> SourceEdit {
    let cells = row.cells();
    if at < cells {
        let start = row.starts[at];
        return if at > 0 {
            SourceEdit { range: start..start, text: format!("|{inside}") }
        } else if row.opened {
            SourceEdit { range: start + 1..start + 1, text: format!("{inside}|") }
        } else {
            SourceEdit { range: start..start, text: format!("|{inside}| ") }
        };
    }
    // After the row's last cell: the empty ones it lacks, then the new one.
    let mut text = String::new();
    if !row.closed {
        text.push_str(" |");
    }
    for _ in cells..at {
        text.push_str(" |");
    }
    text.push_str(inside);
    text.push('|');
    SourceEdit { range: row.end..row.end, text }
}

/// The edit that takes cell `column`, one `row` has, away from it, leaving the row's other
/// cells as they are, and the row a table's row: a `|` stays where only it keeps a cell a cell.
fn remove_cell(row: &RowCells, column: usize) -> SourceEdit {
    let cells = row.cells();
    let start = row.starts[column];
    if cells == 1 {
        // A row's only cell: what's in it goes, and the row keeps one empty cell.
        return SourceEdit { range: start..row.end, text: "| |".to_string() };
    }
    if column + 1 < cells {
        return SourceEdit { range: start..row.starts[column + 1], text: String::new() };
    }
    // The last: its own `|` before it stays to close the row, or its closing one does.
    let range = if row.closed { start..row.end - 1 } else { start + 1..row.end };
    SourceEdit { range, text: String::new() }
}

/// Whether `edits`, made to `found`'s slide, leave it one slide whose table there has exactly
/// `expected`'s rows, and is otherwise as it was: the change, if so.
fn checked(talk: &Talk, found: &Found, mut edits: Vec<SourceEdit>, expected: &[Vec<Vec<Span>>]) -> Option<Change> {
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let change = Change { edits };
    let slide = found.slide;
    let new_text = changed_slide(talk, slide, &change)?;
    let read = parse_talk(&new_text);
    let [new_slide] = read.slides.as_slice() else { return None };
    if new_slide.error.is_some() {
        return None;
    }
    // The slide's blocks, with the table's rows as they're meant to be.
    let mut blocks = slide.blocks.clone();
    if !with_table(&mut blocks, &found.range, &mut |rows| *rows = expected.to_vec()) {
        return None;
    }
    let same = blocks_alike(&new_slide.blocks, &blocks)
        && new_slide.notes.as_ref().map(|notes| &notes.text) == slide.notes.as_ref().map(|notes| &notes.text)
        && new_slide.quotes.len() == slide.quotes.len()
        // A cell taken away can take the last of a tag in a line with it.
        && new_slide.inline_tags.iter().all(|tag| slide.inline_tags.contains(tag));
    // And the table is still one whose cells can be typed in, where it was.
    let start = found.range.start - slide.range.start;
    let still = typed_tables(new_slide).iter().any(|table| table.range().start == start);
    (same && still).then_some(change)
}

/// Calls `change` on the rows of the table at exactly `range` among `blocks`, at any depth.
/// Whether there was one.
fn with_table(blocks: &mut [Block], range: &Range<usize>, change: &mut impl FnMut(&mut Vec<Vec<Vec<Span>>>)) -> bool {
    blocks.iter_mut().any(|block| match block {
        Block::Table { rows, range: own, .. } if own == range => {
            change(rows);
            true
        }
        Block::List { items, .. } => items.iter_mut().any(|item| with_table(item, range, change)),
        Block::Tag(tag) => with_table(&mut tag.children, range, change),
        _ => false,
    })
}

/// Where the line holding `at` starts: after the line ending before it.
fn line_start(source: &str, at: usize) -> usize {
    source[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1)
}

/// Where the line holding `at` ends, before its line ending.
fn content_end(source: &str, at: usize) -> usize {
    source[at..].find(['\n', '\r']).map_or(source.len(), |i| at + i)
}

/// Where the line after the one holding `at` starts, or the end of the text.
fn next_line(source: &str, at: usize) -> usize {
    let end = content_end(source, at);
    let bytes = source.as_bytes();
    match bytes.get(end) {
        Some(b'\r') if bytes.get(end + 1) == Some(&b'\n') => end + 2,
        Some(_) => end + 1,
        None => end,
    }
}
