//! Tables on the slide: one put on it from the Insert tab, typed in cell by cell, walked with
//! Tab, Enter and the arrows, and grown and shrunk a row or a column at a time, one undo step
//! each.

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    ClipboardEntry, ClipboardItem, Image, ImageFormat, Context, ElementId, Entity, EntityInputHandler as _, InputEvent as _, Modifiers, MouseMoveEvent,
    TestAppContext, Window,
};
use slides_engine::Hit;
use slides_format::{FlowPlace, Pos, new_table};
use stagepack_studio::insert_tools::INSERT_TABLE_ID;
use stagepack_studio::inspector::{INSPECTOR_HEADING_ID, INSPECTOR_ID, INSPECTOR_MESSAGE_ID};
use stagepack_studio::ribbon::{MORE_RIGHT_ID, RIBBON_ID, TAB_INSERT_ID};
use stagepack_studio::slide_editor::{EditorEvent, SlideEditor};
use stagepack_studio::table_tools::{
    CLICK_IN_A_CELL, HEADING_ROW_STAYS, LONG_ROW, MOST_COLUMNS, MOST_ROWS, ONE_COLUMN_STAYS, TABLE_COLUMN_LEFT_ID,
    TABLE_COLUMN_RIGHT_ID, TABLE_DELETE_COLUMN_ID, TABLE_DELETE_ROW_ID, TABLE_GRID_SIZE_ID, TABLE_ROW_ABOVE_ID,
    TABLE_ROW_BELOW_ID, TABLE_TOOLS_ID, grid_id,
};
use stagepack_studio::text_editing::FlowKey;
use stagepack_studio::text_tools::{
    ALIGN_CENTER_ID, ALIGN_LEFT_ID, ALIGN_RIGHT_ID, BOLD_ID, BULLETS_ID, INDENT_LESS_ID, INDENT_MORE_ID,
    NOT_IN_A_TABLE, NUMBERING_ID, button_id,
};

const TABLE: &str = "# T\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
const TALK: &str = "---\ntitle: T\n---\n\n# One\n\nSome words\n\n---\n\n# Two\n";
const SHOWCASE: &str = include_str!("../../../fixtures/showcase/slides.md");
/// The showcase's slide of shapes, an icon, a picture and a table.
const THINGS: usize = 5;
const NOT_MADE: &str = "That change couldn't be made, so nothing was changed.";
const BROKE_A_SLIDE: &str = "That change would have broken a slide, so it wasn't made.";
const CELL_WITH_MORE: &str = "This cell holds more than text, so it can't be typed in yet.";

/// The talk `source` in a window that's the active one, as a window being used is.
fn open(cx: &mut TestAppContext, source: &str) -> Typing {
    let typing = typing::open(cx, source);
    typing.with_window(cx, |window, _| window.activate_window());
    typing.settle(cx);
    typing
}

/// The flow of the current slide that's the cell at `row` and `column` of its first table.
fn cell_flow(typing: &Typing, cx: &TestAppContext, row: usize, column: usize) -> usize {
    let slide = typing.current(cx);
    typing
        .flows(cx, slide)
        .iter()
        .position(|flow| matches!(flow.place, FlowPlace::Cell { row: r, column: c, .. } if (r, c) == (row, column)))
        .unwrap_or_else(|| panic!("no cell at {row}, {column}"))
}

/// What the cell at `row` and `column` of the current slide's first table says.
fn cell_text(typing: &Typing, cx: &TestAppContext, row: usize, column: usize) -> String {
    let flow = cell_flow(typing, cx, row, column);
    typing.text_of(cx, typing.current(cx), flow).concat()
}

/// Clicks at the end of the text of the cell at `row` and `column`.
fn click_cell(typing: &Typing, cx: &mut TestAppContext, row: usize, column: usize) {
    let flow = cell_flow(typing, cx, row, column);
    let end = cell_text(typing, cx, row, column).len();
    typing.click_text(cx, flow, pos(0, end));
    assert_eq!(at_cell(typing, cx), Some((row, column)), "the click types in the cell");
}

/// The cell the caret is in, if it's in one.
fn at_cell(typing: &Typing, cx: &TestAppContext) -> Option<(usize, usize)> {
    match typing.editing(cx)?.flow {
        FlowKey::Cell { row, column, .. } => Some((row, column)),
        _ => None,
    }
}

/// Clicks tool `id` in what `scope` holds: the ribbon, scrolled along to it, or the inspector.
fn click_in(typing: &Typing, cx: &mut TestAppContext, scope: &'static str, id: &'static str) {
    if scope == RIBBON_ID {
        for _ in 0..10 {
            let (tool, ribbon) = typing.with_window(cx, |window, cx| {
                window.render_frame(cx);
                (window.within(RIBBON_ID).find(id).bounds(), window.find(RIBBON_ID).bounds())
            });
            let more = typing.with_window(cx, |window, _| window.try_find(MORE_RIGHT_ID).is_some());
            if tool.right() <= ribbon.right() - gpui_kit::px(28.) || !more {
                break;
            }
            click_on(typing, cx, MORE_RIGHT_ID);
        }
    }
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(scope).click(id, cx);
    });
    typing.settle(cx);
}

fn shown_in(typing: &Typing, cx: &mut TestAppContext, scope: &'static str, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(scope).try_find(id).is_some()
    })
}

fn disabled(typing: &Typing, cx: &mut TestAppContext, scope: &'static str, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(scope).find(id).disabled() == Some(true)
    })
}

/// What tool `id` says when it's pointed at.
fn tip(typing: &Typing, cx: &mut TestAppContext, scope: &'static str, id: impl Into<ElementId>) -> Option<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(scope).find(id).description().map(str::to_string)
    })
}

