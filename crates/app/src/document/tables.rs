//! The table tools: which table and cell they act on, and growing and shrinking it.

use super::*;

impl Document {
    /// The table the table tools are about, if any: the one whose cell the caret is in, or, with
    /// nothing typed, the table selected, which they only add to at its end. It's worked out once for
    /// each state of the text, the text being typed and the thing selected.
    pub fn table_here(&self) -> Option<TableHere> {
        let key = (
            self.deck.text_version(),
            self.editing.as_ref().map(|editing| (editing.slide, editing.flow.clone())),
            self.selected.clone(),
        );
        if let Some((seen, here)) = &*self.table_here.borrow()
            && *seen == key
        {
            return here.clone();
        }
        let here = guarded(|| self.work_out_table_here()).ok().flatten();
        *self.table_here.borrow_mut() = Some((key, here.clone()));
        here
    }

    fn work_out_table_here(&self) -> Option<TableHere> {
        let content = self.content_tags();
        let talk = self.talk();
        let (slide, table, cell) = match &self.editing {
            Some(editing) => {
                if !editing.is_cell() || editing.slide >= self.slide_count() {
                    return None;
                }
                let flows = flows_in(talk, editing.slide, &content);
                let FlowPlace::Cell { table, row, column } = &flows.get(editing.flow.find(&flows)?)?.place else {
                    return None;
                };
                (editing.slide, table.clone(), Some((*row, *column)))
            }
            None => {
                // A table moved out of the text is selected by the box that places it.
                let selected = self.selected.as_ref()?;
                let table = match selection::block_at(&talk.slides.get(selected.slide)?.blocks, &selected.source)? {
                    Block::Tag(tag) if tag.name == "Box" && matches!(tag.children.as_slice(), [Block::Table { .. }]) => tag.children[0].range().clone(),
                    _ => selected.source.clone(),
                };
                (selected.slide, table, None)
            }
        };
        let Some(Block::Table { rows, .. }) = selection::block_at(&talk.slides.get(slide)?.blocks, &table) else {
            return None;
        };
        let (columns, count) = table_size(talk, slide, &table)?;
        let long_row = rows.iter().any(|row| row.len() > columns);
        let (row, column) = cell.unwrap_or((count.saturating_sub(1), columns.saturating_sub(1)));
        Some(TableHere { slide, table, row, column, caret: cell.is_some(), columns, rows: count, long_row })
    }

    /// The table whose cell the caret is in, if it's in one: the table's whole source, and the
    /// cell's row, the heading row being row 0, and column.
    pub fn table_at_caret(&self) -> Option<(Range<usize>, usize, usize)> {
        self.table_here().filter(|here| here.caret).map(|here| (here.table, here.row, here.column))
    }

    /// Adds an empty row to the table the tools are about, under the cell's row, or with `below`
    /// false over it, as one step named "Add row", and returns the slides to show again. The
    /// caret goes to the new row, in the same column. Undone, it goes back where it was.
    pub fn add_row(&mut self, below: bool) -> Result<Vec<usize>, EditRefused> {
        let tool = if below { TableTool::RowBelow } else { TableTool::RowAbove };
        let Some(here) = self.table_for(tool)? else {
            return Ok(Vec::new());
        };
        let after = if below { here.row } else { here.row - 1 };
        let to = (after + 1, here.column);
        self.change_table(ADD_ROW, &here, |talk, slide, table| slides_format::add_row(talk, slide, table, after), to)
    }

    /// Adds an empty column to the table the tools are about, after the cell's column, or with
    /// `right` false before it, as one step named "Add column", the caret going to the new
    /// column, in the same row, as [`Self::add_row`] does.
    pub fn add_column(&mut self, right: bool) -> Result<Vec<usize>, EditRefused> {
        let tool = if right { TableTool::ColumnRight } else { TableTool::ColumnLeft };
        let Some(here) = self.table_for(tool)? else {
            return Ok(Vec::new());
        };
        let after = if right { Some(here.column) } else { here.column.checked_sub(1) };
        let to = (here.row, after.map_or(0, |after| after + 1));
        self.change_table(ADD_COLUMN, &here, |talk, slide, table| slides_format::add_column(talk, slide, table, after), to)
    }

    /// Takes the cell's row away from the table the tools are about, as one step named "Delete
    /// row", the caret going to the row that took its place, or the one above for the last.
    pub fn remove_row(&mut self) -> Result<Vec<usize>, EditRefused> {
        let Some(here) = self.table_for(TableTool::DeleteRow)? else {
            return Ok(Vec::new());
        };
        let row = if here.row + 1 < here.rows { here.row } else { here.row - 1 };
        let to = (row, here.column);
        self.change_table(DELETE_ROW, &here, |talk, slide, table| slides_format::remove_row(talk, slide, table, here.row), to)
    }

    /// Takes the cell's column away from the table the tools are about, as one step named
    /// "Delete column", the caret going to the column that took its place, or the one before for
    /// the last.
    pub fn remove_column(&mut self) -> Result<Vec<usize>, EditRefused> {
        let Some(here) = self.table_for(TableTool::DeleteColumn)? else {
            return Ok(Vec::new());
        };
        let column = if here.column + 1 < here.columns { here.column } else { here.column.saturating_sub(1) };
        let to = (here.row, column);
        self.change_table(DELETE_COLUMN, &here, |talk, slide, table| slides_format::remove_column(talk, slide, table, here.column), to)
    }

