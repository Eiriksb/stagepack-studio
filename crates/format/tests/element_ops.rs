mod common;

use std::ops::Range;

use common::*;
use proptest::prelude::*;
use slides_format::*;

/// Runs `test` on text with `\n` line endings, and again with every `\n` made `\r\n`.
fn on_both(test: fn(fn(&str) -> String)) {
    test(|text| text.to_string());
    test(|text| text.replace('\n', "\r\n"));
}

const STAR: &str = "<Icon name=\"star\" />";

/// What the property tests put on a slide.
const INSERTS: [&str; 6] = [
    STAR,
    "<Box x=\"1\" y=\"2\">\n\nIn a *box*\n\n</Box>",
    "<Shape kind='a \"round\" one' />",
    "| a | b |\n| --- | --- |\n| 1 | 2 |",
    "![A dog](dog.png)",
    "Plain words",
];

/// Every block on `slide`, at any depth, with how many blocks it is with those inside it.
fn all_blocks(slide: &Slide) -> Vec<(Range<usize>, usize)> {
    fn walk(blocks: &[Block], out: &mut Vec<(Range<usize>, usize)>) -> usize {
        let mut total = 0;
        for block in blocks {
            let at = out.len();
            out.push((block.range().clone(), 0));
            let inside = match block {
                Block::List { items, .. } => items.iter().map(|item| walk(item, out)).sum(),
                Block::Tag(tag) => walk(&tag.children, out),
                _ => 0,
            };
            out[at].1 = 1 + inside;
            total += 1 + inside;
        }
        total
    }
    let mut out = Vec::new();
    walk(&slide.blocks, &mut out);
    out
}