fn label_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

fn undo(typing: &Typing, cx: &mut TestAppContext) {
    typing.press(cx, "ctrl-z");
}

fn redo(typing: &Typing, cx: &mut TestAppContext) {
    typing.press(cx, "ctrl-y");
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn selected(typing: &Typing, cx: &TestAppContext) -> Option<std::ops::Range<usize>> {
    typing.document(cx).read_with(cx, |document, _| document.selected().map(|selected| selected.source.clone()))
}

fn editor(typing: &Typing, cx: &TestAppContext) -> Entity<SlideEditor> {
    cx.read(|cx| typing.view.read(cx).slide_editor().clone())
}

/// Does `ask` of the editor as the system's input method would, and draws.
fn system<T>(
    typing: &Typing,
    cx: &mut TestAppContext,
    ask: impl FnOnce(&mut SlideEditor, &mut Window, &mut Context<SlideEditor>) -> T,
) -> T {
    let editor = editor(typing, cx);
    let answer = typing.with_window(cx, |window, cx| editor.update(cx, |editor, cx| ask(editor, window, cx)));
    typing.settle(cx);
    answer
}

/// A point on the line under the first cell of the current slide's first table, which a click
/// on picks the table.
fn border_point(typing: &Typing, cx: &mut TestAppContext) -> gpui_kit::Point<gpui_kit::Pixels> {
    let layout = typing.layout(cx);
    let slide = typing.current(cx);
    let table = typing
        .flows(cx, slide)
        .iter()
        .find_map(|flow| match &flow.place {
            FlowPlace::Cell { table, .. } => Some(table.clone()),
            _ => None,
        })
        .expect("a table");
    let rect = layout.cell_rect(&table, 0, 0).expect("the first cell");
    let x = rect.x + rect.w / 2.;
    // The middle of the band around the line, which a click lands in whatever the rounding.
    let band: Vec<f32> = (0..120)
        .map(|step| rect.y + rect.h - 6. + step as f32 * 0.1)
        .filter(|&y| matches!(layout.hit(x, y), Some(Hit::Element(element)) if element.source == table))
        .collect();
    assert!(!band.is_empty(), "the line under the cell picks the table");
    typing.at(cx, x, band[band.len() / 2])
}

/// Opens the Insert tab's grid of table sizes.
fn open_grid(typing: &Typing, cx: &mut TestAppContext) {
    click_on(typing, cx, TAB_INSERT_ID);
    click_on(typing, cx, INSERT_TABLE_ID);
}

/// Moves the mouse onto square `id` of the grid.
fn hover(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let bounds = window.find(id).bounds();
        window.dispatch_event(
            MouseMoveEvent { position: bounds.center(), pressed_button: None, modifiers: Modifiers::default() }
                .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    });
    typing.settle(cx);
}

// ---------------------------------------------------------------------------------------
// Inserting one

#[gpui_kit::test]
fn the_grid_inserts_that_many_cells_and_takes_the_caret(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    open_grid(&typing, cx);
    hover(&typing, cx, grid_id(3, 4));
    assert_eq!(label_of(&typing, cx, TABLE_GRID_SIZE_ID).as_deref(), Some("3 × 4 table"));
    hover(&typing, cx, grid_id(8, 6));
    assert_eq!(label_of(&typing, cx, TABLE_GRID_SIZE_ID).as_deref(), Some("8 × 6 table"));
    hover(&typing, cx, grid_id(3, 4));
    click_on(&typing, cx, grid_id(3, 4));
    let table = new_table(3, 4).unwrap();
    assert_eq!(typing.source(cx), TALK.replacen("Some words", &format!("Some words\n\n{table}"), 1));
    assert_eq!(table.lines().count(), 6, "a heading row, the rule and four rows");
    assert_eq!(at_cell(&typing, cx), Some((0, 0)), "the caret is in the first heading cell");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 0)));
    assert!(typing.caret(cx).is_some());
    assert!(selected(&typing, cx).is_none());
    assert_eq!(undo_label(&typing, cx), Some("Insert table"));
    typing.type_text(cx, "Name");
    assert!(typing.source(cx).contains("| Name |  |  |\n| --- |"), "{}", typing.source(cx));
    undo(&typing, cx);
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TALK, "one step for the table");
    assert!(!can_undo(&typing, cx));
    redo(&typing, cx);
    assert_eq!(typing.source(cx), TALK.replacen("Some words", &format!("Some words\n\n{table}"), 1));
}

#[gpui_kit::test]
fn a_table_that_does_not_fit_on_the_slide_is_refused(cx: &mut TestAppContext) {
    // A slide just short of the most a slide can hold.
    let words = "word ".repeat((slides_format::MAX_SLIDE_BYTES - 40) / 5);
    let source = format!("# T\n\n{}\n", words.trim_end());
    let typing = open(cx, &source);
    open_grid(&typing, cx);
    hover(&typing, cx, grid_id(8, 6));
    click_on(&typing, cx, grid_id(8, 6));
    assert_eq!(typing.source(cx), source);
    assert!(!can_undo(&typing, cx));
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_MADE));
}

#[gpui_kit::test]
fn a_row_or_a_column_that_does_not_fit_on_the_slide_is_refused(cx: &mut TestAppContext) {
    // A slide a few bytes short of the most a slide can hold.
    // The words are in the notes, so the table is on the slide where it can be clicked.
    let table = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let frame = "# T\n\n\n<Notes>\n\n</Notes>\n";
    let short = slides_format::MAX_SLIDE_BYTES - frame.len() - table.len() - 3;
    let words = "word ".repeat(short / 5 + 1);
    let source = format!("# T\n\n{table}\n<Notes>\n{}\n</Notes>\n", &words[..short]);
    assert_eq!(source.len(), slides_format::MAX_SLIDE_BYTES - 3);
    let typing = open(cx, &source);
    for tool in [TABLE_ROW_BELOW_ID, TABLE_COLUMN_RIGHT_ID] {
        click_cell(&typing, cx, 1, 0);
        click_in(&typing, cx, RIBBON_ID, tool);
        assert_eq!(typing.source(cx), source, "{tool}");
        assert!(!can_undo(&typing, cx), "{tool}");
        assert_eq!(typing.status(cx).as_deref(), Some(NOT_MADE), "{tool}");
    }
}

