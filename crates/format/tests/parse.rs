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
const LEVELS: usize = 5000;

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
    let deep: String = (0..LEVELS).map(|level| format!("{}- item\n", "  ".repeat(level))).collect();
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