/// What `blocks` say and how they nest, wherever they are.
fn outline(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { spans, .. } => format!("p{spans:?}"),
            Block::List { ordered, items, .. } => format!("l{ordered}[{}]", items.iter().map(|item| outline(item)).collect::<Vec<_>>().join("|")),
            Block::Tag(tag) => format!("t{}{:?}[{}]", tag.name, attrs_of(tag), outline(&tag.children)),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn block_count(slide: &Slide) -> usize {
    all_blocks(slide).len()
}

/// Every tag on the talk, at any depth, by slide.
fn all_tags(talk: &Talk) -> Vec<(usize, Tag)> {
    fn walk(index: usize, blocks: &[Block], out: &mut Vec<(usize, Tag)>) {
        for block in blocks {
            match block {
                Block::List { items, .. } => items.iter().for_each(|item| walk(index, item, out)),
                Block::Tag(tag) => {
                    out.push((index, tag.clone()));
                    walk(index, &tag.children, out);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for (index, slide) in talk.slides.iter().enumerate() {
        walk(index, &slide.blocks, &mut out);
    }
    out
}

fn attrs_of(tag: &Tag) -> Vec<(String, String)> {
    tag.attrs.iter().map(|attr| (attr.name.clone(), attr.value.clone())).collect()
}

/// Whether every slide but `index` has the same bytes in `after` as in `before`.
fn other_slides_alike(before: &Talk, after: &Talk, index: usize) -> bool {
    before.slides.len() == after.slides.len()
        && (0..before.slides.len())
            .filter(|&other| other != index)
            .all(|other| before.source[before.slides[other].range.clone()] == after.source[after.slides[other].range.clone()])
}

/// Whether every `\n` in `text` is part of a `\r\n`.
fn only_crlf(text: &str) -> bool {
    text.match_indices('\n').all(|(at, _)| at > 0 && text.as_bytes()[at - 1] == b'\r')
}

/// Text someone might type into an attribute's field, with everything that could be read as
/// something else.
fn typed_value() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "\"", "'", "<", "/>", ">", "{", "}", "|", "&quot;", "&amp;", "&apos;", "&#65;", "&", "&a", "&#1", "---", "`", "```", "\\", "\\\"", " ",
            "  ", "a", "Z", "é", "🎉", "👩‍👩‍👧", "</Box>", "=", "\n", "\t", "\r\n",
        ]),
        0..10,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(256), ..ProptestConfig::default() })]

    #[test]
    fn inserting_adds_only_its_own_lines(generated in gen_talk(), index in 0..9usize, which in 0..INSERTS.len()) {
        let source = generated.text();
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let eol = if generated.crlf { "\r\n" } else { "\n" };
        let text = INSERTS[which];
        let change = insert_block(&talk, index, text);
        if talk.slides[index].error.is_some() {
            prop_assert!(change.is_none());
            return Ok(());
        }
        let change = change.expect("a block can be added to a slide that reads");
        prop_assert_eq!(change.edits.len(), 1);
        let edit = &change.edits[0];
        prop_assert!(edit.range.is_empty());
        let slide = &talk.slides[index];
        prop_assert!(slide.range.start <= edit.range.start && edit.range.start <= slide.range.end);
        let own = text.replace('\n', eol);
        let rest = edit.text.replacen(&own, "", 1);
        prop_assert!(edit.text.contains(&own) && rest.chars().all(|c| c == '\r' || c == '\n'), "added {:?}", edit.text);
        if generated.crlf {
            prop_assert!(only_crlf(&edit.text));
        }
        let new = change.apply(&source);
        let after = parse_talk(&new);
        prop_assert!(other_slides_alike(&talk, &after, index));
        prop_assert_eq!(after.slides[index].blocks.len(), slide.blocks.len() + 1);
        prop_assert_eq!(change.inverse(&source).apply(&new), source);
    }

    #[test]
    fn deleting_then_undoing_is_exact(generated in gen_talk(), index in 0..9usize, which in 0..64usize) {
        let source = generated.text();
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let blocks = all_blocks(&talk.slides[index]);
        if blocks.is_empty() {
            return Ok(());
        }
        let (range, _) = &blocks[which % blocks.len()];
        if let Some(change) = delete_block(&talk, index, range) {
            let new = change.apply(&source);
            prop_assert_eq!(change.inverse(&source).apply(&new), source);
        }
    }

    #[test]
    fn deleting_removes_only_its_own_lines(generated in gen_talk(), index in 0..9usize, which in 0..64usize) {
        let source = generated.text();
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let slide = &talk.slides[index];
        let blocks = all_blocks(slide);
        if blocks.is_empty() {
            return Ok(());
        }
        let (range, size) = &blocks[which % blocks.len()];
        let Some(change) = delete_block(&talk, index, range) else { return Ok(()) };
        prop_assert_eq!(change.edits.len(), 1);
        let edit = &change.edits[0];
        prop_assert!(edit.text.is_empty());
        prop_assert!(edit.range.start <= range.start && range.end <= edit.range.end);
        let around = format!("{}{}", &source[edit.range.start..range.start], &source[range.end..edit.range.end]);
        prop_assert!(around.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n' | '>')), "also removed {:?}", around);
        let after = parse_talk(&change.apply(&source));
        prop_assert!(other_slides_alike(&talk, &after, index));
        prop_assert_eq!(block_count(&after.slides[index]), block_count(slide) - size);
    }

    #[test]
    fn any_value_reads_back_as_typed(
        generated in gen_talk(),
        which in 0..64usize,
        name in prop::sample::select(vec!["name", "x", "color", "flag", "new-one"]),
        value in typed_value(),
    ) {
        let source = generated.text();
        let talk = parse_talk(&source);
        let tags = all_tags(&talk);
        if tags.is_empty() {
            return Ok(());
        }
        let (index, tag) = &tags[which % tags.len()];
        let change = set_tag_attr(&talk, *index, &tag.range, name, Some(&value));
        let writable = !value.chars().any(char::is_control) && !(value.contains('"') && value.contains('\''));
        let Some(change) = change else {
            prop_assert!(!writable, "refused {:?}", value);
            return Ok(());
        };
        prop_assert!(writable);
        for edit in &change.edits {
            prop_assert!(tag.range.start < edit.range.start && edit.range.end < tag.open_end);
        }
        let new = change.apply(&source);
        let after = parse_talk(&new);
        prop_assert!(other_slides_alike(&talk, &after, *index));
        let shift = new.len() as isize - source.len() as isize;
        let moved = tag.range.start..tag.range.end.wrapping_add_signed(shift);
        let read = find_tag(&after, *index, &moved).expect("the tag is where it was");
        prop_assert_eq!(read.attr(name), Some(value.as_str()));
        let mut expected = attrs_of(tag);
        match expected.iter_mut().find(|(found, _)| found == name) {
            Some((_, old)) => *old = value.clone(),
            None => expected.push((name.to_string(), value.clone())),
        }
        prop_assert_eq!(attrs_of(read), expected);
        prop_assert_eq!(change.inverse(&source).apply(&new), source);
    }
}

