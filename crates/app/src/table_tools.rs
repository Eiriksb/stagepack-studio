//! The tools for tables: the Insert tab's grid that chooses a new table's size, and the Table
//! group that adds rows and columns and takes them away. The group shows at the end of the Home
//! tab and in the inspector while the caret is in a cell or a table is selected. Each tool asks
//! the window to do its work, and is off, saying why, when it can't be done.
//!
//! A press on the tools leaves the keyboard where it is, so typing in the cell goes on.

use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::document::{TableHere, TableTool};
use crate::ribbon::group;

pub const TABLE_ROW_ABOVE_ID: &str = "table-row-above";
pub const TABLE_ROW_BELOW_ID: &str = "table-row-below";
pub const TABLE_COLUMN_LEFT_ID: &str = "table-column-left";
pub const TABLE_COLUMN_RIGHT_ID: &str = "table-column-right";
pub const TABLE_DELETE_ROW_ID: &str = "table-delete-row";
pub const TABLE_DELETE_COLUMN_ID: &str = "table-delete-column";
/// What holds the Table group, on the Home tab and in the inspector.
pub const TABLE_TOOLS_ID: &str = "table-tools";
/// The grid of sizes the Insert tab's Table tool opens.
pub const TABLE_GRID_ID: &str = "table-grid";
/// What says under the grid how big a table the square pointed at gives.
pub const TABLE_GRID_SIZE_ID: &str = "table-grid-size";

/// How many columns and rows the grid offers, the rows under the heading row.
pub const GRID_COLUMNS: usize = 8;
pub const GRID_ROWS: usize = 6;

/// What the tools say when they can't be used.
pub const HEADING_ROW_STAYS: &str = "A table keeps its heading row.";
pub const ONE_COLUMN_STAYS: &str = "A table keeps at least one column.";
pub const MOST_COLUMNS: &str = "A table can have 20 columns at most.";
pub const MOST_ROWS: &str = "A table can have 50 rows at most.";
pub const CLICK_IN_A_CELL: &str = "Click in a cell first.";
pub const LONG_ROW: &str = "This table has a row with more cells than its heading, so columns can't be changed here.";

/// What the grid says before a square is pointed at.
const GRID_HINT: &str = "Choose the table's size";

/// The id of the grid's square that gives a table of `columns` columns and `rows` rows under
/// its heading row.
pub fn grid_id(columns: usize, rows: usize) -> SharedString {
    format!("table-grid-{columns}-{rows}").into()
}

actions!(
    table_tools,
    [TableRowAbove, TableRowBelow, TableColumnLeft, TableColumnRight, TableDeleteRow, TableDeleteColumn]
);

/// A square of the grid was clicked: a table of `columns` columns and `rows` rows under its
/// heading row is to go on the slide.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InsertTable {
    pub columns: usize,
    pub rows: usize,
}

impl EventEmitter<InsertTable> for TableGrid {}

