use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use proptest::prelude::*;
use slides_engine::*;
use slides_format::{FlowPlace, Pos, text_flows};
use unicode_segmentation::UnicodeSegmentation;

/// The format crate's generated talks.
#[path = "../../format/tests/common/mod.rs"]
mod talks;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

fn deck(source: &str) -> Deck { Deck::open(Box::new(NoFiles), Fonts::bundled_only(), source.to_string()) }

fn layout(source: &str) -> Arc<TextLayout> { deck(source).text_layout(0) }

fn at(para: usize, offset: usize) -> Pos { Pos { para, offset } }

fn caret(layout: &TextLayout, flow: usize, pos: Pos) -> CaretRect {
    layout.caret(flow, pos).unwrap_or_else(|| panic!("no caret at {pos:?} of flow {flow}"))
}

/// Where the middle of the caret is on the slide, turned with its layer.
fn middle(caret: &CaretRect) -> (f32, f32) { caret.on_slide(caret.x, caret.top + caret.height / 2.0) }

fn hit_at(layout: &TextLayout, point: (f32, f32)) -> Hit { layout.hit(point.0, point.1).expect("a point on the slide") }

/// The source of the element a hit is on.
fn hit_source<'a>(source: &'a str, hit: &Hit) -> &'a str {
    let Hit::Element(element) = hit else { panic!("{hit:?} isn't on an element") };
    source[element.source.clone()].trim_end()
}

fn near(a: f32, b: f32, within: f32) -> bool { (a - b).abs() <= within }

fn run(text: &str) -> TextRun {
    let theme = toolkit::Theme::default();
    TextRun { text: text.into(), size: 40.0, bold: false, italic: false, underline: false, color: theme.text, family: theme.font }
}

/// A body paragraph long enough to wrap in the content layout's body.
fn long_text() -> String { "Words that wrap ".repeat(16).trim_end().to_string() }

/// The offsets of `flow`'s paragraph `para` where a caret can be: its grapheme edges.
fn edges(layout_source: &str, flow: usize, para: usize) -> Vec<usize> {
    let talk = slides_format::parse_talk(layout_source);
    let text: String = text_flows(&talk, 0)[flow].paras[para].spans.iter().map(|span| span.text.as_str()).collect();
    text.grapheme_indices(true).map(|(at, _)| at).chain([text.len()]).collect()
}

#[test]
fn the_caret_at_the_start_is_at_the_paragraphs_left() {
    let layout = layout("# Title\n\nHello world\n");
    let start = caret(&layout, 1, at(0, 0));
    let placed = layout.paras().iter().find(|para| para.flow == 1 && para.para == 0).expect("the body is placed");
    assert!(near(start.x, 100.0, 0.5), "x is {}", start.x);
    assert!(near(start.x, placed.rect.x, 0.01));
    assert!(near(start.top, placed.rect.y, 0.01));
    assert!(near(start.top, 220.0, 0.5), "top is {}", start.top);
    assert!(near(start.height, 50.0, 0.5), "height is {}", start.height);
    assert_eq!(start.rotate, 0.0);
}

#[test]
fn the_caret_moves_right_by_each_characters_width() {
    let layout = layout("# Title\n\nHello\n");
    let mut fonts = Fonts::bundled_only();
    let start = caret(&layout, 1, at(0, 0)).x;
    for end in 1..="Hello".len() {
        let width = shape_text(&mut fonts, &[run(&"Hello"[..end])], None, Align::Left).width;
        let x = caret(&layout, 1, at(0, end)).x;
        assert!(near(x - start, width, 0.5), "after {end} letters the caret moved {}, the letters are {width} wide", x - start);
    }
}

#[test]
fn the_caret_after_a_wrap_is_on_the_next_line() {
    let text = long_text();
    let layout = layout(&format!("# Title\n\n{text}\n"));
    let first = caret(&layout, 1, at(0, 0));
    let on_next = (0..=text.len()).find(|&offset| caret(&layout, 1, at(0, offset)).top > first.top).expect("the text wraps");
    let wrapped = caret(&layout, 1, at(0, on_next));
    assert!(near(wrapped.x, 100.0, 0.5), "the next line starts at the left, not at {}", wrapped.x);
    assert!(near(wrapped.top, first.top + first.height, 0.5));
    let before = caret(&layout, 1, at(0, on_next - 1));
    assert_eq!(before.top, first.top);
    assert!(before.x > 1000.0, "the caret before the wrap is at the line's end, not at {}", before.x);
}

#[test]
fn a_caret_in_centred_text_follows_the_alignment() {
    let layout = layout("---\nlayout: title\n---\n\n# Hi there\n");
    let (start, end) = (caret(&layout, 0, at(0, 0)), caret(&layout, 0, at(0, "Hi there".len())));
    assert!(start.x > 600.0, "the start is at {}", start.x);
    assert!(near((start.x + end.x) / 2.0, 960.0, 1.0), "the text is centred on {}", (start.x + end.x) / 2.0);
}

#[test]
fn a_click_on_a_letter_gives_the_gap_nearest_it() {
    let layout = layout("# Title\n\nHello\n");
    let (one, two) = (caret(&layout, 1, at(0, 1)), caret(&layout, 1, at(0, 2)));
    let y = one.top + one.height / 2.0;
    assert_eq!(hit_at(&layout, (one.x + (two.x - one.x) * 0.3, y)), Hit::Text { flow: 1, pos: at(0, 1) });
    assert_eq!(hit_at(&layout, (one.x + (two.x - one.x) * 0.7, y)), Hit::Text { flow: 1, pos: at(0, 2) });
}

#[test]
fn a_click_past_the_end_of_a_line_gives_the_lines_end() {
    let layout = layout("# Title\n\nHello\n");
    let start = caret(&layout, 1, at(0, 0));
    assert_eq!(hit_at(&layout, (1500.0, start.top + 10.0)), Hit::Text { flow: 1, pos: at(0, 5) });

    let text = long_text();
    let layout = self::layout(&format!("# Title\n\n{text}\n"));
    let first = caret(&layout, 1, at(0, 0));
    let Hit::Text { flow: 1, pos } = hit_at(&layout, (1815.0, first.top + 10.0)) else { panic!("a click on the line's right is on the text") };
    let end = caret(&layout, 1, pos);
    assert_eq!(end.top, first.top, "the end of the first line, not the start of the next");
    assert_eq!(pos, layout.line_move(1, at(0, 0), LineMove::LineEnd, None));
    let next = caret(&layout, 1, at(0, pos.offset + 1));
    assert!(next.top > first.top || next.x > end.x);
}

#[test]
fn a_click_in_a_turned_box_is_turned_back() {
    let layout = layout("<Text x=\"600\" y=\"300\" w=\"600\" h=\"200\" rotate=\"90\">\n\nHello world\n\n</Text>\n");
    let third = caret(&layout, 0, at(0, 3));
    assert_eq!(third.rotate, 90.0);
    assert_eq!(third.centre, (900.0, 400.0));
    let turned = middle(&third);
    assert!(!near(turned.0, third.x, 1.0), "the point is turned");
    assert_eq!(hit_at(&layout, turned), Hit::Text { flow: 0, pos: at(0, 3) });
    assert!(!matches!(hit_at(&layout, (third.x, third.top + third.height / 2.0)), Hit::Text { .. }), "the box isn't there unturned");
}

/// The index among `flows` of the flow of the cell at `row` and `column`.
fn cell_flow(flows: &[slides_format::TextFlow], row: usize, column: usize) -> usize {
    flows
        .iter()
        .position(|flow| matches!(flow.place, FlowPlace::Cell { row: r, column: c, .. } if (r, c) == (row, column)))
        .unwrap_or_else(|| panic!("no flow for cell {row} {column}"))
}

#[test]
fn a_click_on_a_tables_text_gives_its_cell() {
    let source = "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let mut deck = deck(source);
    let layout = deck.text_layout(0);
    let flows = text_flows(deck.talk(), 0);
    let Hit::Text { flow, .. } = hit_at(&layout, (300.0, 240.0)) else { panic!("not on text") };
    assert_eq!(flow, cell_flow(&flows, 0, 0));
}

#[test]
fn a_click_in_each_cell_gives_its_flow() {
    let mut showcase = fixture("showcase");
    let index = slide_with(&showcase, "| Layout |");
    let layout = showcase.text_layout(index);
    let table = block_starting(&showcase, index, "| Layout |");
    let flows = text_flows(showcase.talk(), index);
    for row in 0..3 {
        for column in 0..2 {
            let rect = layout.cell_rect(&table, row, column).unwrap_or_else(|| panic!("cell {row} {column}"));
            let flow = cell_flow(&flows, row, column);
            // On its text, and anywhere else in the cell away from its lines: its corners, below its text.
            let corners = [(rect.x + 8.0, rect.y + 8.0), (rect.x + rect.w - 8.0, rect.y + rect.h - 8.0), centre(rect)];
            for point in corners {
                let Hit::Text { flow: found, pos } = hit_at(&layout, point) else { panic!("{point:?} of cell {row} {column} isn't on its text") };
                assert_eq!((found, pos.para), (flow, 0), "cell {row} {column} at {point:?}");
            }
            let words: String = flows[flow].paras[0].spans.iter().map(|span| span.text.as_str()).collect();
            let Hit::Text { pos, .. } = hit_at(&layout, (rect.x + rect.w - 8.0, rect.y + 8.0)) else { panic!() };
            assert_eq!(pos.offset, words.len(), "the nearest place, at the end of the words");
        }
    }
    assert_eq!(layout.cell_rect(&table, 3, 0), None);
    assert_eq!(layout.cell_rect(&table, 0, 2), None);
}