/// Tags as people write them by hand.
const HAND_WRITTEN: [&str; 9] = [
    "<Icon name=\"star\" />",
    "<Box x=\"10\" y=\"20\" w=\"30\">\n\nIn a *box*\n\n</Box>",
    "<Shape\n  kind=\"circle\"\n  color=\"red\"\n/>",
    "<Icon name='a \"quoted\" star' size=\"2\" />",
    "<Icon name=\"first\" flag name=\"second\" />",
    "<Icon name = \"spaced\"   size='2'/>",
    "<Box>\n\n<Icon name=\"in-a-box\" />\n\n</Box>",
    "- an item\n\n  <Icon name=\"in-a-list\" />",
    "<Chart\tdata=\"x.csv\"   tall >\n\nSome words\n\n</Chart>",
];

#[test]
fn a_hand_written_tag_keeps_its_other_bytes() {
    for shape in HAND_WRITTEN {
        for crlf in [false, true] {
            let mut source = format!("# Title\n\n{shape}\n\nAfter\n");
            if crlf {
                source = source.replace('\n', "\r\n");
            }
            let talk = parse_talk(&source);
            assert!(talk.slides[0].error.is_none(), "{shape}");
            for (_, tag) in all_tags(&talk) {
                let names: Vec<String> = tag.attrs.iter().map(|attr| attr.name.clone()).chain(["new-one".to_string()]).collect();
                for name in &names {
                    // The bytes the attribute is written in, with the space before it: the only
                    // ones that may change.
                    let own = tag.attrs.iter().find(|attr| &attr.name == name).map(|attr| {
                        let end = attr.value_range.end + usize::from(source[attr.value_range.end..].starts_with(['"', '\'']));
                        attr.name_range.start - 1..end
                    });
                    for value in [Some("v2"), Some("it's"), None] {
                        let change = set_tag_attr(&talk, 0, &tag.range, name, value).unwrap_or_else(|| panic!("{shape}: {name} = {value:?}"));
                        let new = change.apply(&source);
                        match (&own, value) {
                            (None, None) => assert!(change.is_empty()),
                            (None, Some(_)) => {
                                assert_eq!(change.edits.len(), 1);
                                assert!(change.edits[0].range.is_empty());
                            }
                            (Some(own), Some(_)) => {
                                assert_eq!(change.edits.len(), 1, "{shape}: {name}");
                                let edit = &change.edits[0];
                                assert!(own.start <= edit.range.start && edit.range.end <= own.end, "{shape}: {name} {edit:?} {own:?}");
                            }
                            (Some(_), None) => {
                                for (edit, attr) in change.edits.iter().zip(tag.attrs.iter().filter(|attr| &attr.name == name)) {
                                    assert!(edit.text.is_empty());
                                    assert_eq!(edit.range.start, attr.name_range.start - 1, "{shape}: {name}");
                                    assert!(edit.range.end >= attr.value_range.end && edit.range.end <= attr.value_range.end + 1);
                                }
                            }
                        }
                        let after = parse_talk(&new);
                        let shift = new.len() as isize - source.len() as isize;
                        let read = find_tag(&after, 0, &(tag.range.start..tag.range.end.wrapping_add_signed(shift))).expect("the tag stays");
                        assert_eq!(read.attr(name), value, "{shape}: {name}");
                        assert_eq!(change.inverse(&source).apply(&new), source);
                    }
                }
            }
        }
    }
}

