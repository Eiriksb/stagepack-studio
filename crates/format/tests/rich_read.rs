mod common;

use common::*;
use proptest::prelude::*;
use slides_format::*;

fn plain(text: &str) -> Span {
    Span { text: text.to_string(), style: Style::default() }
}

fn styled(text: &str, style: Style) -> Span {
    Span { text: text.to_string(), style }
}

fn para(kind: ParaKind, text: &str) -> Para {
    Para { kind, spans: vec![plain(text)] }
}

fn flows(source: &str) -> Vec<TextFlow> {
    text_flows(&parse_talk(source), 0)
}

fn region(name: &str) -> FlowPlace {
    FlowPlace::Region(name.to_string())
}

#[test]
fn a_title_and_body_are_two_flows() {
    let flows = flows("# Hello\n\nSome words.\n\nMore words.\n");
    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0].place, region("title"));
    assert_eq!(flows[0].paras, vec![para(ParaKind::Heading(1), "Hello")]);
    assert_eq!(flows[1].place, region("body"));
    assert_eq!(flows[1].paras, vec![para(ParaKind::Body, "Some words."), para(ParaKind::Body, "More words.")]);
}

#[test]
fn a_list_reads_as_bullets_with_levels() {
    let flows = flows("# T\n\n- one\n- two\n  - deeper\n    - deepest\n- back\n");
    let bullet = |level| ParaKind::Bullet { level };
    assert_eq!(
        flows[1].paras,
        vec![para(bullet(0), "one"), para(bullet(0), "two"), para(bullet(1), "deeper"), para(bullet(2), "deepest"), para(bullet(0), "back")]
    );
}

#[test]
fn a_numbered_list_reads_as_numbered() {
    let flows = flows("# T\n\n1. first\n2. second\n   - a point\n3. third\n");
    assert_eq!(
        flows[1].paras,
        vec![
            para(ParaKind::Numbered { level: 0 }, "first"),
            para(ParaKind::Numbered { level: 0 }, "second"),
            para(ParaKind::Bullet { level: 1 }, "a point"),
            para(ParaKind::Numbered { level: 0 }, "third"),
        ]
    );
}

#[test]
fn an_empty_item_is_an_empty_paragraph() {
    let flows = flows("-\n- a\n");
    assert_eq!(flows[0].paras, vec![Para { kind: ParaKind::Bullet { level: 0 }, spans: vec![] }, para(ParaKind::Bullet { level: 0 }, "a")]);
}

#[test]
fn an_empty_line_reads_as_an_empty_paragraph_in_either_spelling() {
    let empty = Para { kind: ParaKind::Body, spans: vec![] };
    for line in ["&nbsp;", "**<Text></Text>**"] {
        let flows = flows(&format!("# T\n\na\n\n{line}\n\nb\n"));
        assert_eq!(flows[1].paras, [para(ParaKind::Body, "a"), empty.clone(), para(ParaKind::Body, "b")], "{line}");
    }
    // Only alone: beside other text, or written as a number, it's the character.
    assert_eq!(flows("# T\n\na&nbsp;\n")[1].paras, [para(ParaKind::Body, "a\u{a0}")]);
    assert_eq!(flows("# T\n\n&#160;\n")[1].paras, [para(ParaKind::Body, "\u{a0}")]);
}

#[test]
fn a_no_break_space_item_with_items_under_it_can_be_typed_in() {
    let source = "# T\n\n- &nbsp;\n  - b\n- c\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let bullet = |level| ParaKind::Bullet { level };
    assert_eq!(flows[1].paras, [para(bullet(0), "\u{a0}"), para(bullet(1), "b"), para(bullet(0), "c")]);
    let mut paras = flows[1].paras.clone();
    paras[1].spans[0].text.push('x');
    let after = write_flow(&talk, 0, &flows[1], &paras).expect("typing is written").apply(source);
    assert_eq!(after, "# T\n\n- &nbsp;\n  - bx\n- c\n");
    // Alone, with nothing under it, it's an empty item.
    assert_eq!(text_flows(&parse_talk("# T\n\n- &nbsp;\n- c\n"), 0)[1].paras[0], Para { kind: bullet(0), spans: vec![] });
}

