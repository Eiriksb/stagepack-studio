use proptest::prelude::*;
use slides_format::*;

fn plain(text: &str) -> Span {
    Span { text: text.to_string(), style: Style::default() }
}

fn styled(text: &str, style: Style) -> Span {
    Span { text: text.to_string(), style }
}

fn bold() -> Style {
    Style { bold: true, ..Style::default() }
}

fn para(kind: ParaKind, text: &str) -> Para {
    Para { kind, spans: vec![plain(text)] }
}

const BODY: ParaKind = ParaKind::Body;
const BULLET: ParaKind = ParaKind::Bullet { level: 0 };
const NUMBERED: ParaKind = ParaKind::Numbered { level: 0 };

/// The talk `source` becomes when flow `flow` of slide `slide` is made `paras`, and the
/// slide's flows read from it.
fn written(source: &str, slide: usize, flow: usize, paras: &[Para]) -> (String, Vec<TextFlow>) {
    let talk = parse_talk(source);
    let flows = text_flows(&talk, slide);
    let change = write_flow(&talk, slide, &flows[flow], paras).expect("the paragraphs can be written");
    let after = change.apply(&talk.source);
    let flows = text_flows(&parse_talk(&after), slide);
    (after, flows)
}

/// The flow of slide 0 in `region`.
fn flow_in(flows: &[TextFlow], region: &str) -> usize {
    flows.iter().position(|flow| flow.place == FlowPlace::Region(region.to_string())).expect("a flow in the region")
}

/// `spans` with `letter` added after the first character.
fn with_a_letter(spans: &[Span], letter: char) -> Vec<Span> {
    let mut spans = spans.to_vec();
    let first = spans[0].text.chars().next().unwrap().len_utf8();
    spans[0].text.insert(first, letter);
    spans
}

#[test]
fn typing_a_letter_keeps_the_rest_of_a_hand_written_paragraph() {
    let hand_written = [
        "A paragraph wrapped\nover three\nlines by hand.",
        "Bold **touching**, punctuation and **\"quotes\"**.",
        "Stars \\* and \\_lines\\_ escaped, and 2 \\* 3.",
        "See [the site](https://example.com/a_b?c=1) now.",
        "Run `cargo test -- --nocapture` here.",
        "First line\\\nsecond line after a hard break.",
        "Two spaces  \nmake a break too.",
        "<Text color=\"red\" size=\"64\">big **bold** words</Text> after.",
        "Nested <Text color=\"red\">red <Text size=\"30\">and small</Text></Text> text.",
        "*Emphasis* at the start, and ***both*** later.",
        "A [**bold link**](x.md) and `` code with ` tick ``.",
    ];
    for written_by_hand in hand_written {
        for (lead, after) in [("# Title\n\n", "\n\nAfter.\n"), ("# Title\r\n\r\n", "\r\n\r\n- after\r\n")] {
            let source = format!("{lead}{}{after}", if lead.contains('\r') { written_by_hand.replace('\n', "\r\n") } else { written_by_hand.to_string() });
            let talk = parse_talk(&source);
            let flows = text_flows(&talk, 0);
            let at = flow_in(&flows, "body");
            let flow = &flows[at];
            let mut paras = flow.paras.clone();
            paras[0].spans = with_a_letter(&paras[0].spans, 'x');
            let change = write_flow(&talk, 0, flow, &paras).unwrap_or_else(|| panic!("refused {written_by_hand:?}"));
            assert!(!change.is_empty(), "nothing written for {written_by_hand:?}");
            let after_source = change.apply(&source);
            let read = text_flows(&parse_talk(&after_source), 0);
            assert_eq!(read[at].paras, storable(&paras), "{written_by_hand:?} became {after_source:?}");
            let inline = &flow.source[0].inline;
            assert_eq!(&after_source[..inline.start], &source[..inline.start]);
            assert!(after_source.ends_with(&source[inline.end..]), "{after_source:?}");
        }
    }
}

