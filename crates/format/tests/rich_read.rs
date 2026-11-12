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
fn a_table_splits_the_text_around_it() {
    let flows = flows("# T\n\nBefore.\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\nAfter.\n");
    assert_eq!(flows.len(), 3);
    assert_eq!(flows[1].paras, vec![para(ParaKind::Body, "Before.")]);
    assert_eq!(flows[2].paras, vec![para(ParaKind::Body, "After.")]);
    assert_eq!(flows[1].place, flows[2].place);
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
fn uneditable_reason_names_tables_code_and_quotes() {
    let source = "# T\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n```\nlet x = 1;\n```\n\n> A quote\n\nWords.\n";
    let talk = parse_talk(source);
    let reason = |text: &str| uneditable_reason(&talk, 0, source.find(text).unwrap());
    assert_eq!(reason("| 1"), Some("Tables can't be edited on the slide yet."));
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
        ]),
        0..12,
    )
    .prop_map(|parts| parts.join("\n\n"))
}

fn assert_flows_are_sound(talk: &Talk) -> Result<(), TestCaseError> {
    for (index, slide) in talk.slides.iter().enumerate() {
        let flows = text_flows(talk, index);
        let mut blocks = Vec::new();
        for flow in &flows {
            prop_assert_eq!(flow.paras.len(), flow.source.len());
            prop_assert!(!flow.paras.is_empty());
            if let FlowPlace::Tag(range) = &flow.place {
                prop_assert!(slide.range.start <= range.start && range.end <= slide.range.end);
            }
            for (para, at) in flow.paras.iter().zip(&flow.source) {
                prop_assert!(at.block.start <= at.inline.start && at.inline.end <= at.block.end);
                prop_assert!(slide.range.start <= at.block.start && at.block.end <= slide.range.end);
                prop_assert!(talk.source.is_char_boundary(at.inline.start) && talk.source.is_char_boundary(at.inline.end));
                prop_assert!(para.spans.iter().all(|span| !span.text.is_empty()));
                prop_assert!(para.spans.windows(2).all(|pair| pair[0].style != pair[1].style));
                blocks.push(at.block.clone());
            }
            prop_assert!(uneditable_reason(talk, index, flow.source[0].inline.start).is_none());
        }
        let in_file_order = blocks.clone();
        blocks.sort_by_key(|block| block.start);
        prop_assert_eq!(&blocks, &in_file_order);
        prop_assert!(blocks.windows(2).all(|pair| pair[0].end <= pair[1].start), "overlapping: {:?}", blocks);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

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