#[test]
fn a_no_break_space_item_left_with_nothing_under_it_keeps_its_space() {
    let source = "# T\n\n- &nbsp;\n  - b\n- c\n";
    let talk = parse_talk(source);
    let flow = &text_flows(&talk, 0)[1];
    let paras = [flow.paras[0].clone(), flow.paras[2].clone()];
    let after = write_flow(&talk, 0, flow, &paras).expect("the item under it can go").apply(source);
    assert_eq!(text_flows(&parse_talk(&after), 0)[1].paras, paras, "{after:?}");
    assert!(after.ends_with("\n- c\n"), "{after:?}");
}

#[test]
fn an_item_whose_content_the_parser_drops_is_in_no_flow() {
    for source in ["- ***\n- a\n", "- [r]: http://x\n- a\n"] {
        assert!(flows(source).is_empty(), "{source:?}");
        assert_eq!(uneditable_reason(&parse_talk(source), 0, 3), Some("This list can't be edited on the slide yet."), "{source:?}");
    }
    for source in ["-\n- a\n", "-   \n- a\n", "1.\n2. a\n"] {
        assert_eq!(flows(source)[0].paras[0].spans, vec![], "{source:?}");
    }
}

#[test]
fn text_tags_and_links_the_model_cannot_hold_are_in_no_flow() {
    for inline in ["<Text font=\"Mono\">f</Text>", "<Text click=\"2\">f</Text>", "<Text size=\"x\">f</Text>", "<Text bold=\"yes\">f</Text>", "[l](http://x \"tip\")"] {
        let source = format!("A {inline} b\n");
        let talk = parse_talk(&source);
        assert!(text_flows(&talk, 0).is_empty(), "{source:?}");
        assert!(uneditable_reason(&talk, 0, 0).is_some(), "{source:?}");
    }
    for inline in ["<Text color=\"red\" size=\"64\">f</Text>", "<Text bold=\"false\" underline>f</Text>", "[l](http://x)"] {
        assert_eq!(flows(&format!("A {inline} b\n")).len(), 1, "{inline:?}");
    }
}

#[test]
fn a_table_splits_the_text_around_it() {
    let flows = flows("# T\n\nBefore.\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\nAfter.\n");
    assert_eq!(flows.len(), 3 + 4, "the region's three, then the table's four cells");
    assert_eq!(flows[1].paras, vec![para(ParaKind::Body, "Before.")]);
    assert_eq!(flows[2].paras, vec![para(ParaKind::Body, "After.")]);
    assert_eq!(flows[1].place, flows[2].place);
}

#[test]
fn each_cell_of_a_table_is_a_flow_after_the_others() {
    let source = "# T\n\n| a | `b\\|c` |\n| :- | -: |\n| **1** |  |\n| x |\n\nAfter.\n";
    let flows = flows(source);
    let table = 5..source.find("\n\nAfter").unwrap();
    let cells: Vec<_> = flows[2..].iter().map(|flow| (flow.place.clone(), flow.paras.clone(), &source[flow.source[0].inline.clone()])).collect();
    let cell = |row, column| FlowPlace::Cell { table: table.clone(), row, column };
    let code = Span { text: "b|c".into(), style: Style { code: true, ..Style::default() } };
    let bold = Span { text: "1".into(), style: Style { bold: true, ..Style::default() } };
    assert_eq!(
        cells,
        [
            (cell(0, 0), vec![para(ParaKind::Body, "a")], "a"),
            (cell(0, 1), vec![Para { kind: ParaKind::Body, spans: vec![code] }], "`b\\|c`"),
            (cell(1, 0), vec![Para { kind: ParaKind::Body, spans: vec![bold] }], "**1**"),
            (cell(1, 1), vec![Para { kind: ParaKind::Body, spans: Vec::new() }], "  "),
            (cell(2, 0), vec![para(ParaKind::Body, "x")], "x"),
        ],
        "a row shorter than the header has only its own cells"
    );
    let empty = &flows[5].source[0];
    assert_eq!(&source[empty.inline.clone()], "  ", "an empty cell's content is all the space between its `|`s");
    let Block::Table { rule, .. } = &parse_talk(source).slides[0].blocks[1] else { panic!() };
    assert_eq!(&source[rule.clone()], "| :- | -: |");
}

#[test]
fn a_table_in_a_quote_or_a_component_has_no_flows() {
    for source in ["> | a |\n> | - |\n> | 1 |\n", "<Chart>\n\n| a |\n| - |\n| 1 |\n\n</Chart>\n"] {
        assert!(flows(source).is_empty(), "{source:?}");
        assert!(uneditable_reason(&parse_talk(source), 0, source.find('1').unwrap()).is_some(), "{source:?}");
    }
    let boxed = "<Box x=\"10\" y=\"10\">\n\n| a |\n| - |\n| 1 |\n\n</Box>\n\n- item\n\n  | b |\n  | - |\n";
    let places: Vec<_> = flows(boxed).into_iter().map(|flow| flow.place).collect();
    assert!(places.iter().all(|place| matches!(place, FlowPlace::Cell { .. })) && places.len() == 3, "{places:?}");
}