#[test]
fn a_list_items_continued_lines_keep_its_column() {
    let source = "# T\n\n- one\n  continued\n- two\n";
    let talk = parse_talk(source);
    let flow = &text_flows(&talk, 0)[1];
    let mut paras = flow.paras.clone();
    paras[0].spans = vec![plain("onex\ncontinued\nand more")];
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(after, "# T\n\n- onex\\\n  continued\\\n  and more\n- two\n");
    assert_eq!(flows[1].paras, paras);
}

#[test]
fn a_new_paragraph_takes_the_lists_marker() {
    let source = "# T\n\n* a\n* b\n\nAfter.\n";
    let paras = [para(BULLET, "a"), para(BULLET, "b"), para(BULLET, "c"), para(BODY, "After.")];
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(after, "# T\n\n* a\n* b\n* c\n\nAfter.\n");
    assert_eq!(flows[1].paras, paras);

    let nested = "# T\n\n- a\n  + b\n";
    let paras = [para(BULLET, "a"), para(ParaKind::Bullet { level: 1 }, "b"), para(ParaKind::Bullet { level: 1 }, "c")];
    let (after, _) = written(nested, 0, 1, &paras);
    assert_eq!(after, "# T\n\n- a\n  + b\n  + c\n");
}

#[test]
fn numbers_run_on() {
    let source = "# T\n\n1. a\n2. b\n";
    let paras = [para(NUMBERED, "a"), para(NUMBERED, "b"), para(NUMBERED, "c")];
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(after, "# T\n\n1. a\n2. b\n3. c\n");
    assert_eq!(flows[1].paras, paras);

    let from_seven = "# T\n\n7) a\n";
    let paras = [para(NUMBERED, "a"), para(NUMBERED, "b")];
    assert_eq!(written(from_seven, 0, 1, &paras).0, "# T\n\n7) a\n8) b\n");
}

#[test]
fn a_paragraph_removed_takes_its_blank_line() {
    let source = "# T\n\nA\n\nB\n\nC\n";
    let (after, _) = written(source, 0, 1, &[para(BODY, "A"), para(BODY, "C")]);
    assert_eq!(after, "# T\n\nA\n\nC\n");
    let (after, _) = written(source, 0, 1, &[para(BODY, "B"), para(BODY, "C")]);
    assert_eq!(after, "# T\n\nB\n\nC\n");
    let crlf = "# T\r\n\r\n- a\r\n- b\r\n- c\r\n";
    let (after, _) = written(crlf, 0, 1, &[para(BULLET, "a"), para(BULLET, "c")]);
    assert_eq!(after, "# T\r\n\r\n- a\r\n- c\r\n");
}

#[test]
fn a_paragraph_split_in_two_keeps_the_rest() {
    let source = "# T\n\nOne two.\n\nThree.\n";
    let (after, _) = written(source, 0, 1, &[para(BODY, "One"), para(BODY, "two."), para(BODY, "Three.")]);
    assert_eq!(after, "# T\n\nOne\n\ntwo.\n\nThree.\n");
}

#[test]
fn a_paragraph_made_a_bullet_gets_a_marker() {
    let source = "# T\n\nOne.\n\nTwo.\n";
    let paras = [para(BODY, "One."), para(BULLET, "Two.")];
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(after, "# T\n\nOne.\n\n- Two.\n");
    assert_eq!(flows[1].paras, paras);
}

#[test]
fn an_indented_bullet_is_written_under_the_one_before() {
    let source = "# T\n\n- a\n- b\n- c\n";
    let paras = [para(BULLET, "a"), para(ParaKind::Bullet { level: 1 }, "b"), para(BULLET, "c")];
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(flows[1].paras, paras);
    assert!(after.starts_with("# T\n\n- a\n") && after.ends_with("- c\n"), "{after:?}");
}