#[test]
fn a_caret_in_an_empty_cell_sits_in_the_cell() {
    let source = "# T\n\n| a | b |\n|---|---|\n| | c |\n| | |\n";
    let mut deck = deck(source);
    let layout = deck.text_layout(0);
    let talk = deck.talk();
    let table = talk.slides[0].blocks[1].range().clone();
    let flows = text_flows(talk, 0);
    for (row, column) in [(1, 0), (2, 0), (2, 1)] {
        let flow = cell_flow(&flows, row, column);
        let rect = layout.cell_rect(&table, row, column).unwrap();
        let caret = caret(&layout, flow, at(0, 0));
        assert!(caret.x > rect.x && caret.x < rect.x + rect.w / 2.0, "{caret:?} in {rect:?}");
        assert!(caret.top >= rect.y && caret.top + caret.height <= rect.y + rect.h + 0.01, "{caret:?} in {rect:?}");
        assert!(near(caret.height, rect.h.min(50.0), 0.5), "as tall as a line of the cell's text, as the cell lets it be: {caret:?}");
        assert_eq!(hit_at(&layout, centre(rect)), Hit::Text { flow, pos: at(0, 0) });
    }
    // Where text typed in a cell of spaces starts.
    let typed = |text: &str| {
        let source = format!("# T\n\n| a |{text}|\n|---|---|\n");
        let mut deck = self::deck(&source);
        let layout = deck.text_layout(0);
        let flow = cell_flow(&text_flows(deck.talk(), 0), 0, 1);
        caret(&layout, flow, at(0, 0))
    };
    let (empty, written) = (typed("  "), typed(" x "));
    assert!(near(empty.x, written.x, 0.01) && near(empty.top, written.top, 0.01), "{empty:?} and {written:?}");
}

#[test]
fn a_click_below_or_beside_a_table_is_not_in_a_cell() {
    let table = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
    let after_text = format!("# T\n\nText\n\n{table}");
    let layout = layout(&after_text);
    for point in [(300.0, 1000.0), (1800.0, 1000.0)] {
        assert_eq!(hit_at(&layout, point), Hit::Text { flow: 1, pos: at(0, 4) }, "the region's last text, at {point:?}");
    }
    let alone = format!("# T\n\n{table}");
    let layout = self::layout(&alone);
    for point in [(300.0, 1000.0), (1800.0, 1000.0)] {
        assert_eq!(hit_at(&layout, point), Hit::Nothing, "no text in the region but the table's, at {point:?}");
    }
}

#[test]
fn a_click_on_a_tables_border_gives_the_table() {
    let source = "# T\n\n| a | <Icon name=\"star\" /> |\n|---|---|\n| 1 | 2 |\n| 3 |\n";
    let mut deck = deck(source);
    let layout = deck.text_layout(0);
    let table = deck.talk().slides[0].blocks[1].range().clone();
    let whole = "| a | <Icon name=\"star\" /> |\n|---|---|\n| 1 | 2 |\n| 3 |";
    let (first, second, short) = (layout.cell_rect(&table, 1, 0).unwrap(), layout.cell_rect(&table, 1, 1).unwrap(), layout.cell_rect(&table, 2, 0).unwrap());
    let middle = first.y + first.h / 2.0;
    let points = [
        (first.x + 0.5, middle),
        (second.x + 0.5, middle),
        (second.x + 3.0, middle),
        (second.x + second.w + 0.5, middle),
        (first.x + first.w / 2.0, first.y + 0.5),
        (first.x + first.w / 2.0, first.y + first.h + 0.5),
    ];
    for point in points {
        assert_eq!(hit_source(source, &hit_at(&layout, point)), whole, "on a line at {point:?}");
    }
    let icon = layout.cell_rect(&table, 0, 1).unwrap();
    assert_eq!(hit_source(source, &hit_at(&layout, centre(icon))), whole, "a cell holding more than text");
    assert_eq!(layout.cell_rect(&table, 2, 1), None, "a row shorter than the header has no cell there");
    let missing = (second.x + second.w / 2.0, short.y + short.h / 2.0);
    assert_eq!(hit_source(source, &hit_at(&layout, missing)), whole, "where a short row has no cell");
    for point in points.into_iter().chain([centre(first), centre(icon), missing]) {
        assert_eq!(source_at(&deck, &layout, point), whole, "the table is picked anywhere on it at {point:?}");
    }
    assert!(layout.element(&table).unwrap().holds_text);
}

#[test]
fn cell_rects_tile_the_table() {
    let mut showcase = fixture("showcase");
    let index = slide_with(&showcase, "| Layout |");
    let layout = showcase.text_layout(index);
    let table = block_starting(&showcase, index, "| Layout |");
    let drawn = layout.element(&table).unwrap().rect;
    let cell = |row, column| layout.cell_rect(&table, row, column).unwrap();
    for row in 0..3 {
        assert!(near(cell(row, 0).x, drawn.x, 0.01), "each row starts at the table's left");
        assert!(near(cell(row, 1).x, cell(row, 0).x + cell(row, 0).w, 0.01), "its cells meet");
        assert!(near(cell(row, 1).x + cell(row, 1).w + 1.0, drawn.x + drawn.w, 0.01), "and end at its right, but for the last line");
        assert!(near(cell(row, 0).h, cell(row, 1).h, 0.01), "a row's cells are as tall as each other");
        let top = if row == 0 { drawn.y } else { cell(row - 1, 0).y + cell(row - 1, 0).h };
        assert!(near(cell(row, 0).y, top, 0.01), "the rows meet");
    }
    assert!(near(cell(2, 0).y + cell(2, 0).h + 1.0, drawn.y + drawn.h, 0.01), "the last row ends at its bottom, but for the last line");
}

#[test]
fn a_click_in_an_empty_region_names_it() {
    let layout = layout("# Only a title\n");
    assert_eq!(hit_at(&layout, (500.0, 600.0)), Hit::Placeholder("body".to_string()));
    let layout = self::layout("Body only\n");
    assert_eq!(hit_at(&layout, (500.0, 100.0)), Hit::Placeholder("title".to_string()));
    let layout = self::layout("---\nlayout: two-columns\n---\n\n# T\n\nLeft\n");
    assert_eq!(hit_at(&layout, (1400.0, 600.0)), Hit::Placeholder("right".to_string()));
}

#[test]
fn a_click_on_the_background_is_nothing() {
    let layout = layout("# T\n\nBody\n");
    assert_eq!(hit_at(&layout, (10.0, 1070.0)), Hit::Nothing);
    assert_eq!(layout.hit(f32::NAN, 10.0), None);
}

#[test]
fn a_selection_over_two_lines_is_two_rects() {
    let layout = layout("# T\n\nOne\nTwo\n");
    let rects = layout.selection_rects(1, at(0, 1), at(0, 6)).rects;
    assert_eq!(rects.len(), 2, "{rects:?}");
    assert!(rects[1].y > rects[0].y);
    let (one, three) = (caret(&layout, 1, at(0, 1)), caret(&layout, 1, at(0, 3)));
    assert!(near(rects[0].x, one.x, 0.01));
    assert!(rects[0].x + rects[0].w >= three.x, "the first runs to the line's end, and over its break");
    let (two_start, six) = (caret(&layout, 1, at(0, 4)), caret(&layout, 1, at(0, 6)));
    assert!(near(rects[1].x, two_start.x, 0.01));
    assert!(near(rects[1].x + rects[1].w, six.x, 0.01));
    assert_eq!(layout.selection_rects(1, at(0, 6), at(0, 1)).rects, rects, "either way round");
    assert!(layout.selection_rects(1, at(0, 2), at(0, 2)).rects.is_empty());
}