#[test]
fn a_tag_is_written_on_one_line_or_around_its_body() {
    assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[("name", "star")], body: None }).unwrap(), STAR);
    assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[], body: None }).unwrap(), "<Icon />");
    assert_eq!(
        tag_text(&NewTag { name: "Box", attrs: &[("x", "1"), ("y", "2")], body: Some("Hello\r\n\r\nthere\n") }).unwrap(),
        "<Box x=\"1\" y=\"2\">\n\nHello\n\nthere\n\n</Box>"
    );
    assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[("name", "a \"b\"")], body: None }).unwrap(), "<Icon name='a \"b\"' />");
    assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[("name", "Q&A &amp;")], body: None }).unwrap(), "<Icon name=\"Q&A &amp;amp;\" />");
    for name in ["icon", "Icon-x", "", "Ic on", "Ïcon"] {
        assert_eq!(tag_text(&NewTag { name, attrs: &[], body: None }), None, "{name}");
    }
    for attr in ["Name", "1x", "", "a_b", "a b", "on:click"] {
        assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[(attr, "1")], body: None }), None, "{attr}");
    }
    for value in ["one\ntwo", "tab\there", "bell\u{7}"] {
        assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[("name", value)], body: None }), None, "{value:?}");
    }
}

#[test]
fn a_written_tag_reads_back_as_given() {
    let values = ["star", "a \"b\"", "it's", "&quot;", "&amp;", "&#65;", "\\", "{x}", "<b>", "/>", " spaced ", "🎉", "---", "`code`", "", "Q&A", "Q&a", "&#1", "&"];
    for value in values {
        let text = tag_text(&NewTag { name: "Icon", attrs: &[("name", value)], body: None }).unwrap();
        let talk = parse_talk(&format!("{text}\n"));
        let [Block::Tag(tag)] = talk.slides[0].blocks.as_slice() else { panic!("{text}: {:?}", talk.slides[0]) };
        assert_eq!(tag.attr("name"), Some(value), "{text}");
    }
}

#[test]
fn a_value_with_both_quotes_is_refused() {
    let both = "it's \"quoted\"";
    assert_eq!(tag_text(&NewTag { name: "Icon", attrs: &[("name", both)], body: None }), None);
    let source = "# T\n\n<Icon name=\"star\" />\n";
    let talk = parse_talk(source);
    let tag = all_tags(&talk)[0].1.range.clone();
    assert_eq!(set_tag_attr(&talk, 0, &tag, "name", Some(both)), None);
    assert_eq!(set_tag_attr(&talk, 0, &tag, "other", Some(both)), None);
}

#[test]
fn a_setting_is_changed_added_and_taken_away() {
    on_both(|e| {
        let source = e("# T\n\n<Icon name=\"star\" size='2' flag />\n");
        let talk = parse_talk(&source);
        let tag = all_tags(&talk)[0].1.range.clone();
        let set = |name: &str, value: Option<&str>| set_tag_attr(&talk, 0, &tag, name, value).unwrap().apply(&source);
        assert_eq!(set("name", Some("moon")), e("# T\n\n<Icon name=\"moon\" size='2' flag />\n"));
        assert_eq!(set("name", Some("a \"b\"")), e("# T\n\n<Icon name='a \"b\"' size='2' flag />\n"));
        assert_eq!(set("size", Some("it's")), e("# T\n\n<Icon name=\"star\" size=\"it's\" flag />\n"));
        assert_eq!(set("flag", Some("no")), e("# T\n\n<Icon name=\"star\" size='2' flag=\"no\" />\n"));
        assert_eq!(set("color", Some("red")), e("# T\n\n<Icon name=\"star\" size='2' flag color=\"red\" />\n"));
        assert_eq!(set("size", None), e("# T\n\n<Icon name=\"star\" flag />\n"));
        assert_eq!(set("flag", None), e("# T\n\n<Icon name=\"star\" size='2' />\n"));
        assert_eq!(set_tag_attr(&talk, 0, &tag, "color", None), Some(Change { edits: Vec::new() }));
        assert_eq!(set_tag_attr(&talk, 0, &tag, "name", Some("star")), Some(Change { edits: Vec::new() }));
        assert_eq!(set_tag_attr(&talk, 0, &(0..3), "name", Some("x")), None, "no tag there");
        assert_eq!(set_tag_attr(&talk, 0, &tag, "Name", Some("x")), None, "not a name");
    });
}

#[test]
fn the_attribute_written_twice_that_is_read_is_the_one_changed() {
    let source = "<Icon name=\"first\" name=\"second\" />\n";
    let talk = parse_talk(source);
    let tag = all_tags(&talk)[0].1.range.clone();
    assert_eq!(set_tag_attr(&talk, 0, &tag, "name", Some("new")).unwrap().apply(source), "<Icon name=\"new\" name=\"second\" />\n");
    assert_eq!(set_tag_attr(&talk, 0, &tag, "name", None).unwrap().apply(source), "<Icon />\n");
}