#[gpui_kit::test]
fn the_new_table_takes_the_caret_wherever_it_goes(cx: &mut TestAppContext) {
    let only_settings = "---\ntitle: T\n---\n\n# One\n\n---\nlayout: content\n---\n";
    let with_notes = "# One\n\nWords\n\n<Notes>\nSay this.\n</Notes>\n";
    let two_columns = "---\nlayout: two-columns\n---\n\n# T\n\nLeft\n\n<Region name=\"right\" />\n\nRight\n";
    for (source, slide) in [(only_settings, 1), (with_notes, 0), (two_columns, 0)] {
        let typing = open(cx, source);
        for _ in 0..slide {
            typing.press(cx, "pagedown");
        }
        open_grid(&typing, cx);
        hover(&typing, cx, grid_id(2, 1));
        click_on(&typing, cx, grid_id(2, 1));
        assert!(typing.source(cx).contains(&new_table(2, 1).unwrap()), "{source}: {}", typing.source(cx));
        assert_eq!(at_cell(&typing, cx), Some((0, 0)), "{source}: in the first heading cell");
        assert_eq!(typing.selection(cx), caret_at(pos(0, 0)));
        typing.type_text(cx, "H");
        assert!(typing.source(cx).contains("| H |  |\n| --- | --- |"), "{source}: {}", typing.source(cx));
        if source == with_notes {
            assert!(typing.source(cx).ends_with("<Notes>\nSay this.\n</Notes>\n"), "the notes stay last");
        }
    }
}

// ---------------------------------------------------------------------------------------
// Typing in cells

#[gpui_kit::test]
fn typing_in_a_cell_writes_only_that_cell(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 0);
    typing.type_text(cx, "x");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1x |", 1));
    typing.press(cx, "backspace");
    typing.press(cx, "backspace");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "|  |", 1), "an emptied cell is just empty");
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "|  |", 1), "and stays there once typing ends");
}

#[gpui_kit::test]
fn tab_and_shift_tab_walk_the_cells(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 0, 0);
    typing.press(cx, "tab");
    assert_eq!(at_cell(&typing, cx), Some((0, 1)));
    typing.press(cx, "tab");
    assert_eq!(at_cell(&typing, cx), Some((1, 0)));
    assert_eq!(typing.selection(cx), caret_at(pos(0, 1)), "at the end of what the cell says");
    typing.press(cx, "shift-tab");
    assert_eq!(at_cell(&typing, cx), Some((0, 1)));
    typing.press(cx, "shift-tab");
    typing.press(cx, "shift-tab");
    assert_eq!(at_cell(&typing, cx), Some((0, 0)), "the first cell is as far back as it goes");
    assert_eq!(typing.source(cx), TABLE, "walking writes nothing");

    // A cell that holds more than text, and a row too short to have a cell, are gone past.
    let source = "# T\n\n| a | b |\n| --- | --- |\n| <Icon name=\"star\" /> | 2 |\n| 3 |\n| 5 | 6 |\n";
    let typing = open(cx, source);
    click_cell(&typing, cx, 0, 1);
    typing.press(cx, "tab");
    assert_eq!(at_cell(&typing, cx), Some((1, 1)));
    typing.press(cx, "tab");
    assert_eq!(at_cell(&typing, cx), Some((2, 0)));
    typing.press(cx, "tab");
    assert_eq!(at_cell(&typing, cx), Some((3, 0)));
    typing.press(cx, "shift-tab");
    typing.press(cx, "shift-tab");
    typing.press(cx, "shift-tab");
    assert_eq!(at_cell(&typing, cx), Some((0, 1)));
    assert_eq!(typing.source(cx), source, "no row is filled out");
}

#[gpui_kit::test]
fn tab_in_the_last_cell_adds_a_row(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 1);
    typing.press(cx, "tab");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 | 2 |", "| 1 | 2 |\n|  |  |", 1));
    assert_eq!(at_cell(&typing, cx), Some((2, 0)), "the caret is in the new row's first cell");
    typing.type_text(cx, "3");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 | 2 |", "| 1 | 2 |\n| 3 |  |", 1));
    undo(&typing, cx);
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TABLE, "the row was one step");
    assert_eq!(at_cell(&typing, cx), Some((1, 1)), "and undone, the caret is back where Tab was pressed");
}

#[gpui_kit::test]
fn enter_goes_down_a_row(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 0, 1);
    typing.press(cx, "enter");
    assert_eq!(at_cell(&typing, cx), Some((1, 1)));
    assert_eq!(typing.source(cx), TABLE);
    typing.press(cx, "enter");
    assert_eq!(at_cell(&typing, cx), Some((2, 1)), "in the last row, Enter adds one");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 | 2 |", "| 1 | 2 |\n|  |  |", 1));
    // A line break has no place in a cell.
    typing.press(cx, "shift-enter");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 | 2 |", "| 1 | 2 |\n|  |  |", 1));
    assert_eq!(at_cell(&typing, cx), Some((2, 1)));
}

