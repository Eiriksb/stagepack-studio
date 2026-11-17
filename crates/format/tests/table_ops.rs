mod common;

use std::ops::Range;

use common::*;
use proptest::prelude::*;
use slides_format::*;

fn plain(text: &str) -> Span {
    Span { text: text.to_string(), style: Style::default() }
}

fn body(text: &str) -> Vec<Para> {
    let spans = if text.is_empty() { Vec::new() } else { vec![plain(text)] };
    vec![Para { kind: ParaKind::Body, spans }]
}

type Rows = Vec<Vec<Vec<Span>>>;
type Found = (Range<usize>, Rows, Vec<Vec<CellSource>>);
type Wanted<'a> = &'a dyn Fn(&Range<usize>, &Rows) -> bool;

/// The first table on slide `slide` that `wanted` picks: its range, rows and cells.
fn table_where(talk: &Talk, slide: usize, wanted: impl Fn(&Range<usize>, &Rows) -> bool) -> Option<Found> {
    fn walk(blocks: &[Block], wanted: Wanted, out: &mut Option<Found>) {
        for block in blocks {
            match block {
                Block::Table { rows, cells, range, .. } if out.is_none() && wanted(range, rows) => {
                    *out = Some((range.clone(), rows.clone(), cells.clone()));
                }
                Block::List { items, .. } => items.iter().for_each(|item| walk(item, wanted, out)),
                Block::Tag(tag) => walk(&tag.children, wanted, out),
                _ => {}
            }
        }
    }
    let mut found = None;
    walk(&talk.slides.get(slide)?.blocks, &wanted, &mut found);
    found
}

/// The table on slide `slide` whose first header cell holds `TABLE_MARK`.
fn marked_table(talk: &Talk, slide: usize) -> Option<Found> {
    table_where(talk, slide, |_, rows| rows[0].first().is_some_and(|cell| cell == &[plain(TABLE_MARK)]))
}

/// The table on slide `slide` that starts at `start`: where a table stays as it's changed.
fn table_at(talk: &Talk, slide: usize, start: usize) -> Option<Found> {
    table_where(talk, slide, |range, _| range.start == start)
}

/// The flows of the cells of the table at `table` on slide `slide`, by row and column.
fn cell_flows(talk: &Talk, slide: usize, table: &Range<usize>) -> Vec<(usize, usize, TextFlow)> {
    text_flows(talk, slide)
        .into_iter()
        .filter_map(|flow| match &flow.place {
            FlowPlace::Cell { table: own, row, column } if own == table => Some((*row, *column, flow.clone())),
            _ => None,
        })
        .collect()
}

/// `text` with every `\n` made the talk's line ending.
fn in_eol(talk: &str, text: &str) -> String {
    if talk.contains("\r\n") { text.replace('\n', "\r\n") } else { text.to_string() }
}

/// Whether the table's every row has the header's cells and `|` at both ends.
fn is_regular(source: &str, rows: &Rows, cells: &[Vec<CellSource>]) -> bool {
    rows.iter().all(|row| row.len() == rows[0].len())
        && cells.iter().all(|row| {
            let (first, last) = (&row[0], &row[row.len() - 1]);
            source[first.range.clone()].starts_with('|') && source[last.range.clone()].ends_with('|') && last.inline.end < last.range.end
        })
}

/// Whether `text` can be typed in a cell: one line, and nothing the parser reads as another
/// character.
fn typeable(text: &str) -> bool {
    !text.contains(['\n', '\0'])
}

/// What can be typed in a cell: everything that means something to Markdown or to a tag.
fn typed_cell_text() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec![
            "\"", "'", "\"'", "<", "/>", "{", "|", "\n", "---", "`", "&quot;", "\0", "a | b", "|a|", "\\|", "\\", "`|`", " lead", "trail ",
            "<Text>", "**", "# x", "- x", "1. x", ":--", "&#124;", "![i](p.png)", "[l](u)", "  ", " ",
        ])
        .prop_map(str::to_string),
        prop::collection::vec(
            prop::sample::select(vec!["a", "Z", "é", "🎉", " ", "|", "\\", "`", "*", "_", "<", ">", "&", "{", "}", "-", "#", ":", "!", "[", "]", "\t"]),
            0..12
        )
        .prop_map(|parts| parts.concat()),
    ]
}