#[test]
fn up_and_down_keep_the_column() {
    let layout = layout("# T\n\nLong line here\nab\nLong line here\n");
    let from = at(0, 10);
    let x = caret(&layout, 1, from).x;
    let down = layout.line_move(1, from, LineMove::Down, None);
    assert_eq!(down, at(0, 17), "the short line's end");
    let down_again = layout.line_move(1, down, LineMove::Down, Some(x));
    assert_eq!(down_again, at(0, 28), "back in the column it started in");
    let up = layout.line_move(1, down_again, LineMove::Up, Some(x));
    assert_eq!(layout.line_move(1, up, LineMove::Up, Some(x)), from);
    assert_eq!(layout.line_move(1, from, LineMove::LineStart, None), at(0, 0));
    assert_eq!(layout.line_move(1, from, LineMove::LineEnd, None), at(0, 14));
    assert_eq!(layout.line_move(1, at(0, 2), LineMove::Up, None), at(0, 0), "up from the first line goes to its start");
    assert_eq!(layout.line_move(1, at(0, 20), LineMove::Down, None), at(0, 32), "down from the last goes to its end");
}

#[test]
fn up_and_down_cross_paragraphs() {
    let layout = layout("# T\n\nFirst\n\nSecond\n");
    assert_eq!(layout.line_move(1, at(0, 2), LineMove::Down, None).para, 1);
    assert_eq!(layout.line_move(1, at(1, 2), LineMove::Up, None).para, 0);
}

/// Slides covering every layout, an unknown one, a region marker, boxes of each kind,
/// lists, breaks, wrapping, styles, and characters made of more than one code point.
fn mixed_slides() -> Vec<String> {
    let text = long_text();
    let mut slides = vec![
        "---\nlayout: title\n---\n\n# A talk\n\nBy someone\n".to_string(),
        format!("---\nlayout: content\n---\n\n# Content\n\n{text}\n\n- One\n- Two **bold**\n  - Nested *it*\n- \n\n1. First\n2. Second\n"),
        "---\nlayout: two-columns\n---\n\n# Two\n\nOn the left\n\n<Region name=\"right\" />\n\nOn the right\\\nwith a break\n".to_string(),
        "---\nlayout: section\n---\n\n# Part two\n\nWhat comes next\n".to_string(),
        "---\nlayout: blank\n---\n\nAll over the slide\n\n## A heading\n".to_string(),
        "---\nlayout: nonsense\n---\n\n# Unknown\n\nIn the body\n".to_string(),
        "# Characters\n\nCafé e\u{301}t\u{e9} 👍🏽 office `code` [a link](https://example.com)\n\nSplit\u{2029}apart and \u{85}again\n".to_string(),
        "<Text x=\"100\" y=\"100\" w=\"700\" h=\"300\" rotate=\"12\">\n\nA turned box\nof text\n\n</Text>\n\n<Box x=\"900\" y=\"100\" w=\"700\" h=\"300\" fill=\"#dbeafe\">\n\nIn a box\n\n- and a list\n\n</Box>\n\n<Shape x=\"300\" y=\"600\" w=\"600\" h=\"300\" kind=\"ellipse\" fill=\"#fde68a\">\n\nIn a shape\n\n</Shape>\n".to_string(),
        "---\nregion.body: align=right\n---\n\n# Right\n\nTo the right\n\n<Text x=\"100\" y=\"700\" w=\"800\" h=\"200\" align=\"center\">\n\nCentred box\n\n</Text>\n".to_string(),
    ];
    slides.push("# Each\n\n- one\n- two\n\nAnd more\n".replace("# Each", "---\nregion.body: click=each\n---\n\n# Each"));
    // Tables, whose cells are flows after the others: by hand, with empty cells and short rows,
    // in a box and in a list, and after more text.
    slides.push("# Tables\n\nBefore\n\n| a | **b** `c\\|d` |\n|:--|--:|\n| | x \\| y |\n| short |\n\nAfter\n".to_string());
    slides.push("# Edges\n\na | b\n--|--\n| | |\nCafé e\u{301} | 👍🏽 <Text color=\"red\">red</Text>\n".to_string());
    slides.push("<Box x=\"900\" y=\"650\" w=\"900\" h=\"380\">\n\n| in | a box |\n|---|---|\n| one | two |\n\n</Box>\n\n- item\n\n  | in a | list |\n  |---|---|\n  | 1 | 2 |\n".to_string());
    // Empty paragraphs and breaks at a paragraph's edges, as the format crate writes them,
    // and as it wrote them before.
    slides.push("# Edges\n\n&nbsp;\n\nMiddle\n\n&nbsp;\n\nEnds in a break\\\n**<Text></Text>**\n\n\\\nStarts with one\n\n&nbsp;\n".to_string());
    slides.push("<Text x=\"1000\" y=\"500\" w=\"700\" h=\"300\">\n\n&nbsp;\n\nA *break\\\n<Text></Text>*\n\n</Text>\n\n- \n- item\\\n**<Text></Text>**\n".to_string());
    slides.push("# Edges\n\n**<Text></Text>**\n\nMiddle\n\n**<Text></Text>**\n\nEnds in a break\\\n**<Text></Text>**\n\n\\\nStarts with one\n\n**<Text></Text>**\n".to_string());
    slides.push("<Text x=\"1000\" y=\"500\" w=\"700\" h=\"300\">\n\n**<Text></Text>**\n\nA *break\\\n<Text></Text>*\n\n</Text>\n\n- \n- item\\\n**<Text></Text>**\n".to_string());
    slides
}

#[test]
fn geometry_agrees_with_the_model() {
    for source in mixed_slides() {
        let mut deck = deck(&source);
        let layout = deck.text_layout(0);
        let flows = text_flows(deck.talk(), 0);
        assert!(!flows.is_empty(), "{source}");
        for (f, flow) in flows.iter().enumerate() {
            for p in 0..flow.paras.len() {
                assert!(layout.paras().iter().any(|placed| placed.flow == f && placed.para == p), "flow {f} paragraph {p} isn't placed in {source}");
                for offset in edges(&source, f, p) {
                    let caret = caret(&layout, f, at(p, offset));
                    assert_eq!(hit_at(&layout, middle(&caret)), Hit::Text { flow: f, pos: at(p, offset) }, "{offset} of flow {f} paragraph {p} in {source}");
                }
            }
            if let FlowPlace::Tag(range) = &flow.place {
                let layer = layout.paras().iter().find(|placed| placed.flow == f).unwrap().layer;
                assert_eq!(deck.specs(0)[layer].source.as_ref(), Some(range));
            }
        }
    }
}

#[test]
fn an_empty_paragraph_has_a_caret_as_tall_as_its_text() {
    let layout = layout("# T\n\n- \n");
    let caret = caret(&layout, 1, at(0, 0));
    assert!(near(caret.height, 50.0, 0.5), "{caret:?}");
    assert!(caret.x >= 148.0 - 0.5);
    assert_eq!(hit_at(&layout, middle(&caret)), Hit::Text { flow: 1, pos: at(0, 0) });
}

#[test]
fn a_caret_past_what_the_talk_holds_goes_on_a_new_line() {
    let layout = layout("# T\n\nOne\n");
    let end = caret(&layout, 1, at(0, 3));
    let after_a_break = caret(&layout, 1, at(0, 4));
    assert!(near(after_a_break.x, 100.0, 0.5));
    assert!(near(after_a_break.top, end.top + end.height, 0.5));
    let new_paragraph = caret(&layout, 1, at(1, 0));
    assert!(new_paragraph.top > end.top + end.height, "{new_paragraph:?}");
}

/// The text each of `specs` draws, line by line.
fn texts_in(specs: &[LayerSpec]) -> Vec<String> {
    specs.iter().flat_map(|spec| &spec.items).filter_map(|item| match item {
        DisplayItem::Text { text, .. } => Some(text.buffer.layout_runs().map(|run| run.text.to_string()).collect::<Vec<_>>().join("\n")),
        _ => None,
    }).collect()
}

#[test]
fn empty_regions_say_click_to_add_text_only_in_the_editor() {
    let mut d = deck("Body only\n\n---\n\n# Only a title\n");
    for (index, label) in [(0, "Click to add title"), (1, "Click to add text")] {
        let layers = d.layers(index, 0.5);
        let editor = d.editor_layers(index, 0.5);
        assert_eq!(editor.len(), layers.len() + 1);
        for (shown, drawn) in layers.iter().zip(&editor) {
            assert!(Arc::ptr_eq(shown, drawn), "the editor shows the slide's own layers first");
        }
        assert_eq!(texts_in(&d.placeholder_specs(index)), vec![label.to_string()]);
        let everything = d.specs(index);
        assert!(texts_in(everything).iter().all(|text| !text.contains("Click to add")));
        assert_eq!(d.layers_at(index, Moment::START, 0.5).len(), layers.len());
        let thumbnail = d.thumbnail(index, 960).unwrap();
        let mut composed = Pixmap::new(960, 540).unwrap();
        compose(&mut composed, &layers.iter().map(|layer| Layer { pixmap: layer.pixmap.clone(), x: layer.x, y: layer.y }).collect::<Vec<_>>());
        assert_eq!(thumbnail.data(), composed.data(), "the thumbnail has no placeholder");
        let mut with_placeholder = composed.clone();
        let last = editor.last().unwrap();
        compose(&mut with_placeholder, &[Layer { pixmap: last.pixmap.clone(), x: last.x, y: last.y }]);
        assert_ne!(with_placeholder.data(), composed.data(), "the placeholder draws something");
    }
    assert_eq!(d.editor_layers(0, 0.5).len(), d.editor_layers(0, 0.5).len());
    assert!(Arc::ptr_eq(d.editor_layers(0, 0.5).last().unwrap(), d.editor_layers(0, 0.5).last().unwrap()), "drawn once");
    assert!(d.placeholder_specs(0).iter().all(|spec| spec.source.is_none()));
    let mut full = deck("# Title\n\nBody\n");
    assert_eq!(full.editor_layers(0, 0.5).len(), full.layers(0, 0.5).len(), "nothing to add on a full slide");
}