#[test]
fn styles_read_from_markdown_and_from_text_tags() {
    let source = "Plain **bold** *it* <Text underline>under</Text> <Text size=\"64\" color=\"red\">big **and bold**</Text> \
                  [link](https://example.com) `code` ***both***\n";
    let flows = flows(source);
    let bold = Style { bold: true, ..Style::default() };
    let big = Style { size: Some(64), color: Some("red".to_string()), ..Style::default() };
    assert_eq!(
        flows[0].paras[0].spans,
        vec![
            plain("Plain "),
            styled("bold", bold.clone()),
            plain(" "),
            styled("it", Style { italic: true, ..Style::default() }),
            plain(" "),
            styled("under", Style { underline: true, ..Style::default() }),
            plain(" "),
            styled("big ", big.clone()),
            styled("and bold", Style { bold: true, ..big }),
            plain(" "),
            styled("link", Style { link: Some("https://example.com".to_string()), ..Style::default() }),
            plain(" "),
            styled("code", Style { code: true, ..Style::default() }),
            plain(" "),
            styled("both", Style { bold: true, italic: true, ..Style::default() }),
        ]
    );
}

#[test]
fn spans_are_merged_and_never_empty() {
    let flows = flows("a<Text></Text>b **c**<Text bold>d</Text>\n");
    assert_eq!(flows[0].paras[0].spans, vec![plain("ab "), styled("cd", Style { bold: true, ..Style::default() })]);
}

#[test]
fn a_wrapped_paragraph_breaks_where_its_lines_do() {
    // The parser keeps a soft line break as a line ending in the text, and the layout
    // starts a new line there, as it does for a hard break.
    let flows = flows("One line\r\n   and the next  \r\nand a third\\\r\nfourth\n");
    assert_eq!(flows[0].paras[0].spans, vec![plain("One line\nand the next\nand a third\nfourth")]);
}

#[test]
fn inline_ranges_cover_exactly_the_content() {
    let source = "## Heading words ##\n\nA *body*\nparagraph.\n\n- a bullet\n  - nested\n\n3) numbered\n\nSetext title\n===\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let all: Vec<(&Para, &ParaSource)> = flows.iter().flat_map(|flow| flow.paras.iter().zip(&flow.source)).collect();
    let inline: Vec<&str> = all.iter().map(|(_, at)| &source[at.inline.clone()]).collect();
    let block: Vec<&str> = all.iter().map(|(_, at)| &source[at.block.clone()]).collect();
    assert_eq!(inline, ["Heading words", "A *body*\nparagraph.", "a bullet", "nested", "numbered", "Setext title"]);
    assert_eq!(block, ["## Heading words ##", "A *body*\nparagraph.", "- a bullet", "- nested", "3) numbered", "Setext title\n==="]);
    for (_, at) in all {
        assert!(at.block.start <= at.inline.start && at.inline.end <= at.block.end);
    }
}

#[test]
fn content_that_starts_with_an_escape_starts_at_its_backslash() {
    let source = "\\*a\n\n- \\#b\n  - \\[c]\n\n## \\_d\n\n\\-e\n\n\\\\f\n\n&amp;g\n\n\\*h\n===\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let inline: Vec<&str> = flows.iter().flat_map(|flow| &flow.source).map(|at| &source[at.inline.clone()]).collect();
    assert_eq!(inline, ["\\*a", "\\#b", "\\[c]", "\\_d", "\\-e", "\\\\f", "&amp;g", "\\*h"]);
}

#[test]
fn positions_are_in_the_whole_talk() {
    let source = "# One\n\n---\n\n# Two\n\nWords\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 1);
    assert_eq!(&source[flows[1].source[0].inline.clone()], "Words");
    assert_eq!(flows[0].paras, vec![para(ParaKind::Heading(1), "Two")]);
}