#[test]
fn new_table_reads_back_as_that_many_cells() {
    for (columns, rows) in [(1, 1), (2, 3), (3, 4), (20, 1), (1, 50), (20, 50)] {
        let table = new_table(columns, rows).expect("a table of that size");
        assert!(table.lines().all(|line| line.starts_with('|') && line.ends_with('|')), "{table}");
        assert_eq!(table.lines().nth(1).unwrap(), format!("|{}", " --- |".repeat(columns)));
        assert!(table.lines().enumerate().filter(|(n, _)| *n != 1).all(|(_, line)| line == format!("|{}", "  |".repeat(columns))), "two spaces in each empty cell");
        let talk = parse_talk(&format!("# T\n\n{table}\n"));
        let Block::Table { range, rows: read, .. } = &talk.slides[0].blocks[1] else { panic!("{table} isn't a table") };
        assert_eq!(table_size(&talk, 0, range), Some((columns, rows + 1)));
        assert!(read.iter().all(|row| row.len() == columns && row.iter().all(Vec::is_empty)));
        let cells = cell_flows(&talk, 0, range);
        assert_eq!(cells.len(), columns * (rows + 1), "every cell is a flow");
        assert!(cells.iter().all(|(_, _, flow)| flow.paras == body("")));
    }
    for (columns, rows) in [(0, 1), (21, 1), (1, 0), (1, 51)] {
        assert_eq!(new_table(columns, rows), None, "{columns} by {rows}");
    }
}

#[test]
fn the_header_cannot_be_removed() {
    let source = "# T\n\n| a | b |\n| - | - |\n| 1 | 2 |\n| 3 | 4 |\n";
    let talk = parse_talk(source);
    let table = talk.slides[0].blocks[1].range().clone();
    assert_eq!(remove_row(&talk, 0, &table, 0), None);
    assert_eq!(remove_row(&talk, 0, &table, 3), None, "past the last row");
    let change = remove_row(&talk, 0, &table, 1).expect("a body row");
    let one = change.apply(source);
    assert_eq!(one, "# T\n\n| a | b |\n| - | - |\n| 3 | 4 |\n");
    let talk = parse_talk(&one);
    let table = talk.slides[0].blocks[1].range().clone();
    let none = remove_row(&talk, 0, &table, 1).expect("the last body row too").apply(&one);
    assert_eq!(none, "# T\n\n| a | b |\n| - | - |\n", "a table of its header alone");
    let talk = parse_talk(&none);
    let table = talk.slides[0].blocks[1].range().clone();
    assert_eq!(table_size(&talk, 0, &table), Some((2, 1)));
    assert_eq!(remove_row(&talk, 0, &table, 0), None, "the header stays");
    assert_eq!(remove_row(&talk, 0, &table, 1), None, "there's no body row");
    // A row added to a table of its header alone can be taken away again, byte for byte.
    for header_only in [none.as_str(), "| a |\r\n|---|", "a | b\n--|--\n\nAfter\n"] {
        let talk = parse_talk(header_only);
        let table = talk.slides[0].blocks.iter().find(|block| matches!(block, Block::Table { .. })).unwrap().range().clone();
        let added = add_row(&talk, 0, &table, 0).expect("a row").apply(header_only);
        let talk = parse_talk(&added);
        let table = talk.slides[0].blocks.iter().find(|block| matches!(block, Block::Table { .. })).unwrap().range().clone();
        assert_eq!(remove_row(&talk, 0, &table, 1).expect("the row").apply(&added), header_only, "{added:?}");
    }
    assert_eq!(remove_column(&parse_talk("| a |\n| - |\n| 1 |\n"), 0, &(0..17), 0), None, "the only column");
}