#[test]
fn a_region_can_be_centred_from_its_settings_line() {
    let layout = layout("---\nregion.body: align=center\n---\n\n# T\n\nHi\n");
    let (start, end) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(0, 2)));
    assert!(near((start.x + end.x) / 2.0, 960.0, 1.0), "centred on {}", (start.x + end.x) / 2.0);
    assert!(near(caret(&layout, 0, at(0, 0)).x, 100.0, 0.5), "the title is as it was");
    let right = self::layout("---\nregion.body: x=100 align=right\n---\n\n# T\n\nHi\n");
    assert!(near(caret(&right, 1, at(0, 2)).x, 1820.0, 0.5));
}

#[test]
fn a_box_can_be_right_aligned() {
    let layout = layout("<Text x=\"100\" y=\"100\" w=\"800\" h=\"200\" align=\"right\">\n\nHi\n\n</Text>\n");
    assert!(near(caret(&layout, 0, at(0, 2)).x, 900.0, 0.5));
    assert!(caret(&layout, 0, at(0, 0)).x > 800.0);
    let left = self::layout("<Text x=\"100\" y=\"100\" w=\"800\" h=\"200\">\n\nHi\n\n</Text>\n");
    assert!(near(caret(&left, 0, at(0, 0)).x, 100.0, 0.5));
}

#[test]
fn each_paragraph_knows_the_size_and_alignment_it_is_drawn_with() {
    let layout = layout("---\nregion.body: align=center\n---\n\n# T\n\nHi <Text size=\"80\">big</Text>\n\n- item\n");
    assert_eq!(layout.text_size(0, 0), Some(64.0), "a title");
    assert_eq!(layout.text_size(1, 0), Some(40.0), "the body, whatever its spans ask for");
    assert_eq!(layout.text_size(1, 1), Some(40.0), "a list item");
    assert_eq!(layout.text_size(1, 2), None, "no such paragraph");
    assert_eq!(layout.text_size(7, 0), None, "no such flow");
    assert_eq!(layout.align(0), Some(Align::Left));
    assert_eq!(layout.align(1), Some(Align::Center));
    assert_eq!(layout.align(2), None);
    let boxed = self::layout("<Text x=\"100\" y=\"100\" w=\"800\" h=\"200\" align=\"right\">\n\nHi\n\n</Text>\n");
    assert_eq!(boxed.align(0), Some(Align::Right));
    let title = self::layout("---\nlayout: title\n---\n\n# Big\n");
    assert_eq!((title.text_size(0, 0), title.align(0)), (Some(96.0), Some(Align::Center)));
}

#[test]
fn a_caret_in_an_empty_region_is_as_tall_as_its_text_where_its_text_starts() {
    let layout = layout("# Only a title\n");
    let (rect, _) = layout.placeholder("body").expect("the body is empty");
    assert_eq!(layout.placeholder_text("body"), Some((40.0, Align::Left)));
    let caret = layout.placeholder_caret("body").expect("a caret in the body");
    // As a caret in an empty paragraph typed there would be.
    let typed = self::layout("# Only a title\n\n- \n");
    let empty_line = self::caret(&typed, 1, at(0, 0));
    assert!(near(caret.height, empty_line.height, 0.5), "{caret:?} against {empty_line:?}");
    assert!(near(caret.x, rect.x, 0.01) && near(caret.top, rect.y, 0.01), "{caret:?}");
    assert_eq!(caret.rotate, 0.0);

    let centred = self::layout("---\nregion.body: align=center\n---\n\n# T\n");
    let (rect, _) = centred.placeholder("body").unwrap();
    assert_eq!(centred.placeholder_text("body"), Some((40.0, Align::Center)));
    assert!(near(centred.placeholder_caret("body").unwrap().x, rect.x + rect.w / 2.0, 0.01));
    let title = self::layout("---\nlayout: title\n---\n\nSubtitle\n");
    assert_eq!(title.placeholder_text("title"), Some((96.0, Align::Center)));
    let caret = title.placeholder_caret("title").unwrap();
    assert!(near(caret.height, 120.0, 0.5), "{caret:?}");
    assert_eq!(layout.placeholder_caret("title"), None, "the title has text");
}

#[test]
fn list_markers_are_not_part_of_the_text() {
    let layout = layout("# T\n\n- One\n- Two\n\n1. Three\n");
    for para in 0..3 {
        let start = caret(&layout, 1, at(para, 0));
        assert!(start.x >= 148.0 - 0.5, "paragraph {para} starts at {}", start.x);
    }
    let first = caret(&layout, 1, at(0, 0));
    assert_eq!(hit_at(&layout, (first.x + 1.0, first.top + 5.0)), Hit::Text { flow: 1, pos: at(0, 0) });
}

#[test]
fn a_moved_slide_finds_its_text_where_a_fresh_deck_does() {
    let talk = "# One\n\nFirst\n\n---\n\n# Two\n\n- a\n- b\n\n<Text x=\"100\" y=\"700\" w=\"500\">\n\nBoxed\n\n</Text>\n";
    let mut d = deck(talk);
    d.layers(1, 0.5);
    let edited = talk.replace("First", "First, made longer");
    assert_eq!(d.update_text(slides_format::parse_talk(&edited)), vec![0]);
    let mut fresh = deck(&edited);
    assert_eq!(d.drawn_text_sources(1), fresh.drawn_text_sources(1));
    let sources = |deck: &mut Deck| deck.specs(1).iter().map(|spec| spec.source.clone()).collect::<Vec<_>>();
    assert_eq!(sources(&mut d), sources(&mut fresh));
    assert_eq!(*d.text_layout(1), *fresh.text_layout(1));
    assert_eq!(d.text_layout(1).paras().len(), 4);
}

#[test]
fn the_layout_is_kept_until_the_slide_changes() {
    let mut d = deck("# One\n\n---\n\n# Two\n");
    let before = d.text_layout(1);
    assert!(Arc::ptr_eq(&before, &d.text_layout(1)));
    d.update_text(slides_format::parse_talk("# One\n\n---\n\n# Two\n"));
    assert!(Arc::ptr_eq(&before, &d.text_layout(1)), "the same text keeps it");
    d.update_text(slides_format::parse_talk("# One changed\n\n---\n\n# Two\n"));
    // Its elements say where each is written, which moved: it's made again, the same but for that.
    let moved = d.text_layout(1);
    assert_eq!(before.paras(), moved.paras(), "another slide's edit leaves its text where it was");
    assert!(Arc::ptr_eq(&moved, &d.text_layout(1)), "and it's kept from then on");
    d.update_text(slides_format::parse_talk("# One changed\n\n---\n\n# Two changed\n"));
    assert!(!Arc::ptr_eq(&moved, &d.text_layout(1)));
    assert_eq!(d.text_layout(5).paras().len(), 0, "a slide that isn't there has no text");
}

#[test]
fn a_click_in_a_box_away_from_its_text_goes_to_the_nearest_paragraph() {
    let layout = layout("<Box x=\"100\" y=\"100\" w=\"800\" h=\"400\" fill=\"#dbeafe\">\n\nFirst\n\nSecond\n\n</Box>\n");
    let second = layout.paras().iter().find(|placed| placed.para == 1).unwrap().rect;
    assert_eq!(hit_at(&layout, (850.0, 480.0)), Hit::Text { flow: 0, pos: at(1, "Second".len()) });
    assert!(second.y + second.h < 480.0);
    assert_eq!(hit_at(&layout, (950.0, 480.0)), Hit::Placeholder("body".to_string()), "outside the box");
}

#[test]
fn a_click_on_a_component_says_why() {
    let source = "# T\n\n<Box fill=\"#dbeafe\">\n\nBoxed but not placed\n\n</Box>\n\n<Text x=\"1000\" y=\"600\" w=\"400\" h=\"200\">\n\n| a |\n|---|\n| b |\n\n</Text>\n";
    let layout = layout(source);
    assert!(hit_source(source, &hit_at(&layout, (300.0, 250.0))).starts_with("<Box fill="), "its words are part of it");
    let Hit::Text { flow, .. } = hit_at(&layout, (1100.0, 620.0)) else { panic!("a table's cell in a box can be typed in") };
    assert!(matches!(text_flows(&slides_format::parse_talk(source), 0)[flow].place, FlowPlace::Cell { row: 0, column: 0, .. }));
    assert!(hit_source(source, &hit_at(&layout, (1350.0, 780.0))).starts_with("<Text x=\"1000\""), "the box, away from its table");
}