#[test]
fn insert_into_an_empty_slide() {
    on_both(|e| {
        let source = e("# One\n---\n");
        let talk = parse_talk(&source);
        assert_eq!(insert_block(&talk, 1, STAR).unwrap().apply(&source), e("# One\n---\n<Icon name=\"star\" />\n"));

        let source = e("# One\n---\nlayout: blank\n---\n\n---\n# Three\n");
        let talk = parse_talk(&source);
        let after = insert_block(&talk, 1, STAR).unwrap().apply(&source);
        assert_eq!(after, e("# One\n---\nlayout: blank\n---\n\n<Icon name=\"star\" />\n\n---\n# Three\n"));

        let talk = parse_talk("");
        if !talk.slides.is_empty() {
            assert_eq!(insert_block(&talk, 0, STAR).unwrap().apply(""), "<Icon name=\"star\" />\n");
        }
    });
}

#[test]
fn insert_after_the_last_block() {
    on_both(|e| {
        let source = e("# One\n\nWords\n---\n# Two\n");
        let talk = parse_talk(&source);
        assert_eq!(insert_block(&talk, 0, STAR).unwrap().apply(&source), e("# One\n\nWords\n\n<Icon name=\"star\" />\n---\n# Two\n"));
        let source = e("# One\n\nWords");
        let talk = parse_talk(&source);
        assert_eq!(insert_block(&talk, 0, "| a |\n| - |\n| 1 |").unwrap().apply(&source), e("# One\n\nWords\n\n| a |\n| - |\n| 1 |"));
    });
}

#[test]
fn insert_before_notes() {
    on_both(|e| {
        let source = e("# T\n\nWords\n\n<Notes>\nSay hi\n</Notes>\n");
        let talk = parse_talk(&source);
        let after = insert_block(&talk, 0, STAR).unwrap().apply(&source);
        assert_eq!(after, e("# T\n\nWords\n\n<Icon name=\"star\" />\n\n<Notes>\nSay hi\n</Notes>\n"));

        let source = e("# T\n\nWords\n<Notes>\nSay hi\n</Notes>\n");
        let talk = parse_talk(&source);
        let after = insert_block(&talk, 0, STAR).unwrap().apply(&source);
        assert_eq!(after, e("# T\n\nWords\n\n<Icon name=\"star\" />\n\n<Notes>\nSay hi\n</Notes>\n"));

        let source = e("---\n<Notes>\nSay hi\n</Notes>\n");
        let talk = parse_talk(&source);
        let after = insert_block(&talk, 0, STAR).unwrap().apply(&source);
        assert_eq!(after, e("---\n<Icon name=\"star\" />\n\n<Notes>\nSay hi\n</Notes>\n"));
    });
}

#[test]
fn insert_after_a_list_stays_out_of_the_list() {
    on_both(|e| {
        for list in ["- a\n- b", "1. a\n2. b", "- a\n    - deep", "- an item\n\n  <Icon name=\"in\" />"] {
            let source = e(&format!("# T\n\n{list}\n"));
            let talk = parse_talk(&source);
            let after = parse_talk(&insert_block(&talk, 0, STAR).unwrap().apply(&source));
            let blocks = &after.slides[0].blocks;
            assert_eq!(blocks.len(), 3, "{list}");
            assert_eq!(outline(&blocks[1..2]), outline(&talk.slides[0].blocks[1..2]), "{list}");
            assert!(matches!(&blocks[2], Block::Tag(tag) if tag.attr("name") == Some("star")), "{list}");
        }
    });
}