#[test]
fn rows_and_columns_go_where_they_are_asked_for() {
    let source = "# T\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n";
    let talk = parse_talk(source);
    let table = talk.slides[0].blocks[1].range().clone();
    let apply = |change: Option<Change>| change.expect("a change").apply(source);
    assert_eq!(apply(add_row(&talk, 0, &table, 0)), "# T\n\n| a | b |\n|---|:-:|\n|  |  |\n| 1 | 2 |\n");
    assert_eq!(apply(add_row(&talk, 0, &table, 1)), "# T\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n|  |  |\n");
    assert_eq!(apply(add_column(&talk, 0, &table, None)), "# T\n\n|  | a | b |\n| --- |---|:-:|\n|  | 1 | 2 |\n");
    assert_eq!(apply(add_column(&talk, 0, &table, Some(0))), "# T\n\n| a |  | b |\n|---| --- |:-:|\n| 1 |  | 2 |\n");
    assert_eq!(apply(add_column(&talk, 0, &table, Some(1))), "# T\n\n| a | b |  |\n|---|:-:| --- |\n| 1 | 2 |  |\n");
    assert_eq!(apply(remove_column(&talk, 0, &table, 0)), "# T\n\n| b |\n|:-:|\n| 2 |\n");
    assert_eq!(apply(remove_column(&talk, 0, &table, 1)), "# T\n\n| a |\n|---|\n| 1 |\n");
    assert_eq!(add_column(&talk, 0, &table, Some(2)), None, "past the last column");
    assert_eq!(add_row(&talk, 0, &table, 2), None, "past the last row");

    // With no `|` at the edges, and in a list item.
    let source = "- item\n\n  a | b\r\n  --|--\r\n  1 | 2\r\n".replace("\n\n", "\r\n\r\n");
    let talk = parse_talk(&source);
    let Block::List { items, .. } = &talk.slides[0].blocks[0] else { panic!() };
    let table = items[0][1].range().clone();
    let apply = |change: Option<Change>| change.expect("a change").apply(&source);
    assert_eq!(apply(add_row(&talk, 0, &table, 1)), "- item\r\n\r\n  a | b\r\n  --|--\r\n  1 | 2\r\n  |  |  |\r\n");
    assert_eq!(apply(add_column(&talk, 0, &table, None)), "- item\r\n\r\n  |  | a | b\r\n  | --- | --|--\r\n  |  | 1 | 2\r\n");
    assert_eq!(apply(add_column(&talk, 0, &table, Some(1))), "- item\r\n\r\n  a | b |  |\r\n  --|-- | --- |\r\n  1 | 2 |  |\r\n");
    assert_eq!(apply(remove_column(&talk, 0, &table, 0)), "- item\r\n\r\n  | b\r\n  |--\r\n  | 2\r\n");
    assert_eq!(apply(remove_column(&talk, 0, &table, 1)), "- item\r\n\r\n  a |\r\n  --|\r\n  1 |\r\n");
}

#[test]
fn a_short_row_is_filled_only_as_far_as_needed() {
    let source = "| a | b | c | d |\n| - | - | - | - |\n| x |\n";
    let talk = parse_talk(source);
    let table = talk.slides[0].blocks[0].range().clone();
    let change = add_column(&talk, 0, &table, Some(1)).expect("a change");
    assert_eq!(change.apply(source), "| a | b |  | c | d |\n| - | - | --- | - | - |\n| x |  |  |\n");
    let change = add_column(&talk, 0, &table, Some(0)).expect("a change");
    assert_eq!(change.apply(source), "| a |  | b | c | d |\n| - | --- | - | - | - |\n| x |  |\n");
    let change = remove_column(&talk, 0, &table, 2).expect("a change");
    assert_eq!(change.apply(source), "| a | b | d |\n| - | - | - |\n| x |\n", "a row without that cell is left alone");
}