#[test]
fn an_empty_paragraph_is_a_line_with_a_caret() {
    for line in ["&nbsp;", "**<Text></Text>**"] {
        an_empty_line_is_a_line_with_a_caret(&format!("# T\n\nFirst\n\n{line}\n\nThird\n"));
    }
}

#[test]
fn an_empty_line_is_drawn_the_same_in_either_spelling() {
    let (mut new, mut old) = (deck("# T\n\nFirst\n\n&nbsp;\n\nThird\n"), deck("# T\n\nFirst\n\n**<Text></Text>**\n\nThird\n"));
    let rects = |d: &mut Deck| d.text_layout(0).paras().iter().map(|placed| (placed.flow, placed.para, placed.rect)).collect::<Vec<_>>();
    assert_eq!(rects(&mut new), rects(&mut old));
    assert_eq!(texts_in(new.specs(0)), texts_in(old.specs(0)));
    assert_eq!(new.thumbnail(0, 960).unwrap().data(), old.thumbnail(0, 960).unwrap().data(), "the same pixels");
}

fn an_empty_line_is_a_line_with_a_caret(source: &str) {
    let mut d = deck(source);
    let layout = d.text_layout(0);
    let body: Vec<_> = layout.paras().iter().filter(|placed| placed.flow == 1).collect();
    assert_eq!(body.iter().map(|placed| placed.para).collect::<Vec<_>>(), vec![0, 1, 2], "every paragraph is placed, in order");
    let (first, empty, third) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(1, 0)), caret(&layout, 1, at(2, 0)));
    assert!(near(empty.height, 50.0, 0.5), "{empty:?}");
    assert!(near(empty.x, 100.0, 0.5));
    assert!(near(empty.top, first.top + first.height + 24.0, 0.5), "{empty:?}");
    assert!(near(third.top, empty.top + empty.height + 24.0, 0.5), "{third:?}");
    assert!(near(body[1].rect.h, 50.0, 0.5), "the empty line is drawn as tall as the region's text");
    assert_eq!(hit_at(&layout, middle(&empty)), Hit::Text { flow: 1, pos: at(1, 0) });
    assert_eq!(hit_at(&layout, (900.0, empty.top + 10.0)), Hit::Text { flow: 1, pos: at(1, 0) });
    let texts = texts_in(d.specs(0));
    assert!(texts.iter().all(|text| !text.contains("Text")), "nothing of its spelling is drawn: {texts:?}");
    let without = self::layout("# T\n\nFirst\n\nThird\n");
    assert!(near(caret(&without, 1, at(1, 0)).top + 74.0, third.top, 0.5));
}

#[test]
fn a_break_at_the_end_has_a_caret_on_its_own_line() {
    let layout = layout("# T\n\nab\\\n**<Text></Text>**\n\nNext\n");
    let (end, after) = (caret(&layout, 1, at(0, 2)), caret(&layout, 1, at(0, 3)));
    assert!(near(after.top, end.top + end.height, 0.5), "{after:?}");
    assert!(near(after.x, 100.0, 0.5));
    assert_eq!(hit_at(&layout, middle(&after)), Hit::Text { flow: 1, pos: at(0, 3) });
    let placed = layout.paras().iter().find(|placed| placed.flow == 1 && placed.para == 0).unwrap();
    assert!(near(placed.rect.h, 100.0, 0.5), "two lines are drawn: {placed:?}");
    let next = caret(&layout, 1, at(1, 0));
    assert!(near(next.top, after.top + after.height + 24.0, 0.5), "the next paragraph is below the empty line");
}

#[test]
fn a_click_between_paragraphs_goes_to_the_nearest() {
    let layout = layout("# T\n\nFirst\n\nSecond\n");
    let (first, second) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(1, 0)));
    let gap = first.top + first.height + 4.0;
    assert!(gap < second.top);
    assert_eq!(hit_at(&layout, (100.0, gap)), Hit::Text { flow: 1, pos: at(0, 0) });
    assert_eq!(hit_at(&layout, (100.0, second.top - 4.0)), Hit::Text { flow: 1, pos: at(1, 0) });
}

#[test]
fn a_click_on_a_marker_goes_to_its_items_start() {
    let layout = layout("# T\n\n- One\n- Two\n\n1. Three\n");
    let two = caret(&layout, 1, at(1, 0));
    assert_eq!(hit_at(&layout, (110.0, two.top + 20.0)), Hit::Text { flow: 1, pos: at(1, 0) });
    let three = caret(&layout, 1, at(2, 0));
    assert_eq!(hit_at(&layout, (105.0, three.top + 20.0)), Hit::Text { flow: 1, pos: at(2, 0) });
}

#[test]
fn a_click_below_the_last_paragraph_goes_to_it() {
    let layout = layout("# T\n\nFirst\n\nLast\n");
    assert_eq!(hit_at(&layout, (1500.0, 900.0)), Hit::Text { flow: 1, pos: at(1, 4) });
    assert_eq!(hit_at(&layout, (50.0, 900.0)), Hit::Nothing, "outside the region");
    let with_table = "# T\n\nText\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let table = self::layout(with_table);
    let Hit::Text { flow, .. } = hit_at(&table, (300.0, 320.0)) else { panic!("on the table's text") };
    assert!(matches!(text_flows(&slides_format::parse_talk(with_table), 0)[flow].place, FlowPlace::Cell { .. }), "a cell, not the text above");
}

#[test]
fn a_turned_boxs_selection_turns_with_it() {
    let layout = layout("<Text x=\"600\" y=\"300\" w=\"600\" h=\"200\" rotate=\"30\">\n\nHello world\n\n</Text>\n");
    let selection = layout.selection_rects(0, at(0, 0), at(0, 5));
    assert_eq!((selection.rotate, selection.centre), (30.0, (900.0, 400.0)));
    assert_eq!(selection.rects.len(), 1);
    assert!(layout.selection_rects(7, at(0, 0), at(0, 5)).rects.is_empty());
}

#[test]
fn a_caret_far_past_the_text_is_none() {
    let layout = layout("# T\n\nOne\n");
    assert!(layout.caret(1, at(0, 5)).is_none());
    assert!(layout.caret(1, at(2, 0)).is_none());
    assert!(layout.caret(1, at(1, 3)).is_none());
    assert!(layout.caret(9, at(0, 0)).is_none());
    let mut d = deck("# T\n");
    assert!(d.editor_layers(4, 0.5).is_empty());
    assert!(d.placeholder_specs(4).is_empty());
    assert!(d.drawn_text_sources(4).is_empty());
}

#[test]
fn align_where_it_does_nothing_says_so() {
    let said = "Only a box placed on the slide can be aligned, so align is ignored here".to_string();
    for source in ["<Box align=\"right\">\n\nHi\n\n</Box>\n", "Hi <Text align=\"right\">there</Text>\n", "- Hi <Text bold align=\"center\">there</Text>\n"] {
        assert!(deck(source).notices_whole(0).contains(&said), "{source}");
    }
    assert!(!deck("<Text x=\"10\" y=\"10\" w=\"500\" align=\"right\">\n\nHi\n\n</Text>\n").notices_whole(0).contains(&said));
    let layout = layout("---\nregion.body: align=center align=right\n---\n\n# T\n\nHi\n");
    let (start, end) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(0, 2)));
    assert!(near((start.x + end.x) / 2.0, 960.0, 1.0), "the first align wins");
}

#[test]
fn the_alignment_asked_for_is_told_whatever_a_list_is_drawn_at() {
    let region = |name: &str| FlowPlace::Region(name.to_string());
    let talk = |source: &str| slides_format::parse_talk(source);
    let plain = talk("# T\n\n- One\n");
    assert_eq!(text_align(&plain, 0, &region("body")), Some(Align::Left));
    assert_eq!(text_align(&plain, 0, &region("left")), None, "no such region in this layout");
    assert_eq!(text_align(&plain, 3, &region("body")), None, "no such slide");
    let centred = talk("---\nregion.body: x=10 align=center\n---\n\n# T\n\n- One\n");
    assert_eq!(text_align(&centred, 0, &region("body")), Some(Align::Center));
    assert_eq!(text_align(&centred, 0, &region("title")), Some(Align::Left));
    let title = talk("---\nlayout: title\n---\n\n# Big\n");
    assert_eq!(text_align(&title, 0, &region("title")), Some(Align::Center), "as the layout has it");
    let boxes = talk("<Text x=\"1\" align=\"right\">\n\nHi\n\n</Text>\n\n<Text x=\"2\">\n\nHo\n\n</Text>\n");
    let places: Vec<FlowPlace> = text_flows(&boxes, 0).into_iter().map(|flow| flow.place).collect();
    assert_eq!(text_align(&boxes, 0, &places[0]), Some(Align::Right));
    assert_eq!(text_align(&boxes, 0, &places[1]), Some(Align::Left));
    assert_eq!(text_align(&boxes, 0, &FlowPlace::Tag(0..1)), None);
}