#[test]
fn text_in_a_box_is_a_flow_of_its_own() {
    let source = "# T\n\nBefore.\n\n<Box x=\"100\" y=\"200\" w=\"400\">\n\nIn the box.\n\n- and a point\n\n</Box>\n\nAfter.\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let Block::Tag(tag) = &talk.slides[0].blocks[2] else { panic!("the box is a tag") };
    let boxed: Vec<&TextFlow> = flows.iter().filter(|flow| flow.place == FlowPlace::Tag(tag.range.clone())).collect();
    assert_eq!(boxed.len(), 1);
    assert_eq!(boxed[0].paras, vec![para(ParaKind::Body, "In the box."), para(ParaKind::Bullet { level: 0 }, "and a point")]);
    // The box sits between the body's paragraphs in the file, so they're two runs.
    let body: Vec<&TextFlow> = flows.iter().filter(|flow| flow.place == region("body")).collect();
    assert_eq!(body.len(), 2);
}

#[test]
fn a_box_with_a_component_in_it_is_in_no_flow() {
    let source = "<Box x=\"1\" y=\"2\">\n\nWords\n\n<Chart />\n\n</Box>\n";
    assert!(flows(source).is_empty());
    let at = source.find("Words").unwrap();
    assert!(uneditable_reason(&parse_talk(source), 0, at).is_some());
}

#[test]
fn a_region_marker_starts_a_new_flow() {
    let source = "---\nlayout: two-columns\n---\n# Title\n\nLeft words.\n\n<Region name=\"right\" />\n\nRight words.\n\n<Region name=\"right\" />\n\nMore right.\n";
    let flows = flows(source);
    let places: Vec<&FlowPlace> = flows.iter().map(|flow| &flow.place).collect();
    assert_eq!(places, [&region("title"), &region("left"), &region("right"), &region("right")]);
    assert_eq!(flows[3].paras, vec![para(ParaKind::Body, "More right.")]);
}

#[test]
fn after_a_marker_a_heading_stays_where_it_is() {
    let flows = flows("<Region name=\"body\" />\n\n# Not the title\n");
    assert_eq!(flows[0].place, region("body"));
    assert_eq!(flows[0].paras, vec![para(ParaKind::Heading(1), "Not the title")]);
}

#[test]
fn a_marker_for_a_region_the_layout_lacks_goes_to_its_main_region() {
    let flows = flows("<Region name=\"nowhere\" />\n\nWords\n");
    assert_eq!(flows[0].place, region("body"));
}

#[test]
fn a_blank_slide_has_no_title() {
    let flows = flows("---\nlayout: blank\n---\n# Heading\n\nWords\n");
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].place, region("body"));
    assert_eq!(flows[0].paras.len(), 2);
}

#[test]
fn a_paragraph_with_a_component_in_it_is_in_no_flow() {
    let source = "# T\n\nOne.\n\nA <Badge /> here.\n\nTwo.\n\nA ![picture](a.png) here.\n\nThree.\n";
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let body: Vec<Vec<Para>> = flows[1..].iter().map(|flow| flow.paras.clone()).collect();
    assert_eq!(body, vec![vec![para(ParaKind::Body, "One.")], vec![para(ParaKind::Body, "Two.")], vec![para(ParaKind::Body, "Three.")]]);
    for word in ["Badge", "picture"] {
        let reason = uneditable_reason(&talk, 0, source.find(word).unwrap()).expect("a reason");
        assert!(reason.contains("component or picture"), "{reason}");
    }
}

#[test]
fn a_list_with_a_component_in_an_item_is_in_no_flow() {
    let source = "- one\n- two <Badge /> here\n";
    assert!(flows(source).is_empty());
    assert!(uneditable_reason(&parse_talk(source), 0, source.find("one").unwrap()).is_some());
}

#[test]
fn a_picture_or_an_icon_placed_on_the_slide_is_named_as_one() {
    // Seen in the real window: clicking a placed picture said "Text inside a component".
    let source = "# T\n\n<Picture src=\"a.png\" x=\"10\" y=\"10\" w=\"100\" h=\"100\" />\n\n<Icon name=\"arrow-right\" x=\"200\" y=\"10\" size=\"96\" />\n";
    let talk = parse_talk(source);
    let at = |word: &str| source.find(word).unwrap();
    assert_eq!(uneditable_reason(&talk, 0, at("Picture")), Some("Pictures can't be edited on the slide yet."));
    assert_eq!(uneditable_reason(&talk, 0, at("Icon")), Some("Icons can't be edited on the slide yet."));
}

