use slides_format::*;

const SLIDE: &str = r#"# Results

Revenue is **up** <Text color="red" size="64">12%</Text> this year.

<Box x="120" y="240" w="800" rotate="5">

Inside the box

</Box>

<Region name="right" />

- one
- two

<Notes>
Mention the office.
</Notes>
"#;

fn slide() -> Talk { parse_talk(SLIDE) }

#[test]
fn blocks_come_out_in_order() {
    let talk = slide();
    let blocks = &talk.slides[0].blocks;
    assert!(matches!(blocks[0], Block::Heading { level: 1, .. }));
    assert!(matches!(blocks[1], Block::Paragraph { .. }));
    assert!(matches!(blocks[2], Block::Tag(_)));
    assert!(matches!(&blocks[3], Block::Region { name, .. } if name == "right"));
    assert!(matches!(blocks[4], Block::List { ordered: false, .. }));
    assert_eq!(blocks.len(), 5);
}

#[test]
fn spans_carry_their_styles() {
    let talk = slide();
    let Block::Paragraph { spans, .. } = &talk.slides[0].blocks[1] else { panic!() };
    assert_eq!(spans[1].text, "up");
    assert!(spans[1].style.bold);
    assert_eq!(spans[3].text, "12%");
    assert_eq!(spans[3].style.color.as_deref(), Some("red"));
    assert_eq!(spans[3].style.size, Some(64));
}

#[test]
fn every_block_range_points_at_its_source() {
    let talk = slide();
    assert_eq!(&SLIDE[talk.slides[0].blocks[0].range().clone()], "# Results");
    assert!(SLIDE[talk.slides[0].blocks[2].range().clone()].starts_with("<Box x=\"120\""));
    assert!(SLIDE[talk.slides[0].blocks[2].range().clone()].ends_with("</Box>"));
}

#[test]
fn attributes_know_their_exact_bytes() {
    let talk = slide();
    let Block::Tag(tag) = &talk.slides[0].blocks[2] else { panic!() };
    let rotate = tag.attrs.iter().find(|a| a.name == "rotate").unwrap();
    assert_eq!(&SLIDE[rotate.value_range.clone()], "5");
    assert_eq!(&SLIDE[rotate.name_range.clone()], "rotate");
    assert_eq!(tag.attr("x"), Some("120"));
    assert!(matches!(&tag.children[0], Block::Paragraph { .. }));
}

#[test]
fn notes_come_out_of_the_blocks() {
    assert_eq!(slide().slides[0].notes.as_ref().unwrap().text, "Mention the office.");
}

#[test]
fn positions_are_file_offsets_on_later_slides() {
    let src = "# One\n\n---\n\n# Two\n";
    let talk = parse_talk(src);
    assert_eq!(&src[talk.slides[1].blocks[0].range().clone()], "# Two");
}

#[test]
fn braces_are_plain_text() {
    let talk = parse_talk("Use {curly} braces.\n");
    let Block::Paragraph { spans, .. } = &talk.slides[0].blocks[0] else { panic!() };
    assert_eq!(spans[0].text, "Use {curly} braces.");
}

#[test]
fn a_picture_alone_on_a_line_is_a_picture_block() {
    let talk = parse_talk("![A cat](assets/cat.png)\n");
    assert!(matches!(&talk.slides[0].blocks[0], Block::Picture { src, alt, .. } if src == "assets/cat.png" && alt == "A cat"));
}

#[test]
fn tables_keep_rows_and_cells() {
    let talk = parse_talk("| a | b |\n|---|---|\n| 1 | **2** |\n");
    let Block::Table { rows, .. } = &talk.slides[0].blocks[0] else { panic!() };
    assert_eq!(rows.len(), 2);
    assert!(rows[1][1][0].style.bold);
}

#[test]
fn bad_slide_doesnt_break_the_others() {
    let talk = parse_talk("# One\n\n---\n\n<Box>\n\nnever closed\n\n---\n\n# Three\n");
    assert_eq!(talk.slides.len(), 3);
    assert!(talk.slides[1].error.as_ref().unwrap().message.contains("Box"));
    assert!(talk.slides[0].error.is_none());
    assert!(matches!(talk.slides[2].blocks[0], Block::Heading { .. }));
}