/// The deck of fixture `name`, with its files.
fn fixture(name: &str) -> Deck {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name);
    let source = std::fs::read_to_string(root.join("slides.md")).unwrap();
    Deck::open(Box::new(FolderFiles::new(root)), Fonts::bundled_only(), source)
}

/// The slide of `deck` whose source holds `text`.
fn slide_with(deck: &Deck, text: &str) -> usize {
    let talk = deck.talk();
    talk.slides.iter().position(|slide| talk.source[slide.range.clone()].contains(text)).unwrap_or_else(|| panic!("no slide holds {text}"))
}

/// Every block of `blocks`, at any depth, with the blocks inside it after it.
fn all_blocks(blocks: &[slides_format::Block]) -> Vec<&slides_format::Block> {
    let mut all = Vec::new();
    for block in blocks {
        all.push(block);
        match block {
            slides_format::Block::List { items, .. } => all.extend(items.iter().flat_map(|item| all_blocks(item))),
            slides_format::Block::Tag(tag) => all.extend(all_blocks(&tag.children)),
            _ => {}
        }
    }
    all
}

/// The range of the block of slide `index` whose text starts at `text`'s first place in it.
fn block_starting(deck: &Deck, index: usize, text: &str) -> Range<usize> {
    let talk = deck.talk();
    let slide = &talk.slides[index];
    let at = slide.range.start + talk.source[slide.range.clone()].find(text).unwrap_or_else(|| panic!("{text} isn't on slide {index}"));
    all_blocks(&slide.blocks).into_iter().map(|block| block.range().clone()).find(|range| range.start == at).unwrap_or_else(|| panic!("no block starts with {text}"))
}

fn centre(rect: Rect) -> (f32, f32) { (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0) }

/// Where the point (`x`, `y`) of `element`, before it's turned, is on the slide.
fn on_slide(element: &ElementBox, (x, y): (f32, f32)) -> (f32, f32) {
    let (cx, cy) = centre(element.rect);
    let (sin, cos) = element.rotate.to_radians().sin_cos();
    (cx + (x - cx) * cos - (y - cy) * sin, cy + (x - cx) * sin + (y - cy) * cos)
}

fn element_at(layout: &TextLayout, point: (f32, f32)) -> Option<ElementBox> { layout.element_at(point.0, point.1) }

/// The source of the element at `point`, or "" if there's none.
fn source_at<'a>(deck: &'a Deck, layout: &TextLayout, point: (f32, f32)) -> &'a str {
    element_at(layout, point).map_or("", |element| &deck.source()[element.source])
}

#[test]
fn a_click_on_each_kind_of_thing_gives_its_source() {
    let mut showcase = fixture("showcase");
    let index = slide_with(&showcase, "<Shape kind=\"ellipse\"");
    let layout = showcase.text_layout(index);
    // Where each is drawn, by the place its tag gives.
    let placed = [
        ((360.0, 660.0), "<Box x=\"100\""),
        ((820.0, 660.0), "<Shape kind=\"ellipse\""),
        ((1120.0, 640.0), "<Shape kind=\"arrow\""),
        ((148.0, 888.0), "<Icon name=\"arrow-right\""),
        ((1560.0, 695.0), "<Picture src="),
    ];
    for (point, start) in placed {
        assert!(source_at(&showcase, &layout, point).starts_with(start), "{start} at {point:?}: {:?}", element_at(&layout, point));
    }
    let table = block_starting(&showcase, index, "| Layout |");
    let drawn = layout.element(&table).expect("the table is drawn");
    assert!(drawn.rect.y > 200.0 && drawn.rect.w > 100.0 && drawn.rect.h > 50.0, "{drawn:?}");
    assert!(source_at(&showcase, &layout, centre(drawn.rect)).starts_with("| Layout | Regions |"));
    assert!(drawn.holds_text, "its cells can be typed in");

    let picture = "# A picture\n\n![A photo](assets/photo.png)\n";
    let mut flowing = Deck::open(Box::new(FolderFiles::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase"))), Fonts::bundled_only(), picture.to_string());
    let layout = flowing.text_layout(0);
    let drawn = layout.element(&block_starting(&flowing, 0, "![A photo]")).expect("a picture in the text is drawn");
    assert!(source_at(&flowing, &layout, centre(drawn.rect)).starts_with("![A photo]"));

    let mut components = fixture("components");
    for tag in ["<Chart kind=\"bar\"", "<Score team=\"Oslo\"", "<Callout title="] {
        let index = slide_with(&components, tag);
        let layout = components.text_layout(index);
        let drawn = layout.element(&block_starting(&components, index, tag)).unwrap_or_else(|| panic!("{tag} is drawn"));
        assert!(drawn.rect.w > 0.0 && drawn.rect.h > 0.0, "{tag}: {drawn:?}");
        assert!(source_at(&components, &layout, centre(drawn.rect)).starts_with(tag), "{tag}");
    }
}

#[test]
fn a_point_in_a_turned_box_follows_the_turn() {
    let source = "<Box x=\"600\" y=\"300\" w=\"600\" h=\"200\" rotate=\"90\" fill=\"#dbeafe\">\n\n<Icon name=\"arrow-right\" size=\"80\" />\n\n</Box>\n";
    let mut d = deck(source);
    let layout = d.text_layout(0);
    let boxed = layout.element(&block_starting(&d, 0, "<Box")).expect("the box");
    assert_eq!((boxed.rect, boxed.rotate), (Rect { x: 600.0, y: 300.0, w: 600.0, h: 200.0 }, 90.0));
    // Turned a quarter about (900, 400), the box stands from y 100 to 700, x 800 to 1000.
    assert!(source_at(&d, &layout, (900.0, 650.0)).starts_with("<Box"), "inside it as it's turned");
    assert_eq!(source_at(&d, &layout, (650.0, 400.0)), "", "where it would be unturned");
    let icon = layout.element(&block_starting(&d, 0, "<Icon")).expect("the icon in it");
    assert_eq!(icon.rotate, 90.0, "it turns with its box");
    assert!((icon.rect.w - 80.0).abs() < 0.01 && (icon.rect.h - 80.0).abs() < 0.01, "{icon:?}");
    // Unturned it sits at the box's top left corner: turned, it's near the top right.
    let (x, y) = centre(icon.rect);
    assert!((900.0..1000.0).contains(&x) && (100.0..300.0).contains(&y), "{icon:?}");
    assert!(source_at(&d, &layout, (x, y)).starts_with("<Icon"));
    let near_its_corner = on_slide(&icon, (icon.rect.x + 2.0, icon.rect.y + 2.0));
    assert!(source_at(&d, &layout, near_its_corner).starts_with("<Icon"), "its turned corner");
}

#[test]
fn the_last_drawn_wins_where_two_overlap() {
    let source = "# T\n\n<Box x=\"100\" y=\"100\" w=\"400\" h=\"400\" fill=\"red\" />\n\n<Box x=\"300\" y=\"300\" w=\"400\" h=\"400\" fill=\"blue\">\n\n<Icon name=\"arrow-right\" size=\"100\" />\n\n</Box>\n";
    let mut d = deck(source);
    let layout = d.text_layout(0);
    assert!(source_at(&d, &layout, (200.0, 200.0)).contains("red"));
    assert!(source_at(&d, &layout, (450.0, 450.0)).contains("blue"), "the second is drawn over the first");
    assert!(source_at(&d, &layout, (650.0, 650.0)).contains("blue"));
    let icon = layout.element(&block_starting(&d, 0, "<Icon")).expect("the icon");
    assert!(source_at(&d, &layout, centre(icon.rect)).starts_with("<Icon"), "what's in a box is drawn over it");
    // A table in the body, under a box placed over it.
    let source = "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<Box x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" />\n";
    let mut d = deck(source);
    let layout = d.text_layout(0);
    let table = layout.element(&block_starting(&d, 0, "| a |")).expect("the table");
    assert!(source_at(&d, &layout, centre(table.rect)).starts_with("<Box"), "the box is drawn after the body");
}

#[test]
fn text_in_a_box_is_still_text() {
    let source = "<Box x=\"100\" y=\"100\" w=\"800\" h=\"400\" fill=\"#dbeafe\">\n\nHello\n\n</Box>\n\n<Icon name=\"arrow-right\" x=\"1000\" y=\"100\" />\n";
    let mut d = deck(source);
    let layout = d.text_layout(0);
    let on_text = middle(&caret(&layout, 0, at(0, 2)));
    assert_eq!(hit_at(&layout, on_text), Hit::Text { flow: 0, pos: at(0, 2) });
    let boxed = element_at(&layout, on_text).expect("the box is under its text");
    assert!(d.source()[boxed.source.clone()].starts_with("<Box"));
    assert!(boxed.holds_text);
    assert_eq!(layout.element(&boxed.source), Some(boxed));
    let icon = element_at(&layout, (1032.0, 132.0)).expect("the icon");
    assert!(!icon.holds_text);
    assert!(d.source()[icon.source.clone()].starts_with("<Icon"));
    assert_eq!(hit_at(&layout, (1032.0, 132.0)), Hit::Element(icon), "a click on an icon is on it");
}

#[test]
fn a_point_on_nothing_gives_nothing() {
    let mut d = deck("# T\n\nBody\n\n```\ncode\n```\n\n> A quote\n");
    let layout = d.text_layout(0);
    assert_eq!(element_at(&layout, (10.0, 1070.0)), None, "the background");
    let body = middle(&caret(&layout, 1, at(0, 1)));
    assert_eq!(element_at(&layout, body), None, "text in a region isn't a thing of its own");
    assert_eq!(element_at(&layout, (f32::NAN, 10.0)), None);
    assert_eq!(layout.element(&(0..2)), None);
    // Code and a quote still say why they can't be edited.
    let code = (300.0, caret(&layout, 1, at(0, 0)).top + 100.0);
    assert!(matches!(hit_at(&layout, code), Hit::Uneditable(_)), "{:?}", hit_at(&layout, code));
    assert_eq!(element_at(&layout, code), None);
}

/// The points just outside where `element` is picked, on each side, turned with it.
fn just_outside(element: &ElementBox) -> [(f32, f32); 4] {
    // Picked as at least 16 thick, about its middle.
    let (cx, cy) = centre(element.rect);
    let (w, h) = (element.rect.w.max(16.0), element.rect.h.max(16.0));
    let (x, y) = (cx - w / 2.0, cy - h / 2.0);
    [(x - 0.5, cy), (x + w + 0.5, cy), (cx, y - 0.5), (cx, y + h + 0.5)].map(|point| on_slide(element, point))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: talks::cases(32), ..ProptestConfig::default() })]

    #[test]
    fn every_element_is_found_at_its_centre_and_nowhere_outside_its_rect(talk in talks::gen_talk()) {
        let mut d = deck(&talk.text());
        for index in 0..d.slide_count() {
            let layout = d.text_layout(index);
            let slide = &d.talk().slides[index];
            for block in all_blocks(&slide.blocks) {
                let positioned = matches!(block, slides_format::Block::Tag(tag) if tag.attr("x").is_some() && slide.blocks.contains(block));
                let Some(element) = layout.element(block.range()) else {
                    prop_assert!(!positioned || slide.error.is_some(), "a placed tag is always there: {:?}", block.range());
                    continue;
                };
                let found = element_at(&layout, centre(element.rect)).expect("something is at its centre");
                let inside = |inner: &Range<usize>, outer: &Range<usize>| outer.start <= inner.start && inner.end <= outer.end;
                prop_assert!(
                    found.source == element.source || inside(&found.source, &element.source) || !inside(&element.source, &found.source),
                    "what holds {:?} is drawn over it: {:?}", element, found
                );
                for point in just_outside(&element) {
                    prop_assert_ne!(element_at(&layout, point).map(|found| found.source), Some(element.source.clone()), "{:?} at {:?}", element, point);
                }
            }
        }
    }
}