#[test]
fn what_cannot_be_written_is_refused() {
    let source = "# T\n\nWords.\n";
    let talk = parse_talk(source);
    let flow = &text_flows(&talk, 0)[1];
    let refused = [
        vec![para(ParaKind::Bullet { level: 1 }, "no item above")],
        vec![Para { kind: BODY, spans: vec![styled("a\nb", Style { code: true, ..Style::default() })] }],
        vec![para(ParaKind::Heading(3), "two\nlines")],
    ];
    for paras in refused {
        assert!(write_flow(&talk, 0, flow, &paras).is_none(), "{paras:?}");
    }
    let stale = TextFlow { paras: vec![para(BODY, "Other")], ..flow.clone() };
    assert!(write_flow(&talk, 0, &stale, &[para(BODY, "x")]).is_none());
}

#[test]
fn text_goes_into_an_empty_title_as_a_heading() {
    let source = "---\nlayout: content\n---\n\nBody text.\n";
    let talk = parse_talk(source);
    let title = [para(ParaKind::Heading(1), "Hello")];
    let after = write_into_region(&talk, 0, "title", &title).unwrap().apply(source);
    assert_eq!(after, "---\nlayout: content\n---\n# Hello\n\nBody text.\n");
    let flows = text_flows(&parse_talk(&after), 0);
    assert_eq!(flows[flow_in(&flows, "title")].paras, title);

    let empty = "# A\n\n---\n";
    let talk = parse_talk(empty);
    let after = write_into_region(&talk, 1, "title", &title).unwrap().apply(empty);
    assert_eq!(after, "# A\n\n---\n# Hello\n");

    // Not a heading, or a title that's there already.
    assert!(write_into_region(&talk, 1, "title", &[para(BODY, "x")]).is_none());
    assert!(write_into_region(&talk, 0, "title", &title).is_none());
}

#[test]
fn text_goes_into_the_right_column_after_its_marker() {
    let source = "---\nlayout: two-columns\n---\n# T\n\nLeft.\n";
    let talk = parse_talk(source);
    let words = [para(BODY, "Right.")];
    let after = write_into_region(&talk, 0, "right", &words).unwrap().apply(source);
    assert_eq!(after, "---\nlayout: two-columns\n---\n# T\n\nLeft.\n\n<Region name=\"right\" />\n\nRight.\n");
    let flows = text_flows(&parse_talk(&after), 0);
    assert_eq!(flows[flow_in(&flows, "right")].paras, words);

    let marked = "---\nlayout: two-columns\n---\n# T\n\n<Region name=\"right\" />\n";
    let talk = parse_talk(marked);
    let after = write_into_region(&talk, 0, "right", &words).unwrap().apply(marked);
    assert_eq!(after, "---\nlayout: two-columns\n---\n# T\n\n<Region name=\"right\" />\n\nRight.\n");

    let main = "# T\n";
    let talk = parse_talk(main);
    let after = write_into_region(&talk, 0, "body", &[para(BULLET, "a"), para(BULLET, "b")]).unwrap().apply(main);
    assert_eq!(after, "# T\n\n- a\n- b\n");
    assert!(write_into_region(&talk, 0, "nowhere", &words).is_none());
}

#[test]
fn bold_that_cannot_close_falls_back_to_a_tag() {
    let spans = [plain("a"), styled("\"b\"", bold()), plain("c")];
    assert_eq!(write_inline(&spans), "a<Text bold=\"true\">\"b\"</Text>c");
    let spaced = [plain("a"), styled(" b ", bold()), plain("c")];
    assert_eq!(write_inline(&spaced), "a<Text bold=\"true\"> b </Text>c");
    for spans in [&spans[..], &spaced[..]] {
        let flows = text_flows(&parse_talk(&write_inline(spans)), 0);
        assert_eq!(flows[0].paras[0].spans, spans);
    }
    assert_eq!(write_inline(&[plain("a "), styled("b", bold()), plain(" c")]), "a **b** c");
}

