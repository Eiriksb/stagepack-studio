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