#[test]
fn uneditable_reason_names_tables_code_and_quotes() {
    let source = "# T\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n```\nlet x = 1;\n```\n\n> A quote\n\nWords.\n";
    let talk = parse_talk(source);
    let reason = |text: &str| uneditable_reason(&talk, 0, source.find(text).unwrap());
    assert_eq!(reason("| 1"), None, "a table's cells can be typed in");
    assert_eq!(reason("let x"), Some("Code can't be edited on the slide yet."));
    assert_eq!(reason("A quote"), Some("Quotes can't be edited on the slide yet."));
    assert_eq!(reason("Words"), None);
    assert_eq!(reason("# T"), None);
    let flows = text_flows(&talk, 0);
    assert!(flows.iter().flat_map(|flow| &flow.paras).all(|para| para.spans != vec![plain("A quote")]));
}

#[test]
fn a_list_inside_a_quote_is_in_no_flow() {
    let source = "> - quoted\n> - points\n";
    assert!(flows(source).is_empty());
    assert_eq!(uneditable_reason(&parse_talk(source), 0, source.find("points").unwrap()), Some("Quotes can't be edited on the slide yet."));
}

#[test]
fn a_slide_that_could_not_be_read_has_no_flows() {
    let deep = format!("{}x\n", "> ".repeat(70));
    let talk = parse_talk(&deep);
    assert!(talk.slides[0].error.is_some());
    assert!(text_flows(&talk, 0).is_empty());
    assert!(uneditable_reason(&talk, 0, deep.len() - 2).is_some());
}

#[test]
fn an_empty_slide_has_no_flows() {
    assert!(flows("").is_empty());
    assert!(flows("---\nlayout: blank\n---\n").is_empty());
    assert!(flows("<Notes>\nSay hi\n</Notes>\n").is_empty());
    assert!(text_flows(&parse_talk("# One\n"), 5).is_empty());
}

/// Source made of pieces that read as every kind of block, so that the flows have
/// something to get wrong.
fn mixed_source() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "# A heading", "Words", "Two\nlines", "- a\n- b", "- a\n  - b\n    - c", "1. x\n2. y", "-", "> quote", "> - quoted",
            "| a |\n| - |\n| 1 |", "```\ncode\n```", "![pic](a.png)", "A <Badge /> b", "<Region name=\"right\" />",
            "<Region name=\"body\" />", "<Box x=\"1\">\n\nboxed\n\n</Box>", "<Box x=\"1\" y=\"2\" />", "<Text color=\"red\">\n\nred\n\n</Text>",
            "<Chart />", "<Notes>\nnotes\n</Notes>", "**bold** and *it*", "a  \nb", "---", "---\nlayout: two-columns\n---", "\r\n",
            "  indented", "    code by indent", "* star", "<Text size=\"40\">x</Text> y",
            "- ***", "- [r]: http://x", "-   ", "- a\n\n- b\n\n  - c", "<Text font=\"Mono\">f</Text> g", "[l](http://x \"tip\")",
            "<Box x=\"1\">\n\n- a\n\n  b\n\n</Box>",
            "\\*a", "- \\*a\n  - c\n- b", "# \\#tag", "\\[x]", "\\_y_", "\\- z", "\\\\w", "&amp; co", "1\\. not a list",
            "- \\[x] \\[y]", "## \\* starred", "\\\\\n===",
            "- a\n    - deep\n- b", "- a\n\t- deep", "1. a\n    - nested", " - a\n - b", "Intro:\n- a\n- b",
        ]),
        0..12,
    )
    .prop_map(|parts| parts.join("\n\n"))
}