#[test]
fn a_row_longer_than_the_header_stops_column_changes() {
    let source = "| a | b |\n| - | - |\n| 1 | 2 | 3 |\n";
    let talk = parse_talk(source);
    let table = talk.slides[0].blocks[0].range().clone();
    assert_eq!(add_column(&talk, 0, &table, Some(0)), None);
    assert_eq!(remove_column(&talk, 0, &table, 0), None);
    assert_eq!(add_row(&talk, 0, &table, 1).expect("rows still can").apply(source), "| a | b |\n| - | - |\n| 1 | 2 | 3 |\n|  |  |\n");
}

#[test]
fn a_cell_holding_an_icon_tag_has_no_flow() {
    let source = "| a | <Icon name=\"star\" /> |\n| - | - |\n| <Text font=\"Mono\">f</Text> | **b** |\n";
    let talk = parse_talk(source);
    let table = talk.slides[0].blocks[0].range().clone();
    let cells: Vec<(usize, usize)> = cell_flows(&talk, 0, &table).iter().map(|(r, c, _)| (*r, *c)).collect();
    assert_eq!(cells, [(0, 0), (1, 1)]);
    let more = "This cell holds more than text, so it can't be typed in yet.";
    assert_eq!(uneditable_reason(&talk, 0, source.find("Icon").unwrap()), Some(more));
    assert_eq!(uneditable_reason(&talk, 0, source.find("f<").unwrap()), Some(more));
    assert_eq!(uneditable_reason(&talk, 0, source.find("**b").unwrap()), None);
    assert_eq!(uneditable_reason(&talk, 0, 0), None, "a `|` is the table's");
    assert_eq!(uneditable_reason(&talk, 0, source.find("| -").unwrap() + 2), None, "so is the alignment row");
}

#[test]
fn an_empty_cell_can_be_typed_in_and_emptied_again() {
    let start = format!("# T\n\n{}\n\nAfter\n", new_table(2, 1).unwrap());
    let mut source = start.clone();
    for text in ["x", "xy", "", "| a", ""] {
        let talk = parse_talk(&source);
        let table = talk.slides[0].blocks[1].range().clone();
        let (_, _, flow) = cell_flows(&talk, 0, &table).into_iter().find(|(r, c, _)| (*r, *c) == (1, 0)).unwrap();
        source = write_flow(&talk, 0, &flow, &body(text)).expect("a cell's text can be written").apply(&source);
        let talk = parse_talk(&source);
        let table = talk.slides[0].blocks[1].range().clone();
        let (_, _, flow) = cell_flows(&talk, 0, &table).into_iter().find(|(r, c, _)| (*r, *c) == (1, 0)).unwrap();
        assert_eq!(flow.paras, body(text), "{source}");
    }
    assert_eq!(source, start, "the bytes are back as they were");

    // A hand-written empty cell with other spacing between its `|`s takes typed text between
    // a space on each side, and ends as the app writes an empty cell: two spaces.
    for (empty, typed) in [("||", "| x |"), ("| |", "| x |"), ("|   |", "| x |"), ("|\t|", "| x |"), ("|  |", "| x |")] {
        let source = format!("| a | b |\n|---|---|\n| c {empty}\n");
        let talk = parse_talk(&source);
        let flows = text_flows(&talk, 0);
        let flow = flows.iter().find(|flow| matches!(flow.place, FlowPlace::Cell { row: 1, column: 1, .. })).unwrap();
        let after = write_flow(&talk, 0, flow, &body("x")).expect("typed").apply(&source);
        assert_eq!(after, format!("| a | b |\n|---|---|\n| c {typed}\n"), "{empty:?}");
        let talk = parse_talk(&after);
        let flows = text_flows(&talk, 0);
        let flow = flows.iter().find(|flow| matches!(flow.place, FlowPlace::Cell { row: 1, column: 1, .. })).unwrap();
        let emptied = write_flow(&talk, 0, flow, &body("")).expect("emptied").apply(&after);
        assert_eq!(emptied, "| a | b |\n|---|---|\n| c |  |\n", "{empty:?} ends as two spaces");
    }
    // A cell with text keeps its own spacing.
    let source = "|a|   b |\n|-|-|\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    assert_eq!(write_flow(&talk, 0, &flows[1], &body("bc")).unwrap().apply(source), "|a|   bc |\n|-|-|\n");
    assert_eq!(write_flow(&talk, 0, &flows[0], &body("")).unwrap().apply(source), "|  |   b |\n|-|-|\n");

    // Where a space won't do, at an edge with no `|`.
    let source = "a | b\n--|--\nc | d\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let last = flows.iter().find(|flow| matches!(flow.place, FlowPlace::Cell { row: 1, column: 1, .. })).unwrap();
    let emptied = write_flow(&talk, 0, last, &body("")).expect("it can be emptied").apply(source);
    let flows = text_flows(&parse_talk(&emptied), 0);
    assert_eq!(flows.iter().filter(|flow| matches!(flow.place, FlowPlace::Cell { .. })).count(), 4, "{emptied}");
    assert_eq!(flows[3].paras, body(""), "{emptied}");
}