    /// The table the tools are about, if there's one and `tool` can be used on it.
    fn table_for(&self, tool: TableTool) -> Result<Option<TableHere>, EditRefused> {
        let Some(here) = self.table_here() else {
            return Ok(None);
        };
        match here.why_not(tool) {
            Some(refused) => Err(refused),
            None => Ok(Some(here)),
        }
    }

    /// Makes the change `change` works out for the table `here` is about, as one step named
    /// `label`. If the caret was in one of its cells, it goes to the cell at `to`, or one near
    /// it, and the step goes back to where it was once undone; if the table was selected, it
    /// stays selected.
    fn change_table(
        &mut self,
        label: &'static str,
        here: &TableHere,
        change: impl FnOnce(&Talk, usize, &Range<usize>) -> Option<Change>,
        to: (usize, usize),
    ) -> Result<Vec<usize>, EditRefused> {
        let slide = here.slide;
        let was = self
            .editing
            .as_ref()
            .filter(|_| here.caret)
            .map(|editing| (editing.flow.clone(), editing.edit.selection, editing.session));
        // Typing is finished before the change is worked out: that can write a colour held for
        // the cell, which moves what's after it. The table is found again where it starts.
        self.finish_before_edit()?;
        let table = table_starting_at(self.talk(), slide, here.table.start).ok_or(EditRefused::CouldNotBeMade)?;
        let change = built(self.talk(), |talk| change(talk, slide, &table))?.ok_or(EditRefused::CouldNotBeMade)?;
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        let made = self.history.depth() > depth;
        let start = here.table.start;
        match was {
            Some((flow, before, session)) => {
                if self.land_in_cell(slide, start, to, false)
                    && made
                    && let Some(editing) = &self.editing
                {
                    let mark = TextMark {
                        slide,
                        flow,
                        before,
                        after: Some(editing.edit.selection),
                        flow_after: Some(editing.flow.clone()),
                        session,
                    };
                    self.history.mark_last(mark);
                }
            }
            None => {
                if let Some(after) = table_starting_at(self.talk(), slide, start) {
                    self.set_selection(Some(selection::Selection::block(slide, after.clone())));
                    if made {
                        self.history.select_mark_last(SelectMark::blocks(slide, Some(table), Some(after)));
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Starts typing in the cell at `(row, column)` of the table that starts at byte `start` of
    /// slide `slide`, at its start, or with `end` at its end. A cell there that can't be typed
    /// in, or that isn't there, gives way to the nearest of that row that can, or else the
    /// table's first. Says whether typing started.
    pub(super) fn land_in_cell(&mut self, slide: usize, start: usize, (row, column): (usize, usize), end: bool) -> bool {
        let content = self.content_tags();
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let cells: Vec<(usize, usize, usize)> = flows
            .iter()
            .enumerate()
            .filter_map(|(index, flow)| match &flow.place {
                FlowPlace::Cell { table, row, column } if table.start == start => Some((index, *row, *column)),
                _ => None,
            })
            .collect();
        let found = cells
            .iter()
            .find(|cell| (cell.1, cell.2) == (row, column))
            .or_else(|| cells.iter().filter(|cell| cell.1 == row).min_by_key(|cell| cell.2.abs_diff(column)))
            .or_else(|| cells.first());
        let Some(&(index, ..)) = found else {
            return false;
        };
        let pos = if end { end_of(&flows[index].paras) } else { Pos::default() };
        self.begin_text(slide, index, pos)
    }

    /// Moves the caret on from the cell it's in, as Tab does with `Next`, Shift+Tab with
    /// `Previous` and Enter with `Below`, to the end of what that cell says: cells that can't be
    /// typed in, and those a short row doesn't have, are gone past. Tab from the table's last
    /// cell, and Enter from its last row, add a row, as one step named "Add row", and go into it.
    pub fn move_in_table(&mut self, step: CellMove) -> Result<Vec<usize>, EditRefused> {
        let content = self.content_tags();
        let Some(here) = self.table_here().filter(|here| here.caret) else {
            return Ok(Vec::new());
        };
        let beside = match step {
            CellMove::Next => Beside::After,
            CellMove::Previous => Beside::Before,
            CellMove::Below => Beside::Below,
        };
        if let Some(index) = self.cell_beside(beside) {
            let flows = guarded(|| flows_in(self.talk(), here.slide, &content))?;
            let end = end_of(&flows[index].paras);
            self.begin_text(here.slide, index, end);
            return Ok(Vec::new());
        }
        let (after, column) = match step {
            CellMove::Next => (here.rows - 1, 0),
            CellMove::Below if here.row + 1 == here.rows => (here.row, here.column),
            CellMove::Below | CellMove::Previous => return Ok(Vec::new()),
        };
        if here.rows >= MAX_TABLE_ROWS {
            return Err(EditRefused::TooManyRows);
        }
        self.change_table(ADD_ROW, &here, |talk, slide, table| slides_format::add_row(talk, slide, table, after), (after + 1, column))
    }

    /// The flow of the cell `beside` the caret's in its table, if there's one to type in: the
    /// one before or after it in the order they're written, or the nearest above or below it in
    /// its column.
    pub fn cell_beside(&self, beside: Beside) -> Option<usize> {
        let content = self.content_tags();
        let editing = self.editing.as_ref().filter(|editing| editing.is_cell())?;
        let flows = guarded(|| flows_in(self.talk(), editing.slide, &content)).ok()?;
        let at = editing.flow.find(&flows)?;
        let FlowPlace::Cell { table, row, column } = &flows[at].place else {
            return None;
        };
        let (row, column) = (*row, *column);
        let mut cells = flows.iter().enumerate().filter_map(|(index, flow)| match &flow.place {
            FlowPlace::Cell { table: other, row, column } if other == table => Some((index, *row, *column)),
            _ => None,
        });
        let found = match beside {
            Beside::Before => cells.rfind(|cell| cell.0 < at),
            Beside::After => cells.find(|cell| cell.0 > at),
            Beside::Above => cells.filter(|cell| cell.2 == column && cell.1 < row).max_by_key(|cell| cell.1),
            Beside::Below => cells.filter(|cell| cell.2 == column && cell.1 > row).min_by_key(|cell| cell.1),
        };
        found.map(|cell| cell.0)
    }
}

/// The table the table tools are about, and the cell of it they act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableHere {
    pub slide: usize,
    /// The table's whole source.
    pub table: Range<usize>,
    /// The cell's row, the heading row being row 0, and its column: the caret's, or, for a table
    /// selected with nothing typed, its last row and column, where Row below and Column right
    /// add at its end.
    pub row: usize,
    pub column: usize,
    /// Whether the caret is in that cell.
    pub caret: bool,
    /// How many columns the heading row has, and how many rows there are, the heading row with
    /// them.
    pub columns: usize,
    pub rows: usize,
    /// Whether a row has more cells than the heading row, which are in no column.
    pub long_row: bool,
}

/// One of the tools that grow and shrink a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableTool {
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
}

impl TableHere {
    /// Why `tool` can't be used on this table, at this cell, if it can't.
    pub fn why_not(&self, tool: TableTool) -> Option<EditRefused> {
        use TableTool::*;
        match tool {
            // A table selected, with no caret, is only added to at its end.
            RowAbove | ColumnLeft | DeleteRow | DeleteColumn if !self.caret => Some(EditRefused::NeedsACell),
            RowAbove | DeleteRow if self.row == 0 => Some(EditRefused::KeepsHeadingRow),
            RowAbove | RowBelow if self.rows >= MAX_TABLE_ROWS => Some(EditRefused::TooManyRows),
            ColumnLeft | ColumnRight | DeleteColumn if self.long_row => Some(EditRefused::RowTooLong),
            ColumnLeft | ColumnRight if self.columns >= MAX_TABLE_COLUMNS => Some(EditRefused::TooManyColumns),
            DeleteColumn if self.columns <= 1 => Some(EditRefused::KeepsOneColumn),
            _ => None,
        }
    }
}

/// Where the caret goes from a cell of a table: on, as Tab takes it, back, as Shift+Tab does,
/// or down, as Enter does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellMove {
    Next,
    Previous,
    Below,
}

/// Which cell beside a cell of a table: the one written before it or after it, or the one above
/// or below it in its column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beside {
    Before,
    After,
    Above,
    Below,
}

/// The whole source of the table on slide `slide` of `talk`, at any depth, that starts at byte
/// `start`.
fn table_starting_at(talk: &Talk, slide: usize, start: usize) -> Option<Range<usize>> {
    fn within(blocks: &[Block], start: usize) -> Option<Range<usize>> {
        blocks.iter().find_map(|block| match block {
            Block::Table { range, .. } if range.start == start => Some(range.clone()),
            Block::Tag(tag) => within(&tag.children, start),
            Block::List { items, .. } => items.iter().find_map(|item| within(item, start)),
            _ => None,
        })
    }
    let slide = talk.slides.get(slide).filter(|slide| slide.error.is_none())?;
    within(&slide.blocks, start)
}

/// The whole source of each table on slide `slide` of `talk`, at any depth.
pub(super) fn tables_on(talk: &Talk, slide: usize) -> Vec<Range<usize>> {
    fn within(blocks: &[Block], found: &mut Vec<Range<usize>>) {
        for block in blocks {
            match block {
                Block::Table { range, .. } => found.push(range.clone()),
                Block::Tag(tag) => within(&tag.children, found),
                Block::List { items, .. } => items.iter().for_each(|item| within(item, found)),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    if let Some(slide) = talk.slides.get(slide).filter(|slide| slide.error.is_none()) {
        within(&slide.blocks, &mut found);
    }
    found
}

/// Where the end of `paras` is.
fn end_of(paras: &[Para]) -> Pos {
    TextEdit::new(paras.to_vec()).select_all().selection.end()
}