#[test]
fn styles_are_written_as_markdown_where_they_can_be() {
    let red = Style { color: Some("red".into()), size: Some(64), ..Style::default() };
    let spans = [plain("Revenue is "), styled("up", bold()), plain(" "), styled("12%", red), plain(" this year.")];
    assert_eq!(write_inline(&spans), "Revenue is **up** <Text color=\"red\" size=\"64\">12%</Text> this year.");
    let linked = Style { link: Some("https://example.com".into()), ..Style::default() };
    let code = Style { code: true, ..Style::default() };
    assert_eq!(write_inline(&[styled("site", linked), plain(" and "), styled("a`b", code)]), "[site](https://example.com) and ``a`b``");
    assert_eq!(write_inline(&[plain("# not a heading\n- not a list")]), "\\# not a heading\\\n\\- not a list");
}

#[test]
fn a_bom_does_not_stop_the_first_flow_being_edited() {
    for source in ["\u{feff}# T\n\nWords.\n", "\u{feff}Plain.\n", "\u{feff}- a\n- b\n", "\u{feff}# T\r\n\r\nWords.\r\n"] {
        let talk = parse_talk(source);
        let flow = &text_flows(&talk, 0)[0];
        let mut paras = flow.paras.clone();
        paras[0].spans = with_a_letter(&paras[0].spans, 'x');
        if flow.place != FlowPlace::Region("title".into()) {
            paras.push(para(BULLET, "added"));
        }
        let change = write_flow(&talk, 0, flow, &paras).unwrap_or_else(|| panic!("refused {source:?}"));
        let after = change.apply(source);
        assert!(after.starts_with('\u{feff}'), "{after:?}");
        assert_eq!(text_flows(&parse_talk(&after), 0)[0].paras, paras, "{after:?}");
    }
}

#[test]
fn ordinary_sentences_are_written_as_typed() {
    let sentences = [
        "Q&A",
        "R&D and P&L",
        "snake_case and file_name.txt",
        "2 * 3 = 6",
        "C:\\Users\\me",
        "a | b",
        "Revenue grew 12% in Q3 - up from 9.5% a year ago.",
        "Wait... what?!",
        "50/50 (roughly)",
        "it's #1",
    ];
    for typed in sentences {
        assert_eq!(write_inline(&[plain(typed)]), typed);
        let source = "# T\n\nOld.\n";
        let (after, flows) = written(source, 0, 1, &[para(BODY, typed)]);
        assert_eq!(after, format!("# T\n\n{typed}\n"));
        assert_eq!(flows[1].paras, [para(BODY, typed)]);
    }
    // What would be read as something is still escaped.
    for (typed, escaped) in [("&amp;", "\\&amp;"), ("_a_", "\\_a\\_"), ("2*3*4", "2\\*3\\*4"), ("a\\*", "a\\\\\\*"), ("&#32;", "\\&#32;")] {
        assert_eq!(write_inline(&[plain(typed)]), escaped);
    }
}

#[test]
fn an_exclamation_mark_before_a_link_stays_text() {
    let link = Style { link: Some("https://example.com".into()), ..Style::default() };
    let spans = [plain("Look!"), styled("here", link)];
    let written = write_inline(&spans);
    assert_eq!(written, "Look\\![here](https://example.com)");
    assert_eq!(text_flows(&parse_talk(&written), 0)[0].paras[0].spans, spans);
}