/// A tool of the Table group: which it is, its id, icon and name, what it says, and what it asks.
type Tool = (TableTool, &'static str, Icons, &'static str, &'static str, Box<dyn Action>);

/// The tools of the Table group.
fn tools() -> [Tool; 6] {
    [
        (TableTool::RowAbove, TABLE_ROW_ABOVE_ID, Icons::BetweenHorizontalStart, "Row above", "Put a row above this one", TableRowAbove.boxed_clone()),
        (TableTool::RowBelow, TABLE_ROW_BELOW_ID, Icons::BetweenHorizontalEnd, "Row below", "Put a row below this one", TableRowBelow.boxed_clone()),
        (TableTool::ColumnLeft, TABLE_COLUMN_LEFT_ID, Icons::BetweenVerticalStart, "Column left", "Put a column to the left of this one", TableColumnLeft.boxed_clone()),
        (TableTool::ColumnRight, TABLE_COLUMN_RIGHT_ID, Icons::BetweenVerticalEnd, "Column right", "Put a column to the right of this one", TableColumnRight.boxed_clone()),
        (TableTool::DeleteRow, TABLE_DELETE_ROW_ID, Icons::Trash, "Delete row", "Take this row away", TableDeleteRow.boxed_clone()),
        (TableTool::DeleteColumn, TABLE_DELETE_COLUMN_ID, Icons::Trash, "Delete column", "Take this column away", TableDeleteColumn.boxed_clone()),
    ]
}

/// The tools for the table `here` is about, each off, saying why, when it can't be used there.
fn buttons(here: &TableHere) -> Vec<AnyElement> {
    tools()
        .into_iter()
        .map(|(tool, id, icon, label, tip, action)| {
            let why = here.why_not(tool);
            // A table selected, with no caret in it, is added to at its end.
            let tip = match tool {
                TableTool::RowBelow if !here.caret => "Add a row at the end",
                TableTool::ColumnRight if !here.caret => "Add a column at the end",
                _ => tip,
            };
            let tip: SharedString = why.map_or_else(|| tip.into(), |why| why.to_string().into());
            Button::new(id)
                .ghost()
                .small()
                .icon(icon)
                .label(label)
                .tooltip(tip)
                .disabled(why.is_some())
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
                .into_any_element()
        })
        .collect()
}

/// What holds the tools, `child`, across the inspector if `wide`, or down the ribbon: a press
/// on it, a tool that's off among them, leaves the keyboard where it is.
fn holder(wide: bool, child: impl IntoElement) -> AnyElement {
    div()
        .id(TABLE_TOOLS_ID)
        .test_support()
        .when(wide, |this| this.w_full())
        .when(!wide, |this| this.h_full())
        .capture_any_mouse_down(|_, window, _| window.prevent_default())
        .child(child)
        .into_any_element()
}

/// The Table group at the end of the Home tab, for the table `here` is about.
pub fn ribbon_group(here: &TableHere, cx: &App) -> AnyElement {
    holder(false, group("Table", buttons(here), cx))
}

/// The Table tools in the inspector, for the table `here` is about.
pub fn inspector_group(here: &TableHere, cx: &App) -> AnyElement {
    holder(
        true,
        v_flex()
            .w_full()
            .gap_1()
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Rows and columns"))
            .child(h_flex().w_full().flex_wrap().gap_1().children(buttons(here))),
    )
}

/// The grid the Insert tab's Table tool opens: 8 columns by 6 rows of squares, a click on one
/// putting a table of that many columns and rows under its heading row on the slide.
#[derive(Default)]
pub struct TableGrid {
    open: bool,
    /// The square pointed at: its columns and rows.
    pointed: Option<(usize, usize)>,
}

impl TableGrid {
    pub fn new() -> Self {
        TableGrid::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            self.pointed = None;
            cx.notify();
        }
    }

    fn point_at(&mut self, at: Option<(usize, usize)>, cx: &mut Context<Self>) {
        if self.pointed != at {
            self.pointed = at;
            cx.notify();
        }
    }
}

/// What the grid says of a table of `columns` columns and `rows` rows, such as "3 × 4 table".
pub fn size_name(columns: usize, rows: usize) -> String {
    format!("{columns} × {rows} table")
}

impl Render for TableGrid {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pointed = self.pointed;
        let (lit, edge) = (cx.theme().primary.opacity(0.35), cx.theme().border);
        let lit_edge = cx.theme().primary;
        let rows = (1..=GRID_ROWS).map(|rows| {
            h_flex().gap(px(3.)).children((1..=GRID_COLUMNS).map(|columns| {
                let on = pointed.is_some_and(|(c, r)| columns <= c && rows <= r);
                let name = size_name(columns, rows);
                div()
                    .id(grid_id(columns, rows))
                    .test_support()
                    .aria_label(SharedString::from(name))
                    .size(px(18.))
                    .border_1()
                    .border_color(if on { lit_edge } else { edge })
                    .when(on, |this| this.bg(lit))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.point_at(Some((columns, rows)), cx);
                        } else if this.pointed == Some((columns, rows)) {
                            this.point_at(None, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.emit(InsertTable { columns, rows });
                        this.set_open(false, cx);
                    }))
            }))
        });
        let said: SharedString = pointed.map_or_else(|| GRID_HINT.to_string(), |(c, r)| size_name(c, r)).into();
        v_flex()
            .id(TABLE_GRID_ID)
            .test_support()
            .p_1()
            .gap(px(3.))
            .children(rows)
            .child(
                div()
                    .id(TABLE_GRID_SIZE_ID)
                    .test_support()
                    .aria_label(said.clone())
                    .pt_1()
                    .text_sm()
                    .child(said),
            )
    }
}