fn assert_flows_are_sound(talk: &Talk) -> Result<(), TestCaseError> {
    for (index, slide) in talk.slides.iter().enumerate() {
        let flows = text_flows(talk, index);
        // A table's cells come after the other flows, so they're in order among themselves.
        let (mut blocks, mut cells) = (Vec::new(), Vec::new());
        for flow in &flows {
            let in_a_cell = matches!(flow.place, FlowPlace::Cell { .. });
            if in_a_cell {
                prop_assert!(flow.paras.len() == 1 && flow.paras[0].kind == ParaKind::Body);
            }
            prop_assert_eq!(flow.paras.len(), flow.source.len());
            prop_assert!(!flow.paras.is_empty());
            if let FlowPlace::Tag(range) = &flow.place {
                prop_assert!(slide.range.start <= range.start && range.end <= slide.range.end);
            }
            for (para, at) in flow.paras.iter().zip(&flow.source) {
                prop_assert!(at.block.start <= at.inline.start && at.inline.end <= at.block.end);
                prop_assert!(slide.range.start <= at.block.start && at.block.end <= slide.range.end);
                prop_assert!(talk.source.is_char_boundary(at.inline.start) && talk.source.is_char_boundary(at.inline.end));
                prop_assert!(talk.source.is_char_boundary(at.block.start) && talk.source.is_char_boundary(at.block.end));
                prop_assert!(para.spans.iter().all(|span| !span.text.is_empty()));
                prop_assert!(para.spans.windows(2).all(|pair| pair[0].style != pair[1].style));
                // The content's bytes, read on their own as a paragraph of the same kind,
                // give the same spans, where they read as one paragraph at all: a lone
                // `- *` is a list in a list, though it's text after another line.
                let content = &talk.source[at.inline.clone()];
                // A heading's content is tried both ways it's written: underlined, `>` starts
                // a quote, and after `# `, a `#` can be read as part of the marker.
                let alone = match para.kind {
                    ParaKind::Heading(_) => vec![format!("{content}\n==="), format!("# {content}")],
                    // An item whose first line is a marker alone, as `*` is, holds a list
                    // when it's read alone, and its other lines go with that.
                    ParaKind::Bullet { .. } | ParaKind::Numbered { .. }
                        if matches!(content.split(['\n', '\r']).next().map(str::trim_end), Some("*" | "-" | "+")) =>
                    {
                        vec![]
                    }
                    ParaKind::Bullet { .. } | ParaKind::Numbered { .. } => vec![format!("- {content}")],
                    // A cell's content is read alone in a cell: a `|` in it is escaped there.
                    ParaKind::Body if in_a_cell => vec![format!("| {content} |\n| - |")],
                    ParaKind::Body => vec![content.to_string()],
                };
                let read_alone: Vec<Vec<Span>> = alone
                    .iter()
                    .filter_map(|alone| match text_flows(&parse_talk(alone), 0).as_slice() {
                        [flow] if flow.paras.len() == 1 => Some(flow.paras[0].spans.clone()),
                        _ => None,
                    })
                    .collect();
                if content.is_empty() {
                    prop_assert!(para.spans.is_empty());
                } else if content == "&nbsp;" {
                    // Empty, or, in an item with items under it, the character.
                    prop_assert!(para.spans.is_empty() || para.spans == [plain("\u{a0}")], "{:?}", &para.spans);
                } else if !read_alone.is_empty() {
                    prop_assert!(read_alone.contains(&para.spans), "content {:?} of {:?} reads alone as {:?}", content, &talk.source, read_alone);
                }
                if in_a_cell { cells.push(at.block.clone()) } else { blocks.push(at.block.clone()) }
            }
            // Every byte of what the flow was read from is in it: a box from its opening tag
            // to its closing one, a list's markers and the lines between its items.
            let read: Vec<std::ops::Range<usize>> = match &flow.place {
                FlowPlace::Tag(range) => vec![range.clone()],
                FlowPlace::Cell { .. } => flow.source.iter().map(|at| at.block.clone()).collect(),
                FlowPlace::Region(_) => slide
                    .blocks
                    .iter()
                    .map(|block| block.range().clone())
                    .filter(|range| flow.source.iter().any(|at| range.start <= at.block.start && at.block.end <= range.end))
                    .collect(),
            };
            for range in read {
                for at in range {
                    prop_assert_eq!(uneditable_reason(talk, index, at), None, "at {} of {:?}", at, &talk.source);
                }
            }
        }
        for blocks in [&mut blocks, &mut cells] {
            let in_file_order = blocks.clone();
            blocks.sort_by_key(|block| block.start);
            prop_assert_eq!(&*blocks, &in_file_order);
            prop_assert!(blocks.windows(2).all(|pair| pair[0].end <= pair[1].start), "overlapping: {:?}", blocks);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    #[test]
    fn flows_never_overlap_and_stay_inside_their_slide(generated in gen_talk()) {
        assert_flows_are_sound(&parse_talk(&generated.text()))?;
    }

    #[test]
    fn flows_of_mixed_blocks_never_overlap_and_stay_inside_their_slide(source in mixed_source()) {
        let talk = parse_talk(&source);
        assert_flows_are_sound(&talk)?;
        for index in 0..talk.slides.len() {
            for at in 0..=source.len() {
                let _ = uneditable_reason(&talk, index, at);
            }
        }
    }

    #[test]
    fn reading_odd_text_never_panics(source in typed_notes()) {
        let talk = parse_talk(&source);
        assert_flows_are_sound(&talk)?;
    }
}