#[test]
fn an_app_made_table_reads_like_a_hand_written_one() {
    let mut source = format!("{}\n", new_table(3, 2).unwrap());
    for (column, text) in ["a", "bb", "ccc"].into_iter().enumerate() {
        let talk = parse_talk(&source);
        let flows = text_flows(&talk, 0);
        let flow = flows.iter().find(|flow| matches!(flow.place, FlowPlace::Cell { row: 1, column: c, .. } if c == column)).unwrap();
        source = write_flow(&talk, 0, flow, &body(text)).expect("typed").apply(&source);
    }
    assert_eq!(source, "|  |  |  |\n| --- | --- | --- |\n| a | bb | ccc |\n|  |  |  |\n");
}

#[test]
fn a_link_with_a_pipe_in_a_cell_can_be_typed_in() {
    let source = "# T\n\n| [l](a\\|b) x | c |\n|---|---|\n| 1 | 2 |\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let flow = flows.iter().find(|flow| matches!(flow.place, FlowPlace::Cell { row: 0, column: 0, .. })).unwrap();
    let spans = flow.paras[0].spans.clone();
    assert_eq!(spans[0].style.link.as_deref(), Some("a|b"));
    let mut typed = spans.clone();
    typed.push(plain("Q"));
    for paras in [flow.paras.clone(), vec![Para { kind: ParaKind::Body, spans: typed }]] {
        let Some(change) = write_flow(&talk, 0, flow, &paras) else { panic!("refused {paras:?}") };
        let after = parse_talk(&change.apply(source));
        let flows = text_flows(&after, 0);
        let now = flows.iter().find(|flow| matches!(flow.place, FlowPlace::Cell { row: 0, column: 0, .. })).unwrap();
        assert_eq!(now.paras, storable(&paras));
    }
}

#[test]
fn a_cell_takes_one_line_of_body_text() {
    let source = "| a |\n| - |\n| b |\n";
    let talk = parse_talk(source);
    let flow = text_flows(&talk, 0).remove(0);
    assert_eq!(write_flow(&talk, 0, &flow, &body("one\ntwo")), None);
    let two = [body("one"), body("two")].concat();
    assert_eq!(write_flow(&talk, 0, &flow, &two), None);
    let heading = [Para { kind: ParaKind::Heading(1), spans: vec![plain("h")] }];
    assert_eq!(write_flow(&talk, 0, &flow, &heading), None);
    let change = write_flow(&talk, 0, &flow, &body("x | y")).expect("a pipe can be typed");
    assert_eq!(change.apply(source), "| x \\| y |\n| - |\n| b |\n");
    let code = [Para { kind: ParaKind::Body, spans: vec![Span { text: "a|b".into(), style: Style { code: true, ..Style::default() } }] }];
    assert_eq!(write_flow(&talk, 0, &flow, &code).expect("code with a pipe").apply(source), "| `a\\|b` |\n| - |\n| b |\n");
}