#[test]
fn insert_into_the_right_column_writes_its_marker() {
    on_both(|e| {
        let source = e("---\nlayout: two-columns\n---\n# Title\n\nLeft words\n");
        let talk = parse_talk(&source);
        let after = insert_block_in_region(&talk, 0, "right", STAR).unwrap().apply(&source);
        assert_eq!(after, e("---\nlayout: two-columns\n---\n# Title\n\nLeft words\n\n<Region name=\"right\" />\n\n<Icon name=\"star\" />\n"));

        // With the marker there, after what the column holds, and before the next column.
        let source = e("---\nlayout: two-columns\n---\n# Title\n\n<Region name=\"right\" />\n\nRight words\n\n<Region name=\"left\" />\n\nLeft\n");
        let talk = parse_talk(&source);
        let after = insert_block_in_region(&talk, 0, "right", STAR).unwrap().apply(&source);
        assert_eq!(
            after,
            e("---\nlayout: two-columns\n---\n# Title\n\n<Region name=\"right\" />\n\nRight words\n\n<Icon name=\"star\" />\n\n<Region name=\"left\" />\n\nLeft\n")
        );
        let after = insert_block_in_region(&talk, 0, "left", STAR).unwrap().apply(&source);
        assert!(after.ends_with(&e("Left\n\n<Icon name=\"star\" />\n")));

        // Into a column with a marker and nothing else.
        let source = e("---\nlayout: two-columns\n---\n# Title\n\n<Region name=\"right\" />\n");
        let talk = parse_talk(&source);
        let after = insert_block_in_region(&talk, 0, "right", STAR).unwrap().apply(&source);
        assert_eq!(after, e("---\nlayout: two-columns\n---\n# Title\n\n<Region name=\"right\" />\n\n<Icon name=\"star\" />\n"));

        // On an empty slide, the main column needs no marker.
        let source = e("---\nlayout: two-columns\n---\n");
        let talk = parse_talk(&source);
        assert_eq!(insert_block_in_region(&talk, 0, "left", STAR).unwrap().apply(&source), e("---\nlayout: two-columns\n---\n<Icon name=\"star\" />\n"));
        let after = insert_block_in_region(&talk, 0, "right", STAR).unwrap().apply(&source);
        assert_eq!(after, e("---\nlayout: two-columns\n---\n<Region name=\"right\" />\n\n<Icon name=\"star\" />\n"));

        assert_eq!(insert_block_in_region(&talk, 0, "middle", STAR), None, "not one of the layout's");
    });
}

#[test]
fn a_heading_put_in_the_body_that_would_be_read_as_the_title_is_refused() {
    let source = "Words\n";
    let talk = parse_talk(source);
    assert_eq!(insert_block_in_region(&talk, 0, "body", "# Heading"), None);
    assert!(insert_block_in_region(&talk, 0, "title", "# Heading").is_some());
}

#[test]
fn what_is_inserted_is_one_block() {
    let source = "# T\n";
    let talk = parse_talk(source);
    for text in ["", "\n\n", "One\n\nTwo", "<Notes>\nHi\n</Notes>", "a\n---\nb", "<Box x=1 />"] {
        assert_eq!(insert_block(&talk, 0, text), None, "{text:?}");
    }
}

#[test]
fn a_block_is_deleted_with_its_lines() {
    on_both(|e| {
        let cases = [
            ("# T\n\n<Icon name=\"star\" />\n\nAfter\n", "# T\n\nAfter\n"),
            ("# T\n\nBefore\n\n<Icon name=\"star\" />\n", "# T\n\nBefore\n"),
            ("# T\n\nBefore\n\n<Icon name=\"star\" />", "# T\n\nBefore"),
            ("# T\n\n<Icon name=\"one\" />\n<Icon name=\"star\" />\n", "# T\n\n<Icon name=\"one\" />\n"),
            ("# T\n\n<Box>\n\n<Icon name=\"star\" />\n\n</Box>\n", "# T\n\n<Box>\n\n</Box>\n"),
            ("# T\n\n- an item\n\n  <Icon name=\"star\" />\n", "# T\n\n- an item\n"),
            ("# T\n\n<Shape\n  kind=\"circle\"\n  name=\"star\"\n/>\n\nAfter\n", "# T\n\nAfter\n"),
            ("# T\n\n<Icon name=\"star\" />\n\n<Notes>\nHi\n</Notes>\n", "# T\n\n<Notes>\nHi\n</Notes>\n"),
        ];
        for (before, after) in cases {
            let source = e(before);
            let talk = parse_talk(&source);
            let tag = all_tags(&talk).into_iter().find(|(_, tag)| tag.attr("name") == Some("star")).unwrap().1;
            let change = delete_block(&talk, 0, &tag.range).unwrap_or_else(|| panic!("{before}"));
            assert_eq!(change.apply(&source), e(after), "{before:?}");
        }
        let source = e("# T\n\n| a |\n| - |\n| 1 |\n\n![A cat](cat.png)\n\nEnd\n");
        let talk = parse_talk(&source);
        let table = talk.slides[0].blocks[1].range().clone();
        assert_eq!(delete_block(&talk, 0, &table).unwrap().apply(&source), e("# T\n\n![A cat](cat.png)\n\nEnd\n"));
        let picture = talk.slides[0].blocks[2].range().clone();
        assert_eq!(delete_block(&talk, 0, &picture).unwrap().apply(&source), e("# T\n\n| a |\n| - |\n| 1 |\n\nEnd\n"));
        assert_eq!(delete_block(&talk, 0, &(table.start..table.end - 1)), None, "no block has that range");
    });
}

