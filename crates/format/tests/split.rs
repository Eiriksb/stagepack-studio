use slides_format::*;

const TWO: &str = "---\ntitle: Review\nformat: 1\n---\n\n# One\n\n---\nlayout: section\n---\n\n# Two\n";

#[test]
fn settings_blocks_belong_to_their_slides() {
    let slides = split_slides(TWO);
    assert_eq!(slides.len(), 2);
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("Review"));
    assert_eq!(slides[1].settings.as_ref().unwrap().get("layout"), Some("section"));
    assert_eq!(&TWO[slides[1].body.clone()], "\n# Two\n");
}

#[test]
fn plain_rule_starts_a_slide_with_no_settings() {
    let src = "# A\n\n---\n\n# B\n";
    let slides = split_slides(src);
    assert_eq!(slides.len(), 2);
    assert!(slides[1].settings.is_none());
    assert_eq!(&src[slides[1].body.clone()], "\n# B\n");
}

#[test]
fn rule_inside_code_fence_is_not_a_slide_break() {
    assert_eq!(split_slides("```\n---\n```\n").len(), 1);
    assert_eq!(split_slides("~~~\n---\n~~~\n").len(), 1);
}

#[test]
fn crlf_offsets_are_exact() {
    let src = "---\r\ntitle: A\r\n---\r\n# One\r\n---\r\n# Two\r\n";
    let slides = split_slides(src);
    assert_eq!(slides.len(), 2);
    assert_eq!(&src[slides[0].body.clone()], "# One\r\n");
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("A"));
    assert_eq!(&src[slides[1].body.clone()], "# Two\r\n");
}

#[test]
fn bom_is_skipped() {
    let slides = split_slides("\u{feff}---\ntitle: A\n---\n# One\n");
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("A"));
}

#[test]
fn value_range_points_at_the_value() {
    let src = "---\nlayout:   title  \n---\n";
    let settings = split_slides(src).remove(0).settings.unwrap();
    assert_eq!(&src[settings.entries[0].value_range.clone()], "title");
}

#[test]
fn every_byte_belongs_to_exactly_one_slide() {
    let slides = split_slides(TWO);
    assert_eq!(slides[0].range.start, 0);
    assert_eq!(slides[0].range.end, slides[1].range.start);
    assert_eq!(slides[1].range.end, TWO.len());
}

#[test]
fn a_fence_closes_only_on_as_many_of_its_marks_and_nothing_else() {
    // Too few marks, or words after them, leave the fence open.
    assert_eq!(split_slides("````\n```\n---\n````\n").len(), 1);
    assert_eq!(split_slides("```\n```x\n---\n```\n").len(), 1);
    assert_eq!(split_slides("~~~\n~~~ \n---\n").len(), 2);
    // Four spaces in, it's not a fence, at the top of a slide.
    assert_eq!(split_slides("    ```\n---\n").len(), 2);
    assert_eq!(split_slides("```\n    ```\n---\n```\n").len(), 1);
}

/// Whether line `line` of `source` is inside a block of code, as Markdown reads it alone.
fn in_code(source: &str, line: usize) -> bool {
    let root = markdown::to_mdast(source, &markdown::ParseOptions::default()).expect("plain Markdown always parses");
    let mut to_visit = vec![&root];
    while let Some(node) = to_visit.pop() {
        if let (markdown::mdast::Node::Code(_), Some(at)) = (node, node.position())
            && (at.start.line..=at.end.line).contains(&(line + 1))
        {
            return true;
        }
        to_visit.extend(node.children().into_iter().flatten());
    }
    false
}

proptest::proptest! {
    /// A `---` line starts a slide exactly when Markdown doesn't read it as part of a block
    /// of code. Lines here sit at the top level: in a list or a quote, the slide splitter
    /// can't see where code ends.
    #[test]
    fn a_rule_breaks_a_slide_unless_markdown_reads_it_as_code(
        lines in proptest::collection::vec(
            proptest::sample::select(vec![
                "```", "````", "```rust", "```a```b", "``` a ` b", "```x", "```  ", "``", " ```", "   ```", "    ```", "\t```",
                "~~~", "~~~~", "~~~a`b", "~~~ ```", "   ~~~", "~~~x", "---", "words", "", "`code` here",
            ]),
            0..14,
        )
    ) {
        let source = lines.join("\n");
        let slides = split_slides(&source);
        let mut start = 0;
        for (index, line) in lines.iter().enumerate() {
            if *line == "---" {
                let breaks = if index == 0 { slides[0].body.start > 0 } else { slides.iter().any(|slide| slide.range.start == start) };
                proptest::prop_assert_eq!(breaks, !in_code(&source, index), "line {} of {:?}", index, source);
            }
            start += line.len() + 1;
        }
    }
}