#[gpui_kit::test]
fn the_arrows_go_from_cell_to_cell_at_the_edges(cx: &mut TestAppContext) {
    let source = "# T\n\n| ab | cd |\n| --- | --- |\n| ef | gh |\n";
    let typing = open(cx, source);
    click_cell(&typing, cx, 0, 0);
    typing.press(cx, "right");
    assert_eq!(at_cell(&typing, cx), Some((0, 1)), "Right at a cell's end goes on to the next");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 0)), "at its start");
    typing.press(cx, "left");
    assert_eq!(at_cell(&typing, cx), Some((0, 0)), "Left at a cell's start goes back");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 2)), "to its end");
    typing.press(cx, "down");
    assert_eq!(at_cell(&typing, cx), Some((1, 0)), "Down from a cell's last line goes to the cell below");
    typing.press(cx, "up");
    assert_eq!(at_cell(&typing, cx), Some((0, 0)), "and Up to the one above");
    typing.press(cx, "home");
    assert_eq!(at_cell(&typing, cx), Some((0, 0)), "Home stays in the cell");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 0)));
    typing.press(cx, "backspace");
    assert_eq!(typing.source(cx), source, "Backspace at a cell's start joins nothing");
    typing.press(cx, "end");
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), source, "nor does Delete at its end");
    typing.press(cx, "shift-left");
    typing.press(cx, "shift-left");
    typing.press(cx, "shift-left");
    assert_eq!(at_cell(&typing, cx), Some((0, 0)), "a selection stays in its cell");
    assert_eq!(typing.selection(cx).start(), pos(0, 0));
    typing.press(cx, "ctrl-a");
    assert_eq!(typing.selection(cx).end(), pos(0, 2), "all of the cell");
    assert_eq!(typing.selection(cx).start(), pos(0, 0));
}

#[gpui_kit::test]
fn a_pipe_typed_in_a_cell_stays_in_the_cell(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 0);
    typing.type_text(cx, "a|b");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1a\\|b |", 1));
    assert_eq!(cell_text(&typing, cx, 1, 0), "1a|b");
    assert_eq!(cell_text(&typing, cx, 1, 1), "2", "the table still has two columns");
}

#[gpui_kit::test]
fn a_pasted_list_goes_in_as_one_line(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 0);
    cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo\r\nthree".to_string()));
    typing.press(cx, "ctrl-v");
    assert_eq!(cell_text(&typing, cx, 1, 0), "1one two three");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1one two three |", 1));
    assert_eq!(undo_label(&typing, cx), Some("Paste"));

    // Paragraphs copied in the app, styled, are joined by a space too.
    let source = "# T\n\nFirst **bold**\n\nSecond\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let typing = open(cx, source);
    typing.click_text(cx, 1, pos(0, 0));
    typing.press(cx, "ctrl-a");
    typing.press(cx, "ctrl-c");
    click_cell(&typing, cx, 1, 1);
    typing.press(cx, "ctrl-v");
    assert_eq!(cell_text(&typing, cx, 1, 1), "2First bold Second");
    assert!(typing.source(cx).contains("| 1 | 2First **bold** Second |"), "{}", typing.source(cx));
    // And a new line the system types is a space.
    system(&typing, cx, |editor, window, cx| editor.replace_text_in_range(None, "x\ny", window, cx));
    assert!(typing.source(cx).contains("| 2First **bold** Secondx y |"), "{}", typing.source(cx));
}

#[gpui_kit::test]
fn bold_and_colour_work_in_a_cell(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n\n| a | b |\n| --- | --- |\n| one | 2 |\n");
    click_cell(&typing, cx, 1, 0);
    typing.press(cx, "shift-home");
    assert!(!disabled(&typing, cx, RIBBON_ID, button_id(BOLD_ID)));
    typing.press(cx, "ctrl-b");
    assert_eq!(typing.source(cx), "# T\n\n| a | b |\n| --- | --- |\n| **one** | 2 |\n");
    system(&typing, cx, |editor, _, cx| editor.format(|edit| edit.set_color(Some("#ff0000".into())), cx));
    let flow = cell_flow(&typing, cx, 1, 0);
    let flows = typing.flows(cx, 0);
    let span = &flows[flow].paras[0].spans[0];
    assert!(span.style.bold);
    assert_eq!(span.style.color.as_deref(), Some("#ff0000"));
    assert_eq!(cell_text(&typing, cx, 1, 1), "2", "only that cell changed");
    assert!(typing.source(cx).ends_with("| 2 |\n"));
}

#[gpui_kit::test]
fn bullets_are_off_in_a_cell(cx: &mut TestAppContext) {
    let source = "# T\n\nWords\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let typing = open(cx, source);
    click_cell(&typing, cx, 1, 0);
    for tool in [button_id(BULLETS_ID), button_id(NUMBERING_ID)] {
        assert!(disabled(&typing, cx, RIBBON_ID, tool.clone()), "{tool}");
        assert_eq!(tip(&typing, cx, RIBBON_ID, tool.clone()).as_deref(), Some(NOT_IN_A_TABLE), "{tool}");
    }
    for tool in [INDENT_LESS_ID, INDENT_MORE_ID, ALIGN_LEFT_ID, ALIGN_CENTER_ID, ALIGN_RIGHT_ID] {
        assert!(disabled(&typing, cx, RIBBON_ID, tool), "{tool}");
        assert_eq!(tip(&typing, cx, RIBBON_ID, tool).as_deref(), Some(NOT_IN_A_TABLE), "{tool}");
    }
    click_on(&typing, cx, button_id(BULLETS_ID));
    system(&typing, cx, |editor, _, cx| editor.toggle_list(false, cx));
    system(&typing, cx, |editor, _, cx| editor.align_text(slides_engine::Align::Center, cx));
    assert_eq!(typing.source(cx), source, "nothing makes a cell a list or aligns it");
    assert_eq!(at_cell(&typing, cx), Some((1, 0)), "and typing goes on");
    // Out of the table, they're as they were.
    typing.click_text(cx, 1, pos(0, 1));
    assert!(!disabled(&typing, cx, RIBBON_ID, button_id(BULLETS_ID)));
    assert!(!disabled(&typing, cx, RIBBON_ID, ALIGN_CENTER_ID));
    assert_ne!(tip(&typing, cx, RIBBON_ID, ALIGN_CENTER_ID).as_deref(), Some(NOT_IN_A_TABLE));
}