#[test]
fn geometry_never_panics_on_any_point() {
    let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN, -0.0, f32::MIN_POSITIVE, 1.0e30];
    let mut points: Vec<(f32, f32)> = odd.iter().flat_map(|&x| odd.iter().map(move |&y| (x, y))).collect();
    points.extend(odd.iter().map(|&odd| (odd, 500.0)).chain(odd.iter().map(|&odd| (500.0, odd))));
    points.extend((0..=24).flat_map(|row| (0..=24).map(move |column| (column as f32 * 80.0 - 1.0, row as f32 * 45.0 - 1.0))));
    let mut layouts = Vec::new();
    let mut showcase = fixture("showcase");
    layouts.extend((0..showcase.slide_count()).map(|index| showcase.text_layout(index)));
    for source in mixed_slides().into_iter().chain([
        "<Box x=\"600\" y=\"300\" w=\"0\" h=\"0\" rotate=\"45\" />\n".to_string(),
        "<Box x=\"-3840\" y=\"7680\" w=\"3840\" h=\"3840\" rotate=\"-360\">\n\n<Icon name=\"x\" />\n\n</Box>\n".to_string(),
        "<Shape kind=\"line\" x=\"1e40\" y=\"NaN\" w=\"inf\" h=\"-5\" />\n".to_string(),
        "---\nregion.body: rotate=33\n---\n\n# T\n\n| a |\n|---|\n| b |\n".to_string(),
        "<Box x=\"1\"\n".to_string(),
    ]) {
        layouts.push(deck(&source).text_layout(0));
    }
    layouts.push(Arc::new(TextLayout::default()));
    for layout in &layouts {
        for &(x, y) in &points {
            let _ = layout.hit(x, y);
            let element = layout.element_at(x, y);
            if !x.is_finite() || !y.is_finite() {
                assert_eq!(element, None, "({x}, {y})");
            }
        }
        let (five, two) = (5, 2);
        for range in [0..0, five..two, usize::MAX - 1..usize::MAX, 0..usize::MAX] {
            let _ = layout.element(&range);
        }
    }
}

#[test]
fn what_cannot_be_typed_in_a_component_is_the_component() {
    let source = "# T\n\n<Box fill=\"#ff0000\">\n\nBoxed words\n\n```\ncode\n```\n\n</Box>\n";
    let layout = layout(source);
    let boxed = "<Box fill=\"#ff0000\">\n\nBoxed words\n\n```\ncode\n```\n\n</Box>";
    assert_eq!(hit_source(source, &hit_at(&layout, (103.0, 223.0))), boxed, "its padding");
    let words = (160.0, 260.0);
    assert_eq!(hit_source(source, &hit_at(&layout, words)), boxed, "its words");
    let code = layout.element_at(150.0, 340.0).expect("the box is under its code");
    assert_eq!(hit_at(&layout, (150.0, 340.0)), Hit::Element(code), "code in it");
    // Outside a component, they still say why.
    let mixed = self::layout("# T\n\nWords and ![a picture](x.png) mixed\n");
    assert!(matches!(hit_at(&mixed, (120.0, 240.0)), Hit::Uneditable(_)), "{:?}", hit_at(&mixed, (120.0, 240.0)));
    let quote = self::layout("# T\n\n> A quote\n");
    assert_eq!(hit_at(&quote, (130.0, 240.0)), Hit::Uneditable("Quotes can't be edited on the slide yet."));
}

#[test]
fn an_icon_in_a_quote_is_the_icon() {
    let source = "# T\n\n> <Icon name=\"star\" />\n";
    let layout = layout(source);
    let tag = "<Icon name=\"star\" />";
    let start = source.find(tag).unwrap();
    let icon = layout.element(&(start..start + tag.len())).expect("the icon is a thing of its own, with its tag's exact range");
    let at = centre(icon.rect);
    assert_eq!(hit_at(&layout, at), Hit::Element(icon.clone()), "a click on it is on the icon, not the quote");
    assert_eq!(hit_source(source, &hit_at(&layout, at)), tag);
    assert_eq!(layout.element_at(at.0, at.1), Some(icon));
}

#[test]
fn a_thin_line_is_picked_near_it() {
    let source = "# T\n\n<Shape kind=\"line\" x=\"100\" y=\"500\" w=\"400\" h=\"0\" outline=\"black\" />\n\n<Box x=\"350\" y=\"504\" w=\"200\" h=\"100\" fill=\"blue\" />\n";
    let mut d = deck(source);
    let layout = d.text_layout(0);
    let line = layout.element(&block_starting(&d, 0, "<Shape")).expect("the line");
    assert_eq!(line.rect, Rect { x: 100.0, y: 500.0, w: 400.0, h: 0.0 }, "its box is as drawn");
    for y in [493.0, 500.0, 507.0] {
        assert!(source_at(&d, &layout, (200.0, y)).starts_with("<Shape"), "at y {y}");
        assert!(hit_source(d.source(), &hit_at(&layout, (200.0, y))).starts_with("<Shape"), "hit at y {y}");
    }
    assert_eq!(source_at(&d, &layout, (200.0, 509.0)), "", "too far below");
    assert!(source_at(&d, &layout, (400.0, 506.0)).starts_with("<Box"), "the box drawn after it is on top");
    assert!(hit_source(d.source(), &hit_at(&layout, (400.0, 506.0))).starts_with("<Box"));
    assert!(source_at(&d, &layout, (400.0, 502.0)).starts_with("<Shape"), "above the box, the line's own");
    // Turned a quarter about its centre (300, 500), it stands upright from y 300 to 700.
    let turned = source.replace("h=\"0\"", "h=\"0\" rotate=\"90\"");
    let mut d = deck(&turned);
    let layout = d.text_layout(0);
    assert!(source_at(&d, &layout, (293.0, 400.0)).starts_with("<Shape"));
    assert_eq!(source_at(&d, &layout, (291.0, 400.0)), "");
    assert_eq!(source_at(&d, &layout, (200.0, 500.0)), "", "where it would be unturned");
}