const TOO_DEEP: &str = "This slide is nested too deeply to read";
/// Far deeper than the nesting limit, and still under the size limit.
const LEVELS: usize = 500;

/// `deep` as a slide of its own: an error with no blocks, and the slide after it parses.
fn assert_too_deep_but_the_next_slide_parses(deep: &str) {
    let talk = parse_talk(&format!("{deep}\n---\n\n# Next\n"));
    assert_eq!(talk.slides.len(), 2);
    assert_eq!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_DEEP));
    assert!(talk.slides[0].blocks.is_empty());
    assert!(talk.slides[1].error.is_none());
    assert!(matches!(talk.slides[1].blocks[0], Block::Heading { .. }));
}

fn nested_boxes(levels: usize) -> String {
    format!("{}Deep\n{}", "<Box>\n\n".repeat(levels), "\n</Box>\n".repeat(levels))
}

#[test]
fn deeply_nested_tags_are_a_slide_error_not_a_crash() {
    assert_too_deep_but_the_next_slide_parses(&nested_boxes(LEVELS));
}

#[test]
fn deeply_nested_quotes_are_a_slide_error_not_a_crash() {
    assert_too_deep_but_the_next_slide_parses(&format!("{} deep\n", ">".repeat(LEVELS)));
}

#[test]
fn deeply_nested_lists_are_a_slide_error_not_a_crash() {
    // Each level is longer than the last, so this is about as deep as the size limit allows.
    let deep: String = (0..170).map(|level| format!("{}- item\n", "  ".repeat(level))).collect();
    assert_too_deep_but_the_next_slide_parses(&deep);
}

#[test]
fn ordinary_nesting_still_parses() {
    let talk = parse_talk(&nested_boxes(10));
    assert!(talk.slides[0].error.is_none());
    let mut levels = 0;
    let mut blocks = &talk.slides[0].blocks;
    while let [Block::Tag(tag)] = blocks.as_slice() {
        levels += 1;
        blocks = &tag.children;
    }
    assert_eq!(levels, 10);
}

#[test]
fn lists_nested_on_one_line_are_a_slide_error_not_a_crash() {
    assert_too_deep_but_the_next_slide_parses(&format!("{}item\n", "- ".repeat(LEVELS)));
}

#[test]
fn a_closing_tag_that_is_really_code_or_a_link_does_not_cancel_an_opening_one() {
    for not_a_close in ["    </Box>", "[link](</Box>)", "[link](target \"</Box>\")", "[link]: </Box>"] {
        let opens = format!("<Box>\n\n{not_a_close}\n\n").repeat(LEVELS);
        assert_too_deep_but_the_next_slide_parses(&format!("{opens}Deep\n{}", "\n</Box>\n".repeat(LEVELS)));
    }
}

#[test]
fn something_that_only_looks_like_a_code_fence_cannot_hide_quotes() {
    // No next slide here: the splitter takes that line for a fence too, so a break after it wouldn't count.
    let talk = parse_talk(&format!("<Box a=\"\n```\n\">\n\n{} deep\n\n</Box>\n", ">".repeat(LEVELS)));
    assert_eq!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_DEEP));
}

#[test]
fn code_self_closed_tags_and_sibling_tags_are_not_nesting() {
    let many = 200;
    let sources = [
        format!("```\n{}```\n", "<Box>\n".repeat(many)),
        format!("~~~~\n```\n{}~~~~\n", "<Box>\n".repeat(many)),
        format!("{}\n", "`<Box>` ".repeat(many)),
        format!("{}\n", "``<Box> ` </Box>`` ".repeat(many)),
        "<Box />\n\n".repeat(many),
        "<Box title=\"a/>b\" />\n\n".repeat(many),
        "<Box>\n\nInside\n\n</Box>\n\n".repeat(many),
        "A <Text color=\"red\">word</Text> or two.\n\n".repeat(many),
        format!("{}\n", "\\<Box> ".repeat(many)),
    ];
    for source in sources {
        assert!(parse_talk(&source).slides[0].error.is_none(), "{}", &source[..40.min(source.len())]);
    }
}