// ---------------------------------------------------------------------------------------
// Rows and columns

#[gpui_kit::test]
fn each_row_and_column_tool_does_its_one_thing(cx: &mut TestAppContext) {
    let cases: [(&str, &str, (usize, usize)); 6] = [
        (TABLE_ROW_BELOW_ID, "# T\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n|  |  |\n", (2, 0)),
        (TABLE_ROW_ABOVE_ID, "# T\n\n| a | b |\n| --- | --- |\n|  |  |\n| 1 | 2 |\n", (1, 0)),
        (TABLE_COLUMN_LEFT_ID, "# T\n\n|  | a | b |\n| --- | --- | --- |\n|  | 1 | 2 |\n", (1, 0)),
        (TABLE_COLUMN_RIGHT_ID, "# T\n\n| a |  | b |\n| --- | --- | --- |\n| 1 |  | 2 |\n", (1, 1)),
        (TABLE_DELETE_ROW_ID, "# T\n\n| a | b |\n| --- | --- |\n", (0, 0)),
        (TABLE_DELETE_COLUMN_ID, "# T\n\n| b |\n| --- |\n| 2 |\n", (1, 0)),
    ];
    for scope in [RIBBON_ID, INSPECTOR_ID] {
        for (tool, after, caret) in cases {
            let typing = open(cx, TABLE);
            click_cell(&typing, cx, 1, 0);
            assert!(!disabled(&typing, cx, scope, tool), "{tool} in {scope}");
            click_in(&typing, cx, scope, tool);
            assert_eq!(typing.source(cx), after, "{tool} in {scope}");
            assert_eq!(at_cell(&typing, cx), Some(caret), "{tool} in {scope}: where the caret goes");
            assert!(typing.caret(cx).is_some(), "{tool} in {scope}: typing goes on");
            undo(&typing, cx);
            assert_eq!(typing.source(cx), TABLE, "{tool} in {scope}: one step");
            assert_eq!(at_cell(&typing, cx), Some((1, 0)), "{tool} in {scope}: undone, the caret is back");
            assert_eq!(typing.selection(cx), caret_at(pos(0, 1)));
        }
    }
    // Below the last row in a table with spacing of its own, the new row keeps its indent.
    let listed = "# T\n\n- Item\n\n  | a |\n  | - |\n  | 1 |\n";
    let typing = open(cx, listed);
    click_cell(&typing, cx, 1, 0);
    click_in(&typing, cx, RIBBON_ID, TABLE_ROW_BELOW_ID);
    assert_eq!(typing.source(cx), "# T\n\n- Item\n\n  | a |\n  | - |\n  | 1 |\n  |  |\n");
}

#[gpui_kit::test]
fn the_table_tools_show_only_for_a_table(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n\nWords\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    assert!(!shown_in(&typing, cx, RIBBON_ID, TABLE_TOOLS_ID));
    assert!(!shown_in(&typing, cx, INSPECTOR_ID, TABLE_TOOLS_ID));
    typing.click_text(cx, 1, pos(0, 1));
    assert!(!shown_in(&typing, cx, RIBBON_ID, TABLE_TOOLS_ID), "not for other text");
    click_cell(&typing, cx, 1, 1);
    assert!(shown_in(&typing, cx, RIBBON_ID, TABLE_TOOLS_ID));
    assert!(shown_in(&typing, cx, INSPECTOR_ID, TABLE_TOOLS_ID));
    assert_eq!(label_of(&typing, cx, INSPECTOR_HEADING_ID).as_deref(), Some("Table"));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("2 columns, 1 row"), "as the grid counts");
    // The tools follow the table as it grows.
    click_in(&typing, cx, INSPECTOR_ID, TABLE_ROW_BELOW_ID);
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("2 columns, 2 rows"));
    // Selected, the table has them too.
    typing.press(cx, "escape");
    assert!(selected(&typing, cx).is_some(), "Escape in a cell selects its table");
    assert!(typing.editing(cx).is_none());
    assert!(shown_in(&typing, cx, RIBBON_ID, TABLE_TOOLS_ID));
    assert!(shown_in(&typing, cx, INSPECTOR_ID, TABLE_TOOLS_ID));
    typing.press(cx, "escape");
    assert!(selected(&typing, cx).is_none(), "Escape again lets it go");
    assert!(!shown_in(&typing, cx, RIBBON_ID, TABLE_TOOLS_ID));
    assert!(!shown_in(&typing, cx, INSPECTOR_ID, TABLE_TOOLS_ID));
}