#[test]
fn the_limits_hold() {
    let wide = format!("{}\n", new_table(20, 2).unwrap());
    let talk = parse_talk(&wide);
    let table = talk.slides[0].blocks[0].range().clone();
    assert_eq!(add_column(&talk, 0, &table, Some(3)), None, "21 columns");
    assert!(add_row(&talk, 0, &table, 1).is_some());
    let tall = format!("{}\n", new_table(2, 50).unwrap());
    let talk = parse_talk(&tall);
    let table = talk.slides[0].blocks[0].range().clone();
    assert_eq!(add_row(&talk, 0, &table, 3), None, "52 rows");
    assert!(add_column(&talk, 0, &table, None).is_some());
    // A slide at its size limit can't grow.
    let filler = format!("{}\n\n", "x".repeat(MAX_SLIDE_BYTES - 200));
    let full = format!("{filler}{}\n", new_table(3, 3).unwrap());
    let talk = parse_talk(&full);
    assert!(talk.slides[0].error.is_none());
    let table = talk.slides[0].blocks[1].range().clone();
    let grown = (0..20).try_fold(full.clone(), |source, _| {
        let talk = parse_talk(&source);
        add_row(&talk, 0, &table, 1).map(|change| change.apply(&source))
    });
    assert_eq!(grown, None, "the slide fills");
}

/// One thing done to a table.
#[derive(Clone, Debug)]
enum TableOp {
    Type(prop::sample::Index, String),
    AddRow(prop::sample::Index),
    RemoveRow(prop::sample::Index),
    AddColumn(Option<prop::sample::Index>),
    RemoveColumn(prop::sample::Index),
}

fn table_op() -> impl Strategy<Value = TableOp> {
    prop_oneof![
        3 => (any::<prop::sample::Index>(), typed_cell_text()).prop_map(|(cell, text)| TableOp::Type(cell, text)),
        1 => any::<prop::sample::Index>().prop_map(TableOp::AddRow),
        1 => any::<prop::sample::Index>().prop_map(TableOp::RemoveRow),
        1 => prop::option::of(any::<prop::sample::Index>()).prop_map(TableOp::AddColumn),
        1 => any::<prop::sample::Index>().prop_map(TableOp::RemoveColumn),
    ]
}