#[test]
fn a_closing_tag_inside_a_long_attribute_does_not_cancel_an_opening_one() {
    let opens = "<Box a=\"\n</Box>\n\">\n\n".repeat(LEVELS);
    assert_too_deep_but_the_next_slide_parses(&format!("{opens}Deep\n{}", "\n</Box>\n".repeat(LEVELS)));
}

#[test]
fn a_fence_the_scan_cannot_trust_does_not_hide_tags() {
    for start in ["<Box a=\"\n```\n\" />\n\n", "  ```\n```\n\n"] {
        let talk = parse_talk(&format!("{start}{}", nested_boxes(LEVELS)));
        assert_eq!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_DEEP), "{start}");
    }
}

#[test]
fn tags_spread_over_several_lines_still_parse() {
    let talk = parse_talk(&"<Box\n  x=\"1\"\n/>\n\n<Box\n  y=\"2\"\n>\n\nInside\n\n</Box>\n\n".repeat(10));
    assert!(talk.slides[0].error.is_none());
    assert_eq!(talk.slides[0].blocks.len(), 20);
}

const TOO_LARGE: &str = "This slide is too large to read";
const MAX_BODY: usize = 32 * 1024;
const MANY: usize = 20_000;
const FEWER: usize = 1_000;

/// `body`, as a slide of its own, is refused, and the slide after it parses.
fn assert_refused_but_the_next_slide_parses(body: &str) -> Talk {
    let talk = parse_talk(&format!("{body}\n---\n\n# Next\n"));
    assert_eq!(talk.slides.len(), 2);
    assert!(talk.slides[0].error.is_some());
    assert!(talk.slides[0].blocks.is_empty());
    assert!(talk.slides[1].error.is_none());
    assert!(matches!(talk.slides[1].blocks[0], Block::Heading { .. }));
    talk
}

/// `body(n)` is refused at `MANY` repeats, which is over the size limit, at `FEWER`, and at
/// as many as fit under the limit, where the refusal has to come from reading the slide.
fn assert_refused_at_every_size(body: impl Fn(usize) -> String) {
    assert_refused_but_the_next_slide_parses(&body(MANY));
    assert_refused_but_the_next_slide_parses(&body(FEWER));
    let (empty, repeat) = (body(0).len(), body(1).len() - body(0).len());
    let most = body((MAX_BODY - 1 - empty) / repeat);
    assert!(most.len() < MAX_BODY && MAX_BODY - most.len() <= repeat + 1, "{} bytes", most.len());
    let talk = assert_refused_but_the_next_slide_parses(&most);
    assert_ne!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_LARGE));
}

#[test]
fn quotes_after_a_bare_carriage_return_are_refused_not_a_crash() {
    assert_refused_but_the_next_slide_parses(&format!("x\r\r{} deep\r", ">".repeat(MANY)));
}

#[test]
fn a_self_close_inside_an_attribute_expression_is_refused_not_a_crash() {
    assert_refused_at_every_size(|n| "<Box a={/>\n}>\n\n".repeat(n));
}

#[test]
fn a_closing_tag_inside_an_attribute_expression_is_refused_not_a_crash() {
    assert_refused_at_every_size(|n| "<Box a={1>2\n</Box>\n}>\n\n".repeat(n));
}

#[test]
fn a_closing_tag_in_a_link_target_on_the_next_line_is_refused_not_a_crash() {
    assert_refused_at_every_size(|n| "<Box>\n\n[a](\n</Box>)\n\n".repeat(n));
}

#[test]
fn tags_between_backticks_on_different_lines_are_refused_not_a_crash() {
    assert_refused_at_every_size(|n| format!("`x\ny` {}d{} `z\n", "<a>".repeat(n), "</a>".repeat(n)));
}

#[test]
fn deep_emphasis_is_refused_not_a_crash() {
    assert_refused_at_every_size(|n| format!("{}b{}", "*a ".repeat(n), " a*".repeat(n)));
}

#[test]
fn emphasis_packed_as_deep_as_the_size_limit_allows_is_refused_not_a_crash() {
    assert_refused_at_every_size(|n| format!("{}b{}", "*".repeat(n), "*".repeat(n)));
}

#[test]
fn reading_a_deep_slide_does_not_use_up_the_callers_stack() {
    let deep = format!("{}b{}\n", "*a ".repeat(5000), " a*".repeat(5000));
    let caller = std::thread::Builder::new().stack_size(64 * 1024).spawn(move || parse_talk(&deep)).unwrap();
    let talk = caller.join().unwrap();
    assert_eq!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_DEEP));
}