#[gpui_kit::test]
fn the_tools_are_off_when_they_cannot_be_done(cx: &mut TestAppContext) {
    // The heading row stays.
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 0, 0);
    for scope in [RIBBON_ID, INSPECTOR_ID] {
        for tool in [TABLE_ROW_ABOVE_ID, TABLE_DELETE_ROW_ID] {
            assert!(disabled(&typing, cx, scope, tool), "{tool}");
            assert_eq!(tip(&typing, cx, scope, tool).as_deref(), Some(HEADING_ROW_STAYS), "{tool}");
        }
        for tool in [TABLE_ROW_BELOW_ID, TABLE_COLUMN_LEFT_ID, TABLE_COLUMN_RIGHT_ID, TABLE_DELETE_COLUMN_ID] {
            assert!(!disabled(&typing, cx, scope, tool), "{tool}");
        }
    }
    click_in(&typing, cx, RIBBON_ID, TABLE_DELETE_ROW_ID);
    assert_eq!(typing.source(cx), TABLE);
    // Any other row can go, the last too: a heading row alone is a table still.
    click_cell(&typing, cx, 1, 0);
    assert!(!disabled(&typing, cx, RIBBON_ID, TABLE_DELETE_ROW_ID));
    click_in(&typing, cx, RIBBON_ID, TABLE_DELETE_ROW_ID);
    assert_eq!(typing.source(cx), "# T\n\n| a | b |\n| --- | --- |\n");
    assert!(disabled(&typing, cx, RIBBON_ID, TABLE_DELETE_ROW_ID));

    // A column stays.
    let typing = open(cx, "# T\n\n| a |\n| --- |\n| 1 |\n");
    click_cell(&typing, cx, 1, 0);
    assert!(disabled(&typing, cx, RIBBON_ID, TABLE_DELETE_COLUMN_ID));
    assert_eq!(tip(&typing, cx, RIBBON_ID, TABLE_DELETE_COLUMN_ID).as_deref(), Some(ONE_COLUMN_STAYS));

    // Twenty columns at most.
    let wide = format!("# T\n\n{}\n", new_table(20, 1).unwrap());
    let typing = open(cx, &wide);
    click_cell(&typing, cx, 1, 0);
    for tool in [TABLE_COLUMN_LEFT_ID, TABLE_COLUMN_RIGHT_ID] {
        assert!(disabled(&typing, cx, RIBBON_ID, tool), "{tool}");
        assert_eq!(tip(&typing, cx, RIBBON_ID, tool).as_deref(), Some(MOST_COLUMNS), "{tool}");
    }
    assert!(!disabled(&typing, cx, RIBBON_ID, TABLE_DELETE_COLUMN_ID));

    // Fifty rows under the heading at most.
    let long = format!("# T\n\n{}\n", new_table(2, 50).unwrap());
    let typing = open(cx, &long);
    click_cell(&typing, cx, 1, 1);
    for tool in [TABLE_ROW_ABOVE_ID, TABLE_ROW_BELOW_ID] {
        assert!(disabled(&typing, cx, RIBBON_ID, tool), "{tool}");
        assert_eq!(tip(&typing, cx, RIBBON_ID, tool).as_deref(), Some(MOST_ROWS), "{tool}");
    }
    // The last row runs off the slide: the caret is put there as a click in it would.
    let last = cell_flow(&typing, cx, 50, 1);
    let document = typing.document(cx);
    cx.update(|cx| document.update(cx, |document, cx| {
        assert!(document.begin_text(0, last, Pos::default()));
        cx.notify();
    }));
    typing.settle(cx);
    assert_eq!(at_cell(&typing, cx), Some((50, 1)));
    let said = said_count(&typing, cx, MOST_ROWS);
    typing.press(cx, "tab");
    assert_eq!(said.0.get(), 1, "said once");
    typing.press(cx, "enter");
    assert_eq!(said.0.get(), 2, "and once again");
    assert_eq!(typing.source(cx), long, "nor does Tab or Enter add one");
    assert_eq!(typing.status(cx).as_deref(), Some(MOST_ROWS));
    assert!(!can_undo(&typing, cx), "and no step is made");

    // A row longer than the heading leaves the columns as they are.
    let ragged = "# T\n\n| a | b |\n| --- | --- |\n| 1 | 2 | 3 |\n";
    let typing = open(cx, ragged);
    click_cell(&typing, cx, 1, 0);
    for tool in [TABLE_COLUMN_LEFT_ID, TABLE_COLUMN_RIGHT_ID, TABLE_DELETE_COLUMN_ID] {
        assert!(disabled(&typing, cx, RIBBON_ID, tool), "{tool}");
        assert_eq!(tip(&typing, cx, RIBBON_ID, tool).as_deref(), Some(LONG_ROW), "{tool}");
    }
    assert!(!disabled(&typing, cx, RIBBON_ID, TABLE_ROW_BELOW_ID), "rows can still be added");
}

// ---------------------------------------------------------------------------------------
// Undo

#[gpui_kit::test]
fn a_run_of_typing_in_a_cell_is_one_undo_step(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 0);
    typing.type_text(cx, "a");
    typing.type_text(cx, "b");
    typing.type_text(cx, "c");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1abc |", 1));
    typing.press(cx, "tab");
    typing.type_text(cx, "d");
    typing.type_text(cx, "e");
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 | 2 |", "| 1abc | 2de |", 1));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1abc |", 1), "the run in the second cell");
    assert_eq!(at_cell(&typing, cx), Some((1, 1)));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TABLE, "the run in the first");
    assert_eq!(at_cell(&typing, cx), Some((1, 0)));
    assert_eq!(typing.selection(cx), caret_at(pos(0, 1)));
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn undo_of_a_new_row_puts_the_caret_back(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 1);
    click_in(&typing, cx, RIBBON_ID, TABLE_ROW_BELOW_ID);
    assert_eq!(at_cell(&typing, cx), Some((2, 1)));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TABLE);
    assert_eq!(at_cell(&typing, cx), Some((1, 1)));
    assert_eq!(typing.selection(cx), caret_at(pos(0, 1)));
    redo(&typing, cx);
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 | 2 |", "| 1 | 2 |\n|  |  |", 1));
    assert_eq!(at_cell(&typing, cx), Some((2, 1)), "made again, the caret is in the new row");
    assert_eq!(undo_label(&typing, cx), Some("Add row"));
}