/// What `op` does to the table at `table` on slide `slide`, if it can be done.
fn run_op(talk: &Talk, slide: usize, table: &Range<usize>, op: &TableOp) -> Option<Change> {
    let (columns, rows) = table_size(talk, slide, table)?;
    match op {
        TableOp::Type(cell, text) => {
            let cells = cell_flows(talk, slide, table);
            if cells.is_empty() {
                return None;
            }
            write_flow(talk, slide, &cells[cell.index(cells.len())].2, &body(text))
        }
        TableOp::AddRow(after) => add_row(talk, slide, table, after.index(rows)),
        TableOp::RemoveRow(row) => remove_row(talk, slide, table, row.index(rows)),
        TableOp::AddColumn(after) => add_column(talk, slide, table, after.map(|after| after.index(columns))),
        TableOp::RemoveColumn(column) => remove_column(talk, slide, table, column.index(columns)),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn typing_in_a_cell_changes_only_that_cell((source, slide, _) in talk_with_a_table(), cell in any::<prop::sample::Index>(), text in typed_cell_text()) {
        let talk = parse_talk(&source);
        let Some((table, rows, sources)) = marked_table(&talk, slide) else { return Ok(()) };
        let cells = cell_flows(&talk, slide, &table);
        prop_assume!(!cells.is_empty());
        let (row, column, flow) = &cells[cell.index(cells.len())];
        let Some(change) = write_flow(&talk, slide, flow, &body(text.trim_matches(' '))) else {
            prop_assert!(!(typeable(&text) && is_regular(&source, &rows, &sources)), "refused {:?} in a regular table {:?}", text, &source);
            return Ok(());
        };
        let after = change.apply(&source);
        let own = &flow.source[0];
        // Only the cell's content changes, or, for an emptied cell, what's between its `|`s.
        let (start, end) = if text.trim_matches(' ').is_empty() { (own.block.start, own.block.end) } else { (own.inline.start, own.inline.end) };
        prop_assert_eq!(&after[..start], &source[..start]);
        prop_assert_eq!(&after[after.len() - (source.len() - end)..], &source[end..]);
        let talk = parse_talk(&after);
        let (_, now, _) = table_at(&talk, slide, table.start).expect("still the table");
        prop_assert_eq!(now.len(), rows.len());
        for (r, (now, then)) in now.iter().zip(&rows).enumerate() {
            prop_assert_eq!(now.len(), then.len());
            for (c, (now, then)) in now.iter().zip(then).enumerate() {
                if (r, c) != (*row, *column) {
                    prop_assert_eq!(now, then, "cell {} {} of {:?}", r, c, &after);
                }
            }
        }
    }

    #[test]
    fn any_cell_text_reads_back_as_typed((source, slide, _) in talk_with_a_table(), cell in any::<prop::sample::Index>(), text in typed_cell_text()) {
        let talk = parse_talk(&source);
        let Some((table, rows, sources)) = marked_table(&talk, slide) else { return Ok(()) };
        let cells = cell_flows(&talk, slide, &table);
        prop_assume!(!cells.is_empty());
        let (row, column, flow) = &cells[cell.index(cells.len())];
        let Some(change) = write_flow(&talk, slide, flow, &body(&text)) else {
            prop_assert!(!(typeable(&text) && is_regular(&source, &rows, &sources)), "refused {:?} in a regular table {:?}", text, &source);
            return Ok(());
        };
        prop_assert!(!text.contains('\n'), "a new line is refused");
        let after = parse_talk(&change.apply(&source));
        prop_assert_eq!(after.slides.len(), talk.slides.len());
        let (range, now, _) = table_at(&after, slide, table.start).expect("still the table");
        let shape = |rows: &Vec<Vec<Vec<Span>>>| rows.iter().map(Vec::len).collect::<Vec<_>>();
        prop_assert_eq!(shape(&now), shape(&rows));
        let (_, _, flow) = cell_flows(&after, slide, &range).into_iter().find(|(r, c, _)| (r, c) == (row, column)).expect("still a flow");
        prop_assert_eq!(&flow.paras, &body(&text));
    }

    #[test]
    fn a_new_row_keeps_every_other_byte((source, slide, _) in talk_with_a_table(), after in any::<prop::sample::Index>()) {
        let talk = parse_talk(&source);
        let Some((table, rows, _)) = marked_table(&talk, slide) else { return Ok(()) };
        let after = after.index(rows.len());
        let change = add_row(&talk, slide, &table, after);
        prop_assert!(change.is_some(), "a row can be added after {} in {:?}", after, &source);
        let change = change.unwrap();
        prop_assert_eq!(change.edits.len(), 1);
        prop_assert!(change.edits[0].range.is_empty(), "nothing is taken away");
        prop_assert!(change.edits[0].text.starts_with(&in_eol(&source, "\n")), "a line of its own");
        let new = parse_talk(&change.apply(&source));
        let (_, now, _) = table_at(&new, slide, table.start).expect("still the table");
        let mut expected = rows.clone();
        expected.insert(after + 1, vec![Vec::new(); rows[0].len()]);
        prop_assert_eq!(now, expected);
    }

    #[test]
    fn a_new_column_keeps_every_other_cells_bytes((source, slide, _) in talk_with_a_table(), after in prop::option::of(any::<prop::sample::Index>())) {
        let talk = parse_talk(&source);
        let Some((table, rows, cells)) = marked_table(&talk, slide) else { return Ok(()) };
        let columns = rows[0].len();
        let after = after.map(|after| after.index(columns));
        let change = add_column(&talk, slide, &table, after);
        if rows.iter().any(|row| row.len() > columns) {
            prop_assert!(change.is_none(), "a row longer than the header stops it");
            return Ok(());
        }
        prop_assert!(change.is_some(), "a column can be added after {:?} in {:?}", after, &source);
        let change = change.unwrap();
        for edit in &change.edits {
            prop_assert!(edit.range.is_empty(), "nothing is taken away");
            for cell in cells.iter().flatten() {
                prop_assert!(!(cell.inline.start < edit.range.start && edit.range.start < cell.inline.end), "into a cell's content");
            }
        }
        let new = parse_talk(&change.apply(&source));
        let (_, now, _) = table_at(&new, slide, table.start).expect("still the table");
        let at = after.map_or(0, |after| after + 1);
        let expected: Vec<Vec<Vec<Span>>> = rows.iter().map(|row| {
            let mut row = row.clone();
            if row.len() < at {
                row.resize(at, Vec::new());
            }
            row.insert(at, Vec::new());
            row
        }).collect();
        prop_assert_eq!(now, expected);
    }

    #[test]
    fn adding_then_removing_is_the_start_again((source, slide, generated) in talk_with_a_regular_table(), at in any::<prop::sample::Index>(), column in prop::option::of(any::<prop::sample::Index>())) {
        let talk = parse_talk(&source);
        let Some((table, rows, _)) = marked_table(&talk, slide) else { return Ok(()) };
        prop_assume!(rows.len() == generated.rows.len());
        let after = at.index(rows.len());
        let added = add_row(&talk, slide, &table, after).expect("a row").apply(&source);
        let talk = parse_talk(&added);
        let (grown, _, _) = table_at(&talk, slide, table.start).expect("the table");
        prop_assert_eq!(remove_row(&talk, slide, &grown, after + 1).expect("the row").apply(&added), source.clone());

        let talk = parse_talk(&source);
        let after = column.map(|column| column.index(rows[0].len()));
        let added = add_column(&talk, slide, &table, after).expect("a column").apply(&source);
        let talk = parse_talk(&added);
        let (table, _, _) = table_at(&talk, slide, table.start).expect("the table");
        let column = after.map_or(0, |after| after + 1);
        prop_assert_eq!(remove_column(&talk, slide, &table, column).expect("the column").apply(&added), source);
    }

    #[test]
    fn no_change_breaks_the_table_or_the_slide((source, slide, _) in talk_with_a_table(), ops in prop::collection::vec(table_op(), 1..8)) {
        let mut source = source;
        let slides = parse_talk(&source).slides.len();
        let Some((table, _, _)) = marked_table(&parse_talk(&source), slide) else { return Ok(()) };
        let start = table.start;
        for op in &ops {
            let talk = parse_talk(&source);
            let (table, _, _) = table_at(&talk, slide, start).expect("the table");
            let Some(change) = run_op(&talk, slide, &table, op) else { continue };
            let after = change.apply(&source);
            let range = &talk.slides[slide].range;
            let tail = source.len() - range.end;
            prop_assert_eq!(&after[..range.start], &source[..range.start], "slides before it are untouched");
            prop_assert_eq!(&after[after.len() - tail..], &source[range.end..], "slides after it are untouched");
            let new = parse_talk(&after);
            prop_assert_eq!(new.slides.len(), slides);
            prop_assert!(new.slides[slide].error.is_none());
            let (table, rows, _) = table_at(&new, slide, start).expect("still the table");
            prop_assert!(rows[0].len() <= MAX_TABLE_COLUMNS && rows.len() <= MAX_TABLE_ROWS);
            prop_assert_eq!(table_size(&new, slide, &table), Some((rows[0].len(), rows.len())));
            source = after;
        }
    }

    #[test]
    fn the_limits_hold_wherever_they_are_reached(columns in 1..=20usize, rows in 1..=50usize, ops in prop::collection::vec(table_op(), 1..6)) {
        let mut source = format!("# T\n\n{}\n", new_table(columns, rows).unwrap());
        for op in &ops {
            let talk = parse_talk(&source);
            let (table, _, _) = table_at(&talk, 0, 5).expect("the table");
            let Some(change) = run_op(&talk, 0, &table, op) else { continue };
            source = change.apply(&source);
            let talk = parse_talk(&source);
            let (table, rows, _) = table_at(&talk, 0, 5).expect("the table");
            let (columns, all) = table_size(&talk, 0, &table).unwrap();
            prop_assert!(columns <= MAX_TABLE_COLUMNS && all <= MAX_TABLE_ROWS && all == rows.len());
        }
    }
}