#[test]
fn deleting_the_tag_between_two_lists_is_refused() {
    on_both(|e| {
        let source = e("# T\n\n- a\n- b\n\n<Icon name=\"star\" />\n\n- c\n");
        let talk = parse_talk(&source);
        let tag = all_tags(&talk)[0].1.range.clone();
        assert_eq!(delete_block(&talk, 0, &tag), None);
    });
}

#[test]
fn a_slide_that_cannot_be_read_is_left_alone() {
    let source = "# T\n\n<Icon name=\"star\" />\n\n<Icon size=3 />\n";
    let talk = parse_talk(source);
    assert!(talk.slides[0].error.is_some());
    assert_eq!(insert_block(&talk, 0, STAR), None);
    assert_eq!(insert_block_in_region(&talk, 0, "body", STAR), None);
    assert_eq!(delete_block(&talk, 0, &(4..24)), None);
    assert_eq!(set_tag_attr(&talk, 0, &(4..24), "name", Some("x")), None);
    assert_eq!(insert_block(&talk, 5, STAR), None, "no such slide");
}

#[test]
fn a_tag_is_found_and_copied_at_any_depth() {
    on_both(|e| {
        let source = e("# T\n\n- an item\n\n  <Icon name=\"in-a-list\" />\n\n<Box>\n\n<Icon name=\"in-a-box\" />\n\n</Box>\n");
        let talk = parse_talk(&source);
        let tags = all_tags(&talk);
        assert_eq!(tags.len(), 3);
        for (_, tag) in &tags {
            assert_eq!(find_tag(&talk, 0, &tag.range), Some(tag));
        }
        let boxed = &tags[1].1;
        assert_eq!(block_text(&talk, 0, &boxed.range).unwrap(), "<Box>\n\n<Icon name=\"in-a-box\" />\n\n</Box>");
        assert_eq!(block_text(&talk, 0, &tags[0].1.range).unwrap(), "<Icon name=\"in-a-list\" />");
        let heading = talk.slides[0].blocks[0].range().clone();
        assert_eq!(find_tag(&talk, 0, &heading), None, "a heading isn't a tag");
        assert_eq!(block_text(&talk, 0, &heading).unwrap(), "# T");
        assert_eq!(block_text(&talk, 0, &(0..2)), None);
    });
}

#[test]
fn crlf_talks_keep_crlf() {
    let source = "# T\r\n\r\n- a\r\n- b\r\n\r\n<Icon name=\"star\" />\r\n\r\n<Notes>\r\nHi\r\n</Notes>\r\n";
    let talk = parse_talk(source);
    let tag = all_tags(&talk)[0].1.range.clone();
    let multi_line = tag_text(&NewTag { name: "Box", attrs: &[("x", "1")], body: Some("One\n\nTwo") }).unwrap();
    for change in [
        insert_block(&talk, 0, &multi_line).unwrap(),
        insert_block_in_region(&talk, 0, "body", &multi_line).unwrap(),
        delete_block(&talk, 0, &tag).unwrap(),
        set_tag_attr(&talk, 0, &tag, "name", Some("moon")).unwrap(),
    ] {
        let after = change.apply(source);
        assert!(only_crlf(&after), "{after:?}");
    }
    let after = insert_block(&talk, 0, &multi_line).unwrap().apply(source);
    assert!(after.contains("<Icon name=\"star\" />\r\n\r\n<Box x=\"1\">\r\n\r\nOne\r\n\r\nTwo\r\n\r\n</Box>\r\n\r\n<Notes>"), "{after:?}");
}