// ---------------------------------------------------------------------------------------
// Tables written by hand

#[gpui_kit::test]
fn a_hand_written_table_is_typed_in_without_changing_its_spacing(cx: &mut TestAppContext) {
    let source = "# T\n\n|a|  b   |\n|:-|--:|\n| 1|2  |\n";
    let typing = open(cx, source);
    click_cell(&typing, cx, 0, 0);
    typing.type_text(cx, "x");
    click_cell(&typing, cx, 0, 1);
    typing.type_text(cx, "y");
    click_cell(&typing, cx, 1, 1);
    typing.type_text(cx, "z");
    assert_eq!(typing.source(cx), "# T\n\n|ax|  by   |\n|:-|--:|\n| 1|2z  |\n");

    // The showcase's own table.
    let typing = open(cx, SHOWCASE);
    for _ in 0..THINGS {
        typing.press(cx, "pagedown");
    }
    click_cell(&typing, cx, 1, 1);
    typing.type_text(cx, "!");
    assert_eq!(typing.source(cx), SHOWCASE.replacen("| title | title, subtitle |", "| title | title, subtitle! |", 1));
}

#[gpui_kit::test]
fn a_cell_holding_an_icon_says_so(cx: &mut TestAppContext) {
    let source = "# T\n\n| a | b |\n| --- | --- |\n| <Icon name=\"star\" /> | 2 |\n";
    let typing = open(cx, source);
    let layout = typing.layout(cx);
    let slide = typing.current(cx);
    let table = typing
        .flows(cx, slide)
        .iter()
        .find_map(|flow| match &flow.place {
            FlowPlace::Cell { table, .. } => Some(table.clone()),
            _ => None,
        })
        .unwrap();
    let rect = layout.cell_rect(&table, 1, 0).expect("the icon's cell");
    let at = typing.at(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.);
    typing.click(cx, at);
    assert_eq!(typing.status(cx).as_deref(), Some(CELL_WITH_MORE));
    assert!(typing.editing(cx).is_none(), "nothing is typed in");
    assert_eq!(typing.source(cx), source);
}

// ---------------------------------------------------------------------------------------
// The table as a whole

#[gpui_kit::test]
fn a_table_selected_by_its_border_can_be_deleted(cx: &mut TestAppContext) {
    let source = "# T\n\nWords\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let typing = open(cx, source);
    let at = border_point(&typing, cx);
    typing.click(cx, at);
    assert_eq!(typing.status(cx).as_deref(), Some("Table selected"));
    assert!(typing.editing(cx).is_none());
    typing.press(cx, "delete");
    assert_eq!(typing.source(cx), "# T\n\nWords\n");
    assert_eq!(undo_label(&typing, cx), Some("Delete"));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), source);
    assert!(selected(&typing, cx).is_some(), "undone, it's selected again");

    // A click in a cell types in it, and Escape then picks the table.
    let typing = open(cx, source);
    click_cell(&typing, cx, 1, 0);
    assert!(selected(&typing, cx).is_none());
    typing.press(cx, "escape");
    assert_eq!(typing.status(cx).as_deref(), Some("Table selected"));
    assert!(typing.editing(cx).is_none());
    typing.press(cx, "escape");
    assert!(selected(&typing, cx).is_none());
    assert_eq!(typing.source(cx), source);
}

#[gpui_kit::test]
fn a_dead_key_in_a_cell_composes(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 0);
    system(&typing, cx, |editor, window, cx| editor.replace_and_mark_text_in_range(None, "´", None, window, cx));
    assert_eq!(typing.editing(cx).and_then(|editing| editing.composing).map(|composing| composing.text).as_deref(), Some("´"));
    assert_eq!(typing.source(cx), TABLE, "nothing is written while it composes");
    system(&typing, cx, |editor, window, cx| editor.replace_text_in_range(None, "é", window, cx));
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1é |", 1));
    assert_eq!(typing.selection(cx), caret_at(Pos { para: 0, offset: 1 + "é".len() }));
    assert!(typing.editing(cx).is_some_and(|editing| editing.composing.is_none()));
    assert_eq!(at_cell(&typing, cx), Some((1, 0)));
}

// ---------------------------------------------------------------------------------------
// Fix round 1

/// Counts how often the editor says `sentence` from now on.
fn said_count(typing: &Typing, cx: &mut TestAppContext, sentence: &'static str) -> (std::rc::Rc<std::cell::Cell<usize>>, gpui_kit::Subscription) {
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = count.clone();
    let editor = editor(typing, cx);
    let watching = cx.update(|cx| {
        cx.subscribe(&editor, move |_, event: &EditorEvent, _| {
            if matches!(event, EditorEvent::Said(said) if said.as_ref() == sentence) {
                counter.set(counter.get() + 1);
            }
        })
    });
    (count, watching)
}

/// How many columns the current slide's first table has, and how many rows under its heading.
fn size(typing: &Typing, cx: &TestAppContext) -> (usize, usize) {
    let slide = typing.current(cx);
    let cells: Vec<(usize, usize)> = typing
        .flows(cx, slide)
        .iter()
        .filter_map(|flow| match flow.place {
            FlowPlace::Cell { row, column, .. } => Some((row, column)),
            _ => None,
        })
        .collect();
    let columns = cells.iter().filter(|(row, _)| *row == 0).count();
    let rows = cells.iter().map(|(row, _)| *row).max().unwrap_or(0);
    (columns, rows)
}