#[test]
fn a_refusal_is_not_an_empty_change() {
    let source = "# T\n\nWords.\n";
    let talk = parse_talk(source);
    let flow = &text_flows(&talk, 0)[1];
    let same = write_flow(&talk, 0, flow, &flow.paras.clone());
    assert_eq!(same, Some(Change { edits: Vec::new() }));
    assert_eq!(write_flow(&talk, 0, flow, &[para(ParaKind::Bullet { level: 3 }, "x")]), None);
    // The body has text already.
    assert_eq!(write_into_region(&talk, 0, "body", &[para(BODY, "x")]), None);
    assert_eq!(write_into_region(&talk, 0, "body", &[]), None);
    let bare = parse_talk("# T
");
    assert_eq!(write_into_region(&bare, 0, "body", &[]), Some(Change { edits: Vec::new() }));
}

#[test]
fn nothing_goes_into_a_region_the_layout_lacks() {
    let talk = parse_talk("# T
");
    assert_eq!(write_into_region(&talk, 0, "nowhere", &[]), None);
    assert_eq!(write_into_region(&talk, 0, "nowhere", &[para(BODY, "x")]), None);
}

#[test]
fn enter_at_the_end_of_a_paragraph_is_not_refused() {
    let source = "# T\n\na\n";
    let paras = [para(BODY, "a"), Para { kind: BODY, spans: vec![] }];
    assert_eq!(storable(&paras), paras);
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(after, "# T\n\na\n\n**<Text></Text>**\n");
    assert_eq!(flows[1].paras, paras);
    // Typing into the empty paragraph.
    let (after, flows) = written(&after, 0, 1, &[para(BODY, "a"), para(BODY, "b")]);
    assert_eq!(after, "# T\n\na\n\nb\n");
    assert_eq!(flows[1].paras, [para(BODY, "a"), para(BODY, "b")]);
    // An empty bullet and an empty heading are a marker alone.
    let (after, flows) = written(source, 0, 1, &[para(BODY, "a"), Para { kind: BULLET, spans: vec![] }, Para { kind: ParaKind::Heading(2), spans: vec![] }]);
    assert_eq!(after, "# T\n\na\n\n-\n\n##\n");
    assert_eq!(flows[1].paras.len(), 3);
}

#[test]
fn a_flow_of_empty_paragraphs_reads_back() {
    let empty = Para { kind: BODY, spans: vec![] };
    let paras = [empty.clone(), empty.clone(), empty];
    let (after, flows) = written("# T\n\nWords.\n\n---\n\n# Next\n", 0, 1, &paras);
    assert_eq!(flows[1].paras, paras, "{after:?}");
    assert!(after.ends_with("\n\n---\n\n# Next\n"));
}

#[test]
fn a_break_at_the_end_reads_back() {
    let source = "# T\n\nOld.\n";
    let cases = [
        vec![para(BODY, "a\n")],
        vec![para(BODY, "\n")],
        vec![para(BODY, "\n\na\n\n")],
        vec![Para { kind: BODY, spans: vec![plain("\nwords"), styled("bold\n", bold())] }],
        vec![Para { kind: BODY, spans: vec![styled("red\n", Style { color: Some("red".into()), ..Style::default() })] }],
        vec![Para { kind: BODY, spans: vec![styled("linked\n", Style { link: Some("x.md".into()), ..Style::default() })] }],
        vec![para(BODY, "x"), para(BULLET, "item\n"), para(NUMBERED, "\n")],
        vec![para(ParaKind::Heading(2), "two\n")],
    ];
    for paras in cases {
        let (after, flows) = written(source, 0, 1, &paras);
        assert_eq!(flows[1].paras, paras, "{after:?}");
    }
    assert_eq!(written(source, 0, 1, &[para(BODY, "a\n")]).0, "# T\n\na\\\n**<Text></Text>**\n");
}

#[test]
fn all_the_text_can_be_deleted() {
    for (source, slide, flow, expected) in [
        ("# A\n\nWords.\n\n---\n\n# B\n\nMore.\n", 1, 1, "# A\n\nWords.\n\n---\n\n# B\n\n"),
        ("# A\n\nWords.\n\n---\n\n# B\n\nMore.\n", 0, 1, "# A\n\n---\n\n# B\n\nMore.\n"),
        ("Only words.\n\n---\n\n# B\n", 0, 0, "\n\n---\n\n# B\n"),
        ("---\nlayout: content\n---\n\nOnly words.\n", 0, 0, "---\nlayout: content\n---\n\n"),
        ("---\nlayout: content\n---\nOnly words.\n", 0, 0, "---\nlayout: content\n---\n"),
    ] {
        let talk = parse_talk(source);
        let flows = text_flows(&talk, slide);
        {
            let paras: Vec<Para> = Vec::new();
            let change = write_flow(&talk, slide, &flows[flow], &paras).unwrap_or_else(|| panic!("refused {source:?}"));
            let after = change.apply(source);
            assert_eq!(after, expected);
            let read = parse_talk(&after);
            assert_eq!(read.slides.len(), talk.slides.len());
            assert_eq!(text_flows(&read, slide).len(), flows.len() - 1);
        }
    }
}

#[test]
fn untouched_items_keep_their_markers_when_the_list_is_written_again() {
    let source = "# T\n\n1) one\n   - inner\n     1. deep\n2) two\n";
    let talk = parse_talk(source);
    let flow = &text_flows(&talk, 0)[1];
    let mut paras = flow.paras.clone();
    paras[3].kind = BODY;
    let after = write_flow(&talk, 0, flow, &paras).unwrap().apply(source);
    assert_eq!(after, "# T\n\n1) one\n   - inner\n     1. deep\n\ntwo\n");
    assert_eq!(text_flows(&parse_talk(&after), 0)[1].paras, paras);
}

/// `source`'s body flow with item `which` made a body paragraph.
fn made_body(source: &str, which: usize) -> String {
    let talk = parse_talk(source);
    let flows = text_flows(&talk, 0);
    let at = flow_in(&flows, "body");
    let mut paras = flows[at].paras.clone();
    paras[which].kind = BODY;
    let after = write_flow(&talk, 0, &flows[at], &paras).expect("written").apply(source);
    assert_eq!(text_flows(&parse_talk(&after), 0)[at].paras, paras, "{after:?}");
    after
}

#[test]
fn untouched_numbers_stay_as_they_were() {
    assert_eq!(made_body("# T\n\n1. a\n2. b\n3. c\n4. d\n", 3), "# T\n\n1. a\n2. b\n3. c\n\nd\n");
}

#[test]
fn untouched_numbers_after_a_nested_list_stay_as_they_were() {
    let source = "# T\n\n1) one\n   - inner\n     1. deep\n2) two\n3) three\n";
    assert_eq!(made_body(source, 4), "# T\n\n1) one\n   - inner\n     1. deep\n2) two\n\nthree\n");
}

#[test]
fn untouched_numbers_from_ten_stay_as_they_were() {
    assert_eq!(made_body("# T\n\n10) ten\n11) eleven\n12) twelve\n", 2), "# T\n\n10) ten\n11) eleven\n\ntwelve\n");
}

#[test]
fn an_untouched_items_spacing_stays() {
    assert_eq!(made_body("# T\n\n-   wide\n-   wider\n- last\n", 2), "# T\n\n-   wide\n-   wider\n\nlast\n");
    assert_eq!(made_body("# T\n\n1.  one\n    - in\n2.  two\n", 2), "# T\n\n1.  one\n    - in\n\ntwo\n");
}

#[test]
fn code_of_three_backticks_is_written() {
    let code = Style { code: true, ..Style::default() };
    let paras = [Para { kind: BODY, spans: vec![styled("```", code.clone())] }];
    let source = "# T\n\nOld.\n\n---\n\n# Next\n";
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(flows[1].paras, paras, "{after:?}");
    assert!(after.ends_with("\n\n---\n\n# Next\n"));
    assert_eq!(parse_talk(&after).slides.len(), 2);
}

#[test]
fn a_paragraph_between_two_items_has_a_blank_line_on_each_side() {
    let source = "# T\n\n- a\n- b\n";
    let paras = [para(BULLET, "a"), para(BODY, "between"), para(BULLET, "b")];
    let (after, flows) = written(source, 0, 1, &paras);
    assert_eq!(after, "# T\n\n- a\n\nbetween\n\n- b\n");
    assert_eq!(flows[1].paras, paras);
}

// ---------------------------------------------------------------------------------------
// Properties

fn text_piece() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "a", "Z", "word", "é", "e\u{301}", "🎉", "👨\u{200d}👩\u{200d}👧", "🇳🇴", " ", "  ", "\t", "*", "**", "_", "#", "-", "+", "=", "1.", "2)",
        ">", "<", "</Text>", "<Text>", "&", "&amp;", "&#32;", "\\", "`", "```", "~~~", "[", "]", "(", ")", "[x](y)", "|", "!", ":", "{", "}",
        "\"", "'", "---", "***", "    ", "\n", "\n", ".",
    ])
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(text_piece(), 1..6).prop_map(|parts| parts.concat())
}