#[test]
fn markers_repeated_along_a_line_are_refused_without_the_long_wait() {
    // Each of these keeps the `markdown` crate busy for many seconds if it gets that far.
    let sources = [
        format!("{}item\n", "- ".repeat(MAX_BODY / 2 - 8)),
        format!("x\r\r{} deep\r", ">".repeat(MAX_BODY - 20)),
    ];
    for source in sources {
        let started = std::time::Instant::now();
        let talk = parse_talk(&source);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "took {:?}", started.elapsed());
        assert_eq!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_DEEP));
    }
}

fn empty_nested_boxes(levels: usize) -> String {
    format!("{}{}", "<Box>\n".repeat(levels), "</Box>\n".repeat(levels))
}

fn nested_emphasis(levels: usize) -> String {
    format!("{}b{}\n", "*a ".repeat(levels), " a*".repeat(levels))
}

fn assert_reads_at(levels: usize, body: impl Fn(usize) -> String) {
    let talk = parse_talk(&body(levels));
    assert_eq!(talk.slides[0].error, None, "{levels} levels");
    assert!(!talk.slides[0].blocks.is_empty());
    assert_too_deep_but_the_next_slide_parses(&body(levels + 1));
}

#[test]
fn exactly_64_nested_tags_are_read() {
    assert_reads_at(64, nested_boxes);
    assert_reads_at(64, empty_nested_boxes);
    let talk = parse_talk(&nested_boxes(64));
    let mut levels = 0;
    let mut blocks = &talk.slides[0].blocks;
    while let [Block::Tag(tag)] = blocks.as_slice() {
        levels += 1;
        blocks = &tag.children;
    }
    assert_eq!(levels, 64);
}

#[test]
fn exactly_64_levels_of_quotes_lists_or_emphasis_are_read() {
    assert_reads_at(64, |levels| format!("{} deep\n", ">".repeat(levels)));
    assert_reads_at(64, |levels| format!("{}item\n", "- ".repeat(levels)));
    assert_reads_at(64, nested_emphasis);
}

#[test]
fn a_slide_over_the_size_limit_is_turned_away_unread() {
    let talk = parse_talk(&format!("{}\n---\n\n# Next\n", "a".repeat(MAX_BODY)));
    assert_eq!(talk.slides[0].error.as_ref().map(|e| e.message.as_str()), Some(TOO_LARGE));
    assert!(talk.slides[0].blocks.is_empty());
    assert!(talk.slides[1].error.is_none());
    assert!(matches!(talk.slides[1].blocks[0], Block::Heading { .. }));
}

#[test]
fn a_slide_exactly_at_the_size_limit_is_read() {
    let talk = parse_talk(&format!("{}\n---\n\n# Next\n", "a".repeat(MAX_BODY - 1)));
    assert_eq!(talk.slides[0].error, None);
    assert!(matches!(talk.slides[0].blocks[0], Block::Paragraph { .. }));
}

#[test]
fn a_real_fenced_block_full_of_tags_is_not_refused() {
    let many = 200;
    let sources = [
        format!("  ```\n{}  ```\n", "<a></a>\n".repeat(many)),
        format!("<Box\n  x=\"1\"\n/>\n\n```\n{}```\n", "<a></a>\n".repeat(many)),
    ];
    for source in sources {
        assert_eq!(parse_talk(&source).slides[0].error, None, "{}", &source[..20]);
    }
}

#[test]
fn a_line_of_backticks_with_more_backticks_is_not_a_fence() {
    // Markdown reads `` ```a```b `` as code in a paragraph, so the `---` after it starts a slide.
    assert_eq!(parse_talk("```a```b\n- \n---\na").slides.len(), 2);
    assert_eq!(parse_talk("``` a ` b\n---\na").slides.len(), 2);
    // Tildes open a fence whatever follows them, and backticks with none after them do.
    assert_eq!(parse_talk("~~~a`b\n---\n~~~\n").slides.len(), 1);
    assert_eq!(parse_talk("```rust\n---\n```\n").slides.len(), 1);
}