#[test]
fn an_element_moves_with_its_slide_when_text_before_it_changes() {
    let before = "# One\n\n---\n\n# Two\n\n<Icon name=\"arrow-right\" x=\"100\" y=\"300\" size=\"96\" />\n";
    let mut d = deck(before);
    let icon = |source: &str| {
        let start = source.find("<Icon").unwrap();
        start..start + source[start..].find("/>").unwrap() + 2
    };
    assert!(d.text_layout(1).element(&icon(before)).is_some());
    let after = before.replacen("# One", "# One more", 1);
    assert_eq!(d.update_text(slides_format::parse_talk(&after)), vec![0], "the second slide is only moved");
    let moved = d.text_layout(1).element(&icon(&after)).expect("the icon is found where it's written now");
    assert_eq!(moved.source, icon(&after));
    assert!(d.text_layout(1).element(&icon(before)).is_none());
}

/// How long a table's cells take to be read as flows and laid out, as each keystroke in a cell
/// needs. Only meaningful in a release build:
/// `scripts/cargo-safe.sh test --release -p slides-engine --test text_geometry -- --ignored big_tables`.
#[test]
#[ignore = "timing; run in release"]
fn big_tables_are_read_and_laid_out_quickly() {
    use std::time::{Duration, Instant};
    for (columns, rows) in [(6, 8), (20, 50)] {
        let table = slides_format::new_table(columns, rows).unwrap().replace("|  ", "| word ");
        let source = format!("# A table\n\n{table}\n");
        // Every cell holds a word, as a table typed in does.
        let flows = text_flows(&slides_format::parse_talk(&source), 0);
        let cells: Vec<_> = flows.iter().filter(|flow| matches!(flow.place, FlowPlace::Cell { .. })).collect();
        assert_eq!(cells.len(), columns * (rows + 1));
        assert!(cells.iter().all(|cell| cell.paras.iter().any(|para| !para.spans.is_empty())), "a cell is empty");
        let mut deck = deck(&source);
        deck.text_layout(0);
        let median = |mut times: Vec<Duration>| {
            times.sort();
            times[times.len() / 2]
        };
        let (mut flows_took, mut layout_took, mut write_took, mut slide_took) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut source = source;
        for key in 0..60 {
            let talk = slides_format::parse_talk(&source);
            let start = Instant::now();
            let flows = text_flows(&talk, 0);
            flows_took.push(start.elapsed());
            let cell = flows.iter().position(|flow| matches!(flow.place, FlowPlace::Cell { row: 1, column: 0, .. })).unwrap();
            let text = format!("word{}", "a".repeat(key + 1));
            let paras = [slides_format::Para { kind: slides_format::ParaKind::Body, spans: vec![slides_format::Span { text, style: Default::default() }] }];
            let start = Instant::now();
            let change = slides_format::write_flow(&talk, 0, &flows[cell], &paras).expect("a cell's text");
            write_took.push(start.elapsed());
            source = change.apply(&source);
            let start = Instant::now();
            deck.update_text(slides_format::parse_talk(&source));
            // The slide's own layout, which its text's is read from.
            deck.specs(0);
            slide_took.push(start.elapsed());
            let layout = deck.text_layout(0);
            assert!(layout.caret(cell, at(0, key + 5)).is_some());
            layout_took.push(start.elapsed());
        }
        let cells = text_flows(deck.talk(), 0).len() - 1;
        println!(
            "{columns} by {} table, {cells} cells: text_flows {:?}, text_layout after an edit {:?} of which the slide's layout {:?}, write_flow {:?} (medians)",
            rows + 1,
            median(flows_took),
            median(layout_took),
            median(slide_took),
            median(write_took)
        );
    }
}

#[test]
fn every_region_of_the_layout_is_listed() {
    let layout = layout("---\nlayout: two-columns\n---\n\n# T\n\nOn the left\n");
    let names: Vec<&str> = layout.regions().iter().map(|region| region.name.as_str()).collect();
    assert_eq!(names, ["title", "left", "right"]);
    let empty: Vec<bool> = layout.regions().iter().map(|region| region.empty).collect();
    assert_eq!(empty, [false, false, true]);
    assert!(layout.regions().iter().all(|region| !region.moved && region.rotate == 0.0));
    assert_eq!(layout.region("right").unwrap().rect, Rect { x: 980.0, y: 220.0, w: 840.0, h: 780.0 });
    assert_eq!(layout.region("body"), None);
    assert!(self::layout("<Box x=\"0\" y=\"0\" w=\"10\" h=\"10\" />\n").regions().iter().all(|region| region.empty), "a placed box reads into none");
}

#[test]
fn a_moved_region_says_so() {
    let layout = layout("---\nregion.body: x=200 w=900 rotate=5 click=each\n---\n\n# T\n\nBody\n");
    let body = layout.region("body").unwrap();
    assert!(body.moved);
    assert_eq!((body.rect.x, body.rect.w, body.rotate), (200.0, 900.0, 5.0));
    assert!(!layout.region("title").unwrap().moved);
    for line in ["click=each enter=fade align=center", "x=abc", ""] {
        let layout = self::layout(&format!("---\nregion.body: {line}\n---\n\n# T\n\nBody\n"));
        assert!(!layout.region("body").unwrap().moved, "{line}");
    }
    let empty = self::layout("---\nregion.body: y=300\n---\n\n# T\n");
    let body = empty.region("body").unwrap();
    assert!(body.moved && body.empty);
    assert_eq!(body.rect.y, 300.0);
}

#[test]
fn a_flow_knows_its_region() {
    let source = "# T\n\n- one\n- two\n\n<Box x=\"1000\" y=\"600\" w=\"400\" h=\"200\">\n\nIn a box\n\n</Box>\n";
    let d = &mut deck(source);
    let layout = d.text_layout(0);
    let flows = text_flows(d.talk(), 0);
    for (flow, text) in flows.iter().enumerate() {
        let expected = match &text.place {
            FlowPlace::Region(name) => Some(name.as_str()),
            _ => None,
        };
        assert_eq!(layout.region_of_flow(flow), expected, "flow {flow}");
    }
    assert!(flows.iter().any(|flow| matches!(flow.place, FlowPlace::Tag(_))));
    assert_eq!(layout.region_of_flow(flows.len()), None);
    assert!(layout.region("body").unwrap().is_list);
    assert!(!layout.region("title").unwrap().is_list);
    assert!(self::layout("# T\n\nIntro\n\nMore\n").region("body").unwrap().is_list, "two blocks");
    assert!(!self::layout("# T\n\nJust one\n").region("body").unwrap().is_list);
}

#[test]
fn a_picture_keeps_its_shape_and_a_shape_does_not() {
    let source = "<Icon x=\"100\" y=\"100\" name=\"star\" size=\"64\" />\n\n<Shape x=\"400\" y=\"100\" w=\"200\" h=\"100\" kind=\"ellipse\" />\n\n<Picture x=\"700\" y=\"100\" w=\"200\" h=\"100\" src=\"missing.png\" />\n\n<Box x=\"1000\" y=\"100\" w=\"200\" h=\"100\" />\n";
    let d = &mut deck(source);
    let layout = d.text_layout(0);
    let element = |start: &str| layout.element(&block_starting(d, 0, start)).unwrap_or_else(|| panic!("{start}"));
    for (start, keeps) in [("<Icon", true), ("<Shape", false), ("<Picture", true), ("<Box", false)] {
        let element = element(start);
        assert_eq!(element.keeps_shape, keeps, "{start}");
        assert!(element.placed, "{start}");
    }
}

#[test]
fn a_tag_in_the_text_is_not_placed() {
    let source = "# T\n\n<Box fill=\"#dbeafe\">\n\nBoxed but not placed\n\n</Box>\n\n| a |\n|---|\n| b |\n\n<Box x=\"1000\" y=\"600\" w=\"400\" h=\"200\" fill=\"#dbeafe\">\n\n<Icon name=\"star\" />\n\n</Box>\n";
    let d = &mut deck(source);
    let layout = d.text_layout(0);
    let element = |start: &str| layout.elements().iter().find(|element| element.source == block_starting(d, 0, start)).cloned().unwrap_or_else(|| panic!("{start}"));
    assert!(!element("<Box fill").placed);
    assert!(!element("| a |").placed);
    assert!(element("<Box x=").placed);
    let icon = element("<Icon");
    assert!(!icon.placed, "inside a placed box");
    assert!(icon.keeps_shape);
}