fn style() -> impl Strategy<Value = Style> {
    (
        any::<[bool; 4]>(),
        prop::option::of(prop::sample::select(vec!["red", "#ff0000", "a\"b&c"])),
        prop::option::of(prop::sample::select(vec![12u32, 64])),
        prop::option::of(prop::sample::select(vec!["https://example.com/a_b", "a b", "(x)", "", "x&amp;y"])),
    )
        .prop_map(|([bold, italic, underline, code], color, size, link)| Style {
            bold,
            italic,
            underline,
            code,
            color: color.map(str::to_string),
            size,
            link: link.map(str::to_string),
        })
}

fn spans() -> impl Strategy<Value = Vec<Span>> {
    prop::collection::vec((text(), style()).prop_map(|(text, style)| Span { text, style }), 0..5)
}

/// Kinds and spans made into paragraphs the editor could hold: no line break in code, a
/// heading of several lines only at level 1 or 2, and each item no deeper than the one
/// before it allows. Empty paragraphs and line breaks at the start and end are in.
fn paras() -> impl Strategy<Value = Vec<Para>> {
    let kind = prop_oneof![
        3 => Just(BODY),
        1 => (1..=6u8).prop_map(ParaKind::Heading),
        3 => (0..4u8).prop_map(|level| ParaKind::Bullet { level }),
        2 => (0..4u8).prop_map(|level| ParaKind::Numbered { level }),
    ];
    prop::collection::vec((kind, spans()), 1..6).prop_map(|raw| {
        let mut out: Vec<Para> = Vec::new();
        let mut kept: Option<Para> = None;
        for (mut kind, mut spans) in raw {
            let most = match &kept {
                Some(Para { kind: ParaKind::Bullet { level } | ParaKind::Numbered { level }, spans }) => level + u8::from(!spans.is_empty()),
                _ => 0,
            };
            match &mut kind {
                ParaKind::Bullet { level } | ParaKind::Numbered { level } => *level = (*level).min(most),
                ParaKind::Heading(level) if *level > 2 => spans.iter_mut().for_each(|span| span.text = span.text.replace('\n', "")),
                _ => {}
            }
            for span in spans.iter_mut().filter(|span| span.style.code) {
                span.text = span.text.replace('\n', "");
            }
            let para = Para { kind, spans };
            kept = storable(std::slice::from_ref(&para)).pop();
            out.push(para);
        }
        out
    })
}