#[gpui_kit::test]
fn a_colour_held_for_a_cell_is_written_before_a_tool_acts(cx: &mut TestAppContext) {
    // The tool or the key, the cell typed in, what the table then is, and where the caret goes.
    type Cell = (usize, usize);
    type Case = (Option<&'static str>, Option<&'static str>, Cell, Cell, Cell);
    let cases: [Case; 8] = [
        (Some(TABLE_ROW_BELOW_ID), None, (1, 0), (2, 2), (2, 0)),
        (Some(TABLE_ROW_ABOVE_ID), None, (1, 0), (2, 2), (1, 0)),
        (Some(TABLE_COLUMN_LEFT_ID), None, (1, 0), (3, 1), (1, 0)),
        (Some(TABLE_COLUMN_RIGHT_ID), None, (1, 0), (3, 1), (1, 1)),
        (Some(TABLE_DELETE_ROW_ID), None, (1, 0), (2, 0), (0, 0)),
        (Some(TABLE_DELETE_COLUMN_ID), None, (1, 0), (1, 1), (1, 0)),
        (None, Some("tab"), (1, 1), (2, 2), (2, 0)),
        (None, Some("enter"), (1, 1), (2, 2), (2, 1)),
    ];
    for (tool, key, cell, after, caret) in cases {
        let what = tool.or(key).unwrap();
        let typing = open(cx, TABLE);
        click_cell(&typing, cx, cell.0, cell.1);
        typing.press(cx, "shift-home");
        system(&typing, cx, |editor, _, cx| editor.hold_color("#ff0000".into(), cx));
        match (tool, key) {
            (Some(tool), _) => click_in(&typing, cx, RIBBON_ID, tool),
            (_, Some(key)) => typing.press(cx, key),
            _ => unreachable!(),
        }
        assert_ne!(typing.status(cx).as_deref(), Some(BROKE_A_SLIDE), "{what}");
        assert_eq!(size(&typing, cx), after, "{what}: the tool acted");
        assert_eq!(at_cell(&typing, cx), Some(caret), "{what}: where the caret goes");
        undo(&typing, cx);
        assert!(typing.source(cx).contains("#ff0000"), "{what}: the colour was written first: {}", typing.source(cx));
        assert_eq!(size(&typing, cx), (2, 1), "{what}: undone, the table is as it was");
        assert_eq!(at_cell(&typing, cx), Some(cell), "{what}: and the caret is back");
        undo(&typing, cx);
        assert_eq!(typing.source(cx), TABLE, "{what}: the colour was a step of its own");
    }
}

#[gpui_kit::test]
fn a_selected_table_is_added_to_at_its_end(cx: &mut TestAppContext) {
    let source = "# T\n\nWords\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    for scope in [RIBBON_ID, INSPECTOR_ID] {
        let typing = open(cx, source);
        let at = border_point(&typing, cx);
        typing.click(cx, at);
        assert!(selected(&typing, cx).is_some());
        for tool in [TABLE_ROW_ABOVE_ID, TABLE_COLUMN_LEFT_ID, TABLE_DELETE_ROW_ID, TABLE_DELETE_COLUMN_ID] {
            assert!(disabled(&typing, cx, scope, tool), "{tool} in {scope}");
            assert_eq!(tip(&typing, cx, scope, tool).as_deref(), Some(CLICK_IN_A_CELL), "{tool} in {scope}");
        }
        assert_eq!(tip(&typing, cx, scope, TABLE_ROW_BELOW_ID).as_deref(), Some("Add a row at the end"));
        assert_eq!(tip(&typing, cx, scope, TABLE_COLUMN_RIGHT_ID).as_deref(), Some("Add a column at the end"));
        click_in(&typing, cx, scope, TABLE_DELETE_ROW_ID);
        assert_eq!(typing.source(cx), source, "{scope}: an off tool does nothing");
        click_in(&typing, cx, scope, TABLE_ROW_BELOW_ID);
        let grown = "# T\n\nWords\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n|  |  |\n";
        assert_eq!(typing.source(cx), grown, "{scope}");
        let now = typing.source(cx);
        assert_eq!(
            selected(&typing, cx).map(|range| now[range].to_string()).as_deref(),
            Some("| a | b |\n| --- | --- |\n| 1 | 2 |\n|  |  |"),
            "{scope}: it stays selected"
        );
        assert!(typing.editing(cx).is_none(), "{scope}: and nothing is typed in");
        click_in(&typing, cx, scope, TABLE_COLUMN_RIGHT_ID);
        assert_eq!(typing.source(cx), "# T\n\nWords\n\n| a | b |  |\n| --- | --- | --- |\n| 1 | 2 |  |\n|  |  |  |\n", "{scope}");
        assert!(selected(&typing, cx).is_some(), "{scope}");
        undo(&typing, cx);
        undo(&typing, cx);
        assert_eq!(typing.source(cx), source, "{scope}: a step each");
        assert!(selected(&typing, cx).is_some(), "{scope}: undone, it's still selected");
    }
}

#[gpui_kit::test]
fn a_picture_pasted_while_typing_in_a_cell_is_its_own_step(cx: &mut TestAppContext) {
    let typing = open(cx, TABLE);
    click_cell(&typing, cx, 1, 0);
    typing.type_text(cx, "x");
    let picture = image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 0, 0, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture).write_to(&mut png, image::ImageFormat::Png).unwrap();
    cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, png.into_inner()))],
    });
    typing.press(cx, "ctrl-v");
    assert!(typing.editing(cx).is_none(), "typing ends");
    assert_eq!(cell_text(&typing, cx, 1, 0), "1x", "the cell keeps its text");
    assert!(typing.source(cx).contains("<Picture"), "{}", typing.source(cx));
    assert_eq!(undo_label(&typing, cx), Some("Insert picture"));
    undo(&typing, cx);
    assert_eq!(typing.source(cx), TABLE.replacen("| 1 |", "| 1x |", 1), "the picture was its own step");
}