/// Hand-written slides holding text flows, with the flow to write into.
fn talk() -> impl Strategy<Value = (String, usize, usize)> {
    let slides = vec![
        ("# Title\n\nPara one.\n\n- a\n- b\n  - c\n\n1. x\n2. y\n", 0usize, 1usize),
        ("---\nlayout: two-columns\n---\n# T\n\nLeft\n\n<Region name=\"right\" />\n\n* r1\n* r2\n", 0, 2),
        ("# T\n\n<Box x=\"10\" y=\"10\">\n\nIn a box\n\n</Box>\n\n| a |\n| - |\n| 1 |\n\nAfter the table\n", 0, 1),
        ("# T\n\n| a |\n| - |\n| 1 |\n\nAfter the table   \nwrapped\n\n<Notes>\nSay it\n</Notes>\n", 0, 1),
        ("# A\n\nFirst slide.\n\n---\n\n#   Odd   heading\n\n10. ten\n11. eleven\n\n```\n---\n```\n", 1, 1),
        ("Plain words first.\n\n- a\n- b\n", 0, 0),
    ];
    (prop::sample::select(slides), any::<[bool; 3]>()).prop_map(|((source, slide, flow), [crlf, more, bom])| {
        let mut source = source.to_string();
        if more {
            source.push_str("\n---\n\n# Another\n\nWith *words*.\n");
        }
        if crlf {
            source = source.replace('\n', "\r\n");
        }
        if bom {
            source.insert(0, '\u{feff}');
        }
        (source, slide, flow)
    })
}

proptest! {
    #[test]
    fn what_is_written_reads_back_the_same((source, slide, flow) in talk(), mut paras in paras()) {
        let talk = parse_talk(&source);
        let flows = text_flows(&talk, slide);
        if flow == 0 {
            // With no title above, a heading would be the slide's title.
            for para in paras.iter_mut().filter(|para| matches!(para.kind, ParaKind::Heading(_))) {
                para.kind = BODY;
            }
        }
        let stored = storable(&paras);
        let change = write_flow(&talk, slide, &flows[flow], &paras);
        prop_assert!(change.is_some(), "refused {:?}", stored);
        let after = change.unwrap().apply(&source);
        let read = parse_talk(&after);
        prop_assert_eq!(read.slides.len(), talk.slides.len());
        let mut read_flows = text_flows(&read, slide);
        if stored.is_empty() {
            prop_assert_eq!(read_flows.len() + 1, flows.len(), "{:?}", after);
            read_flows.insert(flow, TextFlow { paras: Vec::new(), ..flows[flow].clone() });
        } else {
            prop_assert_eq!(read_flows.len(), flows.len(), "{:?}", after);
            prop_assert_eq!(&read_flows[flow].paras, &stored, "{:?}", after);
        }
        for (other, (old, new)) in flows.iter().zip(&read_flows).enumerate() {
            if other != flow {
                prop_assert_eq!(&old.paras, &new.paras);
            }
        }
    }

    #[test]
    fn anything_typed_reads_back_as_typed(
        lead in prop::sample::select(vec!["", "#", "# ", "-", "- ", "1.", "1. ", ">", "---", "    ", "<", "<Text>", "* ", "===", "```", "|"]),
        typed in "[ -~\u{a0}-\u{24f}🎉]{0,40}",
    ) {
        let typed = format!("{lead}{typed}");
        prop_assume!(!typed.is_empty());
        let spans = vec![plain(&typed)];
        let alone = text_flows(&parse_talk(&write_inline(&spans)), 0);
        prop_assert_eq!(&alone[0].paras[0].spans, &spans);
        let source = "# Title\n\nOld words.\n\n---\n\n# Next\n";
        let (after, flows) = written(source, 0, 1, &[Para { kind: BODY, spans: spans.clone() }]);
        prop_assert_eq!(&flows[1].paras[0].spans, &spans, "{:?}", after);
        prop_assert!(after.ends_with("\n\n---\n\n# Next\n"));
    }

    #[test]
    fn only_the_changed_paragraphs_bytes_change(
        crlf in any::<bool>(),
        which in 0..6usize,
        new_text in "[a-z *_#<>`\\[\\]-]{1,20}",
        new_bold in any::<bool>(),
    ) {
        let source = "---\nlayout:   content  \n---\n\n#   Spaced   title  \n\n\nA hand-wrapped\n   paragraph with   odd spacing.  \n\n*   loose\n*   bullets\n\n1.  one\n2.  two\n\n## Heading ##\n\n<Notes>\nKeep me\n</Notes>\n";
        let source = if crlf { source.replace('\n', "\r\n") } else { source.to_string() };
        let talk = parse_talk(&source);
        let flows = text_flows(&talk, 0);
        let at = flow_in(&flows, "body");
        let flow = &flows[at];
        let which = which % flow.paras.len();
        let mut paras = flow.paras.clone();
        let style = Style { bold: new_bold, ..Style::default() };
        paras[which].spans = vec![styled(new_text.trim(), style)];
        prop_assume!(!new_text.trim().is_empty() && paras[which] != flow.paras[which]);
        let change = write_flow(&talk, 0, flow, &paras);
        prop_assert!(change.as_ref().is_some_and(|change| !change.is_empty()));
        let after = change.unwrap().apply(&source);
        let block = &flow.source[which].block;
        prop_assert_eq!(&after[..block.start], &source[..block.start]);
        prop_assert!(after.ends_with(&source[block.end..]), "{:?}", after);
        prop_assert_eq!(&text_flows(&parse_talk(&after), 0)[at].paras, &paras);
    }
}
